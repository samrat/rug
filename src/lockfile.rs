use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::io::{self, ErrorKind};
use std::io::prelude::*;

#[derive(Debug)]
pub struct Lockfile {
    file_path: PathBuf,
    lock_path: PathBuf,
    pub lock: Option<File>,
}

impl Lockfile {
    pub fn new(path: &Path) -> Lockfile {
        Lockfile {
            file_path: path.to_path_buf(),
            lock_path: path.with_extension("lock").to_path_buf(),
            lock: None,
        }
    }

    pub fn hold_for_update(&mut self) -> Result<(), std::io::Error> {
        if self.lock.is_none() {
            let open_file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(self.lock_path.clone())?;
            
            self.lock = Some(open_file);
        }

        Ok(())
    }

    pub fn write(&mut self, contents: &str) -> Result<(), std::io::Error> {
        self.write_bytes(contents.as_bytes())
    }

    pub fn write_bytes(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.raise_on_stale_lock()?;

        let mut lock = self.lock.as_ref().unwrap();
        lock.write_all(data)?;

        Ok(())
    }


    pub fn commit(&mut self) -> Result<(), std::io::Error> {
        self.raise_on_stale_lock()?;
        self.lock = None;
        fs::rename(self.lock_path.clone(), self.file_path.clone())?;

        Ok(())
    }

    pub fn rollback(&mut self) -> Result<(), std::io::Error> {
        self.raise_on_stale_lock()?;
        fs::remove_file(self.lock_path.clone())?;
        self.lock = None;

        Ok(())
    }

    fn raise_on_stale_lock(&self) -> Result<(), std::io::Error> {
        if self.lock.is_none() {
            Err(io::Error::new(ErrorKind::Other,
                               format!("Not holding lock on file: {:?}", self.lock_path)))
        } else {
            Ok(())
        }
    }
}

impl Read for Lockfile {
    fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, io::Error> {
        self.raise_on_stale_lock()?;

        let mut lock = self.lock.as_ref().unwrap();
        lock.read(&mut buf)
    }
}

impl Write for Lockfile {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.raise_on_stale_lock()?;

        let mut lock = self.lock.as_ref().unwrap();
        lock.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        let mut lock = self.lock.as_ref().unwrap();
        lock.flush()
    }
}

impl<'a> Read for &'a Lockfile {
    fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, io::Error> {
        self.raise_on_stale_lock()?;

        let mut lock = self.lock.as_ref().unwrap();
        lock.read(&mut buf)
    }
}

impl<'a> Write for &'a Lockfile {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.raise_on_stale_lock()?;

        let mut lock = self.lock.as_ref().unwrap();
        lock.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        let mut lock = self.lock.as_ref().unwrap();
        lock.flush()
    }
}
