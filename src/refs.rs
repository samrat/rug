use crate::lockfile::Lockfile;
use crate::util;
use regex::{Regex, RegexSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

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

#[derive(Debug, PartialEq, Eq)]
pub enum Ref {
    Ref { oid: String },
    SymRef { path: String },
}

impl Ref {
    pub fn is_head(&self) -> bool {
        match self {
            Ref::Ref { oid: _ } => false,
            Ref::SymRef { path } => path == "HEAD",
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
            None | Some(Ref::Ref { oid: _ }) => Self::write_lockfile(lock, &oid),
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
            Some(Ref::Ref { oid: _ }) | None => Ref::SymRef {
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
}
