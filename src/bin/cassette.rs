//! CassetteDB CLI binary.
//!
//! Commands:
//!   init <file>          Create a new empty database
//!   insert <file> <json> Insert a JSON document
//!   query <file> <expr>  Run a query expression
//!   compact <file>       Compact database + truncate WAL
//!   dump <file>          Dump all documents as JSON
//!   delete <file> <id>   Delete a document by ID
//!   get <file> <id>      Get a single document by ID

use anyhow::Result;
use cassettedb::document::Document;
use cassettedb::engine::CassetteEngine;
use cassettedb::query::Query;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    }

    Ok(())
}
