//! Sharding support for CassetteDB.
//!
//! Documents are distributed across shards using consistent hashing on
//! the document ID. Each shard is backed by a CassetteEngine instance
//! stored in its own file. The shard router maps document IDs to shard
//! IDs and, when running in cluster mode, shard IDs to cluster nodes.

use crate::document::Document;
use crate::engine::CassetteEngine;
use crate::error::{CassetteError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Identifier for a shard.
pub type ShardId = String;

/// Mapping from a document key to a shard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShardMap {
    /// Number of virtual nodes per physical shard for consistent hashing.
    pub virtual_nodes: u32,
    /// Shard ID -> owning node ID (when clustered; may be empty for local sharding).
    pub shard_to_node: BTreeMap<ShardId, String>,
}

impl Default for ShardMap {
    fn default() -> Self {
        Self {
            virtual_nodes: 150,
            shard_to_node: BTreeMap::new(),
        }
    }
}

impl ShardMap {
    /// Create a shard map with the given number of virtual nodes.
    pub fn new(virtual_nodes: u32) -> Self {
        Self {
            virtual_nodes,
            shard_to_node: BTreeMap::new(),
        }
    }

    /// Add a shard and optionally assign it to a node.
    pub fn add_shard(&mut self, shard_id: ShardId, node_id: Option<String>) {
        self.shard_to_node.insert(shard_id, node_id.unwrap_or_default());
    }

    /// Remove a shard.
    pub fn remove_shard(&mut self, shard_id: &ShardId) {
        self.shard_to_node.remove(shard_id);
    }

    /// Get the node assigned to a shard, if any.
    pub fn node_for_shard(&self, shard_id: &ShardId) -> Option<&String> {
        self.shard_to_node.get(shard_id).filter(|s| !s.is_empty())
    }

    /// Serialize the shard map to JSON bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }

    /// Deserialize the shard map from JSON bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

/// A consistent-hashing shard router.
pub struct ShardRouter {
    shard_map: ShardMap,
    ring: BTreeMap<u32, ShardId>,
    base_dir: PathBuf,
    engines: BTreeMap<ShardId, CassetteEngine>,
}

impl ShardRouter {
    /// Create a new router with the given shard map and base storage directory.
    pub fn new(shard_map: ShardMap, base_dir: &Path) -> Result<Self> {
        let mut ring = BTreeMap::new();
        for shard_id in shard_map.shard_to_node.keys() {
            for v in 0..shard_map.virtual_nodes {
                let hash = Self::hash_key(&format!("{}#{}", shard_id, v));
                ring.insert(hash, shard_id.clone());
            }
        }
        let mut router = Self {
            shard_map,
            ring,
            base_dir: base_dir.to_path_buf(),
            engines: BTreeMap::new(),
        };
        router.open_engines()?;
        Ok(router)
    }

    /// Create a router from a fixed list of shard IDs.
    pub fn with_shards(shards: Vec<ShardId>, base_dir: &Path) -> Result<Self> {
        let mut shard_map = ShardMap::default();
        for shard_id in shards {
            shard_map.add_shard(shard_id, None);
        }
        Self::new(shard_map, base_dir)
    }

    /// Compute a simple 32-bit hash for a string key.
    pub fn hash_key(key: &str) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let full = hasher.finish();
        ((full >> 32) ^ full) as u32
    }

    /// Get the shard ID for a document ID using consistent hashing.
    pub fn shard_for(&self, doc_id: &str) -> Option<ShardId> {
        if self.ring.is_empty() {
            return None;
        }
        let hash = Self::hash_key(doc_id);
        // Find the first virtual node with hash >= doc hash.
        let mut candidate = None;
        for (_, shard) in self.ring.range(hash..) {
            candidate = Some(shard.clone());
            break;
        }
        // Wrap around to the first virtual node.
        candidate.or_else(|| self.ring.values().next().cloned())
    }

