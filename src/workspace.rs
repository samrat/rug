use std::collections::HashMap;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::BufReader;
use std::path::{Path, PathBuf};

lazy_static! {
    static ref IGNORE_PATHS: Vec<&'static str> = {
        let v = vec![".git", "target"];
        v
    };
}

pub struct Workspace {
    path: PathBuf,
}

impl Workspace {
    pub fn new(path: &Path) -> Workspace {
        Workspace {
            path: path.to_path_buf(),
        }
    }

    pub fn abs_path(&self, rel_path: &str) -> PathBuf {
        self.path.join(rel_path)
    }

    pub fn is_dir(&self, rel_path: &str) -> bool {
        self.abs_path(rel_path).is_dir()
    }

    /// List contents of directory. Does NOT list contents of
    /// subdirectories
    pub fn list_dir(&self, dir: &Path) -> Result<HashMap<String, fs::Metadata>, std::io::Error> {
        let path = self.path.join(dir);

        let entries = fs::read_dir(&path)?
            .map(|f| f.unwrap().path())
            .filter(|f| !IGNORE_PATHS.contains(&f.file_name().unwrap().to_str().unwrap()));
        let mut stats = HashMap::new();

        for name in entries {
            let relative = self
                .path
                .join(&name)
                .strip_prefix(&self.path)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();

            let stat = self.stat_file(&relative).expect("stat file failed");
            stats.insert(relative, stat);
        }

        Ok(stats)
    }

    /// Return list of files in dir. Nested files are flattened
    /// strings eg. `a/b/c/inner.txt`
    pub fn list_files(&self, dir: &Path) -> Result<Vec<String>, std::io::Error> {
        if dir.is_file() {
            return Ok(vec![dir
                .strip_prefix(&self.path)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()]);
        }

        if IGNORE_PATHS.contains(&dir.file_name().unwrap().to_str().unwrap()) {
            return Ok(vec![]);
        }

        let mut files = vec![];
        for file in fs::read_dir(dir)? {
            let path = file?.path();
            files.extend_from_slice(&self.list_files(&path)?);
        }
        Ok(files)
    }

    // TODO: Should return bytes instead?
    pub fn read_file(&self, file_name: &str) -> Result<String, std::io::Error> {
        let file = File::open(self.path.as_path().join(file_name))?;
        let mut buf_reader = BufReader::new(file);
        let mut contents = String::new();

        buf_reader.read_to_string(&mut contents)?;
        Ok(contents)
    }

    pub fn stat_file(&self, file_name: &str) -> Result<fs::Metadata, std::io::Error> {
        fs::metadata(self.path.join(file_name))
    }
}
