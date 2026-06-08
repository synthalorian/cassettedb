//! High-level engine: ties together WAL, storage, index, and queries.
//!
//! Documents are stored as JSON blobs in the main `.cassette` file.
//! The WAL (`*.wal`) guarantees durability and atomicity.
//! The inverted index lives in memory and is rebuilt from the main file
//! on open, then kept in sync with mutations.

use crate::document::Document;
use crate::error::Result;
use crate::index::InvertedIndex;
use crate::query::{Query, QueryResult};
use crate::storage::Storage;
use crate::wal::{Wal, WalOp};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// CassetteDB engine.
pub struct CassetteEngine {
    db_path: PathBuf,
    wal: Wal,
    storage: Storage,
    docs: HashMap<String, Document>,
    index: InvertedIndex,
}

impl CassetteEngine {
    /// Open (or create) a database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let wal_path = path.with_extension("wal");
        let mut wal = Wal::open(&wal_path)?;
        let mut storage = Storage::open(path)?;

        let mut docs: HashMap<String, Document> = HashMap::new();
        let mut index = InvertedIndex::new();

        // Recover committed documents from WAL.
        if let Ok(iter) = wal.iter_committed() {
            for record in iter {
                let record = record?;
                match record.op {
                    WalOp::Insert | WalOp::Update => {
                        let doc: Document = serde_json::from_slice(&record.payload)?;
                        // Re-index: remove old, add new.
                        if let Some(old) = docs.get(&doc.id) {
                            index.remove_document(&doc.id, &old.data);
                        }
                        index.index_document(&doc.id, &doc.data);
                        docs.insert(doc.id.clone(), doc);
                    }
                    WalOp::Delete => {
                        if let Some(old) = docs.remove(&record.doc_id) {
                            index.remove_document(&record.doc_id, &old.data);
                        }
                    }
                }
            }
        }

        // If WAL was empty, try to load from main storage pages.
        if docs.is_empty() && storage.header().num_pages > 1 {
            let trailer_page = storage.read_page(storage.header().num_pages - 1).ok();
            if let Some(tp) = trailer_page {
                let len =
                    usize::from_le_bytes([tp[0], tp[1], tp[2], tp[3], tp[4], tp[5], tp[6], tp[7]]);
                if len > 0 {
                    let pages_needed =
                        (len + crate::storage::PAGE_SIZE - 1) / crate::storage::PAGE_SIZE;
                    let mut payload = Vec::with_capacity(len);
                    for i in 1..=pages_needed {
                        if let Ok(page) = storage.read_page(i as u32) {
                            payload.extend_from_slice(&page);
                        }
                    }
                    payload.truncate(len);
                    if let Ok(loaded) =
                        serde_json::from_slice::<HashMap<String, Document>>(&payload)
                    {
                        for (id, doc) in loaded {
                            index.index_document(&id, &doc.data);
                            docs.insert(id, doc);
                        }
                    }
                }
            }
        }

