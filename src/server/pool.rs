//! Connection pooling for CassetteDB server mode.
//!
//! Provides a fixed-size pool of CassetteEngine instances per database
//! to handle concurrent client connections efficiently.

use crate::engine::CassetteEngine;
use crate::error::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{Semaphore, OwnedSemaphorePermit};

/// A pooled database connection.
pub struct PooledConnection {
    engine: Option<CassetteEngine>,
    db_path: PathBuf,
    #[allow(dead_code)]
    permit: OwnedSemaphorePermit,
}

impl PooledConnection {
    /// Access the underlying engine.
    pub fn engine(&mut self) -> &mut CassetteEngine {
        self.engine.as_mut().expect("engine should be present in pooled connection")
    }

    /// Get the database path.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

/// Per-database connection pool.
struct DbPool {
    db_path: PathBuf,
    semaphore: Arc<Semaphore>,
    connections: Mutex<Vec<CassetteEngine>>,
}

impl DbPool {
    fn new(db_path: PathBuf, size: usize) -> Self {
        Self {
            db_path,
            semaphore: Arc::new(Semaphore::new(size)),
            connections: Mutex::new(Vec::with_capacity(size)),
        }
    }

    /// Acquire a connection from the pool.
    async fn acquire(&self) -> Result<PooledConnection> {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| crate::error::CassetteError::Io(
                std::io::Error::other("semaphore closed")
            ))?;

        // Try to reuse an existing engine.
        let engine = {
            let mut connections = self.connections.lock().unwrap();
            connections.pop()
        };

        let engine = match engine {
            Some(e) => e,
            None => CassetteEngine::open(&self.db_path)?,
        };

        Ok(PooledConnection {
            engine: Some(engine),
            db_path: self.db_path.clone(),
            permit,
        })
    }
}

/// Multi-database connection pool manager.
pub struct ConnectionPool {
    db_dir: PathBuf,
    pool_size: usize,
    pools: Mutex<HashMap<String, Arc<DbPool>>>,
}

impl ConnectionPool {
    /// Create a new connection pool manager.
    pub fn new(db_dir: impl AsRef<Path>, pool_size: usize) -> Result<Self> {
        let db_dir = db_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&db_dir)?;
        Ok(Self {
            db_dir,
            pool_size,
            pools: Mutex::new(HashMap::new()),
        })
    }

    /// Get or create a pool for a database.
    fn get_or_create_pool(&self, db_name: &str) -> Result<Arc<DbPool>> {
        let mut pools = self.pools.lock().unwrap();
        if let Some(pool) = pools.get(db_name) {
            return Ok(pool.clone());
        }

        let db_path = self.db_dir.join(format!("{}.cassette", db_name));
        // Ensure the database exists.
        if !db_path.exists() {
            CassetteEngine::open(&db_path)?;
        }

        let pool = Arc::new(DbPool::new(db_path, self.pool_size));
        pools.insert(db_name.to_string(), pool.clone());
        Ok(pool)
    }

    /// Acquire a connection for the specified database.
    pub async fn acquire(&self, db_name: &str) -> Result<PooledConnection> {
        let pool = self.get_or_create_pool(db_name)?;
        pool.acquire().await
    }

    /// Return a connection to its pool.
    pub fn release(&self, mut conn: PooledConnection) {
        let db_name = conn.db_path()
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        
        let pools = self.pools.lock().unwrap();
        if let Some(pool) = pools.get(&db_name) {
            // The permit is automatically released when conn is dropped.
            // We just need to return the engine to the pool.
            if let Some(engine) = conn.engine.take() {
                let mut connections = pool.connections.lock().unwrap();
                connections.push(engine);
            }
        }
    }

    /// List all databases in the pool.
    pub fn list_databases(&self) -> Vec<String> {
        let pools = self.pools.lock().unwrap();
        pools.keys().cloned().collect()
    }

    /// Check if a database exists.
    pub fn database_exists(&self, db_name: &str) -> bool {
        let db_path = self.db_dir.join(format!("{}.cassette", db_name));
        db_path.exists()
    }

    /// Create a new database.
    pub fn create_database(&self, db_name: &str) -> Result<()> {
        let db_path = self.db_dir.join(format!("{}.cassette", db_name));
        CassetteEngine::open(&db_path)?;
        Ok(())
    }
}
