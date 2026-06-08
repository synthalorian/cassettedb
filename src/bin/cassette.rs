//! CassetteDB CLI binary.
//!
//! Commands:
//!   init <file>                    Create a new empty database
//!   insert <file> <json>           Insert a JSON document
//!   query <file> <expr>            Run a query expression
//!   compact <file>                 Compact database + truncate WAL
//!   dump <file>                    Dump all documents as JSON
//!   delete <file> <id>             Delete a document by ID
//!   get <file> <id>                Get a single document by ID
//!   backup <file> <snapshot-dir>   Create a snapshot backup
//!   restore <snapshot-dir> <id> <file>  Restore from snapshot
//!   list-backups <snapshot-dir>    List available snapshots
//!   replicate <file> <repl-log>    Start replication log
//!   follow <repl-log>              Poll replication log for changes
//!   server                         Start server mode (TCP and/or HTTP)
//!   cluster init                   Initialize a new cluster
//!   cluster join                   Join an existing cluster
//!   cluster leave                  Leave the cluster
//!   cluster status                 Show cluster status
//!   cluster add-node               Add a node to the cluster
//!   cluster remove-node            Remove a node from the cluster
//!   cluster failover               Trigger failover to this node
//!   cluster shards                 Show shard allocation
//!   cluster rebalance              Rebalance shards
//!   dist-tx begin                  Begin a distributed transaction
//!   dist-tx commit                 Commit a distributed transaction
//!   dist-tx abort                  Abort a distributed transaction
//!   migrate-config <dir>           Migrate configuration files to latest version
//!   feedback <message>             Submit beta testing feedback

