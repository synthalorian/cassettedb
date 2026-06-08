//! Backup and snapshot support for CassetteDB.
//!
//! Snapshots are point-in-time copies of the database files
//! (`.cassette` + `.cassette.wal`) that can be used for backup
//! and disaster recovery.

use crate::error::{CassetteError, Result};
use chrono::Utc;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

/// Metadata for a snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub created_at: String,
    pub db_name: String,
    pub size_bytes: u64,
}

/// Create a snapshot of a database.
pub fn create_snapshot(db_path: &Path, snapshot_dir: &Path) -> Result<SnapshotMeta> {
    if !db_path.exists() {
        return Err(CassetteError::NotFound(format!(
            "Database not found: {}",
            db_path.display()
        )));
    }

    fs::create_dir_all(snapshot_dir)?;

    let id = format!("snap_{}", Utc::now().format("%Y%m%d_%H%M%S"));
    let snap_db_path = snapshot_dir.join(format!("{}.cassette", id));
    let snap_wal_path = snapshot_dir.join(format!("{}.cassette.wal", id));
    let snap_meta_path = snapshot_dir.join(format!("{}.json", id));

    // Copy main database file.
    copy_file(db_path, &snap_db_path)?;

    // Copy WAL if it exists.
    let wal_path = db_path.with_extension("wal");
    if wal_path.exists() {
        copy_file(&wal_path, &snap_wal_path)?;
    }

    // Gather size.
    let size_bytes = fs::metadata(&snap_db_path)?.len()
        + if snap_wal_path.exists() {
            fs::metadata(&snap_wal_path)?.len()
        } else {
            0
        };

    let meta = SnapshotMeta {
        id: id.clone(),
        created_at: Utc::now().to_rfc3339(),
        db_name: db_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        size_bytes,
    };

    let meta_json = serde_json::to_string_pretty(&meta)?;
    fs::write(&snap_meta_path, meta_json)?;

    Ok(meta)
}

/// List all snapshots in a snapshot directory.
pub fn list_snapshots(snapshot_dir: &Path) -> Result<Vec<SnapshotMeta>> {
    if !snapshot_dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();
    for entry in fs::read_dir(snapshot_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let content = fs::read_to_string(&path)?;
            if let Ok(meta) = serde_json::from_str::<SnapshotMeta>(&content) {
                snapshots.push(meta);
            }
        }
    }

    // Sort by creation time descending.
    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(snapshots)
}

/// Restore a database from a snapshot.
pub fn restore_snapshot(snapshot_dir: &Path, snapshot_id: &str, db_path: &Path) -> Result<()> {
    let snap_db_path = snapshot_dir.join(format!("{}.cassette", snapshot_id));
    let snap_wal_path = snapshot_dir.join(format!("{}.cassette.wal", snapshot_id));

    if !snap_db_path.exists() {
        return Err(CassetteError::NotFound(format!(
            "Snapshot '{}' not found in {}",
            snapshot_id,
            snapshot_dir.display()
        )));
    }

    // Restore main database file.
    copy_file(&snap_db_path, db_path)?;

    // Restore WAL if it exists.
    let wal_path = db_path.with_extension("wal");
    if snap_wal_path.exists() {
        copy_file(&snap_wal_path, &wal_path)?;
    } else if wal_path.exists() {
        fs::remove_file(&wal_path)?;
    }

    Ok(())
}

/// Delete a snapshot.
pub fn delete_snapshot(snapshot_dir: &Path, snapshot_id: &str) -> Result<()> {
    let paths = [
        snapshot_dir.join(format!("{}.cassette", snapshot_id)),
        snapshot_dir.join(format!("{}.cassette.wal", snapshot_id)),
        snapshot_dir.join(format!("{}.json", snapshot_id)),
    ];

    for path in &paths {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }

    Ok(())
}

fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    let mut src_file = fs::File::open(src)?;
    let mut dst_file = fs::File::create(dst)?;
    let mut buf = [0u8; 8192];
    loop {
        let n = src_file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst_file.write_all(&buf[..n])?;
    }
    dst_file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_and_restore_snapshot() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let snapshot_dir = dir.path().join("snapshots");

        // Create a dummy database file.
        fs::write(&db_path, b"dummy db content").unwrap();

        let meta = create_snapshot(&db_path, &snapshot_dir).unwrap();
        assert!(meta.size_bytes > 0);
        assert!(meta.id.starts_with("snap_"));

        let snapshots = list_snapshots(&snapshot_dir).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].id, meta.id);

        // Restore to a new location.
        let restored_path = dir.path().join("restored.cassette");
        restore_snapshot(&snapshot_dir, &meta.id, &restored_path).unwrap();
        assert_eq!(fs::read_to_string(&restored_path).unwrap(), "dummy db content");
    }

    #[test]
    fn test_delete_snapshot() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let snapshot_dir = dir.path().join("snapshots");

        fs::write(&db_path, b"content").unwrap();
        let meta = create_snapshot(&db_path, &snapshot_dir).unwrap();

        delete_snapshot(&snapshot_dir, &meta.id).unwrap();
        let snapshots = list_snapshots(&snapshot_dir).unwrap();
        assert!(snapshots.is_empty());
    }
}
