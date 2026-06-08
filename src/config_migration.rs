//! Config migration system for CassetteDB.
//!
//! Manages versioning and automatic migration of configuration files
//! (e.g. cluster.json, node settings) between releases.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

/// Current config format version.
pub const CURRENT_CONFIG_VERSION: u32 = 1;

/// Generic versioned config wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedConfig {
    pub version: u32,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Migration registry: maps version → migration function.
pub type MigrationFn = fn(&mut serde_json::Value) -> Result<()>;

pub struct ConfigMigrator {
    migrations: HashMap<u32, MigrationFn>,
}

impl Default for ConfigMigrator {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigMigrator {
    pub fn new() -> Self {
        let mut migrations: HashMap<u32, MigrationFn> = HashMap::new();
        // Register built-in migrations here.
        // Example: v0 → v1 adds a new required field.
        migrations.insert(0, migrate_v0_to_v1);
        Self { migrations }
    }

    /// Migrate a config file to the latest version in-place.
    pub fn migrate_file(&self, path: &Path) -> Result<()> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut config: VersionedConfig = serde_json::from_str(&raw)
            .with_context(|| format!("parsing config {}", path.display()))?;

        let original_version = config.version;
        self.migrate_value(&mut config)?;

        if config.version != original_version {
            let backup = path.with_extension(format!("json.v{}", original_version));
            fs::copy(path, &backup)
                .with_context(|| format!("creating backup {}", backup.display()))?;
            fs::write(path, serde_json::to_string_pretty(&config)?)
                .with_context(|| format!("writing migrated config {}", path.display()))?;
        }

        Ok(())
    }

    /// Migrate a JSON value through all registered migrations.
    pub fn migrate_value(&self, config: &mut VersionedConfig) -> Result<()> {
        while config.version < CURRENT_CONFIG_VERSION {
            if let Some(migration) = self.migrations.get(&config.version) {
                migration(&mut config.data)?;
                config.version += 1;
            } else {
                anyhow::bail!("no migration registered for version {}", config.version);
            }
        }
        Ok(())
    }

    /// Discover and migrate all `.json` configs in a directory.
    pub fn migrate_directory(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut migrated = Vec::new();
        if !dir.is_dir() {
            return Ok(migrated);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Err(e) = self.migrate_file(&path) {
                    eprintln!("Warning: failed to migrate {}: {}", path.display(), e);
                } else {
                    migrated.push(path);
                }
            }
        }
        Ok(migrated)
    }
}

/// Built-in migration: v0 → v1 adds `tracing_enabled` field if missing.
fn migrate_v0_to_v1(data: &mut serde_json::Value) -> Result<()> {
    if let Some(obj) = data.as_object_mut() {
        obj.entry("tracing_enabled")
            .or_insert_with(|| serde_json::Value::Bool(false));
    }
    Ok(())
}
