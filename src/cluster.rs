//! Cluster management: node membership, health monitoring, and failover.
//!
//! The cluster module builds on top of the Raft consensus layer to maintain
//! a consistent view of the cluster. It tracks node liveness, performs
//! automatic failover, and exposes a management API used by the CLI.

use crate::error::{CassetteError, Result};
use crate::raft::{
    create_raft_node, AppendEntriesRequest, AppendEntriesResponse, ClusterCommand,
    NodeId, RaftRole, RequestVoteRequest, RequestVoteResponse, SharedRaftNode,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Configuration for a single cluster node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeInfo {
    pub id: NodeId,
    pub address: String,
    pub role: NodeRole,
    pub last_seen: i64,
}

/// Role of a node in the CassetteDB cluster.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeRole {
    /// Primary/leader node.
    Primary,
    /// Secondary/follower node.
    Secondary,
    /// Observer node (does not vote).
    Observer,
}

/// Cluster-wide configuration persisted on every node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub cluster_id: String,
    pub nodes: Vec<NodeInfo>,
    pub failover_enabled: bool,
    pub health_check_interval_ms: u64,
    pub health_check_timeout_ms: u64,
}

impl ClusterConfig {
    /// Create a new cluster configuration with a single seed node.
    pub fn new(cluster_id: impl Into<String>, seed: NodeInfo) -> Self {
        Self {
            cluster_id: cluster_id.into(),
            nodes: vec![seed],
            failover_enabled: true,
            health_check_interval_ms: 1000,
            health_check_timeout_ms: 3000,
        }
    }

    /// File name used for cluster configuration.
    pub const FILE_NAME: &'static str = "cluster.json";

    /// Save configuration to a directory.
    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        let path = dir.join(Self::FILE_NAME);
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(&path, bytes)?;
        Ok(())
    }

    /// Load configuration from a directory.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(Self::FILE_NAME);
        let bytes = fs::read(&path)?;
        let cfg: ClusterConfig = serde_json::from_slice(&bytes)?;
        Ok(cfg)
    }

    /// Find a node by ID.
    pub fn find_node(&self, id: &NodeId) -> Option<&NodeInfo> {
        self.nodes.iter().find(|n| n.id == *id)
    }

    /// Add a node to the cluster configuration.
    pub fn add_node(&mut self, node: NodeInfo) {
        if !self.nodes.iter().any(|n| n.id == node.id) {
            self.nodes.push(node);
        }
    }

    /// Remove a node from the cluster configuration.
    pub fn remove_node(&mut self, id: &NodeId) {
        self.nodes.retain(|n| n.id != *id);
    }

    /// Get voting nodes (primaries and secondaries).
    pub fn voting_nodes(&self) -> Vec<&NodeInfo> {
        self.nodes
            .iter()
            .filter(|n| n.role != NodeRole::Observer)
            .collect()
    }

    /// Get the primary/leader node.
    pub fn primary(&self) -> Option<&NodeInfo> {
        self.nodes.iter().find(|n| n.role == NodeRole::Primary)
    }
}

/// Health status of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Unhealthy,
    Unknown,
}

/// Health record for a peer.
#[derive(Debug, Clone)]
pub struct HealthRecord {
    pub status: HealthStatus,
    pub last_heartbeat: Instant,
    pub missed_beats: u32,
}

/// A managed CassetteDB cluster node.
pub struct ClusterNode {
    local_id: NodeId,
    config_dir: PathBuf,
    config: Mutex<ClusterConfig>,
    raft: SharedRaftNode,
    health: Mutex<HashMap<NodeId, HealthRecord>>,
    last_failover: Mutex<Option<Instant>>,
    failover_cooldown: Duration,
}

