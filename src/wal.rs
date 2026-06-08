//! Write-Ahead Log (WAL) for ACID transactions.
//!
//! Each transaction appends a batch of records to the WAL file (`*.wal`).
//! On commit, a commit marker is written. On rollback / crash, uncommitted
//! records are ignored during recovery.

use crate::error::{CassetteError, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher as Crc32;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Types of WAL records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum WalOp {
    Insert = 1,
    Update = 2,
    Delete = 3,
}

/// A single WAL record.
#[derive(Debug, Clone, PartialEq)]
pub struct WalRecord {
    pub op: WalOp,
    pub doc_id: String,
    pub payload: Vec<u8>, // JSON bytes
}

/// WAL file header.
const WAL_MAGIC: &[u8; 4] = b"CWL1";
const WAL_VERSION: u16 = 1;

/// Write-ahead log.
pub struct Wal {
    file: File,
    path: std::path::PathBuf,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let meta = file.metadata()?;
        if meta.len() == 0 {
            // Initialize new WAL file.
            let mut writer = BufWriter::new(&file);
            writer.write_all(WAL_MAGIC)?;
            writer.write_u16::<LittleEndian>(WAL_VERSION)?;
            writer.flush()?;
        } else {
            // Validate header.
            let mut magic = [0u8; 4];
            file.read_exact(&mut magic)?;
            if &magic != WAL_MAGIC {
                return Err(CassetteError::Wal("Invalid WAL magic".into()));
            }
            let version = file.read_u16::<LittleEndian>()?;
            if version != WAL_VERSION {
                return Err(CassetteError::Wal(format!(
                    "Unsupported WAL version {}",
                    version
                )));
            }
        }

        Ok(Wal {
            file,
            path: path.to_path_buf(),
        })
    }

    /// Append a record to the WAL. Returns the file offset of the record.
    pub fn append(&mut self, op: WalOp, doc_id: &str, payload: &[u8]) -> Result<u64> {
        let offset = self.file.seek(SeekFrom::End(0))?;

        let mut hasher = Crc32::new();
        hasher.update(payload);
        let checksum = hasher.finalize();

        let mut writer = BufWriter::new(&self.file);
        writer.write_u8(op as u8)?;
        writer.write_u32::<LittleEndian>(doc_id.len() as u32)?;
        writer.write_all(doc_id.as_bytes())?;
        writer.write_u32::<LittleEndian>(payload.len() as u32)?;
        writer.write_all(payload)?;
        writer.write_u32::<LittleEndian>(checksum)?;
        writer.write_u8(0u8)?; // commit flag = 0 (uncommitted)
        writer.flush()?;

        Ok(offset)
    }

    /// Mark a previously written record (by offset) as committed.
    pub fn commit_record(&mut self, offset: u64) -> Result<()> {
        self.file.seek(SeekFrom::Start(offset))?;
        // Skip op, doc_id_len, doc_id, payload_len, payload, checksum
        let mut buf = [0u8; 1];
        self.file.read_exact(&mut buf)?; // op
        let id_len = self.file.read_u32::<LittleEndian>()? as u64;
        self.file.seek(SeekFrom::Current(id_len as i64))?;
        let payload_len = self.file.read_u32::<LittleEndian>()? as u64;
        self.file.seek(SeekFrom::Current(payload_len as i64))?;
        self.file.seek(SeekFrom::Current(4))?; // checksum
        self.file.write_all(&[1u8])?; // commit flag = 1
        self.file.flush()?;
        Ok(())
    }

    /// Iterate over committed records only.
    pub fn iter_committed(&mut self) -> Result<impl Iterator<Item = Result<WalRecord>> + '_> {
        self.file.seek(SeekFrom::Start(6))?; // after header
        let reader = BufReader::new(&self.file);
        Ok(WalIter { reader })
    }

    /// Truncate the WAL (e.g., after compaction).
    pub fn reset(&mut self) -> Result<()> {
        self.file.set_len(6)?;
        self.file.seek(SeekFrom::Start(6))?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

struct WalIter<R: Read> {
    reader: BufReader<R>,
}

impl<R: Read> Iterator for WalIter<R> {
    type Item = Result<WalRecord>;

    fn next(&mut self) -> Option<Result<WalRecord>> {
        let op = match self.reader.read_u8() {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
            Err(e) => return Some(Err(e.into())),
        };

        let id_len = match self.reader.read_u32::<LittleEndian>() {
            Ok(v) => v as usize,
            Err(e) => return Some(Err(e.into())),
        };
        let mut id_buf = vec![0u8; id_len];
        if let Err(e) = self.reader.read_exact(&mut id_buf) {
            return Some(Err(e.into()));
        }
        let doc_id = String::from_utf8_lossy(&id_buf).into_owned();

        let payload_len = match self.reader.read_u32::<LittleEndian>() {
            Ok(v) => v as usize,
            Err(e) => return Some(Err(e.into())),
        };
        let mut payload = vec![0u8; payload_len];
        if let Err(e) = self.reader.read_exact(&mut payload) {
            return Some(Err(e.into()));
        }

        let _checksum = match self.reader.read_u32::<LittleEndian>() {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        };

        let commit_flag = match self.reader.read_u8() {
            Ok(v) => v,
            Err(e) => return Some(Err(e.into())),
        };

        if commit_flag == 0 {
            // Skip uncommitted records.
            return self.next();
        }

        let op = match op {
            1 => WalOp::Insert,
            2 => WalOp::Update,
            3 => WalOp::Delete,
            _ => return Some(Err(CassetteError::Wal("Unknown op".into()))),
        };

        Some(Ok(WalRecord {
            op,
            doc_id,
            payload,
        }))
    }
}
