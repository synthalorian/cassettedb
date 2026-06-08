//! Optional Tantivy integration for advanced full-text search.
//!
//! Enabled via the `tantivy-search` cargo feature.
//! Provides BM25 scoring, phrase queries, boolean queries,
//! and fuzzy matching over indexed documents.

use crate::document::Document;
use crate::error::{CassetteError, Result};
use std::path::Path;
use tantivy::schema::Value;

/// Tantivy-backed search index.
pub struct TantivySearch {
    index: tantivy::Index,
    #[allow(dead_code)]
    schema: tantivy::schema::Schema,
    doc_id_field: tantivy::schema::Field,
    data_field: tantivy::schema::Field,
}

impl TantivySearch {
    /// Open or create a Tantivy index at the given directory.
    pub fn open(index_dir: &Path) -> Result<Self> {
        let mut schema_builder = tantivy::schema::Schema::builder();
        let doc_id_field = schema_builder.add_text_field(
            "doc_id",
            tantivy::schema::STRING | tantivy::schema::STORED,
        );
        let data_field = schema_builder.add_text_field(
            "data",
            tantivy::schema::TEXT | tantivy::schema::STORED,
        );
        let schema = schema_builder.build();

        let index = if index_dir.exists() && index_dir.read_dir()?.next().is_some() {
            let dir =
                tantivy::directory::MmapDirectory::open(index_dir).map_err(|e| {
                    CassetteError::Index(format!("Failed to open MmapDirectory: {}", e))
                })?;
            tantivy::Index::open(dir).map_err(|e| {
                CassetteError::Index(format!("Failed to open Tantivy index: {}", e))
            })?
        } else {
            std::fs::create_dir_all(index_dir)?;
            let dir = tantivy::directory::MmapDirectory::open(index_dir).map_err(|e| {
                CassetteError::Index(format!("Failed to open MmapDirectory: {}", e))
            })?;
            tantivy::Index::create(dir, schema.clone(), tantivy::IndexSettings::default()).map_err(|e| {
                CassetteError::Index(format!("Failed to create Tantivy index: {}", e))
            })?
        };

        Ok(TantivySearch {
            index,
            schema,
            doc_id_field,
            data_field,
        })
    }

    /// Index a document.
    pub fn index_document(&self, doc: &Document) -> Result<()> {
        let mut index_writer: tantivy::IndexWriter<tantivy::TantivyDocument> = self
            .index
            .writer(50_000_000)
            .map_err(|e| CassetteError::Index(format!("Tantivy writer error: {}", e)))?;

        let mut tantivy_doc = tantivy::TantivyDocument::default();
        tantivy_doc.add_text(self.doc_id_field, &doc.id);
        tantivy_doc.add_text(
            self.data_field,
            &serde_json::to_string(&doc.data).unwrap_or_default(),
        );

        index_writer
            .add_document(tantivy_doc)
            .map_err(|e| CassetteError::Index(format!("Tantivy add_document error: {}", e)))?;
        index_writer
            .commit()
            .map_err(|e| CassetteError::Index(format!("Tantivy commit error: {}", e)))?;

        Ok(())
    }

    /// Remove a document from the index.
    pub fn remove_document(&self, doc_id: &str) -> Result<()> {
        let mut index_writer: tantivy::IndexWriter<tantivy::TantivyDocument> = self
            .index
            .writer(50_000_000)
            .map_err(|e| CassetteError::Index(format!("Tantivy writer error: {}", e)))?;

        let term = tantivy::Term::from_field_text(self.doc_id_field, doc_id);
        index_writer.delete_term(term);
        index_writer
            .commit()
            .map_err(|e| CassetteError::Index(format!("Tantivy commit error: {}", e)))?;

        Ok(())
    }

    /// Search for documents matching the given query string.
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let reader = self
            .index
            .reader()
            .map_err(|e| CassetteError::Index(format!("Tantivy reader error: {}", e)))?;
        let searcher = reader.searcher();

        let query_parser = tantivy::query::QueryParser::for_index(
            &self.index,
            vec![self.data_field],
        );
        let query = query_parser.parse_query(query_str).map_err(|e| {
            CassetteError::InvalidQuery(format!("Tantivy query parse error: {}", e))
        })?;

        let top_docs = searcher
            .search(&query, &tantivy::collector::TopDocs::with_limit(limit))
            .map_err(|e| CassetteError::Index(format!("Tantivy search error: {}", e)))?;

        let mut results = Vec::new();
        for (_score, doc_address) in top_docs {
            let retrieved_doc: tantivy::TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| CassetteError::Index(format!("Tantivy doc retrieval error: {}", e)))?;
            let doc_id = retrieved_doc
                .get_first(self.doc_id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let data_json = retrieved_doc
                .get_first(self.data_field)
                .and_then(|v| v.as_str())
                .unwrap_or("{}")
                .to_string();
            let data = serde_json::from_str(&data_json).unwrap_or(serde_json::Value::Null);

            results.push(SearchResult { doc_id, data, score: _score });
        }

        Ok(results)
    }
}

/// A single search result from Tantivy.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub doc_id: String,
    pub data: serde_json::Value,
    pub score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn test_tantivy_index_and_search() {
        let dir = TempDir::new().unwrap();
        let index_dir = dir.path().join("tantivy_index");

        let search = TantivySearch::open(&index_dir).unwrap();

        let doc1 = Document::new(json!({"title": "Hello world", "body": "This is a test"}));
        let doc2 = Document::new(json!({"title": "Goodbye world", "body": "Another test document"}));

        search.index_document(&doc1).unwrap();
        search.index_document(&doc2).unwrap();

        // Need to give Tantivy a moment to commit (or use the same writer).
        // In tests with separate writers, this should still work due to commit.
        let results = search.search("hello", 10).unwrap();
        assert!(!results.is_empty(), "Expected at least one result for 'hello'");
        assert!(results.iter().any(|r| r.doc_id == doc1.id));
    }
}
