//! Distributed transaction coordinator using two-phase commit (2PC).
//!
//! This module provides a lightweight coordinator that can drive
//! transactions across multiple CassetteDB nodes (or local shards).
//! It is intentionally simple and targeted at the small cluster sizes
//! CassetteDB is designed for.

use crate::raft::NodeId;
use crate::document::Document;
use crate::error::{CassetteError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Phase of a distributed transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxPhase {
    /// Coordinator is collecting votes.
    Voting,
    /// All participants voted yes; ready to commit.
    Prepared,
    /// Transaction has been committed.
    Committed,
    /// Transaction has been aborted.
    Aborted,
}

/// Vote from a participant during the prepare phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticipantVote {
    Yes,
    No,
}

/// Operation type within a distributed transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TxOp {
    Insert { doc: Document, shard: String },
    Update { id: String, data: serde_json::Value, shard: String },
    Delete { id: String, shard: String },
}

/// A participant in a distributed transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Participant {
    pub node_id: NodeId,
    pub vote: Option<ParticipantVote>,
}

/// Distributed transaction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedTransaction {
    pub tx_id: String,
    pub coordinator: NodeId,
    pub participants: Vec<Participant>,
    pub operations: Vec<TxOp>,
    pub phase: TxPhase,
}

impl DistributedTransaction {
    /// Create a new distributed transaction.
    pub fn new(tx_id: impl Into<String>, coordinator: NodeId) -> Self {
        Self {
            tx_id: tx_id.into(),
            coordinator,
            participants: Vec::new(),
            operations: Vec::new(),
            phase: TxPhase::Voting,
        }
    }

    /// Register a participant.
    pub fn add_participant(&mut self, node_id: NodeId) {
        if !self.participants.iter().any(|p| p.node_id == node_id) {
            self.participants.push(Participant {
                node_id,
                vote: None,
            });
        }
    }

    /// Add an operation.
    pub fn add_operation(&mut self, op: TxOp) {
        self.operations.push(op);
    }

    /// Record a participant vote.
    pub fn record_vote(&mut self, node_id: &NodeId, vote: ParticipantVote) -> Result<()> {
        let participant = self
            .participants
            .iter_mut()
            .find(|p| p.node_id == *node_id)
            .ok_or_else(|| CassetteError::DistTx("unknown participant".to_string()))?;
        participant.vote = Some(vote);
        Ok(())
    }

    /// Check whether all participants have voted.
    pub fn all_votes_received(&self) -> bool {
        self.participants.iter().all(|p| p.vote.is_some())
    }

    /// Determine the outcome after all votes are received.
    pub fn outcome(&self) -> Option<TxPhase> {
        if !self.all_votes_received() {
            return None;
        }
        let all_yes = self
            .participants
            .iter()
            .all(|p| p.vote == Some(ParticipantVote::Yes));
        if all_yes {
            Some(TxPhase::Prepared)
        } else {
            Some(TxPhase::Aborted)
        }
    }

    /// Advance to committed phase.
    pub fn commit(&mut self) -> Result<()> {
        if self.phase != TxPhase::Prepared {
            return Err(CassetteError::DistTx(
                "transaction is not prepared".to_string(),
            ));
        }
        self.phase = TxPhase::Committed;
        Ok(())
    }

    /// Abort the transaction.
    pub fn abort(&mut self) -> Result<()> {
        if self.phase == TxPhase::Committed {
            return Err(CassetteError::DistTx(
                "cannot abort a committed transaction".to_string(),
            ));
        }
        self.phase = TxPhase::Aborted;
        Ok(())
    }

    /// Participant node IDs.
    pub fn participant_ids(&self) -> Vec<NodeId> {
        self.participants.iter().map(|p| p.node_id.clone()).collect()
    }
}

/// Participant-side prepare request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareRequest {
    pub tx_id: String,
    pub coordinator: NodeId,
    pub operations: Vec<TxOp>,
}

/// Participant-side prepare response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareResponse {
    pub tx_id: String,
    pub node_id: NodeId,
    pub vote: ParticipantVote,
}

/// Commit request sent by the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRequest {
    pub tx_id: String,
}

/// Abort request sent by the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortRequest {
    pub tx_id: String,
}

