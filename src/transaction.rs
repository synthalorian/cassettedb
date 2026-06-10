use crate::db::Cassette;
use crate::wal::{Wal, WalEntry};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// A pending operation inside a transaction.
#[derive(Debug, Clone)]
pub enum TxOp {
    Insert { collection: String, doc: Value },
    Update { collection: String, id: String, doc: Value },
    Delete { collection: String, id: String },
}

/// An ACID transaction wrapper around a Cassette database.
pub struct Transaction<'a> {
    db: &'a mut Cassette,
    wal: &'a mut Wal,
    tx_id: u64,
    ops: Vec<TxOp>,
    /// Snapshot of the database state at transaction start, used for reads.
    snapshot: Cassette,
    committed: bool,
    rolled_back: bool,
}

impl<'a> Transaction<'a> {
    /// Begin a new transaction. This records a Begin entry in the WAL and
    /// captures an in-memory snapshot for consistent reads.
    pub fn begin(db: &'a mut Cassette, wal: &'a mut Wal, path: &Path) -> Result<Self> {
        let tx_id = wal.begin()?;
        let snapshot = db.clone();
        Ok(Self {
            db,
            wal,
            tx_id,
            ops: Vec::new(),
            snapshot,
            committed: false,
            rolled_back: false,
        })
    }

    /// Queue an insert operation. The document is not visible to other readers
    /// until the transaction commits.
    pub fn insert(&mut self, collection: &str, doc: Value) -> Result<String> {
        self.assert_active()?;
        let id = format!(
            "{}-{}-tx",
            chrono::Utc::now().timestamp_millis(),
            rand::random::<u16>()
        );
        let doc_with_id = if let Some(obj) = doc.as_object() {
            let mut obj = obj.clone();
            obj.insert("_tx_tmp_id".to_string(), Value::String(id.clone()));
            Value::Object(obj)
        } else {
            doc
        };
        self.ops.push(TxOp::Insert {
            collection: collection.to_string(),
            doc: doc_with_id,
        });
        Ok(id)
    }

    /// Queue an update operation.
    pub fn update(&mut self, collection: &str, id: &str, doc: Value) -> Result<bool> {
        self.assert_active()?;
        self.ops.push(TxOp::Update {
            collection: collection.to_string(),
            id: id.to_string(),
            doc,
        });
        Ok(true)
    }

    /// Queue a delete operation.
    pub fn delete(&mut self, collection: &str, id: &str) -> Result<bool> {
        self.assert_active()?;
        self.ops.push(TxOp::Delete {
            collection: collection.to_string(),
            id: id.to_string(),
        });
        Ok(true)
    }

    /// Read operations use the snapshot plus the transaction's own pending ops.
    pub fn query(&self, collection: &str, filter: &str) -> Result<Vec<crate::db::Document>> {
        self.assert_active()?;
        let mut docs = self.snapshot.query_owned(collection, filter)?;
        self.apply_pending_to_results(collection, &mut docs);
        Ok(docs)
    }

    pub fn scan(&self, collection: &str) -> Result<Vec<crate::db::Document>> {
        self.assert_active()?;
        let mut docs = self.snapshot.scan_owned(collection)?;
        self.apply_pending_to_results(collection, &mut docs);
        Ok(docs)
    }

    pub fn get(&self, collection: &str, id: &str) -> Option<crate::db::Document> {
        if !self.is_active() {
            return None;
        }
        // Check pending ops first (own writes)
        for op in self.ops.iter().rev() {
            match op {
                TxOp::Insert { collection: c, doc } if c == collection => {
                    if let Some(obj) = doc.as_object() {
                        if obj.get("_tx_tmp_id") == Some(&Value::String(id.to_string())) {
                            let mut d = doc.clone();
                            if let Some(obj) = d.as_object_mut() {
                                obj.remove("_tx_tmp_id");
                            }
                            return Some(crate::db::Document {
                                id: id.to_string(),
                                data: d,
                                created_at: chrono::Utc::now().to_rfc3339(),
                                updated_at: chrono::Utc::now().to_rfc3339(),
                                deleted: false,
                            });
                        }
                    }
                }
                TxOp::Update { collection: c, id: oid, doc } if c == collection && oid == id => {
                    return Some(crate::db::Document {
                        id: id.to_string(),
                        data: doc.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        updated_at: chrono::Utc::now().to_rfc3339(),
                        deleted: false,
                    });
                }
                TxOp::Delete { collection: c, id: did } if c == collection && did == id => {
                    return None;
                }
                _ => {}
            }
        }
        self.snapshot.get_owned(collection, id)
    }

