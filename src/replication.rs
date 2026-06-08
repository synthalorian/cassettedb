//! Replication / change-feed support.
//!
//! Provides an append-only change log that followers can consume
//! to stay in sync with the primary database.

use crate::error::{CassetteError, Result};
use crate::wal::WalOp;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Replication log header.
const REPL_MAGIC: &[u8; 4] = b"CRL1";
const REPL_VERSION: u16 = 1;

/// A single change record in the replication log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChangeRecord {
    pub sequence: u64,
    pub op: WalOp,
    pub doc_id: String,
    pub payload: Vec<u8>,
    pub timestamp: i64,
}

/// Append-only replication log for change-feed / follower sync.
pub struct ReplicationLog {
    file: File,
    path: PathBuf,
    next_sequence: u64,
}

impl ReplicationLog {
    /// Open (or create) a replication log at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let meta = file.metadata()?;
        let next_sequence = if meta.len() == 0 {
            // Initialize new replication log.
            let mut buf = Vec::new();
            buf.write_all(REPL_MAGIC)?;
            buf.write_all(&REPL_VERSION.to_le_bytes())?;
            file.write_all(&buf)?;
            file.sync_all()?;
            1
        } else {
            // Validate header.
            let mut magic = [0u8; 4];
            file.read_exact(&mut magic)?;
            if &magic != REPL_MAGIC {
                return Err(CassetteError::Corrupt("Invalid replication log magic".into()));
            }
            let mut version_bytes = [0u8; 2];
            file.read_exact(&mut version_bytes)?;
            let version = u16::from_le_bytes(version_bytes);
            if version != REPL_VERSION {
                return Err(CassetteError::Corrupt(format!(
                    "Unsupported replication log version {}",
                    version
                )));
            }
            // Compute next sequence by scanning to end.
            Self::compute_next_sequence(&file)?
        };

        Ok(ReplicationLog {
            file,
            path: path.to_path_buf(),
            next_sequence,
        })
    }

    /// Append a change record to the replication log.
    pub fn append(&mut self, op: WalOp, doc_id: &str, payload: &[u8]) -> Result<u64> {
        let sequence = self.next_sequence;
        let timestamp = chrono::Utc::now().timestamp();

        let record = ChangeRecord {
            sequence,
            op,
            doc_id: doc_id.to_string(),
            payload: payload.to_vec(),
            timestamp,
        };

        let bytes = serde_json::to_vec(&record)?;
        let len = bytes.len() as u32;

        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&bytes)?;
        self.file.sync_all()?;

        self.next_sequence += 1;
        Ok(sequence)
    }

    /// Iterate over all change records starting from a given sequence.
    pub fn iter_from(&self, start_sequence: u64) -> Result<ChangeIter> {
        let mut file = OpenOptions::new().read(true).open(&self.path)?;
        file.seek(SeekFrom::Start(6))?; // skip header
        Ok(ChangeIter {
            reader: BufReader::new(file),
            start_sequence,
        })
    }

    /// Get the next sequence number that will be assigned.
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Truncate the replication log (e.g., after snapshot).
    pub fn reset(&mut self) -> Result<()> {
        self.file.set_len(6)?;
        self.file.seek(SeekFrom::Start(6))?;
        self.file.sync_all()?;
        self.next_sequence = 1;
        Ok(())
    }

    fn compute_next_sequence(file: &File) -> Result<u64> {
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(6))?; // skip header

        let mut max_seq = 0u64;
        loop {
            let mut len_bytes = [0u8; 4];
            match reader.read_exact(&mut len_bytes) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf)?;
            if let Ok(record) = serde_json::from_slice::<ChangeRecord>(&buf) {
                max_seq = max_seq.max(record.sequence);
            }
        }
        Ok(max_seq + 1)
    }
}

/// Iterator over change records in the replication log.
pub struct ChangeIter {
    reader: BufReader<File>,
    start_sequence: u64,
}

impl Iterator for ChangeIter {
    type Item = Result<ChangeRecord>;

