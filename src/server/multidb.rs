//! Multi-database support for CassetteDB server.
//!
//! Manages multiple named databases within a single server instance.

use crate::engine::CassetteEngine;
use crate::error::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Manager for multiple databases.
pub struct MultiDbManager {
    db_dir: PathBuf,
    databases: Mutex<HashMap<String, Arc<Mutex<CassetteEngine>>>>,
}

impl MultiDbManager {
    /// Create a new multi-database manager.
    pub fn new(db_dir: impl AsRef<Path>) -> Result<Self> {
        let db_dir = db_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&db_dir)?;
        Ok(Self {
            db_dir,
            databases: Mutex::new(HashMap::new()),
        })
    }

    /// Get or open a database by name.
    pub fn get_or_open(&self, name: &str) -> Result<Arc<Mutex<CassetteEngine>>> {
        let mut databases = self.databases.lock().unwrap();
        if let Some(db) = databases.get(name) {
            return Ok(db.clone());
        }

        let db_path = self.db_path(name);
        let engine = CassetteEngine::open(&db_path)?;
        let arc = Arc::new(Mutex::new(engine));
        databases.insert(name.to_string(), arc.clone());
        Ok(arc)
    }

    /// Create a new database.
    pub fn create(&self, name: &str) -> Result<Arc<Mutex<CassetteEngine>>> {
        let db_path = self.db_path(name);
        let engine = CassetteEngine::open(&db_path)?;
        let arc = Arc::new(Mutex::new(engine));
        let mut databases = self.databases.lock().unwrap();
        databases.insert(name.to_string(), arc.clone());
        Ok(arc)
    }

    /// Check if a database exists.
    pub fn exists(&self, name: &str) -> bool {
        self.db_path(name).exists()
    }

    /// Delete a database (remove files).
    pub fn delete(&self, name: &str) -> Result<()> {
        let mut databases = self.databases.lock().unwrap();
        databases.remove(name);

        let db_path = self.db_path(name);
        if db_path.exists() {
            std::fs::remove_file(&db_path)?;
        }
        let wal_path = db_path.with_extension("wal");
        if wal_path.exists() {
            std::fs::remove_file(&wal_path)?;
        }
        let repl_path = db_path.with_extension("repl");
        if repl_path.exists() {
            std::fs::remove_file(&repl_path)?;
        }

        Ok(())
    }

    /// List all databases.
    pub fn list(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.db_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("cassette") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names.sort();
        names
    }

    fn db_path(&self, name: &str) -> PathBuf {
        self.db_dir.join(format!("{}.cassette", name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_multidb_create_list_delete() {
        let dir = TempDir::new().unwrap();
        let manager = MultiDbManager::new(dir.path()).unwrap();

        assert!(manager.list().is_empty());

        manager.create("test1").unwrap();
        manager.create("test2").unwrap();

        let list = manager.list();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"test1".to_string()));
        assert!(list.contains(&"test2".to_string()));

        assert!(manager.exists("test1"));
        manager.delete("test1").unwrap();
        assert!(!manager.exists("test1"));

        let list = manager.list();
        assert_eq!(list.len(), 1);
    }
}
