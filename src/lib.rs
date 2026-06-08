//! CassetteDB — A single-file JSON document database inspired by SQLite.
//!
//! # Design Goals
//! - Single `.cassette` file per database (portable, self-contained).
//! - ACID transactions via Write-Ahead Logging (WAL).
//! - JSONPath-like query language.
//! - Full-text search with a custom inverted index.
//! - Zero external server — embeddable library + CLI.

pub mod error;
pub mod wal;
pub mod storage;
pub mod index;
pub mod query;
pub mod engine;
pub mod document;
pub mod replication;
pub mod backup;
pub mod server;
pub mod raft;
pub mod cluster;
pub mod shard;
pub mod dist_tx;
pub mod crash_reporter;
pub mod config_migration;
pub mod feedback;

#[cfg(feature = "tantivy-search")]
pub mod search;

pub use error::{CassetteError, Result};
pub use engine::CassetteEngine;
pub use document::Document;
pub use query::{Query, QueryResult};
pub use replication::{ChangeFeed, ChangeRecord, Follower, ReplicationLog};
pub use backup::{create_snapshot, list_snapshots, restore_snapshot, delete_snapshot, SnapshotMeta};
pub use server::{AuthManager, ConnectionPool, HttpServer, MultiDbManager, TcpServer, run_tcp_server};
pub use raft::{create_raft_node, RaftNode, RaftRole, SharedRaftNode, LogEntry, ClusterCommand, NodeId, Term, LogIndex, PersistentState, RequestVoteRequest, RequestVoteResponse, AppendEntriesRequest, AppendEntriesResponse};
pub use cluster::{ClusterConfig, ClusterManager, ClusterNode, ClusterStatus, NodeInfo, NodeRole};
pub use shard::{ShardAllocator, ShardMap, ShardRouter, ShardId};
pub use dist_tx::{TwoPhaseCoordinator, DistributedTransaction, DistTxLog, TxOp, TxPhase, ParticipantVote, PrepareRequest, PrepareResponse, CommitRequest, AbortRequest, LocalParticipant};
pub use crash_reporter::{install_panic_hook, capture_crash_report};
pub use config_migration::{ConfigMigrator, VersionedConfig, CURRENT_CONFIG_VERSION};
pub use feedback::{submit_feedback, read_feedback, FeedbackCategory, FeedbackEntry};

#[cfg(feature = "tantivy-search")]
pub use search::{TantivySearch, SearchResult};