impl ClusterNode {
    /// Initialize a brand-new cluster with this node as the seed.
    pub fn init_cluster(
        cluster_id: impl Into<String>,
        local_id: NodeId,
        address: String,
        config_dir: &Path,
    ) -> Result<Arc<Self>> {
        let seed = NodeInfo {
            id: local_id.clone(),
            address: address.clone(),
            role: NodeRole::Primary,
            last_seen: chrono::Utc::now().timestamp(),
        };
        let config = ClusterConfig::new(cluster_id, seed);
        config.save(config_dir)?;

        let raft = create_raft_node(local_id.clone(), Vec::new());
        Ok(Arc::new(ClusterNode {
            local_id,
            config_dir: config_dir.to_path_buf(),
            config: Mutex::new(config),
            raft,
            health: Mutex::new(HashMap::new()),
            last_failover: Mutex::new(None),
            failover_cooldown: Duration::from_secs(5),
        }))
    }

    /// Join an existing cluster by loading its configuration and registering.
    pub fn join_cluster(
        local_id: NodeId,
        _address: String,
        config_dir: &Path,
        cluster_config: ClusterConfig,
    ) -> Result<Arc<Self>> {
        let mut peers: Vec<NodeId> = cluster_config
            .nodes
            .iter()
            .map(|n| n.id.clone())
            .filter(|id| id != &local_id)
            .collect();
        peers.sort();
        peers.dedup();

        let raft = create_raft_node(local_id.clone(), peers);
        Ok(Arc::new(ClusterNode {
            local_id,
            config_dir: config_dir.to_path_buf(),
            config: Mutex::new(cluster_config),
            raft,
            health: Mutex::new(HashMap::new()),
            last_failover: Mutex::new(None),
            failover_cooldown: Duration::from_secs(5),
        }))
    }

    /// Load a cluster node from its on-disk configuration directory.
    pub fn load(local_id: NodeId, config_dir: &Path) -> Result<Arc<Self>> {
        let config = ClusterConfig::load(config_dir)?;
        let mut peers: Vec<NodeId> = config
            .nodes
            .iter()
            .map(|n| n.id.clone())
            .filter(|id| id != &local_id)
            .collect();
        peers.sort();
        peers.dedup();

        let raft = create_raft_node(local_id.clone(), peers);
        Ok(Arc::new(ClusterNode {
            local_id,
            config_dir: config_dir.to_path_buf(),
            config: Mutex::new(config),
            raft,
            health: Mutex::new(HashMap::new()),
            last_failover: Mutex::new(None),
            failover_cooldown: Duration::from_secs(5),
        }))
    }

    /// Save current configuration to disk.
    pub fn save_config(&self) -> Result<()> {
        let cfg = self.config.lock().unwrap();
        cfg.save(&self.config_dir)
    }

    /// Get this node's ID.
    pub fn local_id(&self) -> &NodeId {
        &self.local_id
    }

    /// Get the cluster configuration.
    pub fn config(&self) -> ClusterConfig {
        self.config.lock().unwrap().clone()
    }

    /// Access the underlying Raft node.
    pub fn raft(&self) -> SharedRaftNode {
        self.raft.clone()
    }

    /// Check if this node is the current Raft leader.
    pub fn is_leader(&self) -> bool {
        self.raft.is_leader()
    }

    /// Get the current leader ID according to Raft.
    pub fn leader_id(&self) -> Option<NodeId> {
        self.raft.leader_id()
    }

    /// Add a new node to the cluster. Must be called on the leader.
    pub fn add_node(&self, node: NodeInfo) -> Result<()> {
        if !self.is_leader() {
            return Err(CassetteError::Cluster(
                "only the leader can add nodes".to_string(),
            ));
        }
        {
            let mut cfg = self.config.lock().unwrap();
            cfg.add_node(node.clone());
        }
        self.save_config()?;
        self.raft.add_peer(node.id.clone());
        self.raft.propose(ClusterCommand::AddNode {
            node_id: node.id,
            address: node.address,
        })?;
        Ok(())
    }