    /// Commit all queued operations: write to WAL, apply to database, save, truncate WAL.
    pub fn commit(mut self, path: &Path) -> Result<()> {
        self.assert_active()?;

        // Write all ops to WAL
        for op in &self.ops {
            match op {
                TxOp::Insert { collection, doc } => {
                    self.wal.append(&WalEntry::Insert {
                        tx_id: self.tx_id,
                        collection: collection.clone(),
                        doc: doc.clone(),
                    })?;
                }
                TxOp::Update { collection, id, doc } => {
                    self.wal.append(&WalEntry::Update {
                        tx_id: self.tx_id,
                        collection: collection.clone(),
                        id: id.clone(),
                        doc: doc.clone(),
                    })?;
                }
                TxOp::Delete { collection, id } => {
                    self.wal.append(&WalEntry::Delete {
                        tx_id: self.tx_id,
                        collection: collection.clone(),
                        id: id.clone(),
                    })?;
                }
            }
        }

        // Write commit record
        self.wal.commit(self.tx_id)?;

        // Apply ops to the actual database
        for op in self.ops {
            match op {
                TxOp::Insert { collection, doc } => {
                    let mut doc = doc;
                    if let Some(obj) = doc.as_object_mut() {
                        obj.remove("_tx_tmp_id");
                    }
                    let _ = self.db.insert(&collection, doc);
                }
                TxOp::Update { collection, id, doc } => {
                    let _ = self.db.update(&collection, &id, doc);
                }
                TxOp::Delete { collection, id } => {
                    let _ = self.db.delete(&collection, &id);
                }
            }
        }

        // Save database atomically
        self.db.save(path)?;

        // Truncate WAL since state is now persisted
        self.wal.truncate()?;

        self.committed = true;
        Ok(())
    }

    /// Rollback the transaction: write abort record and discard ops.
    pub fn rollback(mut self) -> Result<()> {
        self.assert_active()?;
        self.wal.abort(self.tx_id)?;
        self.rolled_back = true;
        Ok(())
    }

    fn is_active(&self) -> bool {
        !self.committed && !self.rolled_back
    }

    fn assert_active(&self) -> Result<()> {
        if self.committed {
            return Err(anyhow::anyhow!("transaction already committed"));
        }
        if self.rolled_back {
            return Err(anyhow::anyhow!("transaction already rolled back"));
        }
        Ok(())
    }

    fn apply_pending_to_results(&self, collection: &str, docs: &mut Vec<crate::db::Document>) {
        // Remove docs that were deleted or updated in this transaction
        let mut deleted_ids = std::collections::HashSet::new();
        let mut updated: HashMap<String, Value> = HashMap::new();
        let mut inserted: Vec<Value> = Vec::new();

        for op in &self.ops {
            match op {
                TxOp::Delete { collection: c, id } if c == collection => {
                    deleted_ids.insert(id.clone());
                }
                TxOp::Update { collection: c, id, doc } if c == collection => {
                    updated.insert(id.clone(), doc.clone());
                    deleted_ids.remove(id);
                }
                TxOp::Insert { collection: c, doc } if c == collection => {
                    inserted.push(doc.clone());
                }
                _ => {}
            }
        }

        docs.retain(|d| !deleted_ids.contains(&d.id));
        for doc in docs.iter_mut() {
            if let Some(new_data) = updated.get(&doc.id) {
                doc.data = new_data.clone();
            }
        }

        for doc in inserted {
            let mut doc = doc;
            if let Some(obj) = doc.as_object_mut() {
                obj.remove("_tx_tmp_id");
            }
            let id = if let Some(obj) = doc.as_object() {
                obj.get("_tx_tmp_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            };
            docs.push(crate::db::Document {
                id,
                data: doc,
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
                deleted: false,
            });
        }
    }
}
