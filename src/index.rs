//! Custom inverted index for full-text search.
//!
//! Tokenizes text fields, builds a term -> [doc_id] map,
//! and persists it as JSON in a dedicated page region (simplified here
//! by storing in memory and serializing to the main file footer pages).

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A simple inverted index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InvertedIndex {
    /// term -> set of doc_ids
    pub map: HashMap<String, HashSet<String>>,
}

impl InvertedIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tokenize a string into lowercase words.
    pub fn tokenize(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }

    /// Index all string values in a JSON document recursively.
    pub fn index_document(&mut self, doc_id: &str, value: &serde_json::Value) {
        match value {
            serde_json::Value::String(s) => {
                for token in Self::tokenize(s) {
                    self.map
                        .entry(token)
                        .or_default()
                        .insert(doc_id.to_string());
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    self.index_document(doc_id, v);
                }
            }
            serde_json::Value::Object(obj) => {
                for v in obj.values() {
                    self.index_document(doc_id, v);
                }
            }
            _ => {}
        }
    }

    /// Remove a document from the index.
    pub fn remove_document(&mut self, doc_id: &str, value: &serde_json::Value) {
        match value {
            serde_json::Value::String(s) => {
                for token in Self::tokenize(s) {
                    if let Some(set) = self.map.get_mut(&token) {
                        set.remove(doc_id);
                        if set.is_empty() {
                            self.map.remove(&token);
                        }
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    self.remove_document(doc_id, v);
                }
            }
            serde_json::Value::Object(obj) => {
                for v in obj.values() {
                    self.remove_document(doc_id, v);
                }
            }
            _ => {}
        }
    }

    /// Search for documents matching all query terms.
    pub fn search(&self, query: &str) -> Vec<String> {
        let tokens = Self::tokenize(query);
        if tokens.is_empty() {
            return Vec::new();
        }
        let mut result: Option<HashSet<String>> = None;
        for token in &tokens {
            if let Some(set) = self.map.get(token) {
                match &mut result {
                    None => result = Some(set.clone()),
                    Some(r) => {
                        r.retain(|id| set.contains(id));
                    }
                }
            } else {
                return Vec::new();
            }
        }
        result.map(|s| s.into_iter().collect()).unwrap_or_default()
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}
