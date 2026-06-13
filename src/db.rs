use anyhow::Result;
use jsonpath_rust::JsonPathQuery;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Document {
    pub id: String,
    pub data: Value,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub deleted: bool,
}

/// Inverted index mapping: field → token → document IDs
pub type InvertedIndex = HashMap<String, HashMap<String, Vec<String>>>;

/// Secondary index mapping: field → ordered value → document IDs
pub type SecondaryIndex = BTreeMap<String, Vec<String>>;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Collection {
    pub name: String,
    pub documents: Vec<Document>,
    #[serde(default)]
    pub indexes: HashMap<String, Vec<String>>,
    /// Full-text inverted index for fast search
    #[serde(default)]
    pub inverted_index: InvertedIndex,
    /// Secondary indexes for range queries: field_name → ordered index
    #[serde(default)]
    pub secondary_indexes: HashMap<String, SecondaryIndex>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Cassette {
    pub version: u32,
    pub collections: HashMap<String, Collection>,
    pub meta: HashMap<String, String>,
}

impl Cassette {
    pub fn new() -> Self {
        Cassette {
            version: 1,
            collections: HashMap::new(),
            meta: HashMap::new(),
        }
    }

    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut contents = String::new();
        reader.read_to_string(&mut contents)?;
        let cassette: Cassette = serde_json::from_str(&contents)?;
        Ok(cassette)
    }

    /// Open the cassette and replay any committed WAL entries for recovery.
    pub fn open_with_wal(path: &Path) -> Result<Self> {
        let mut cassette = Self::open(path)?;
        let wal_path = path.with_extension("wal");
        if wal_path.exists() {
            let mut wal = crate::wal::Wal::open(path)?;
            wal.replay(|entry| {
                use crate::wal::WalEntry;
                match entry {
                    WalEntry::Insert { collection, doc, .. } => {
                        let _ = cassette.insert(collection, doc.clone());
                    }
                    WalEntry::Update { collection, id, doc, .. } => {
                        let _ = cassette.update(collection, id, doc.clone());
                    }
                    WalEntry::Delete { collection, id, .. } => {
                        let _ = cassette.delete(collection, id);
                    }
                    _ => {}
                }
                Ok(())
            })?;
            // After recovery, save the database and truncate the WAL
            cassette.save(path)?;
        }
        Ok(cassette)
    }

    /// ACID-like save: write to temp file, then atomic rename
    pub fn save(&self, path: &Path) -> Result<()> {
        let temp_path = path.with_extension("tmp");
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;
        std::fs::rename(&temp_path, path)?;
        Ok(())
    }

    pub fn init(path: &Path) -> Result<()> {
        let cassette = Self::new();
        cassette.save(path)?;
        Ok(())
    }

    pub fn insert(&mut self, collection: &str, doc: Value) -> Result<String> {
        let coll = self
            .collections
            .entry(collection.to_string())
            .or_insert_with(|| Collection {
                name: collection.to_string(),
                ..Default::default()
            });

        let id = format!(
            "{}-{}",
            chrono::Utc::now().timestamp_millis(),
            rand::random::<u16>()
        );
        let document = Document {
            id: id.clone(),
            data: doc,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            deleted: false,
        };

        // Update inverted index for all text fields
        Self::index_document(&mut coll.inverted_index, &document);

        // Update secondary indexes
        Self::index_secondary(&mut coll.secondary_indexes, &document);

        coll.documents.push(document);
        Ok(id)
    }

    /// Create a secondary index on a field for range queries.
    pub fn create_index(&mut self, collection: &str, field: &str) -> Result<()> {
        let coll = self
            .collections
            .get_mut(collection)
            .ok_or_else(|| anyhow::anyhow!("collection not found"))?;

        if coll.secondary_indexes.contains_key(field) {
            return Ok(());
        }

        let mut index: SecondaryIndex = BTreeMap::new();
        for doc in &coll.documents {
            if doc.deleted {
                continue;
            }
            Self::add_to_secondary_index(&mut index, field, doc);
        }

        coll.secondary_indexes.insert(field.to_string(), index);
        Ok(())
    }

    /// Drop a secondary index.
    pub fn drop_index(&mut self, collection: &str, field: &str) -> Result<bool> {
        let coll = self
            .collections
            .get_mut(collection)
            .ok_or_else(|| anyhow::anyhow!("collection not found"))?;
        Ok(coll.secondary_indexes.remove(field).is_some())
    }

    /// List all secondary index fields for a collection.
    pub fn list_indexes(&self, collection: &str) -> Vec<String> {
        self.collections
            .get(collection)
            .map(|coll| coll.secondary_indexes.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn add_to_secondary_index(index: &mut SecondaryIndex, field: &str, doc: &Document) {
        if let Some(value) = doc.data.get(field) {
            let key = Self::value_to_index_key(value);
            index.entry(key).or_default().push(doc.id.clone());
        }
    }

    pub fn remove_from_secondary_index(index: &mut SecondaryIndex, field: &str, doc: &Document) {
        if let Some(value) = doc.data.get(field) {
            let key = Self::value_to_index_key(value);
            if let Some(ids) = index.get_mut(&key) {
                ids.retain(|id| id != &doc.id);
                if ids.is_empty() {
                    index.remove(&key);
                }
            }
        }
    }

    pub fn value_to_index_key(value: &Value) -> String {
        match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => value.to_string(),
        }
    }

    fn index_secondary(secondary_indexes: &mut HashMap<String, SecondaryIndex>, doc: &Document) {
        for (field, index) in secondary_indexes.iter_mut() {
            Self::add_to_secondary_index(index, field, doc);
        }
    }

    #[allow(dead_code)]
    fn reindex_secondary(secondary_indexes: &mut HashMap<String, SecondaryIndex>, doc: &Document) {
        for (field, index) in secondary_indexes.iter_mut() {
            Self::remove_from_secondary_index(index, field, doc);
            Self::add_to_secondary_index(index, field, doc);
        }
    }

    /// Tokenize text and update the collection's inverted index
    pub fn index_document(inverted_index: &mut InvertedIndex, doc: &Document) {
        let re = Regex::new(r"[^a-zA-Z0-9]+").expect("regex should be valid");
        if let Some(obj) = doc.data.as_object() {
            for (field, value) in obj {
                if let Some(text) = value.as_str() {
                    let tokens: Vec<String> = re
                        .split(text)
                        .filter(|t| !t.is_empty())
                        .map(|t| t.to_lowercase())
                        .collect();
                    let field_index = inverted_index
                        .entry(field.clone())
                        .or_default();
                    for token in tokens {
                        field_index
                            .entry(token)
                            .or_default()
                            .push(doc.id.clone());
                    }
                }
            }
        }
    }

    /// Full-text search across all indexed text fields
    pub fn search(&self, collection: &str, query: &str) -> Result<Vec<&Document>> {
        let coll = match self.collections.get(collection) {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        let re = Regex::new(r"[^a-zA-Z0-9]+").expect("regex should be valid");
        let query_tokens: Vec<String> = re
            .split(query)
            .filter(|t| !t.is_empty())
            .map(|t| t.to_lowercase())
            .collect();

        if query_tokens.is_empty() {
            return Ok(coll.documents.iter().filter(|d| !d.deleted).collect());
        }

        // Score documents by how many query tokens they match
        let mut scores: HashMap<String, usize> = HashMap::new();
        for token in &query_tokens {
            for field_index in coll.inverted_index.values() {
                if let Some(ids) = field_index.get(token) {
                    for id in ids {
                        *scores.entry(id.clone()).or_insert(0) += 1;
                    }
                }
            }
        }

        // Sort by score (descending) and return matching docs
        let mut scored_ids: Vec<(String, usize)> = scores.into_iter().collect();
        scored_ids.sort_by_key(|b| std::cmp::Reverse(b.1));

        let ids: Vec<String> = scored_ids.into_iter().map(|(id, _)| id).collect();
        let mut results = Vec::new();
        for id in ids {
            if let Some(doc) = coll.documents.iter().find(|d| d.id == id && !d.deleted) {
                results.push(doc);
            }
        }
        Ok(results)
    }

    pub fn query(&self, collection: &str, filter: &str) -> Result<Vec<&Document>> {
        let coll = match self.collections.get(collection) {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        let parts: Vec<&str> = filter.split("=").collect();
        if parts.len() != 2 {
            return Ok(coll.documents.iter().filter(|d| !d.deleted).collect());
        }

        let key = parts[0].trim();
        let val = parts[1].trim().trim_matches('"');

        // Use secondary index if available
        if let Some(index) = coll.secondary_indexes.get(key) {
            if let Some(ids) = index.get(val) {
                let id_set: HashSet<String> = ids.iter().cloned().collect();
                let mut results = Vec::new();
                for doc in &coll.documents {
                    if !doc.deleted && id_set.contains(&doc.id) {
                        results.push(doc);
                    }
                }
                return Ok(results);
            }
            return Ok(vec![]);
        }

        Ok(coll
            .documents
            .iter()
            .filter(|d| !d.deleted)
            .filter(|d| match d.data.get(key) {
                Some(v) => v.as_str() == Some(val),
                None => false,
            })
            .collect())
    }

    /// Range query using secondary indexes if available.
    /// Supported operators: >, <, >=, <=
    pub fn query_range(
        &self,
        collection: &str,
        field: &str,
        op: &str,
        value: &str,
    ) -> Result<Vec<&Document>> {
        let coll = match self.collections.get(collection) {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        let mut results = Vec::new();

        if let Some(index) = coll.secondary_indexes.get(field) {
            match op {
                ">" => {
                    for (_, ids) in index.range(value.to_string()..) {
                        if index.get(value) == Some(ids) && op == ">" {
                            // skip exact match for >
                            continue;
                        }
                        for id in ids {
                            if let Some(doc) = coll.documents.iter().find(|d| d.id == *id && !d.deleted) {
                                if Self::value_to_index_key(&doc.data[field]).as_str() > value {
                                    results.push(doc);
                                }
                            }
                        }
                    }
                }
                "<" => {
                    for (_, ids) in index.range(..value.to_string()) {
                        for id in ids {
                            if let Some(doc) = coll.documents.iter().find(|d| d.id == *id && !d.deleted) {
                                results.push(doc);
                            }
                        }
                    }
                }
                ">=" => {
                    for (_, ids) in index.range(value.to_string()..) {
                        for id in ids {
                            if let Some(doc) = coll.documents.iter().find(|d| d.id == *id && !d.deleted) {
                                results.push(doc);
                            }
                        }
                    }
                }
                "<=" => {
                    for (_, ids) in index.range(..=value.to_string()) {
                        for id in ids {
                            if let Some(doc) = coll.documents.iter().find(|d| d.id == *id && !d.deleted) {
                                results.push(doc);
                            }
                        }
                    }
                }
                _ => {}
            }
        } else {
            // Fallback to full scan
            results = coll
                .documents
                .iter()
                .filter(|d| !d.deleted)
                .filter(|d| {
                    match d.data.get(field) {
                        Some(v) => {
                            let key = Self::value_to_index_key(v);
                            match op {
                                ">" => key.as_str() > value,
                                "<" => key.as_str() < value,
                                ">=" => key.as_str() >= value,
                                "<=" => key.as_str() <= value,
                                _ => false,
                            }
                        }
                        None => false,
                    }
                })
                .collect();
        }

        Ok(results)
    }

    /// Query using JSONPath expression
    pub fn query_jsonpath(&self, collection: &str, path: &str) -> Result<Vec<&Document>> {
        let coll = match self.collections.get(collection) {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        Ok(coll
            .documents
            .iter()
            .filter(|d| !d.deleted)
            .filter(|d| {
                match d.data.clone().path(path) {
                    Ok(results) => !results.is_null() && results.as_array().map_or(true, |a| !a.is_empty()),
                    Err(_) => false,
                }
            })
            .collect())
    }

    pub fn get(&self, collection: &str, id: &str) -> Option<&Document> {
        self.collections.get(collection).and_then(|coll| {
            coll.documents.iter().find(|d| d.id == id && !d.deleted)
        })
    }

    /// Scan all documents in a collection (no filter)
    pub fn scan(&self, collection: &str) -> Result<Vec<&Document>> {
        let coll = match self.collections.get(collection) {
            Some(c) => c,
            None => return Ok(vec![]),
        };
        Ok(coll.documents.iter().filter(|d| !d.deleted).collect())
    }

    pub fn update(&mut self, collection: &str, id: &str, data: Value) -> Result<bool> {
        let coll = self
            .collections
            .get_mut(collection)
            .ok_or_else(|| anyhow::anyhow!("collection not found"))?;

        let pos = match coll.documents.iter().position(|d| d.id == id) {
            Some(p) => p,
            None => return Ok(false),
        };

        let Collection {
            documents,
            inverted_index,
            secondary_indexes,
            ..
        } = coll;
        let doc = &mut documents[pos];
        if doc.deleted {
            return Ok(false);
        }
        Self::remove_from_index(inverted_index, id);
        Self::remove_from_secondary_index_all(secondary_indexes, doc);
        doc.data = data;
        doc.updated_at = chrono::Utc::now().to_rfc3339();
        Self::index_document(inverted_index, doc);
        Self::index_secondary(secondary_indexes, doc);
        Ok(true)
    }

    pub fn delete(&mut self, collection: &str, id: &str) -> Result<bool> {
        let coll = self
            .collections
            .get_mut(collection)
            .ok_or_else(|| anyhow::anyhow!("collection not found"))?;

        if let Some(doc) = coll.documents.iter_mut().find(|d| d.id == id) {
            doc.deleted = true;
            doc.updated_at = chrono::Utc::now().to_rfc3339();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn remove_from_index(inverted_index: &mut InvertedIndex, id: &str) {
        for field_index in inverted_index.values_mut() {
            for ids in field_index.values_mut() {
                ids.retain(|doc_id| doc_id != id);
            }
            field_index.retain(|_, ids| !ids.is_empty());
        }
        inverted_index.retain(|_, idx| !idx.is_empty());
    }

    fn remove_from_secondary_index_all(
        secondary_indexes: &mut HashMap<String, SecondaryIndex>,
        doc: &Document,
    ) {
        for (field, index) in secondary_indexes.iter_mut() {
            Self::remove_from_secondary_index(index, field, doc);
        }
    }

    pub fn collections(&self) -> Vec<&String> {
        self.collections.keys().collect()
    }

    pub fn compact(&mut self) -> Result<usize> {
        let mut removed = 0;
        for coll in self.collections.values_mut() {
            let before = coll.documents.len();
            coll.documents.retain(|d| !d.deleted);
            removed += before - coll.documents.len();

            // Rebuild inverted index after compaction
            let mut new_index: InvertedIndex = HashMap::new();
            for doc in &coll.documents {
                Self::index_document(&mut new_index, doc);
            }
            coll.inverted_index = new_index;

            // Rebuild secondary indexes after compaction
            let mut new_secondary: HashMap<String, SecondaryIndex> = HashMap::new();
            for field in coll.secondary_indexes.keys() {
                let mut index: SecondaryIndex = BTreeMap::new();
                for doc in &coll.documents {
                    Self::add_to_secondary_index(&mut index, field, doc);
                }
                new_secondary.insert(field.clone(), index);
            }
            coll.secondary_indexes = new_secondary;
        }
        Ok(removed)
    }

    // === Owned variants for snapshot isolation in transactions ===

    pub fn query_owned(&self, collection: &str, filter: &str) -> Result<Vec<Document>> {
        let coll = match self.collections.get(collection) {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        let parts: Vec<&str> = filter.split("=").collect();
        if parts.len() != 2 {
            return Ok(coll
                .documents
                .iter()
                .filter(|d| !d.deleted)
                .cloned()
                .collect());
        }

        let key = parts[0].trim();
        let val = parts[1].trim().trim_matches('"');

        Ok(coll
            .documents
            .iter()
            .filter(|d| !d.deleted)
            .filter(|d| match d.data.get(key) {
                Some(v) => v.as_str() == Some(val),
                None => false,
            })
            .cloned()
            .collect())
    }

    pub fn scan_owned(&self, collection: &str) -> Result<Vec<Document>> {
        let coll = match self.collections.get(collection) {
            Some(c) => c,
            None => return Ok(vec![]),
        };
        Ok(coll
            .documents
            .iter()
            .filter(|d| !d.deleted)
            .cloned()
            .collect())
    }

    pub fn get_owned(&self, collection: &str, id: &str) -> Option<Document> {
        self.collections.get(collection).and_then(|coll| {
            coll.documents
                .iter()
                .find(|d| d.id == id && !d.deleted)
                .cloned()
        })
    }
}