/// In-memory transaction log used by the coordinator.
pub struct DistTxLog {
    transactions: Mutex<HashMap<String, DistributedTransaction>>,
}

impl DistTxLog {
    /// Create an empty transaction log.
    pub fn new() -> Self {
        Self {
            transactions: Mutex::new(HashMap::new()),
        }
    }

    /// Register or replace a transaction.
    pub fn register(&self, tx: DistributedTransaction) {
        self.transactions.lock().unwrap().insert(tx.tx_id.clone(), tx);
    }

    /// Get a transaction by ID.
    pub fn get(&self, tx_id: &str) -> Option<DistributedTransaction> {
        self.transactions.lock().unwrap().get(tx_id).cloned()
    }

    /// Update a transaction.
    pub fn update(&self, tx: DistributedTransaction) -> Result<()> {
        let mut map = self.transactions.lock().unwrap();
        if !map.contains_key(&tx.tx_id) {
            return Err(CassetteError::DistTx("transaction not found".to_string()));
        }
        map.insert(tx.tx_id.clone(), tx);
        Ok(())
    }

    /// List active (non-finalized) transactions.
    pub fn active(&self) -> Vec<DistributedTransaction> {
        self.transactions
            .lock()
            .unwrap()
            .values()
            .filter(|tx| tx.phase != TxPhase::Committed && tx.phase != TxPhase::Aborted)
            .cloned()
            .collect()
    }
}

impl Default for DistTxLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Two-phase commit coordinator.
pub struct TwoPhaseCoordinator {
    node_id: NodeId,
    log: Arc<DistTxLog>,
}

impl TwoPhaseCoordinator {
    /// Create a new coordinator.
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            log: Arc::new(DistTxLog::new()),
        }
    }

    /// Access the transaction log.
    pub fn log(&self) -> Arc<DistTxLog> {
        self.log.clone()
    }

    /// Begin a new distributed transaction.
    pub fn begin(&self, tx_id: impl Into<String>, participants: Vec<NodeId>) -> DistributedTransaction {
        let mut tx = DistributedTransaction::new(tx_id, self.node_id.clone());
        for p in participants {
            tx.add_participant(p);
        }
        self.log.register(tx.clone());
        tx
    }

    /// Add an operation to an existing transaction.
    pub fn add_operation(&self, tx_id: &str, op: TxOp) -> Result<()> {
        let mut tx = self
            .log
            .get(tx_id)
            .ok_or_else(|| CassetteError::DistTx("transaction not found".to_string()))?;
        tx.add_operation(op);
        self.log.update(tx)
    }

    /// Prepare phase: send prepare requests to all participants and collect votes.
    /// In a networked system, this would be performed over RPC; here we simulate
    /// by trusting the caller to provide responses.
    pub fn prepare(&self, tx_id: &str, responses: Vec<PrepareResponse>) -> Result<TxPhase> {
        let mut tx = self
            .log
            .get(tx_id)
            .ok_or_else(|| CassetteError::DistTx("transaction not found".to_string()))?;

        if tx.phase != TxPhase::Voting {
            return Err(CassetteError::DistTx(
                "transaction is not in voting phase".to_string(),
            ));
        }

        for resp in responses {
            tx.record_vote(&resp.node_id, resp.vote)?;
        }

        let outcome = tx
            .outcome()
            .ok_or_else(|| CassetteError::DistTx("votes not complete".to_string()))?;

        tx.phase = outcome;
        self.log.update(tx.clone())?;
        Ok(outcome)
    }

    /// Commit phase: finalize a prepared transaction.
    pub fn commit(&self, tx_id: &str) -> Result<()> {
        let mut tx = self
            .log
            .get(tx_id)
            .ok_or_else(|| CassetteError::DistTx("transaction not found".to_string()))?;
        tx.commit()?;
        self.log.update(tx)
    }

    /// Abort a transaction.
    pub fn abort(&self, tx_id: &str) -> Result<()> {
        let mut tx = self
            .log
            .get(tx_id)
            .ok_or_else(|| CassetteError::DistTx("transaction not found".to_string()))?;
        tx.abort()?;
        self.log.update(tx)
    }

    /// Recover a transaction after coordinator failure.
    pub fn recover(&self, tx_id: &str) -> Option<TxPhase> {
        self.log.get(tx_id).map(|tx| tx.phase)
    }
}

