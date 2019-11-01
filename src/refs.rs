use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::lockfile::Lockfile;

extern crate regex;
use regex::RegexSet;

lazy_static! {
    static ref INVALID_FILENAME: RegexSet = {
        RegexSet::new(&[
            r"^\.",
            r"/\.",
            r"\.\.",
            r"/$",
            r"\.lock$",
            r"@\{",
            r"[\x00-\x20*:?\[\\^~\x7f]",
        ])
        .unwrap()
    };
}

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
        (*self.pathname).join("HEAD")
    }

    fn heads_path(&self) -> PathBuf {
        (*self.pathname).join("refs/heads")
    }

    pub fn update_ref_file(&self, path: &Path, oid: &str) -> Result<(), std::io::Error> {
        let mut lock = Lockfile::new(path);
        lock.hold_for_update()?;
        lock.write(oid)?;
        lock.write("\n")?;
        lock.commit()
    }

    pub fn update_head(&self, oid: &str) -> Result<(), std::io::Error> {
        self.update_ref_file(&self.head_path(), oid)
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

    pub fn create_branch(&self, branch_name: &str) -> Result<(), String> {
        let path = self.heads_path().join(branch_name);

        if INVALID_FILENAME.matches(branch_name).into_iter().count() > 0 {
            panic!("{} is not a valid branch name. {:?}", branch_name,
            INVALID_FILENAME.matches(branch_name).into_iter().collect::<Vec<_>>());
            return Err(format!("{} is not a valid branch name.", branch_name));
        }

        if path.as_path().exists() {
            return Err(format!("A branch named {} already exists.", branch_name));
        }

        File::create(&path).expect("failed to create refs file for branch");
        self.update_ref_file(&path, &self.read_head().expect("empty HEAD"));

        Ok(())
    }
}
