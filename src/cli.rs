use crate::db::Cassette;
use crate::sync;
use anyhow::Result;
use std::path::Path;

pub fn init(path: &Path) -> Result<()> {
    Cassette::init(path)?;
    println!("Cassette initialized at {}", path.display());
    Ok(())
}

pub fn insert(path: &Path, collection: &str, doc: &str) -> Result<()> {
    let mut cassette = Cassette::open(path)?;
    let value: serde_json::Value = serde_json::from_str(doc)?;
    let id = cassette.insert(collection, value)?;
    cassette.save(path)?;
    println!("Inserted document with id: {}", id);
    Ok(())
}

pub fn query(path: &Path, collection: &str, query: &str) -> Result<()> {
    let cassette = Cassette::open(path)?;
    let results = cassette.query(collection, query)?;
    if results.is_empty() {
        println!("No documents found.");
    } else {
        for doc in results {
            println!("{}", serde_json::to_string_pretty(&doc)?);
        }
    }
    Ok(())
}

pub fn query_jsonpath(path: &Path, collection: &str, path_expr: &str) -> Result<()> {
    let cassette = Cassette::open(path)?;
    let results = cassette.query_jsonpath(collection, path_expr)?;
    if results.is_empty() {
        println!("No documents found.");
    } else {
        for doc in results {
            println!("{}", serde_json::to_string_pretty(&doc)?);
        }
    }
    Ok(())
}

pub fn search(path: &Path, collection: &str, query: &str) -> Result<()> {
    let cassette = Cassette::open(path)?;
    let results = cassette.search(collection, query)?;
    if results.is_empty() {
        println!("No documents found.");
    } else {
        println!("Found {} document(s):", results.len());
        for doc in results {
            println!("{}", serde_json::to_string_pretty(&doc)?);
        }
    }
    Ok(())
}

pub fn collections(path: &Path) -> Result<()> {
    let cassette = Cassette::open(path)?;
    let cols = cassette.collections();
    if cols.is_empty() {
        println!("No collections.");
    } else {
        for c in cols {
            println!("{}", c);
        }
    }
    Ok(())
}

pub fn compact(path: &Path) -> Result<()> {
    let mut cassette = Cassette::open(path)?;
    let removed = cassette.compact()?;
    cassette.save(path)?;
    println!("Compacted: removed {} deleted documents", removed);
    Ok(())
}

pub fn dump(path: &Path) -> Result<()> {
    let cassette = Cassette::open(path)?;
    println!("{}", serde_json::to_string_pretty(&cassette)?);
    Ok(())
}

pub fn sync(local_path: &Path, remote_path: &Path) -> Result<()> {
    let mut local = Cassette::open(local_path)?;
    let remote = Cassette::open(remote_path)?;

    let result = sync::sync_into(&mut local, &remote)?;
    local.save(local_path)?;

    println!("sync complete:");
    println!("  added:      {}", result.summary.added);
    println!("  updated:    {}", result.summary.updated);
    println!("  deleted:    {}", result.summary.deleted);
    println!("  conflicts:  {}", result.summary.conflicts);
    println!("  unchanged:  {}", result.summary.unchanged);

    if !result.conflicts.is_empty() {
        println!("\nconflicts detected (same document modified independently):");
        for c in &result.conflicts {
            println!("  collection: {}, doc_id: {}", c.collection, c.doc_id);
        }
    }

    Ok(())
}

pub fn repl(path: &Path) -> Result<()> {
    crate::repl::run(path)
}

pub fn scan(path: &Path, collection: &str) -> Result<()> {
    let cassette = Cassette::open(path)?;
    let results = cassette.scan(collection)?;
    if results.is_empty() {
        println!("No documents found.");
    } else {
        println!("Found {} document(s):", results.len());
        for doc in results {
            println!("{}", serde_json::to_string_pretty(&doc)?);
        }
    }
    Ok(())
}

#[cfg(feature = "replication")]
pub fn replicate_leader(_path: &Path, bind_addr: &str) -> Result<()> {
    use crate::replication::ReplicationLeader;
    use std::sync::Arc;

    let rt = tokio::runtime::Runtime::new()?;
    let leader = Arc::new(ReplicationLeader::new());
    let leader_clone = Arc::clone(&leader);

    println!("Starting replication leader on {}", bind_addr);
    rt.block_on(async move {
        leader_clone.run(bind_addr).await
    })
}

#[cfg(feature = "replication")]
pub fn replicate_follower(path: &Path, leader_addr: &str) -> Result<()> {
    use crate::replication::ReplicationFollower;

    let rt = tokio::runtime::Runtime::new()?;
    println!("Starting replication follower, connecting to {}", leader_addr);
    rt.block_on(async move {
        ReplicationFollower::run(leader_addr, path).await
    })
}

pub fn backup(path: &Path, out_path: &Path) -> Result<()> {
    let cassette = Cassette::open(path)?;
    let count = crate::backup::backup(&cassette, out_path)?;
    println!("Backup complete: {} documents written to {}", count, out_path.display());
    Ok(())
}

pub fn restore(path: &Path, in_path: &Path) -> Result<()> {
    let mut cassette = Cassette::open(path)?;
    let count = crate::backup::restore(&mut cassette, in_path)?;
    cassette.save(path)?;
    println!("Restore complete: {} documents restored from {}", count, in_path.display());
    Ok(())
}

pub fn create_index(path: &Path, collection: &str, field: &str) -> Result<()> {
    let mut cassette = Cassette::open(path)?;
    cassette.create_index(collection, field)?;
    cassette.save(path)?;
    println!("Created secondary index on {}.{}", collection, field);
    Ok(())
}

pub fn drop_index(path: &Path, collection: &str, field: &str) -> Result<()> {
    let mut cassette = Cassette::open(path)?;
    if cassette.drop_index(collection, field)? {
        cassette.save(path)?;
        println!("Dropped secondary index on {}.{}", collection, field);
    } else {
        println!("Index {}.{} does not exist", collection, field);
    }
    Ok(())
}

pub fn list_indexes(path: &Path, collection: &str) -> Result<()> {
    let cassette = Cassette::open(path)?;
    let indexes = cassette.list_indexes(collection);
    if indexes.is_empty() {
        println!("No secondary indexes on collection '{}'", collection);
    } else {
        println!("Secondary indexes on '{}':", collection);
        for idx in indexes {
            println!("  - {}", idx);
        }
    }
    Ok(())
}

pub fn query_range(path: &Path, collection: &str, field: &str, op: &str, value: &str) -> Result<()> {
    let cassette = Cassette::open(path)?;
    let results = cassette.query_range(collection, field, op, value)?;
    if results.is_empty() {
        println!("No documents found.");
    } else {
        println!("Found {} document(s):", results.len());
        for doc in results {
            println!("{}", serde_json::to_string_pretty(&doc)?);
        }
    }
    Ok(())
}

#[cfg(feature = "metrics")]
pub fn start_metrics(bind_addr: &str) -> Result<()> {
    crate::metrics::init()?;
    crate::metrics::start_server(bind_addr)?;
    println!("Metrics server running. Press Ctrl+C to stop.");
    loop {
        std::thread::park();
    }
}