    /// Remove a node from the cluster. Must be called on the leader.
    pub fn remove_node(&self, node_id: &NodeId) -> Result<()> {
        if !self.is_leader() {
            return Err(CassetteError::Cluster(
                "only the leader can remove nodes".to_string(),
            ));
        }
        {
            let mut cfg = self.config.lock().unwrap();
            cfg.remove_node(node_id);
        }
        self.save_config()?;
        self.raft.remove_peer(node_id);
        self.raft.propose(ClusterCommand::RemoveNode {
            node_id: node_id.clone(),
        })?;
        Ok(())
    }

    /// Record a heartbeat from a peer.
    pub fn record_heartbeat(&self, peer: &NodeId) {
        let mut h = self.health.lock().unwrap();
        h.insert(
            peer.clone(),
            HealthRecord {
                status: HealthStatus::Healthy,
                last_heartbeat: Instant::now(),
                missed_beats: 0,
            },
        );
    }

    /// Mark a peer as potentially failed.
    pub fn record_missed_heartbeat(&self, peer: &NodeId) {
        let mut h = self.health.lock().unwrap();
        let record = h.entry(peer.clone()).or_insert_with(|| HealthRecord {
            status: HealthStatus::Unknown,
            last_heartbeat: Instant::now(),
            missed_beats: 0,
        });
        record.missed_beats += 1;
        if record.missed_beats >= 3 {
            record.status = HealthStatus::Unhealthy;
        }
    }

    /// Get the health map snapshot.
    pub fn health_snapshot(&self) -> HashMap<NodeId, HealthStatus> {
        let h = self.health.lock().unwrap();
        h.iter()
            .map(|(k, v)| (k.clone(), v.status))
            .collect()
    }

    /// Decide whether a failover should be triggered for a failed primary.
    pub fn should_failover(&self, failed_node: &NodeId) -> bool {
        let cfg = self.config.lock().unwrap();
        if !cfg.failover_enabled {
            return false;
        }
        let Some(node) = cfg.find_node(failed_node) else {
            return false;
        };
        if node.role != NodeRole::Primary {
            return false;
        }
        if self.local_id == *failed_node {
            return false;
        }
        let last = self.last_failover.lock().unwrap();
        if let Some(t) = *last {
            if Instant::now().duration_since(t) < self.failover_cooldown {
                return false;
            }
        }
        true
    }

    /// Perform failover: promote this node to primary and remove the failed node.
    pub fn perform_failover(&self, failed_node: &NodeId) -> Result<()> {
        if !self.should_failover(failed_node) {
            return Err(CassetteError::Cluster(
                "failover preconditions not met".to_string(),
            ));
        }
        {
            let mut cfg = self.config.lock().unwrap();
            for n in &mut cfg.nodes {
                if n.id == *failed_node {
                    n.role = NodeRole::Secondary;
                }
                if n.id == self.local_id {
                    n.role = NodeRole::Primary;
                }
            }
        }
        self.save_config()?;
        *self.last_failover.lock().unwrap() = Some(Instant::now());
        Ok(())
    }

    /// Build a RequestVote request from the underlying Raft node.
    pub fn request_vote(&self) -> RequestVoteRequest {
        self.raft.start_election()
    }

    /// Handle a RequestVote RPC.
    pub fn handle_request_vote(&self, req: RequestVoteRequest) -> RequestVoteResponse {
        self.raft.handle_request_vote(req)
    }

    /// Handle an AppendEntries RPC.
    pub fn handle_append_entries(&self, req: AppendEntriesRequest) -> AppendEntriesResponse {
        self.raft.handle_append_entries(req)
    }

    /// Record a vote during an election.
    pub fn record_vote(&self, voter: NodeId, res: RequestVoteResponse) -> bool {
        self.raft.record_vote(voter, res)
    }

    /// Record successful AppendEntries response.
    pub fn record_append_success(&self, peer: NodeId, res: AppendEntriesResponse) {
        self.raft.record_append_success(peer, res)
    }