    /// Insert a document into the correct shard.
    pub fn insert(&mut self, doc: Document) -> Result<String> {
        let shard_id = self
            .shard_for(&doc.id)
            .ok_or_else(|| CassetteError::Shard("no shards available".to_string()))?;
        let engine = self
            .engines
            .get_mut(&shard_id)
            .ok_or_else(|| CassetteError::Shard(format!("shard {} not open", shard_id)))?;
        let id = engine.insert(doc)?;
        Ok(id)
    }

    /// Get a document by ID from the correct shard.
    pub fn get(&self, id: &str) -> Option<Document> {
        let shard_id = self.shard_for(id)?;
        self.engines.get(&shard_id)?.get(id).cloned()
    }

    /// Update a document by ID in the correct shard.
    pub fn update(&mut self, id: &str, data: serde_json::Value) -> Result<()> {
        let shard_id = self
            .shard_for(id)
            .ok_or_else(|| CassetteError::Shard("no shards available".to_string()))?;
        let engine = self
            .engines
            .get_mut(&shard_id)
            .ok_or_else(|| CassetteError::Shard(format!("shard {} not open", shard_id)))?;
        engine.update(id, data)
    }

    /// Delete a document by ID from the correct shard.
    pub fn delete(&mut self, id: &str) -> Result<()> {
        let shard_id = self
            .shard_for(id)
            .ok_or_else(|| CassetteError::Shard("no shards available".to_string()))?;
        let engine = self
            .engines
            .get_mut(&shard_id)
            .ok_or_else(|| CassetteError::Shard(format!("shard {} not open", shard_id)))?;
        engine.delete(id)
    }

    /// Query all shards and merge results.
    pub fn query_all(&self, q: &crate::query::Query) -> crate::query::QueryResult {
        let mut all_docs = Vec::new();
        for engine in self.engines.values() {
            let res = engine.query(q);
            all_docs.extend(res.documents);
        }
        // Sort by ID for deterministic ordering.
        all_docs.sort_by(|a, b| a.id.cmp(&b.id));
        let count = all_docs.len();
        crate::query::QueryResult {
            documents: all_docs,
            count,
        }
    }

    /// Total document count across all shards.
    pub fn doc_count(&self) -> usize {
        self.engines.values().map(|e| e.doc_count()).sum()
    }

    /// List all shard IDs.
    pub fn shard_ids(&self) -> Vec<ShardId> {
        self.shard_map.shard_to_node.keys().cloned().collect()
    }

    /// Access the underlying engines.
    pub fn engines(&self) -> &BTreeMap<ShardId, CassetteEngine> {
        &self.engines
    }

    /// Access a mutable engine by shard ID.
    pub fn engine_mut(&mut self, shard_id: &ShardId) -> Option<&mut CassetteEngine> {
        self.engines.get_mut(shard_id)
    }

    /// Rebalance: move documents from one shard to another.
    /// This opens both shard engines if necessary and copies documents whose
    /// keys now map to `target_shard`.
    pub fn rebalance(&mut self, source_shard: &ShardId, target_shard: &ShardId) -> Result<usize> {
        let source_path = self.shard_path(source_shard);
        let target_path = self.shard_path(target_shard);
        if !source_path.exists() {
            return Err(CassetteError::Shard(format!(
                "source shard {} does not exist",
                source_shard
            )));
        }
        let mut moved = 0usize;
        {
            let mut source = CassetteEngine::open(&source_path)?;
            // We need a way to iterate all docs in source. The engine does not expose
            // a direct iterator, so we use dump() and parse it back.
            let dump = source.dump()?;
            let docs: Vec<Document> = serde_json::from_str(&dump).unwrap_or_default();
            for doc in docs {
                if self.shard_for(&doc.id).as_ref() == Some(target_shard) {
                    source.delete(&doc.id)?;
                    let target = if self.engines.contains_key(target_shard) {
                        self.engines.get_mut(target_shard).unwrap()
                    } else {
                        let engine = CassetteEngine::open(&target_path)?;
                        self.engines.insert(target_shard.clone(), engine);
                        self.engines.get_mut(target_shard).unwrap()
                    };
                    target.insert(doc)?;
                    moved += 1;
                }
            }
        }
        // Refresh local engine for source.
        if let Some(engine) = self.engines.get_mut(source_shard) {
            *engine = CassetteEngine::open(&source_path)?;
        }
        Ok(moved)
    }

