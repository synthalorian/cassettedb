//! Page-based storage engine for the `.cassette` main file.
//!
//! The file is divided into fixed-size pages (default 4 KiB).
//! Page 0 is the header; subsequent pages store document chunks.
//! A free-page list is maintained for reuse.

use crate::error::{CassetteError, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const PAGE_SIZE: usize = 4096;
pub const MAGIC: &[u8; 4] = b"CDB1";
pub const VERSION: u16 = 1;

/// Header stored in page 0.
#[derive(Debug, Clone)]
pub struct Header {
    pub magic: [u8; 4],
    pub version: u16,
    pub page_size: u16,
    pub num_pages: u32,
    pub free_list_head: u32, // 0 = none
    pub doc_count: u32,
}

impl Default for Header {
    fn default() -> Self {
        Self::new()
    }
}

impl Header {
    pub fn new() -> Self {
        Self {
            magic: *MAGIC,
            version: VERSION,
            page_size: PAGE_SIZE as u16,
            num_pages: 1,
            free_list_head: 0,
            doc_count: 0,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64);
        buf.write_all(&self.magic).expect("writing to Vec<u8> is infallible");
        buf.write_u16::<LittleEndian>(self.version).expect("writing to Vec<u8> is infallible");
        buf.write_u16::<LittleEndian>(self.page_size).expect("writing to Vec<u8> is infallible");
        buf.write_u32::<LittleEndian>(self.num_pages).expect("writing to Vec<u8> is infallible");
        buf.write_u32::<LittleEndian>(self.free_list_head).expect("writing to Vec<u8> is infallible");
        buf.write_u32::<LittleEndian>(self.doc_count).expect("writing to Vec<u8> is infallible");
        buf.resize(PAGE_SIZE, 0);
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 64 {
            return Err(CassetteError::Corrupt("Header too short".into()));
        }
        let mut r = bytes;
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(CassetteError::Corrupt("Invalid magic".into()));
        }
        let version = r.read_u16::<LittleEndian>()?;
        let page_size = r.read_u16::<LittleEndian>()?;
        let num_pages = r.read_u32::<LittleEndian>()?;
        let free_list_head = r.read_u32::<LittleEndian>()?;
        let doc_count = r.read_u32::<LittleEndian>()?;
        Ok(Self {
            magic,
            version,
            page_size,
            num_pages,
            free_list_head,
            doc_count,
        })
    }
}

/// Low-level page storage.
pub struct Storage {
    file: File,
    header: Header,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        let meta = file.metadata()?;
        let header = if meta.len() == 0 {
            let h = Header::new();
            file.write_all(&h.encode())?;
            file.sync_all()?;
            h
        } else {
            let mut buf = vec![0u8; PAGE_SIZE];
            file.read_exact(&mut buf)?;
            Header::decode(&buf)?
        };

        Ok(Storage { file, header })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn allocate_page(&mut self) -> Result<u32> {
        if self.header.free_list_head != 0 {
            let page_id = self.header.free_list_head;
            let offset = page_id as u64 * PAGE_SIZE as u64;
            self.file.seek(SeekFrom::Start(offset))?;
            let next = self.file.read_u32::<LittleEndian>()?;
            self.header.free_list_head = next;
            self.sync_header()?;
            return Ok(page_id);
        }
        let page_id = self.header.num_pages;
        self.header.num_pages += 1;
        self.sync_header()?;
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&vec![0u8; PAGE_SIZE])?;
        self.file.sync_all()?;
        Ok(page_id)
    }

    pub fn read_page(&mut self, page_id: u32) -> Result<Vec<u8>> {
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; PAGE_SIZE];
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn write_page(&mut self, page_id: u32, data: &[u8]) -> Result<()> {
        assert_eq!(data.len(), PAGE_SIZE);
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn free_page(&mut self, page_id: u32) -> Result<()> {
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file
            .write_u32::<LittleEndian>(self.header.free_list_head)?;
        self.header.free_list_head = page_id;
        self.sync_header()?;
        Ok(())
    }

    fn sync_header(&mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&self.header.encode())?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn increment_doc_count(&mut self, delta: i32) -> Result<()> {
        if delta >= 0 {
            self.header.doc_count = self.header.doc_count.saturating_add(delta as u32);
        } else {
            self.header.doc_count = self.header.doc_count.saturating_sub((-delta) as u32);
        }
        self.sync_header()
    }
}
