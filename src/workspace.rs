use crate::database::tree::{TreeEntry, TREE_MODE};
use crate::database::{Database, ParsedObject};
use crate::repository::migration::Action;
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::prelude::*;
use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
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

    pub fn apply_migration(
        &self,
        database: &mut Database,
        changes: &HashMap<Action, Vec<(PathBuf, Option<TreeEntry>)>>,
        rmdirs: &BTreeSet<PathBuf>,
        mkdirs: &BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        self.apply_change_list(database, changes, Action::Delete)
            .map_err(|e| e.to_string())?;
        for dir in rmdirs.iter().rev() {
            let dir_path = self.path.join(dir);
            self.remove_directory(&dir_path).unwrap_or(());
        }

        for dir in mkdirs.iter() {
            self.make_directory(dir).map_err(|e| e.to_string())?;
        }

        self.apply_change_list(database, changes, Action::Update)
            .map_err(|e| e.to_string())?;
        self.apply_change_list(database, changes, Action::Create)
            .map_err(|e| e.to_string())
    }

    fn apply_change_list(
        &self,
        database: &mut Database,
        changes: &HashMap<Action, Vec<(PathBuf, Option<TreeEntry>)>>,
        action: Action,
    ) -> std::io::Result<()> {
        let changes = changes.get(&action).unwrap().clone();
        for (filename, entry) in changes.clone() {
            let path = self.path.join(filename);
            Self::remove_file_or_dir(&path)?;

            if action == Action::Delete {
                continue;
            }

            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;

            let entry = entry
                .expect("entry missing for non-delete");

            if entry.mode() != TREE_MODE {
                let data = Self::blob_data(database, &entry.get_oid());
                file.write_all(&data)?;

                // Set mode
                let metadata = file.metadata()?;
                let mut permissions = metadata.permissions();
                permissions.set_mode(entry.mode());
                fs::set_permissions(path, permissions)?;
            }
        }

        Ok(())
    }

    pub fn blob_data(database: &mut Database, oid: &str) -> Vec<u8> {
        match database.load(oid) {
            ParsedObject::Blob(blob) => blob.data.clone(),
            _ => panic!("not a blob oid"),
        }
    }

    fn remove_file_or_dir(path: &Path) -> std::io::Result<()> {
        if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else if path.is_file() {
            std::fs::remove_file(path)
        } else {
            Ok(())
        }
    }

    fn remove_directory(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_dir(path)?;
        Ok(())
    }

    fn make_directory(&self, dirname: &Path) -> std::io::Result<()> {
        let path = self.path.join(dirname);

        if let Ok(stat) = self.stat_file(dirname.to_str().expect("conversion to str failed")) {
            if stat.is_file() {
                std::fs::remove_file(&path)?;
            }
            if !stat.is_dir() {
                std::fs::create_dir(&path)?;
            }
        } else {
            std::fs::create_dir(&path)?;
        }
        Ok(())
    }
}