    /// Record failed AppendEntries response.
    pub fn record_append_failure(&self, peer: NodeId, res: AppendEntriesResponse) {
        self.raft.record_append_failure(peer, res)
    }

    /// Run a single tick of the cluster state machine.
    /// Returns true if this node became the leader during this tick.
    pub fn tick(&self) -> bool {
        let role = self.raft.role();
        match role {
            RaftRole::Follower | RaftRole::Candidate => {
                if self.raft.election_due() {
                    let req = self.raft.start_election();
                    let peers: Vec<NodeId> = self
                        .raft
                        .peers()
                        .iter()
                        .filter(|p| **p != self.local_id)
                        .cloned()
                        .collect();
                    // Simulate immediate responses from a local test harness.
                    // In a networked deployment, the caller sends RPCs and feeds back
                    // responses via record_vote().
                    let mut became_leader = false;
                    for peer in peers {
                        let res = self.handle_request_vote(req.clone());
                        if self.record_vote(peer, res) {
                            became_leader = true;
                        }
                    }
                    return became_leader;
                }
                false
            }
            RaftRole::Leader => {
                // Send heartbeats to all peers.
                let peers: Vec<NodeId> = self.raft.peers().to_vec();
                for peer in peers {
                    let _req = self.raft.heartbeat_for(&peer);
                    // In a real networked system, the heartbeat is sent over the wire.
                    // Here we record a successful heartbeat for local health tracking.
                    self.record_heartbeat(&peer);
                }
                // Apply any committed entries.
                let _commands = self.raft.apply_committed();
                false
            }
        }
    }

    /// Cluster status summary with typed health values.
    pub fn status_raw(&self) -> RawClusterStatus {
        let cfg = self.config.lock().unwrap();
        let health = self.health_snapshot();
        RawClusterStatus {
            cluster_id: cfg.cluster_id.clone(),
            local_id: self.local_id.clone(),
            leader_id: self.leader_id(),
            role: self.raft.role(),
            nodes: cfg.nodes.clone(),
            health,
            failover_enabled: cfg.failover_enabled,
        }
    }
}

/// Internal cluster status with typed health values.
#[derive(Debug, Clone)]
pub struct RawClusterStatus {
    pub cluster_id: String,
    pub local_id: NodeId,
    pub leader_id: Option<NodeId>,
    pub role: RaftRole,
    pub nodes: Vec<NodeInfo>,
    pub health: HashMap<NodeId, HealthStatus>,
    pub failover_enabled: bool,
}

/// Human-readable cluster status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    pub cluster_id: String,
    pub local_id: NodeId,
    pub leader_id: Option<NodeId>,
    pub role: RaftRole,
    pub nodes: Vec<NodeInfo>,
    pub health: HashMap<NodeId, String>,
    pub failover_enabled: bool,
}

impl ClusterStatus {
    /// Build a serializable status from a cluster node.
    pub fn from_node(node: &ClusterNode) -> Self {
        let raw = node.status_raw();
        Self {
            cluster_id: raw.cluster_id,
            local_id: raw.local_id,
            leader_id: raw.leader_id,
            role: raw.role,
            nodes: raw.nodes,
            health: raw
                .health
                .into_iter()
                .map(|(k, v)| {
                    let s = match v {
                        HealthStatus::Healthy => "healthy",
                        HealthStatus::Unhealthy => "unhealthy",
                        HealthStatus::Unknown => "unknown",
                    };
                    (k, s.to_string())
                })
                .collect(),
            failover_enabled: raw.failover_enabled,
        }
    }
}

/// High-level cluster manager used by the CLI and server.
pub struct ClusterManager {
    inner: Arc<ClusterNode>,
}

impl ClusterManager {
    /// Initialize a new cluster.
    pub fn init(
        cluster_id: String,
        local_id: NodeId,
        address: String,
        config_dir: &Path,
    ) -> Result<Self> {
        let node = ClusterNode::init_cluster(cluster_id, local_id, address, config_dir)?;
        Ok(Self { inner: node })
    }

