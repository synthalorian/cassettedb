//! Raft consensus implementation for leader election and log replication.
//!
//! This is a simplified in-memory Raft implementation suitable for
//! CassetteDB cluster coordination. It supports:
//! - Leader election via RequestVote RPC
//! - Heartbeats via AppendEntries RPC
//! - Persistent log entries for cluster state changes
//!
//! The implementation is intentionally minimal and designed for small
//! clusters (3-7 nodes) where network partitions are rare.

use crate::error::{CassetteError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Unique identifier for a Raft node.
pub type NodeId = String;

/// Raft term number.
pub type Term = u64;

/// Raft log index.
pub type LogIndex = u64;

/// A single entry in the Raft log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    pub index: LogIndex,
    pub term: Term,
    pub command: ClusterCommand,
}

/// Commands that can be replicated through Raft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClusterCommand {
    /// No-op sent by a new leader.
    NoOp,
    /// Add a node to the cluster.
    AddNode { node_id: NodeId, address: String },
    /// Remove a node from the cluster.
    RemoveNode { node_id: NodeId },
    /// Update shard assignment.
    UpdateShardMap { shard_map: crate::shard::ShardMap },
    /// Begin a distributed transaction.
    BeginDistTx { tx_id: String, participants: Vec<NodeId> },
    /// Commit a distributed transaction.
    CommitDistTx { tx_id: String },
    /// Abort a distributed transaction.
    AbortDistTx { tx_id: String },
}

/// Role of a Raft node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RaftRole {
    Follower,
    Candidate,
    Leader,
}

impl fmt::Display for RaftRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RaftRole::Follower => write!(f, "Follower"),
            RaftRole::Candidate => write!(f, "Candidate"),
            RaftRole::Leader => write!(f, "Leader"),
        }
    }
}

/// Persistent Raft state that must survive crashes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentState {
    pub current_term: Term,
    pub voted_for: Option<NodeId>,
    pub log: Vec<LogEntry>,
}

impl PersistentState {
    pub fn new() -> Self {
        Self {
            current_term: 0,
            voted_for: None,
            log: Vec::new(),
        }
    }

    /// Append a new entry to the log.
    pub fn append(&mut self, term: Term, command: ClusterCommand) -> LogIndex {
        let index = self.log.len() as LogIndex + 1;
        self.log.push(LogEntry {
            index,
            term,
            command,
        });
        index
    }

    /// Get the last log index.
    pub fn last_index(&self) -> LogIndex {
        self.log.len() as LogIndex
    }

    /// Get the term of the last log entry (0 if empty).
    pub fn last_term(&self) -> Term {
        self.log.last().map(|e| e.term).unwrap_or(0)
    }

    /// Get entry at a given index (1-based).
    pub fn entry(&self, index: LogIndex) -> Option<&LogEntry> {
        if index == 0 || index > self.log.len() as LogIndex {
            None
        } else {
            self.log.get((index - 1) as usize)
        }
    }
}

impl Default for PersistentState {
    fn default() -> Self {
        Self::new()
    }
}

/// RequestVote RPC request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteRequest {
    pub term: Term,
    pub candidate_id: NodeId,
    pub last_log_index: LogIndex,
    pub last_log_term: Term,
}

/// RequestVote RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteResponse {
    pub term: Term,
    pub vote_granted: bool,
}

/// AppendEntries RPC request (also used as heartbeat).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesRequest {
    pub term: Term,
    pub leader_id: NodeId,
    pub prev_log_index: LogIndex,
    pub prev_log_term: Term,
    pub entries: Vec<LogEntry>,
    pub leader_commit: LogIndex,
}

/// AppendEntries RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    pub term: Term,
    pub success: bool,
    pub match_index: LogIndex,
}

/// Volatile state common to all Raft nodes.
#[derive(Debug, Clone)]
pub struct VolatileState {
    pub commit_index: LogIndex,
    pub last_applied: LogIndex,
    pub role: RaftRole,
    pub leader_id: Option<NodeId>,
    pub last_heartbeat: Instant,
    pub votes_received: Vec<NodeId>,
    pub next_index: HashMap<NodeId, LogIndex>,
    pub match_index: HashMap<NodeId, LogIndex>,
    pub election_timeout: Duration,
    pub heartbeat_interval: Duration,
}

impl VolatileState {
    pub fn new(election_timeout_ms: u64, heartbeat_interval_ms: u64) -> Self {
        Self {
            commit_index: 0,
            last_applied: 0,
            role: RaftRole::Follower,
            leader_id: None,
            last_heartbeat: Instant::now(),
            votes_received: Vec::new(),
            next_index: HashMap::new(),
            match_index: HashMap::new(),
            election_timeout: Duration::from_millis(election_timeout_ms),
            heartbeat_interval: Duration::from_millis(heartbeat_interval_ms),
        }
    }