use anyhow::Result;
use cassettedb::document::Document;
use cassettedb::engine::CassetteEngine;
use cassettedb::query::Query;
use cassettedb::replication::Follower;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "cassette")]
#[command(about = "CassetteDB — single-file JSON document database")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new database file.
    Init { file: PathBuf },
    /// Insert a JSON document.
    Insert { file: PathBuf, json: String },
    /// Query documents.
    Query { file: PathBuf, expr: String },
    /// Compact the database.
    Compact { file: PathBuf },
    /// Dump all documents.
    Dump { file: PathBuf },
    /// Delete a document.
    Delete { file: PathBuf, id: String },
    /// Get a document.
    Get { file: PathBuf, id: String },
    /// Create a snapshot backup.
    Backup {
        file: PathBuf,
        #[arg(default_value = "./snapshots")]
        snapshot_dir: PathBuf,
    },
    /// Restore from a snapshot.
    Restore {
        snapshot_dir: PathBuf,
        snapshot_id: String,
        file: PathBuf,
    },
    /// List available snapshots.
    ListBackups {
        #[arg(default_value = "./snapshots")]
        snapshot_dir: PathBuf,
    },
    /// Delete a snapshot.
    DeleteBackup {
        snapshot_dir: PathBuf,
        snapshot_id: String,
    },
    /// Poll replication log for changes.
    Follow {
        repl_log: PathBuf,
        #[arg(default_value = "0")]
        since: u64,
    },
    /// Start server mode (TCP and/or HTTP).
    Server {
        /// TCP bind address (e.g., 127.0.0.1:6543).
        #[arg(long, default_value = "127.0.0.1:6543")]
        tcp_addr: String,
        /// HTTP bind address (e.g., 127.0.0.1:8080).
        #[arg(long, default_value = "127.0.0.1:8080")]
        http_addr: String,
        /// Directory to store databases.
        #[arg(long, default_value = "./databases")]
        db_dir: PathBuf,
        /// Authentication token (if not set, auth is disabled).
        #[arg(long)]
        auth_token: Option<String>,
        /// Connection pool size per database.
        #[arg(long, default_value = "10")]
        pool_size: usize,
        /// Run only TCP server.
        #[arg(long, group = "mode")]
        tcp_only: bool,
        /// Run only HTTP server.
        #[arg(long, group = "mode")]
        http_only: bool,
    },
    /// Cluster management commands.
    Cluster {
        #[command(subcommand)]
        command: ClusterCommands,
    },
    /// Distributed transaction commands.
    DistTx {
        #[command(subcommand)]
        command: DistTxCommands,
    },
    /// Migrate configuration files to the latest version.
    MigrateConfig {
        /// Directory containing configuration files.
        #[arg(default_value = "./cluster")]
        config_dir: PathBuf,
    },
    /// Submit beta testing feedback.
    Feedback {
        /// Feedback message.
        message: String,
        /// Feedback category.
        #[arg(long, default_value = "general")]
        category: String,
        /// Contact information (optional).
        #[arg(long)]
        contact: Option<String>,
        /// Path to feedback log file.
        #[arg(long, default_value = "./feedback.jsonl")]
        log_path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ClusterCommands {
    /// Initialize a new cluster.
    Init {
        /// Unique cluster identifier.
        #[arg(long)]
        cluster_id: String,
        /// Unique node identifier.
        #[arg(long)]
        node_id: String,
        /// Bind address for this node.
        #[arg(long, default_value = "127.0.0.1:7001")]
        address: String,
        /// Directory to store cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
    },
    /// Join an existing cluster.
    Join {
        /// Unique node identifier.
        #[arg(long)]
        node_id: String,
        /// Bind address for this node.
        #[arg(long, default_value = "127.0.0.1:7001")]
        address: String,
        /// Directory to store cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
        /// Path to a cluster configuration JSON file.
        #[arg(long)]
        cluster_config: PathBuf,
    },
    /// Leave the cluster (remove this node from the local config).
    Leave {
        /// Directory storing cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
    },
    /// Show cluster status.
    Status {
        /// Directory storing cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
    },
    /// Add a node to the cluster.
    AddNode {
        /// Directory storing cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
        /// Node ID to add.
        #[arg(long)]
        node_id: String,
        /// Node address.
        #[arg(long)]
        address: String,
        /// Node role.
        #[arg(long, default_value = "secondary")]
        role: String,
    },
    /// Remove a node from the cluster.
    RemoveNode {
        /// Directory storing cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
        /// Node ID to remove.
        #[arg(long)]
        node_id: String,
    },
    /// Trigger failover to this node.
    Failover {
        /// Directory storing cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
        /// Failed node ID.
        #[arg(long)]
        failed_node: String,
    },
    /// Show shard allocation.
    Shards {
        /// Directory storing cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
    },
    /// Rebalance shard assignments after a node change.
    Rebalance {
        /// Directory storing cluster configuration.
        #[arg(long, default_value = "./cluster")]
        config_dir: PathBuf,
        /// Number of shards.
        #[arg(long, default_value = "16")]
        num_shards: usize,
    },
}

#[derive(Subcommand)]
enum DistTxCommands {
    /// Begin a new distributed transaction.
    Begin {
        /// Transaction ID.
        #[arg(long)]
        tx_id: String,
        /// Comma-separated participant node IDs.
        #[arg(long)]
        participants: String,
    },
    /// Commit a distributed transaction.
    Commit {
        /// Transaction ID.
        #[arg(long)]
        tx_id: String,
    },
    /// Abort a distributed transaction.
    Abort {
        /// Transaction ID.
        #[arg(long)]
        tx_id: String,
    },
}

fn main() -> Result<()> {
    env_logger::init();
    cassettedb::install_panic_hook();
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { file } => {
            let _engine = CassetteEngine::open(&file)?;
            println!("Initialized {}", file.display());
        }
        Commands::Insert { file, json } => {
            let mut engine = CassetteEngine::open(&file)?;
            let data: serde_json::Value = serde_json::from_str(&json)?;
            let id = engine.insert(Document::new(data))?;
            println!("{}", id);
        }
        Commands::Query { file, expr } => {
            let engine = CassetteEngine::open(&file)?;
            let q = Query::parse(&expr)?;
            let res = engine.query(&q);
            println!("{}", serde_json::to_string_pretty(&res.documents)?);
        }
        Commands::Compact { file } => {
            let mut engine = CassetteEngine::open(&file)?;
            engine.compact()?;
            println!("Compacted {}", file.display());
        }
        Commands::Dump { file } => {
            let engine = CassetteEngine::open(&file)?;
            println!("{}", engine.dump()?);
        }
        Commands::Delete { file, id } => {
            let mut engine = CassetteEngine::open(&file)?;
            engine.delete(&id)?;
            println!("Deleted {}", id);
        }
        Commands::Get { file, id } => {
            let engine = CassetteEngine::open(&file)?;
            match engine.get(&id) {
                Some(doc) => println!("{}", serde_json::to_string_pretty(doc)?),
                None => println!("Not found"),
            }
        }
        Commands::Backup { file, snapshot_dir } => {
            let meta = cassettedb::backup::create_snapshot(&file, &snapshot_dir)?;
            println!("Created snapshot {}", meta.id);
            println!("  Size: {} bytes", meta.size_bytes);
            println!("  Created: {}", meta.created_at);
        }
        Commands::Restore {
            snapshot_dir,
            snapshot_id,
            file,
        } => {
            cassettedb::backup::restore_snapshot(&snapshot_dir, &snapshot_id, &file)?;
            println!("Restored {} to {}", snapshot_id, file.display());
        }
        Commands::ListBackups { snapshot_dir } => {
            let snapshots = cassettedb::backup::list_snapshots(&snapshot_dir)?;
            if snapshots.is_empty() {
                println!("No snapshots found");
            } else {
                for meta in snapshots {
                    println!(
                        "{}  {}  {}  {} bytes",
                        meta.id, meta.db_name, meta.created_at, meta.size_bytes
                    );
                }
            }
        }
        Commands::DeleteBackup {
            snapshot_dir,
            snapshot_id,
        } => {
            cassettedb::backup::delete_snapshot(&snapshot_dir, &snapshot_id)?;
            println!("Deleted snapshot {}", snapshot_id);
        }
        Commands::Follow { repl_log, since } => {
            let mut follower = Follower::new(&repl_log);
            // If since is provided, set the starting point.
            if since > 0 {
                follower = Follower::new(&repl_log);
                // We can't directly set last_sequence, so we'll read all and filter.
                // For CLI purposes, we'll just poll and show changes.
            }
            let changes = follower.poll()?;
            if changes.is_empty() {
                println!("No new changes");
            } else {
                for change in changes {
                    println!(
                        "seq={}  op={:?}  doc_id={}  ts={}",
                        change.sequence,
                        change.op,
                        change.doc_id,
                        change.timestamp
                    );
                }
            }
        }
        Commands::Cluster { command } => {
            match command {
                ClusterCommands::Init {
                    cluster_id,
                    node_id,
                    address,
                    config_dir,
                } => {
                    std::fs::create_dir_all(&config_dir)?;
                    let manager = cassettedb::cluster::ClusterManager::init(
                        cluster_id,
                        node_id,
                        address,
                        &config_dir,
                    )?;
                    let status = manager.status();
                    println!("Initialized cluster {}", status.cluster_id);
                    println!("Local node: {}", status.local_id);
                }
                ClusterCommands::Join {
                    node_id,
                    address,
                    config_dir,
                    cluster_config,
                } => {
                    std::fs::create_dir_all(&config_dir)?;
                    let bytes = std::fs::read(&cluster_config)?;
                    let config: cassettedb::cluster::ClusterConfig = serde_json::from_slice(&bytes)?;
                    let manager = cassettedb::cluster::ClusterManager::join(
                        node_id,
                        address,
                        &config_dir,
                        config,
                    )?;
                    let status = manager.status();
                    println!("Joined cluster {}", status.cluster_id);
                    println!("Local node: {}", status.local_id);
                }
                ClusterCommands::Leave { config_dir } => {
                    let path = config_dir.join("cluster.json");
                    if path.exists() {
                        std::fs::remove_file(&path)?;
                    }
                    println!("Left cluster (local config removed)");
                }
                ClusterCommands::Status { config_dir } => {
                    let manager = cassettedb::cluster::ClusterManager::load(
                        "unknown".to_string(),
                        &config_dir,
                    )?;
                    let status = manager.status();
                    println!("{}", serde_json::to_string_pretty(&status)?);
                }
                ClusterCommands::AddNode {
                    config_dir,
                    node_id,
                    address,
                    role,
                } => {
                    let manager =
                        cassettedb::cluster::ClusterManager::load("unknown".to_string(), &config_dir)?;
                    let node_role = match role.as_str() {
                        "primary" => cassettedb::cluster::NodeRole::Primary,
                        "observer" => cassettedb::cluster::NodeRole::Observer,
                        _ => cassettedb::cluster::NodeRole::Secondary,
                    };
                    manager.add_node(cassettedb::cluster::NodeInfo {
                        id: node_id.clone(),
                        address,
                        role: node_role,
                        last_seen: chrono::Utc::now().timestamp(),
                    })?;
                    println!("Added node {}", node_id);
                }
                ClusterCommands::RemoveNode { config_dir, node_id } => {
                    let manager =
                        cassettedb::cluster::ClusterManager::load("unknown".to_string(), &config_dir)?;
                    manager.remove_node(&node_id)?;
                    println!("Removed node {}", node_id);
                }
                ClusterCommands::Failover {
                    config_dir,
                    failed_node,
                } => {
                    let manager =
                        cassettedb::cluster::ClusterManager::load("unknown".to_string(), &config_dir)?;
                    manager.failover(&failed_node)?;
                    println!("Failover to local node triggered (failed node: {})", failed_node);
                }
                ClusterCommands::Shards { config_dir } => {
                    let manager =
                        cassettedb::cluster::ClusterManager::load("unknown".to_string(), &config_dir)?;
                    let cfg = manager.node().config();
                    let nodes: Vec<String> = cfg
                        .nodes
                        .iter()
                        .map(|n| n.id.clone())
                        .collect();
                    let map = cassettedb::shard::ShardAllocator::allocate(16, &nodes);
                    println!("{}", serde_json::to_string_pretty(&map)?);
                }
                ClusterCommands::Rebalance {
                    config_dir,
                    num_shards,
                } => {
                    let manager =
                        cassettedb::cluster::ClusterManager::load("unknown".to_string(), &config_dir)?;
                    let cfg = manager.node().config();
                    let nodes: Vec<String> = cfg
                        .nodes
                        .iter()
                        .map(|n| n.id.clone())
                        .collect();
                    let map = cassettedb::shard::ShardAllocator::allocate(num_shards, &nodes);
                    println!("Rebalanced shard map:");
                    println!("{}", serde_json::to_string_pretty(&map)?);
                }
            }
        }
        Commands::DistTx { command } => {
            match command {
                DistTxCommands::Begin { tx_id, participants } => {
                    let parts: Vec<String> = participants
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let coord =
                        cassettedb::dist_tx::TwoPhaseCoordinator::new("cli".to_string());
                    let tx = coord.begin(tx_id.clone(), parts);
                    println!("Started transaction {}", tx.tx_id);
                    println!("Phase: {:?}", tx.phase);
                }
                DistTxCommands::Commit { tx_id } => {
                    let coord =
                        cassettedb::dist_tx::TwoPhaseCoordinator::new("cli".to_string());
                    coord.commit(&tx_id)?;
                    println!("Committed transaction {}", tx_id);
                }
                DistTxCommands::Abort { tx_id } => {
                    let coord =
                        cassettedb::dist_tx::TwoPhaseCoordinator::new("cli".to_string());
                    coord.abort(&tx_id)?;
                    println!("Aborted transaction {}", tx_id);
                }
            }
        }
        Commands::MigrateConfig { config_dir } => {
            let migrator = cassettedb::ConfigMigrator::new();
            let migrated = migrator.migrate_directory(&config_dir)?;
            if migrated.is_empty() {
                println!("No configuration files needed migration in {}", config_dir.display());
            } else {
                println!("Migrated {} configuration file(s):", migrated.len());
                for path in migrated {
                    println!("  {}", path.display());
                }
            }
        }
        Commands::Feedback {
            message,
            category,
            contact,
            log_path,
        } => {
            let category = match category.as_str() {
                "bug" => cassettedb::feedback::FeedbackCategory::Bug,
                "feature" => cassettedb::feedback::FeedbackCategory::FeatureRequest,
                "performance" => cassettedb::feedback::FeedbackCategory::Performance,
                "docs" => cassettedb::feedback::FeedbackCategory::Documentation,
                _ => cassettedb::feedback::FeedbackCategory::General,
            };
            cassettedb::feedback::submit_feedback(
                &log_path,
                category,
                message,
                env!("CARGO_PKG_VERSION").to_string(),
                contact,
            )?;
            println!("Feedback recorded to {}", log_path.display());
        }
        Commands::Server {
            tcp_addr,
            http_addr,
            db_dir,
            auth_token,
            pool_size,
            tcp_only,
            http_only,
        } => {
            std::fs::create_dir_all(&db_dir)?;
            let pool = Arc::new(cassettedb::ConnectionPool::new(&db_dir, pool_size)?);
            let auth = Arc::new(cassettedb::AuthManager::new(auth_token));

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                if !http_only {
                    let tcp_pool = pool.clone();
                    let tcp_auth = auth.clone();
                    let tcp_addr_clone = tcp_addr.clone();
                    tokio::spawn(async move {
                        if let Err(e) = cassettedb::run_tcp_server(tcp_pool, tcp_auth, &tcp_addr_clone).await {
                            eprintln!("TCP server error: {}", e);
                        }
                    });
                }

                if !tcp_only {
                    let http_pool = pool.clone();
                    let http_auth = auth.clone();
                    tokio::spawn(async move {
                        let http_server = cassettedb::HttpServer::new(http_pool, http_auth);
                        if let Err(e) = http_server.run(&http_addr).await {
                            eprintln!("HTTP server error: {}", e);
                        }
                    });
                }

                println!("Server running. Press Ctrl+C to stop.");
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                }
            });
        }
    }

    Ok(())
}
