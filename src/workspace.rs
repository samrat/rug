use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::io::{BufReader};
use std::io::prelude::*;

pub struct Workspace {
    path: PathBuf,
}

impl Workspace {
    pub fn new(path: &Path) -> Workspace {
        Workspace { path: path.to_path_buf() }
    }

    pub fn list_dir_files(&self, dir: &Path) -> Result<Vec<String>, std::io::Error> {
        if !File::open(&dir)?.metadata()?.is_dir() {
            return Ok(vec![dir.strip_prefix(self.path.clone())
                           .unwrap()
                           .to_str()
                           .unwrap()
                           .to_string()]);
        }

        let ignore_paths = [".git", "target"];
        if ignore_paths.contains(&dir.file_name().unwrap().to_str().unwrap()) {
            return Ok(vec![]);
        }
        
        let mut files = vec![];
        for file in fs::read_dir(dir)? {
            let path = file?.path();
            files.extend_from_slice(&self.list_dir_files(&path)?);
        }
        Ok(files)
    }

    pub fn list_files(&self) -> Result<Vec<String>, std::io::Error> {
        self.list_dir_files(&self.path)
    }

    pub fn read_file(&self, file_name: &str) -> Result<String, std::io::Error> {
        let file = File::open(self.path.as_path().join(file_name))?;
        let mut buf_reader = BufReader::new(file);
        let mut contents = String::new();
        
        buf_reader.read_to_string(&mut contents)?;
        Ok(contents)
    }

    pub fn file_mode(&self, file_name: &str) -> Result<u32, std::io::Error> {
        let file = File::open(self.path.join(file_name))?;
        Ok(file.metadata()?.permissions().mode())
    }

    pub fn stat_file(&self, file_name: &str) -> Result<fs::Metadata, std::io::Error> {
        let file = File::open(self.path.join(file_name))?;
        file.metadata()
    }
}
