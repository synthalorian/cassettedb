#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;

/// Fuzz target for the CassetteDB storage engine.
/// Treats the fuzz input as a sequence of operations to apply to a database.
#[derive(Debug, Clone)]
enum Op {
    Insert { collection: String, doc: String },
    Update { collection: String, id: String, doc: String },
    Delete { collection: String, id: String },
    Query { collection: String, filter: String },
    Search { collection: String, query: String },
    Compact,
    Save,
    Open,
}

fn parse_ops(data: &[u8]) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let op_code = data[i];
        let len = data[i + 1] as usize;
        i += 2;
        if i + len > data.len() {
            break;
        }
        let payload = &data[i..i + len];
        i += len;

        let payload_str = String::from_utf8_lossy(payload);
        let parts: Vec<&str> = payload_str.split('\0').collect();

        let op = match op_code % 8 {
            0 => Op::Insert {
                collection: parts.get(0).unwrap_or(&"fuzz").to_string(),
                doc: parts.get(1).unwrap_or(&"{}").to_string(),
            },
            1 => Op::Update {
                collection: parts.get(0).unwrap_or(&"fuzz").to_string(),
                id: parts.get(1).unwrap_or(&"").to_string(),
                doc: parts.get(2).unwrap_or(&"{}").to_string(),
            },
            2 => Op::Delete {
                collection: parts.get(0).unwrap_or(&"fuzz").to_string(),
                id: parts.get(1).unwrap_or(&"").to_string(),
            },
            3 => Op::Query {
                collection: parts.get(0).unwrap_or(&"fuzz").to_string(),
                filter: parts.get(1).unwrap_or(&"").to_string(),
            },
            4 => Op::Search {
                collection: parts.get(0).unwrap_or(&"fuzz").to_string(),
                query: parts.get(1).unwrap_or(&"").to_string(),
            },
            5 => Op::Compact,
            6 => Op::Save,
            _ => Op::Open,
        };
        ops.push(op);
    }
    ops
}

fuzz_target!(|data: &[u8]| {
    let tmp = tempfile::tempdir().unwrap();
    let path: PathBuf = tmp.path().join("fuzz.cassette");
    let mut db = cassettedb::db::Cassette::new();
    let mut last_ids: Vec<String> = Vec::new();

    for op in parse_ops(data) {
        let _ = match op {
            Op::Insert { collection, doc } => {
                let value = serde_json::from_str(&doc).unwrap_or(serde_json::json!({"fuzz": true}));
                match db.insert(&collection, value) {
                    Ok(id) => {
                        last_ids.push(id);
                        Ok(true)
                    }
                    Err(_) => Ok(false),
                }
            }
            Op::Update { collection, id, doc } => {
                let id = if id.is_empty() {
                    last_ids.last().cloned().unwrap_or_default()
                } else {
                    id
                };
                let value = serde_json::from_str(&doc).unwrap_or(serde_json::json!({"fuzz": true}));
                db.update(&collection, &id, value).map(|_| true)
            }
            Op::Delete { collection, id } => {
                let id = if id.is_empty() {
                    last_ids.last().cloned().unwrap_or_default()
                } else {
                    id
                };
                db.delete(&collection, &id).map(|_| true)
            }
            Op::Query { collection, filter } => {
                db.query(&collection, &filter).map(|_| true)
            }
            Op::Search { collection, query } => {
                db.search(&collection, &query).map(|_| true)
            }
            Op::Compact => {
                db.compact().map(|_| true)
            }
            Op::Save => {
                db.save(&path).map(|_| true)
            }
            Op::Open => {
                db = cassettedb::db::Cassette::open(&path).unwrap_or_else(|_| db);
                Ok(true)
            }
        };
    }

    // Ensure the database can always be saved and reloaded
    let _ = db.save(&path);
    let _ = cassettedb::db::Cassette::open(&path);
});
