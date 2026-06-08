//! Server mode for CassetteDB.
//!
//! Provides TCP (custom protocol) and HTTP (REST API) server implementations
//! with connection pooling, authentication, and multi-database support.

pub mod auth;
pub mod http;
pub mod multidb;
pub mod pool;
pub mod tcp;

pub use auth::{AuthManager, Authenticator};
pub use http::HttpServer;
pub use multidb::MultiDbManager;
pub use pool::{ConnectionPool, PooledConnection};
pub use tcp::{TcpServer, run_tcp_server};

use serde::{Deserialize, Serialize};

/// Standard response envelope for server operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ServerResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

/// Common server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: String,
    pub db_dir: std::path::PathBuf,
    pub auth_token: Option<String>,
    pub pool_size: usize,
    pub max_connections: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:0".to_string(),
            db_dir: std::path::PathBuf::from("./databases"),
            auth_token: None,
            pool_size: 10,
            max_connections: 100,
        }
    }
}
