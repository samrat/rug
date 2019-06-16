use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::lockfile::Lockfile;

pub struct Refs {
    pathname: PathBuf,
}

impl Refs {
    pub fn new(pathname: &Path) -> Refs {
        Refs {
            pathname: pathname.to_path_buf(),
        }
    }

    fn head_path(&self) -> PathBuf {
        self.pathname.as_path().join("HEAD").to_path_buf()
    }

    pub fn update_head(&self, oid: &str) -> Result<(), std::io::Error> {
        let mut lock = Lockfile::new(&self.head_path());
        lock.hold_for_update()?;
        lock.write(oid)?;
        lock.write("\n")?;
        lock.commit()
    }

    // NOTE: Jumping a bit ahead of the book so that we can have a
    // `master` branch
    pub fn update_master_ref(&self, oid: &str) -> Result<(), std::io::Error> {
        let master_ref_path = self.pathname.as_path().join("refs/heads/master");
        fs::create_dir_all(master_ref_path.parent().unwrap())?;

        let mut lock = Lockfile::new(&master_ref_path);
        lock.hold_for_update()?;
        lock.write(oid)?;
        lock.write("\n")?;
        lock.commit()
    }

    pub fn read_head(&self) -> Option<String> {
        if self.head_path().as_path().exists() {
            let mut head_file = File::open(self.head_path()).unwrap();
            let mut contents = String::new();
            head_file.read_to_string(&mut contents).unwrap();
            Some(contents.trim().to_string())
        } else {
            None
        }
    }
}
