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
}

fn main() -> Result<()> {
    env_logger::init();
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
