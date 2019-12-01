use crate::lockfile::Lockfile;
use crate::util;
use regex::{Regex, RegexSet};
use std::fs::{self, DirEntry, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::cmp::{Ord, Ordering};

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
    static ref SYMREF: Regex = Regex::new(r"^ref: (.+)$").unwrap();
}

#[derive(Debug, PartialEq, Eq, PartialOrd)]
pub enum Ref {
    Ref { oid: String },
    SymRef { path: String },
}

impl Ref {
    pub fn is_head(&self) -> bool {
        match self {
            Ref::Ref { .. } => false,
            Ref::SymRef { path } => path == "HEAD",
        }
    }
}

impl Ord for Ref {
    fn cmp(&self, other: &Ref) -> Ordering {
        match (self, other) {
            (Ref::Ref { .. }, Ref::SymRef { ..} ) => Ordering::Less,
            (Ref::SymRef { .. }, Ref::Ref { ..} ) => Ordering::Greater,
            (Ref::SymRef { path: a }, Ref::SymRef { path: b } ) => a.cmp(b),
            (Ref::Ref { oid: a }, Ref::Ref { oid: b } ) => a.cmp(b),
        }
    }
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

    fn refs_path(&self) -> PathBuf {
        (*self.pathname).join("refs")
    }

    fn heads_path(&self) -> PathBuf {
        (*self.pathname).join("refs/heads")
    }

    pub fn update_ref_file(&self, path: &Path, oid: &str) -> Result<(), std::io::Error> {
        let mut lock = Lockfile::new(path);
        lock.hold_for_update()?;
        Self::write_lockfile(lock, &oid)
    }

    pub fn update_head(&self, oid: &str) -> Result<(), std::io::Error> {
        self.update_symref(&self.head_path(), oid)
    }

    pub fn set_head(&self, revision: &str, oid: &str) -> Result<(), std::io::Error> {
        let path = self.heads_path().join(revision);

        if path.exists() {
            let relative = util::relative_path_from(Path::new(&path), &self.pathname);
            self.update_ref_file(&self.head_path(), &format!("ref: {}", relative))
        } else {
            self.update_ref_file(&self.head_path(), oid)
        }
    }

    pub fn read_head(&self) -> Option<String> {
        self.read_symref(&self.head_path())
    }

    fn path_for_name(&self, name: &str) -> Option<PathBuf> {
        let prefixes = [self.pathname.clone(), self.refs_path(), self.heads_path()];
        for prefix in &prefixes {
            if prefix.join(name).exists() {
                return Some(prefix.join(name));
            }
        }
        None
    }

    pub fn read_ref(&self, name: &str) -> Option<String> {
        if let Some(path) = self.path_for_name(name) {
            self.read_symref(&path)
        } else {
            None
        }
    }

    /// Folows chain of references to resolve to an object ID
    pub fn read_oid(&self, r#ref: &Ref) -> Option<String> {
        match r#ref {
            Ref::Ref { oid } => Some(oid.to_string()),
            Ref::SymRef { path } => self.read_ref(&path),
        }
    }

    pub fn read_oid_or_symref(path: &Path) -> Option<Ref> {
        if path.exists() {
            let mut file = File::open(path).unwrap();
            let mut contents = String::new();
            file.read_to_string(&mut contents).unwrap();

            if let Some(caps) = SYMREF.captures(&contents.trim()) {
                Some(Ref::SymRef {
                    path: caps[1].to_string(),
                })
            } else {
                Some(Ref::Ref {
                    oid: contents.trim().to_string(),
                })
            }
        } else {
            None
        }
    }

    pub fn read_symref(&self, path: &Path) -> Option<String> {
        let r#ref = Self::read_oid_or_symref(path);

        match r#ref {
            Some(Ref::SymRef { path }) => self.read_symref(&self.pathname.join(&path)),
            Some(Ref::Ref { oid }) => Some(oid),
            None => None,
        }
    }

    pub fn update_symref(&self, path: &Path, oid: &str) -> Result<(), std::io::Error> {
        let mut lock = Lockfile::new(path);
        lock.hold_for_update()?;

        let r#ref = Self::read_oid_or_symref(path);
        match r#ref {
            None | Some(Ref::Ref { .. }) => Self::write_lockfile(lock, &oid),
            Some(Ref::SymRef { path }) => self.update_symref(&self.pathname.join(path), oid),
        }
    }

    fn write_lockfile(mut lock: Lockfile, oid: &str) -> Result<(), io::Error> {
        lock.write(&oid)?;
        lock.write("\n")?;
        lock.commit()
    }

    pub fn current_ref(&self, source: &str) -> Ref {
        let r#ref = Self::read_oid_or_symref(&self.pathname.join(source));

        match r#ref {
            Some(Ref::SymRef { path }) => self.current_ref(&path),
            Some(Ref::Ref { .. }) | None => Ref::SymRef {
                path: source.to_string(),
            },
        }
    }

    pub fn create_branch(&self, branch_name: &str, start_oid: &str) -> Result<(), String> {
        let path = self.heads_path().join(branch_name);

        if INVALID_FILENAME.matches(branch_name).into_iter().count() > 0 {
            return Err(format!("{} is not a valid branch name.\n", branch_name));
        }

        if path.as_path().exists() {
            return Err(format!("A branch named {} already exists.\n", branch_name));
        }

        File::create(&path).expect("failed to create refs file for branch");
        self.update_ref_file(&path, start_oid)
            .map_err(|e| e.to_string())
    }

    pub fn list_branches(&self) -> Vec<Ref> {
        self.list_refs(&self.heads_path())
    }

    fn name_to_symref(&self, name: DirEntry) -> Vec<Ref> {
        let path = name.path();
        if path.is_dir() {
            self.list_refs(&path)
        } else {
            let path = util::relative_path_from(&path, &self.pathname);
            vec![Ref::SymRef { path }]
        }
    }

    fn list_refs(&self, dirname: &Path) -> Vec<Ref> {
        fs::read_dir(self.pathname.join(dirname))
            .expect("failed to read dir")
            .flat_map(|name| self.name_to_symref(name.unwrap()))
            .collect()
    }

    pub fn ref_short_name(&self, r#ref: &Ref) -> String {
        match r#ref {
            Ref::Ref { oid: _ } => unimplemented!(),
            Ref::SymRef { path } => {
                let path = self.pathname.join(path);

                let dirs = [self.heads_path(), self.pathname.clone()];
                let prefix = dirs.iter().find(|dir| {
                    path.parent()
                        .expect("failed to get parent")
                        .ancestors()
                        .any(|parent| &parent == dir)
                });

                let prefix = prefix.expect("could not find prefix");
                util::relative_path_from(&path, prefix)
            }
        }
    }

    pub fn delete_branch(&self, branch_name: &str) -> Result<String, String> {
        let path = self.heads_path().join(branch_name);

        let mut lockfile = Lockfile::new(&path);
        lockfile.hold_for_update().map_err(|e| e.to_string())?;

        if let Some(oid) = self.read_symref(&path) {
            fs::remove_file(path).map_err(|e| e.to_string())?;
            // To remove the .lock file
            lockfile.rollback().map_err(|e| e.to_string())?;
            Ok(oid)
        } else {
            return Err(format!("branch {} not found", branch_name));
        }
    }
}