    pub fn reset_election_timer(&mut self) {
        self.last_heartbeat = Instant::now();
    }

    pub fn is_election_timed_out(&self) -> bool {
        Instant::now().duration_since(self.last_heartbeat) > self.election_timeout
    }
}

/// In-memory Raft node for leader election.
pub struct RaftNode {
    node_id: NodeId,
    peers: Vec<NodeId>,
    persistent: Mutex<PersistentState>,
    volatile: Mutex<VolatileState>,
}

impl RaftNode {
    /// Create a new Raft node.
    pub fn new(node_id: NodeId, peers: Vec<NodeId>) -> Self {
        let mut volatile = VolatileState::new(150, 50);
        // Initialize next_index and match_index for known peers.
        for peer in &peers {
            volatile.next_index.insert(peer.clone(), 1);
            volatile.match_index.insert(peer.clone(), 0);
        }
        Self {
            node_id,
            peers,
            persistent: Mutex::new(PersistentState::new()),
            volatile: Mutex::new(volatile),
        }
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Get the current term.
    pub fn current_term(&self) -> Term {
        self.persistent.lock().unwrap().current_term
    }

    /// Get the current role.
    pub fn role(&self) -> RaftRole {
        self.volatile.lock().unwrap().role
    }

    /// Get the current leader ID, if known.
    pub fn leader_id(&self) -> Option<NodeId> {
        self.volatile.lock().unwrap().leader_id.clone()
    }

    /// Check if this node is the leader.
    pub fn is_leader(&self) -> bool {
        self.role() == RaftRole::Leader
    }

    /// Get peer node IDs.
    pub fn peers(&self) -> &[NodeId] {
        &self.peers
    }

    /// Get the persistent log.
    pub fn log(&self) -> Vec<LogEntry> {
        self.persistent.lock().unwrap().log.clone()
    }

    /// Propose a command (only succeeds if this node is the leader).
    pub fn propose(&self, command: ClusterCommand) -> Result<LogIndex> {
        let v = self.volatile.lock().unwrap();
        if v.role != RaftRole::Leader {
            return Err(CassetteError::Cluster(
                "only leader can propose commands".to_string(),
            ));
        }
        let term = self.persistent.lock().unwrap().current_term;
        let index = {
            let mut p = self.persistent.lock().unwrap();
            p.append(term, command)
        };
        Ok(index)
    }

    /// Start a new election (transition to candidate).
    pub fn start_election(&self) -> RequestVoteRequest {
        let mut p = self.persistent.lock().unwrap();
        let mut v = self.volatile.lock().unwrap();

        p.current_term += 1;
        p.voted_for = Some(self.node_id.clone());
        v.role = RaftRole::Candidate;
        v.votes_received = vec![self.node_id.clone()];
        v.reset_election_timer();

        RequestVoteRequest {
            term: p.current_term,
            candidate_id: self.node_id.clone(),
            last_log_index: p.last_index(),
            last_log_term: p.last_term(),
        }
    }

    /// Handle a RequestVote request.
    pub fn handle_request_vote(&self, req: RequestVoteRequest) -> RequestVoteResponse {
        let mut p = self.persistent.lock().unwrap();
        let mut v = self.volatile.lock().unwrap();

        if req.term > p.current_term {
            p.current_term = req.term;
            p.voted_for = None;
            v.role = RaftRole::Follower;
            v.leader_id = None;
        }

        if req.term < p.current_term {
            return RequestVoteResponse {
                term: p.current_term,
                vote_granted: false,
            };
        }

        let log_ok = req.last_log_term > p.last_term()
            || (req.last_log_term == p.last_term()
                && req.last_log_index >= p.last_index());

        let vote_granted = log_ok
            && (p.voted_for.is_none() || p.voted_for.as_ref() == Some(&req.candidate_id));

        if vote_granted {
            p.voted_for = Some(req.candidate_id.clone());
            v.reset_election_timer();
        }

        RequestVoteResponse {
            term: p.current_term,
            vote_granted,
        }
    }

    /// Record a vote received during an election.
    pub fn record_vote(&self, voter: NodeId, res: RequestVoteResponse) -> bool {
        let mut p = self.persistent.lock().unwrap();
        let mut v = self.volatile.lock().unwrap();

        if res.term > p.current_term {
            p.current_term = res.term;
            p.voted_for = None;
            v.role = RaftRole::Follower;
            v.leader_id = None;
            return false;
        }

        if res.term < p.current_term || v.role != RaftRole::Candidate {
            return false;
        }

        if res.vote_granted && !v.votes_received.contains(&voter) {
            v.votes_received.push(voter);
        }

        let total_nodes = self.peers.len() + 1;
        let quorum = (total_nodes / 2) + 1;
        let won = v.votes_received.len() >= quorum;

        if won {
            v.role = RaftRole::Leader;
            v.leader_id = Some(self.node_id.clone());
            for peer in &self.peers {
                v.next_index.insert(peer.clone(), p.last_index() + 1);
                v.match_index.insert(peer.clone(), 0);
            }
        }

        won
    }

    /// Build a heartbeat request for a peer.
    pub fn heartbeat_for(&self, peer: &NodeId) -> AppendEntriesRequest {
        let p = self.persistent.lock().unwrap();
        let v = self.volatile.lock().unwrap();

        let next_idx = v.next_index.get(peer).copied().unwrap_or(1);
        let prev_log_index = next_idx.saturating_sub(1);
        let prev_log_term = p.entry(prev_log_index).map(|e| e.term).unwrap_or(0);

        AppendEntriesRequest {
            term: p.current_term,
            leader_id: self.node_id.clone(),
            prev_log_index,
            prev_log_term,
            entries: Vec::new(),
            leader_commit: v.commit_index,
        }
    }

    /// Handle an AppendEntries request (heartbeat or log replication).
    pub fn handle_append_entries(&self, req: AppendEntriesRequest) -> AppendEntriesResponse {
        let mut p = self.persistent.lock().unwrap();
        let mut v = self.volatile.lock().unwrap();

        if req.term > p.current_term {
            p.current_term = req.term;
            p.voted_for = None;
            v.role = RaftRole::Follower;
        }

        if req.term < p.current_term {
            return AppendEntriesResponse {
                term: p.current_term,
                success: false,
                match_index: 0,
            };
        }

        // Valid leader communication: reset election timer.
        v.reset_election_timer();
        v.role = RaftRole::Follower;
        v.leader_id = Some(req.leader_id.clone());

        // Check log consistency at prev_log_index.
        let prev_entry = if req.prev_log_index == 0 {
            Some(&LogEntry {
                index: 0,
                term: 0,
                command: ClusterCommand::NoOp,
            })
        } else {
            p.entry(req.prev_log_index)
        };

        let log_consistent = prev_entry.map(|e| e.term).unwrap_or(req.prev_log_term)
            == req.prev_log_term;

        if !log_consistent {
            return AppendEntriesResponse {
                term: p.current_term,
                success: false,
                match_index: 0,
            };
        }

        // Append entries (simplified: just append all new entries).
        for entry in &req.entries {
            if entry.index > p.last_index() {
                p.log.push(entry.clone());
            } else if let Some(existing) = p.entry(entry.index) {
                if existing.term != entry.term {
                    // Conflict: truncate and append.
                    p.log.truncate((entry.index - 1) as usize);
                    p.log.push(entry.clone());
                }
            }
        }

        // Update commit index.
        if req.leader_commit > v.commit_index {
            v.commit_index = req.leader_commit.min(p.last_index());
        }

        AppendEntriesResponse {
            term: p.current_term,
            success: true,
            match_index: req.prev_log_index + req.entries.len() as LogIndex,
        }
    }

    /// Record a successful AppendEntries response from a peer.
    pub fn record_append_success(&self, peer: NodeId, res: AppendEntriesResponse) {
        let mut p = self.persistent.lock().unwrap();
        let mut v = self.volatile.lock().unwrap();

        if res.term > p.current_term {
            p.current_term = res.term;
            p.voted_for = None;
            v.role = RaftRole::Follower;
            v.leader_id = None;
            return;
        }

        if v.role != RaftRole::Leader {
            return;
        }

        v.match_index.insert(peer.clone(), res.match_index);
        v.next_index.insert(peer.clone(), res.match_index + 1);

        // Try to advance commit index.
        let new_commit = res.match_index;
        if new_commit > v.commit_index {
            let _total_nodes = self.peers.len() + 1;
            let mut match_indices: Vec<LogIndex> = v.match_index.values().copied().collect();
            match_indices.push(p.last_index()); // leader's own log
            match_indices.sort_unstable_by(|a, b| b.cmp(a));
            let quorum_match = match_indices[match_indices.len() / 2];

            if quorum_match > v.commit_index && p.entry(quorum_match).map(|e| e.term).unwrap_or(0) == p.current_term {
                v.commit_index = quorum_match;
            }
        }
    }

    /// Record a failed AppendEntries response.
    pub fn record_append_failure(&self, peer: NodeId, res: AppendEntriesResponse) {
        let mut p = self.persistent.lock().unwrap();
        let mut v = self.volatile.lock().unwrap();

        if res.term > p.current_term {
            p.current_term = res.term;
            p.voted_for = None;
            v.role = RaftRole::Follower;
            v.leader_id = None;
            return;
        }

        if v.role != RaftRole::Leader {
            return;
        }

        let next = v.next_index.get(&peer).copied().unwrap_or(1);
        if next > 1 {
            v.next_index.insert(peer, next - 1);
        }
    }

    /// Add a peer dynamically.
    pub fn add_peer(&self, peer: NodeId) {
        let mut v = self.volatile.lock().unwrap();
        if !self.peers.contains(&peer) && peer != self.node_id {
            v.next_index.insert(peer.clone(), 1);
            v.match_index.insert(peer.clone(), 0);
        }
    }

    /// Remove a peer dynamically.
    pub fn remove_peer(&self, peer: &NodeId) {
        let mut v = self.volatile.lock().unwrap();
        v.next_index.remove(peer);
        v.match_index.remove(peer);
    }

    /// Check whether an election timeout has occurred.
    pub fn election_due(&self) -> bool {
        let v = self.volatile.lock().unwrap();
        v.is_election_timed_out()
    }

    /// Get commit index.
    pub fn commit_index(&self) -> LogIndex {
        self.volatile.lock().unwrap().commit_index
    }

    /// Get last applied index.
    pub fn last_applied(&self) -> LogIndex {
        self.volatile.lock().unwrap().last_applied
    }

    /// Apply committed entries that haven't been applied yet.
    /// Returns the list of newly applied commands.
    pub fn apply_committed(&self) -> Vec<ClusterCommand> {
        let mut v = self.volatile.lock().unwrap();
        let p = self.persistent.lock().unwrap();
        let mut applied = Vec::new();
        while v.last_applied < v.commit_index {
            v.last_applied += 1;
            if let Some(entry) = p.entry(v.last_applied) {
                applied.push(entry.command.clone());
            }
        }
        applied
    }

    /// Serialize persistent state.
    pub fn serialize_state(&self) -> Result<Vec<u8>> {
        let p = self.persistent.lock().unwrap();
        Ok(serde_json::to_vec(&*p)?)
    }

    /// Deserialize and replace persistent state.
    pub fn deserialize_state(&self, bytes: &[u8]) -> Result<()> {
        let state: PersistentState = serde_json::from_slice(bytes)?;
        let mut p = self.persistent.lock().unwrap();
        *p = state;
        Ok(())
    }
}

/// Shared handle to a Raft node.
pub type SharedRaftNode = Arc<RaftNode>;

/// Create a shared Raft node.
pub fn create_raft_node(node_id: NodeId, peers: Vec<NodeId>) -> SharedRaftNode {
    Arc::new(RaftNode::new(node_id, peers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let node = RaftNode::new("a".to_string(), vec!["b".to_string(), "c".to_string()]);
        assert_eq!(node.current_term(), 0);
        assert_eq!(node.role(), RaftRole::Follower);
        assert!(!node.is_leader());
    }

    #[test]
    fn test_handle_request_vote_higher_term() {
        let node = RaftNode::new("a".to_string(), vec!["b".to_string()]);
        let req = RequestVoteRequest {
            term: 2,
            candidate_id: "b".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };
        let res = node.handle_request_vote(req);
        assert!(res.vote_granted);
        assert_eq!(node.current_term(), 2);
    }

    #[test]
    fn test_start_election_and_win() {
        let node = RaftNode::new("a".to_string(), vec!["b".to_string(), "c".to_string()]);
        let req = node.start_election();
        assert_eq!(req.term, 1);
        assert_eq!(node.role(), RaftRole::Candidate);

        // Receive votes from b and c (plus self = majority of 3).
        let res = RequestVoteResponse {
            term: 1,
            vote_granted: true,
        };
        assert!(node.record_vote("b".to_string(), res.clone()));
        assert!(node.is_leader());
    }

    #[test]
    fn test_append_entries_heartbeat() {
        let node = RaftNode::new("a".to_string(), vec!["b".to_string()]);
        let req = AppendEntriesRequest {
            term: 1,
            leader_id: "b".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: 0,
        };
        let res = node.handle_append_entries(req);
        assert!(res.success);
        assert_eq!(node.current_term(), 1);
        assert_eq!(node.leader_id(), Some("b".to_string()));
    }

    #[test]
    fn test_propose_requires_leader() {
        let node = RaftNode::new("a".to_string(), vec!["b".to_string()]);
        let err = node.propose(ClusterCommand::NoOp).unwrap_err();
        assert!(matches!(err, CassetteError::Cluster(_)));
    }
}
