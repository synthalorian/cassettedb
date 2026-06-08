//! Node.js bindings for CassetteDB via napi-rs.
//!
//! This crate exposes a minimal `CassetteDB` class to JavaScript. It wraps
//! the Rust `CassetteEngine` directly, so no separate C FFI build is required.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::path::Path;

use cassettedb::document::Document;
use cassettedb::engine::CassetteEngine;
use cassettedb::query::Query;

/// JavaScript-facing database handle.
#[napi]
pub struct CassetteDB {
    engine: CassetteEngine,
}

#[napi]
impl CassetteDB {
    /// Open (or create) a database at the given path.
    #[napi(constructor)]
    pub fn new(path: String) -> Result<Self> {
        let engine = CassetteEngine::open(Path::new(&path))
            .map_err(|e| Error::new(Status::GenericFailure, format!("{}", e)))?;
        Ok(CassetteDB { engine })
    }

    /// Insert a JSON document and return its assigned ID.
    #[napi]
    pub fn insert(&mut self, json: String) -> Result<String> {
        let value: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid JSON: {}", e)))?;
        let doc = Document::new(value);
        self.engine
            .insert(doc)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{}", e)))
    }

    /// Retrieve a document by ID as a JSON string.
    #[napi]
    pub fn get(&self, id: String) -> Result<Option<String>> {
        match self.engine.get(&id) {
            Some(doc) => serde_json::to_string(doc)
                .map(Some)
                .map_err(|e| Error::new(Status::GenericFailure, format!("{}", e))),
            None => Ok(None),
        }
    }

    /// Replace the document identified by `id`.
    #[napi]
    pub fn update(&mut self, id: String, json: String) -> Result<()> {
        let value: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid JSON: {}", e)))?;
        self.engine
            .update(&id, value)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{}", e)))
    }

    /// Delete a document by ID.
    #[napi]
    pub fn delete(&mut self, id: String) -> Result<()> {
        self.engine
            .delete(&id)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{}", e)))
    }

    /// Execute a query and return a JSON array string of matched documents.
    #[napi]
    pub fn query(&self, query: String) -> Result<String> {
        let q = Query::parse(&query)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid query: {}", e)))?;
        let result = self.engine.query(&q);
        serde_json::to_string(&result.documents)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{}", e)))
    }

    /// Return all documents as a JSON array string.
    #[napi]
    pub fn dump(&self) -> Result<String> {
        self.engine
            .dump()
            .map_err(|e| Error::new(Status::GenericFailure, format!("{}", e)))
    }

    /// Compact the database.
    #[napi]
    pub fn compact(&mut self) -> Result<()> {
        self.engine
            .compact()
            .map_err(|e| Error::new(Status::GenericFailure, format!("{}", e)))
    }
}
