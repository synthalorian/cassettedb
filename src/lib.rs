//! CassetteDB — A single-file JSON document database inspired by SQLite.
//!
//! # Design Goals
//! - Single `.cassette` file per database (portable, self-contained).
//! - ACID transactions via Write-Ahead Logging (WAL).
//! - JSONPath-like query language.
//! - Full-text search with a custom inverted index.
//! - Zero external server — embeddable library + CLI.

pub mod backup;
pub mod cluster;
pub mod config_migration;
pub mod crash_reporter;
pub mod db;
pub mod dist_tx;
pub mod document;
pub mod engine;
pub mod error;
pub mod feedback;
pub mod index;
pub mod query;
pub mod raft;
pub mod replication;
pub mod server;
pub mod shard;
pub mod storage;
pub mod wal;

#[cfg(feature = "tantivy-search")]
pub mod search;

pub use backup::{
    create_snapshot, delete_snapshot, list_snapshots, restore_snapshot, SnapshotMeta,
};
pub use cluster::{ClusterConfig, ClusterManager, ClusterNode, ClusterStatus, NodeInfo, NodeRole};
pub use config_migration::{ConfigMigrator, VersionedConfig, CURRENT_CONFIG_VERSION};
pub use crash_reporter::{capture_crash_report, install_panic_hook};
pub use dist_tx::{
    AbortRequest, CommitRequest, DistTxLog, DistributedTransaction, LocalParticipant,
    ParticipantVote, PrepareRequest, PrepareResponse, TwoPhaseCoordinator, TxOp, TxPhase,
};
pub use document::Document;
pub use engine::CassetteEngine;
pub use error::{CassetteError, Result};
pub use feedback::{read_feedback, submit_feedback, FeedbackCategory, FeedbackEntry};
pub use query::{Query, QueryResult};
pub use raft::{
    create_raft_node, AppendEntriesRequest, AppendEntriesResponse, ClusterCommand, LogEntry,
    LogIndex, NodeId, PersistentState, RaftNode, RaftRole, RequestVoteRequest, RequestVoteResponse,
    SharedRaftNode, Term,
};
pub use replication::{ChangeFeed, ChangeRecord, Follower, ReplicationLog};
pub use server::{
    run_tcp_server, AuthManager, ConnectionPool, HttpServer, MultiDbManager, TcpServer,
};
pub use shard::{ShardAllocator, ShardId, ShardMap, ShardRouter};

#[cfg(feature = "tantivy-search")]
pub use search::{SearchResult, TantivySearch};