        // If we recovered from WAL, sync to main storage (naïve: rewrite all).
        // For the scaffold we just keep docs in memory and write on compact.
        Ok(CassetteEngine {
            db_path: path.to_path_buf(),
            wal,
            storage,
            docs,
            index,
        })
    }

    /// Insert a new document. Returns the assigned ID.
    pub fn insert(&mut self, mut doc: Document) -> Result<String> {
        if doc.id.is_empty() {
            doc = Document::new(doc.data);
        }
        let payload = serde_json::to_vec(&doc)?;
        let offset = self.wal.append(WalOp::Insert, &doc.id, &payload)?;
        self.wal.commit_record(offset)?;

        self.index.index_document(&doc.id, &doc.data);
        self.docs.insert(doc.id.clone(), doc);
        self.storage.increment_doc_count(1)?;
        Ok(self.docs.keys().last().unwrap().clone())
    }

    /// Update an existing document by ID.
    pub fn update(&mut self, id: &str, data: serde_json::Value) -> Result<()> {
        let old = self
            .docs
            .get(id)
            .cloned()
            .ok_or_else(|| crate::error::CassetteError::NotFound(id.to_string()))?;
        let mut doc = old.clone();
        doc.data = data;
        let payload = serde_json::to_vec(&doc)?;
        let offset = self.wal.append(WalOp::Update, id, &payload)?;
        self.wal.commit_record(offset)?;

        self.index.remove_document(id, &old.data);
        self.index.index_document(id, &doc.data);
        self.docs.insert(id.to_string(), doc);
        Ok(())
    }

    /// Delete a document by ID.
    pub fn delete(&mut self, id: &str) -> Result<()> {
        let old = self
            .docs
            .remove(id)
            .ok_or_else(|| crate::error::CassetteError::NotFound(id.to_string()))?;
        let offset = self.wal.append(WalOp::Delete, id, b"")?;
        self.wal.commit_record(offset)?;

        self.index.remove_document(id, &old.data);
        self.storage.increment_doc_count(-1)?;
        Ok(())
    }

    /// Get a single document by ID.
    pub fn get(&self, id: &str) -> Option<&Document> {
        self.docs.get(id)
    }

    /// Execute a query.
    pub fn query(&self, q: &Query) -> QueryResult {
        let all: Vec<Document> = self.docs.values().cloned().collect();
        q.execute(&all, &self.index)
    }

    /// Full-text search shorthand.
    pub fn search(&self, term: &str) -> Vec<Document> {
        let ids = self.index.search(term);
        ids.into_iter()
            .filter_map(|id| self.docs.get(&id).cloned())
            .collect()
    }

    /// Compact the database: rewrite main file with current documents,
    /// then truncate the WAL.
    pub fn compact(&mut self) -> Result<()> {
        // For the scaffold we simply serialize the document map to a single
        // JSON blob and store it in page 1..N of the main file.
        let payload = serde_json::to_vec(&self.docs)?;
        let pages_needed =
            (payload.len() + crate::storage::PAGE_SIZE - 1) / crate::storage::PAGE_SIZE;

        // Ensure we have enough pages.
        while (self.storage.header().num_pages as usize) < pages_needed + 1 {
            self.storage.allocate_page()?;
        }

        for (i, chunk) in payload.chunks(crate::storage::PAGE_SIZE).enumerate() {
            let mut page = vec![0u8; crate::storage::PAGE_SIZE];
            page[..chunk.len()].copy_from_slice(chunk);
            self.storage.write_page((i + 1) as u32, &page)?;
        }

        // Write a trailer page with a length marker so we can reload.
        let mut trailer = vec![0u8; crate::storage::PAGE_SIZE];
        let len_bytes = payload.len().to_le_bytes();
        trailer[..len_bytes.len()].copy_from_slice(&len_bytes);
        self.storage
            .write_page((pages_needed + 1) as u32, &trailer)?;

        self.wal.reset()?;
        Ok(())
    }

    /// Dump all documents as a JSON array.
    pub fn dump(&self) -> Result<String> {
        let arr: Vec<&Document> = self.docs.values().collect();
        Ok(serde_json::to_string_pretty(&arr)?)
    }

    pub fn doc_count(&self) -> usize {
        self.docs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn test_crud() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let mut engine = CassetteEngine::open(&db_path).unwrap();

        let doc = Document::new(json!({"title": "Hello", "views": 42}));
        let id = engine.insert(doc).unwrap();
        assert_eq!(engine.doc_count(), 1);

        engine
            .update(&id, json!({"title": "World", "views": 100}))
            .unwrap();
        assert_eq!(engine.get(&id).unwrap().data["views"], 100);

        engine.delete(&id).unwrap();
        assert_eq!(engine.doc_count(), 0);
    }

    #[test]
    fn test_query_and_search() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let mut engine = CassetteEngine::open(&db_path).unwrap();

        engine
            .insert(Document::new(json!({"name": "Alice", "age": 30})))
            .unwrap();
        engine
            .insert(Document::new(json!({"name": "Bob", "age": 25})))
            .unwrap();
        engine
            .insert(Document::new(json!({"name": "Charlie", "age": 35})))
            .unwrap();

        let q = Query::parse("age > 28").unwrap();
        let res = engine.query(&q);
        assert_eq!(res.count, 2);

        let q2 = Query::parse("search(\"alice\")").unwrap();
        let res2 = engine.query(&q2);
        assert_eq!(res2.count, 1);
    }
}