/// Simple local participant implementation that operates on a shard router.
pub struct LocalParticipant<'a> {
    node_id: NodeId,
    router: &'a mut crate::shard::ShardRouter,
}

impl<'a> LocalParticipant<'a> {
    /// Create a local participant wrapping a shard router.
    pub fn new(node_id: NodeId, router: &'a mut crate::shard::ShardRouter) -> Self {
        Self { node_id, router }
    }

    /// Prepare a transaction by attempting to apply operations locally.
    /// If any operation fails, the participant votes No.
    pub fn prepare(&mut self, req: &PrepareRequest) -> PrepareResponse {
        let vote = if self.apply_ops(&req.operations).is_ok() {
            ParticipantVote::Yes
        } else {
            ParticipantVote::No
        };
        PrepareResponse {
            tx_id: req.tx_id.clone(),
            node_id: self.node_id.clone(),
            vote,
        }
    }

    fn apply_ops(&mut self, ops: &[TxOp]) -> Result<()> {
        for op in ops {
            match op {
                TxOp::Insert { doc, shard } => {
                    let engine = self
                        .router
                        .engine_mut(shard)
                        .ok_or_else(|| CassetteError::Shard("shard not open".to_string()))?;
                    engine.insert(doc.clone())?;
                }
                TxOp::Update { id, data, shard } => {
                    let engine = self
                        .router
                        .engine_mut(shard)
                        .ok_or_else(|| CassetteError::Shard("shard not open".to_string()))?;
                    engine.update(id, data.clone())?;
                }
                TxOp::Delete { id, shard } => {
                    let engine = self
                        .router
                        .engine_mut(shard)
                        .ok_or_else(|| CassetteError::Shard("shard not open".to_string()))?;
                    engine.delete(id)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn test_dist_tx_lifecycle() {
        let coord = TwoPhaseCoordinator::new("coord".to_string());
        let tx = coord.begin("tx-1", vec!["p1".to_string(), "p2".to_string()]);
        assert_eq!(tx.phase, TxPhase::Voting);

        coord
            .add_operation("tx-1", TxOp::Insert {
                doc: Document::new(json!({"x": 1})),
                shard: "s0".to_string(),
            })
            .unwrap();

        let responses = vec![
            PrepareResponse {
                tx_id: "tx-1".to_string(),
                node_id: "p1".to_string(),
                vote: ParticipantVote::Yes,
            },
            PrepareResponse {
                tx_id: "tx-1".to_string(),
                node_id: "p2".to_string(),
                vote: ParticipantVote::Yes,
            },
        ];

        let phase = coord.prepare("tx-1", responses).unwrap();
        assert_eq!(phase, TxPhase::Prepared);

        coord.commit("tx-1").unwrap();
        assert_eq!(coord.recover("tx-1"), Some(TxPhase::Committed));
    }

    #[test]
    fn test_dist_tx_abort_on_no_vote() {
        let coord = TwoPhaseCoordinator::new("coord".to_string());
        coord.begin("tx-2", vec!["p1".to_string()]);
        let responses = vec![PrepareResponse {
            tx_id: "tx-2".to_string(),
            node_id: "p1".to_string(),
            vote: ParticipantVote::No,
        }];
        let phase = coord.prepare("tx-2", responses).unwrap();
        assert_eq!(phase, TxPhase::Aborted);
    }

    #[test]
    fn test_local_participant() {
        let dir = TempDir::new().unwrap();
        let mut router = crate::shard::ShardRouter::with_shards(
            vec!["s0".to_string()],
            dir.path(),
        )
        .unwrap();
        let mut participant = LocalParticipant::new("node-1".to_string(), &mut router);
        let req = PrepareRequest {
            tx_id: "tx-3".to_string(),
            coordinator: "coord".to_string(),
            operations: vec![TxOp::Insert {
                doc: Document::new(json!({"k": "v"})),
                shard: "s0".to_string(),
            }],
        };
        let resp = participant.prepare(&req);
        assert_eq!(resp.vote, ParticipantVote::Yes);
        assert_eq!(router.doc_count(), 1);
    }
}
