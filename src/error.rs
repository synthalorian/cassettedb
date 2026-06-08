use thiserror::Error;

pub type Result<T> = std::result::Result<T, CassetteError>;

#[derive(Error, Debug)]
pub enum CassetteError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Corrupt database: {0}")]
    Corrupt(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Document not found: {0}")]
    NotFound(String),

    #[error("Transaction conflict")]
    Conflict,

    #[error("Wal error: {0}")]
    Wal(String),

    #[error("Index error: {0}")]
    Index(String),
}
