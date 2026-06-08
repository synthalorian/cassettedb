use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A document stored in CassetteDB.
/// Every document has a unique ID, optional metadata, and arbitrary JSON data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Document {
    /// Unique document identifier (UUID v4).
    pub id: String,
    /// User-defined JSON payload.
    pub data: serde_json::Value,
    /// Internal metadata (created_at, updated_at, version).
    #[serde(default)]
    pub meta: HashMap<String, serde_json::Value>,
}

impl Document {
    pub fn new(data: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            data,
            meta: HashMap::new(),
        }
    }

    pub fn with_id(id: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            data,
            meta: HashMap::new(),
        }
    }
}
