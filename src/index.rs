use std::path::{Path, PathBuf};
use std::fs::{self};
use std::os::unix::fs::MetadataExt;
use std::cmp;
use std::collections::BTreeMap;
use crypto::digest::Digest;
use crypto::sha1::Sha1;

use crate::lockfile::Lockfile;
use crate::util::*;

const MAX_PATH_SIZE: u16 = 0xfff;

#[derive(Debug, Clone)]
pub struct Entry {
    ctime: i64,
    ctime_nsec: i64,
    mtime: i64,
    mtime_nsec: i64,
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    oid: String,
    flags: u16,
    path: String,
}

impl Entry {
    fn is_executable(mode: u32) -> bool {
        (mode >> 6) & 0b1 == 1
    }

    fn mode(mode: u32) -> u32 {
        if Entry::is_executable(mode) {
            0o100755u32
        } else {
            0o100644u32
        }
    }
    
    fn new(pathname: &str, oid: &str, metadata: fs::Metadata) -> Entry {
        let path = pathname.to_string();
        Entry {
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            mode: Entry::mode(metadata.mode()),
            uid: metadata.uid(),
            gid: metadata.gid(),
            size: metadata.size(),
            oid: oid.to_string(),
            flags: cmp::min(path.len() as u16, MAX_PATH_SIZE),
            path: path,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // 10 32-bit integers
        bytes.extend_from_slice(&(self.ctime as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.ctime_nsec as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.mtime as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.mtime_nsec as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.dev as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.ino as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.mode as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.uid as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.gid as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.size as u32).to_be_bytes());

        // 20 bytes (40-char hex-string)
        bytes.extend_from_slice(&decode_hex(&self.oid).expect("invalid oid"));

        // 16-bit
        bytes.extend_from_slice(&self.flags.to_be_bytes());

        bytes.extend_from_slice(self.path.as_bytes());
        bytes.push(0x0);

        // TODO: add padding
        while bytes.len() % 8 != 0 {
            bytes.push(0x0)
        }

        bytes
    }
}

pub struct Index {
    entries: BTreeMap<String, Entry>,
    lockfile: Lockfile,
    hasher: Option<Sha1>,
}

impl Index {
    pub fn new(path: &Path) -> Index {
        Index { entries: BTreeMap::new(),
                lockfile: Lockfile::new(path),
                hasher: None,}
    }

    pub fn begin_write(&mut self) {
        self.hasher = Some(Sha1::new());
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.lockfile.write_bytes(data)?;
        self.hasher.expect("Sha1 hasher not initialized").input(data);

        Ok(())
    }

    pub fn finish_write(&mut self) -> Result<(), std::io::Error> {
        let hash = self.hasher
            .expect("Sha1 hasher not initialized")
            .result_str();
        self.lockfile.write_bytes(&decode_hex(&hash).expect("invalid sha1"))?;
        self.lockfile.commit()?;

        Ok(())
    }

    pub fn write_updates(&mut self) -> Result<(), std::io::Error> {
        self.lockfile.hold_for_update();

        let mut header_bytes : Vec<u8> = vec![];
        header_bytes.extend_from_slice("DIRC".as_bytes());
        header_bytes.extend_from_slice(&2u32.to_be_bytes()); // version no.
        header_bytes.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        self.begin_write();
        self.write(&header_bytes);
        for (_key, entry) in self.entries.clone().iter() {
            self.write(&entry.to_bytes());
        }
        self.finish_write();
        Ok(())
    }

    pub fn add(&mut self, pathname: &str, oid: &str, metadata: fs::Metadata) {
        let entry = Entry::new(pathname, oid, metadata);
        self.entries.insert(pathname.to_string(), entry);
    }
}
