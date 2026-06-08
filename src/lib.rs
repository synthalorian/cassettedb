//! CassetteDB — A single-file JSON document database inspired by SQLite.
//!
//! # Design Goals
//! - Single `.cassette` file per database (portable, self-contained).
//! - ACID transactions via Write-Ahead Logging (WAL).
//! - JSONPath-like query language.
//! - Full-text search with a custom inverted index.
//! - Zero external server — embeddable library + CLI.

pub mod error;
pub mod wal;
pub mod storage;
pub mod index;
pub mod query;
pub mod engine;
pub mod document;

pub use error::{CassetteError, Result};
pub use engine::CassetteEngine;
pub use document::Document;
pub use query::{Query, QueryResult};