    fn open_engines(&mut self) -> Result<()> {
        for shard_id in self.shard_map.shard_to_node.keys() {
            let path = self.shard_path(shard_id);
            let engine = CassetteEngine::open(&path)?;
            self.engines.insert(shard_id.clone(), engine);
        }
        Ok(())
    }

    fn shard_path(&self, shard_id: &ShardId) -> PathBuf {
        self.base_dir.join(format!("shard_{}.cassette", shard_id))
    }
}

/// Cluster-aware shard allocator that distributes shards across nodes.
pub struct ShardAllocator;

impl ShardAllocator {
    /// Allocate `num_shards` shards across `node_ids` in round-robin fashion.
    pub fn allocate(num_shards: usize, node_ids: &[String]) -> ShardMap {
        let mut shard_map = ShardMap::new(150);
        for i in 0..num_shards {
            let shard_id = format!("shard-{:04}", i);
            let node = if node_ids.is_empty() {
                String::new()
            } else {
                node_ids[i % node_ids.len()].clone()
            };
            shard_map.add_shard(shard_id, Some(node));
        }
        shard_map
    }

    /// Reassign shards from a failed node to healthy nodes.
    pub fn reassign_failed_node(shard_map: &mut ShardMap, failed_node: &str, healthy_nodes: &[String]) {
        if healthy_nodes.is_empty() {
            return;
        }
        let mut idx = 0usize;
        for node_id in shard_map.shard_to_node.values_mut() {
            if node_id == failed_node {
                *node_id = healthy_nodes[idx % healthy_nodes.len()].clone();
                idx += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn test_shard_allocation() {
        let nodes = vec!["a".to_string(), "b".to_string()];
        let map = ShardAllocator::allocate(4, &nodes);
        assert_eq!(map.shard_to_node.len(), 4);
        let nodes_assigned: Vec<_> = map.shard_to_node.values().collect();
        assert_eq!(nodes_assigned[0], "a");
        assert_eq!(nodes_assigned[1], "b");
        assert_eq!(nodes_assigned[2], "a");
        assert_eq!(nodes_assigned[3], "b");
    }

    #[test]
    fn test_router_consistent() {
        let dir = TempDir::new().unwrap();
        let router = ShardRouter::with_shards(
            vec!["s0".to_string(), "s1".to_string(), "s2".to_string()],
            dir.path(),
        )
        .unwrap();
        let shard1 = router.shard_for("doc-1");
        let shard2 = router.shard_for("doc-1");
        assert_eq!(shard1, shard2);
    }

    #[test]
    fn test_router_crud() {
        let dir = TempDir::new().unwrap();
        let mut router = ShardRouter::with_shards(
            vec!["s0".to_string(), "s1".to_string()],
            dir.path(),
        )
        .unwrap();

        let doc = Document::new(json!({"hello": "world"}));
        let id = router.insert(doc.clone()).unwrap();
        assert_eq!(router.doc_count(), 1);

        let got = router.get(&id).unwrap();
        assert_eq!(got.data, json!({"hello": "world"}));

        router.update(&id, json!({"hello": "sharded"})).unwrap();
        let got = router.get(&id).unwrap();
        assert_eq!(got.data["hello"], "sharded");

        router.delete(&id).unwrap();
        assert_eq!(router.doc_count(), 0);
    }

    #[test]
    fn test_reassign_failed_node() {
        let nodes = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut map = ShardAllocator::allocate(6, &nodes);
        let healthy = vec!["b".to_string(), "c".to_string()];
        ShardAllocator::reassign_failed_node(&mut map, "a", &healthy);
        for node_id in map.shard_to_node.values() {
            assert_ne!(node_id, "a");
        }
    }
}