    fn next(&mut self) -> Option<Result<ChangeRecord>> {
        loop {
            let mut len_bytes = [0u8; 4];
            match self.reader.read_exact(&mut len_bytes) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
                Err(e) => return Some(Err(e.into())),
            }
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut buf = vec![0u8; len];
            if let Err(e) = self.reader.read_exact(&mut buf) {
                return Some(Err(e.into()));
            }
            match serde_json::from_slice::<ChangeRecord>(&buf) {
                Ok(record) => {
                    if record.sequence >= self.start_sequence {
                        return Some(Ok(record));
                    }
                }
                Err(e) => return Some(Err(CassetteError::Serialization(e))),
            }
        }
    }
}

/// Change-feed that wraps a replication log and provides subscription-style access.
pub struct ChangeFeed {
    log: ReplicationLog,
    subscribers: Vec<crossbeam_channel::Sender<ChangeRecord>>,
}

impl ChangeFeed {
    pub fn open(path: &Path) -> Result<Self> {
        let log = ReplicationLog::open(path)?;
        Ok(ChangeFeed {
            log,
            subscribers: Vec::new(),
        })
    }

    /// Publish a change to the feed.
    pub fn publish(&mut self, op: WalOp, doc_id: &str, payload: &[u8]) -> Result<u64> {
        let seq = self.log.append(op, doc_id, payload)?;
        let record = ChangeRecord {
            sequence: seq,
            op,
            doc_id: doc_id.to_string(),
            payload: payload.to_vec(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        // Notify subscribers.
        self.subscribers.retain(|tx| tx.send(record.clone()).is_ok());
        Ok(seq)
    }

    /// Subscribe to the change feed. Returns a receiver for change records.
    pub fn subscribe(&mut self) -> crossbeam_channel::Receiver<ChangeRecord> {
        let (tx, rx) = crossbeam_channel::unbounded();
        self.subscribers.push(tx);
        rx
    }

    /// Get a copy of all changes since a given sequence.
    pub fn changes_since(&self, sequence: u64) -> Result<Vec<ChangeRecord>> {
        let mut changes = Vec::new();
        for record in self.log.iter_from(sequence)? {
            changes.push(record?);
        }
        Ok(changes)
    }

    pub fn next_sequence(&self) -> u64 {
        self.log.next_sequence()
    }

    pub fn reset(&mut self) -> Result<()> {
        self.log.reset()
    }
}

/// Follower client that consumes a replication log.
pub struct Follower {
    log_path: PathBuf,
    last_sequence: u64,
}

impl Follower {
    pub fn new(log_path: &Path) -> Self {
        Follower {
            log_path: log_path.to_path_buf(),
            last_sequence: 0,
        }
    }

    /// Poll for new changes.
    pub fn poll(&mut self) -> Result<Vec<ChangeRecord>> {
        let log = ReplicationLog::open(&self.log_path)?;
        let mut changes = Vec::new();
        for record in log.iter_from(self.last_sequence + 1)? {
            let record = record?;
            self.last_sequence = record.sequence;
            changes.push(record);
        }
        Ok(changes)
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_replication_log_append_and_read() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("repl.log");
        let mut log = ReplicationLog::open(&log_path).unwrap();

        let seq1 = log.append(WalOp::Insert, "doc1", b"{\"hello\":1}").unwrap();
        let seq2 = log.append(WalOp::Update, "doc1", b"{\"hello\":2}").unwrap();

        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
        assert_eq!(log.next_sequence(), 3);

        let records: Vec<_> = log.iter_from(1).unwrap().collect::<Result<Vec<_>>>().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sequence, 1);
        assert_eq!(records[1].sequence, 2);
    }

    #[test]
    fn test_change_feed_subscribe() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("feed.log");
        let mut feed = ChangeFeed::open(&log_path).unwrap();

        let rx = feed.subscribe();
        feed.publish(WalOp::Insert, "doc1", b"test").unwrap();

        let record = rx.recv().unwrap();
        assert_eq!(record.doc_id, "doc1");
        assert_eq!(record.op, WalOp::Insert);
    }

    #[test]
    fn test_follower_poll() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("repl.log");
        let mut log = ReplicationLog::open(&log_path).unwrap();
        log.append(WalOp::Insert, "doc1", b"data1").unwrap();
        log.append(WalOp::Delete, "doc1", b"").unwrap();

        let mut follower = Follower::new(&log_path);
        let changes = follower.poll().unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(follower.last_sequence(), 2);

        // Poll again — no new changes.
        let changes = follower.poll().unwrap();
        assert!(changes.is_empty());
    }
}