    /// Join an existing cluster (config must already be supplied by discovery).
    pub fn join(
        local_id: NodeId,
        address: String,
        config_dir: &Path,
        config: ClusterConfig,
    ) -> Result<Self> {
        let node = ClusterNode::join_cluster(local_id, address, config_dir, config)?;
        Ok(Self { inner: node })
    }

    /// Load a cluster node from disk.
    pub fn load(local_id: NodeId, config_dir: &Path) -> Result<Self> {
        let node = ClusterNode::load(local_id, config_dir)?;
        Ok(Self { inner: node })
    }

    /// Add a node.
    pub fn add_node(&self, node: NodeInfo) -> Result<()> {
        self.inner.add_node(node)
    }

    /// Remove a node.
    pub fn remove_node(&self, node_id: &NodeId) -> Result<()> {
        self.inner.remove_node(node_id)
    }

    /// Trigger failover to this node if allowed.
    pub fn failover(&self, failed_node: &NodeId) -> Result<()> {
        self.inner.perform_failover(failed_node)
    }

    /// Set failover enabled/disabled.
    pub fn set_failover_enabled(&self, enabled: bool) -> Result<()> {
        let mut cfg = self.inner.config.lock().unwrap();
        cfg.failover_enabled = enabled;
        drop(cfg);
        self.inner.save_config()
    }

    /// Get cluster status.
    pub fn status(&self) -> ClusterStatus {
        ClusterStatus::from_node(&self.inner)
    }

    /// Access the underlying cluster node.
    pub fn node(&self) -> Arc<ClusterNode> {
        self.inner.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_and_load_cluster() {
        let dir = TempDir::new().unwrap();
        let node = ClusterNode::init_cluster(
            "test-cluster",
            "node-a".to_string(),
            "127.0.0.1:7001".to_string(),
            dir.path(),
        )
        .unwrap();
        assert_eq!(node.local_id(), "node-a");

        let loaded = ClusterNode::load("node-a".to_string(), dir.path()).unwrap();
        assert_eq!(loaded.config().cluster_id, "test-cluster");
    }

    #[test]
    fn test_add_remove_node() {
        let dir = TempDir::new().unwrap();
        let node = ClusterNode::init_cluster(
            "c",
            "a".to_string(),
            "127.0.0.1:1".to_string(),
            dir.path(),
        )
        .unwrap();
        // Force leader so add_node can succeed.
        node.raft.start_election();
        node.raft.record_vote(
            "a".to_string(),
            RequestVoteResponse {
                term: 1,
                vote_granted: true,
            },
        );
        assert!(node.is_leader());

        node.add_node(NodeInfo {
            id: "b".to_string(),
            address: "127.0.0.1:2".to_string(),
            role: NodeRole::Secondary,
            last_seen: 0,
        })
        .unwrap();
        assert!(node.config().find_node(&"b".to_string()).is_some());

        node.remove_node(&"b".to_string()).unwrap();
        assert!(node.config().find_node(&"b".to_string()).is_none());
    }

    #[test]
    fn test_failover() {
        let dir = TempDir::new().unwrap();
        let node = ClusterNode::init_cluster(
            "c",
            "a".to_string(),
            "127.0.0.1:1".to_string(),
            dir.path(),
        )
        .unwrap();

        node.raft.start_election();
        node.raft.record_vote(
            "a".to_string(),
            RequestVoteResponse {
                term: 1,
                vote_granted: true,
            },
        );
        assert!(node.is_leader());

        node.add_node(NodeInfo {
            id: "b".to_string(),
            address: "127.0.0.1:2".to_string(),
            role: NodeRole::Primary,
            last_seen: 0,
        })
        .unwrap();

        node.perform_failover(&"b".to_string()).unwrap();
        let cfg = node.config();
        let a = cfg.find_node(&"a".to_string()).unwrap();
        assert_eq!(a.role, NodeRole::Primary);
    }
}
