use crate::database::ParsedObject;
use crate::repository::Repository;
use regex::{Regex, RegexSet};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

lazy_static! {
    static ref INVALID_NAME: RegexSet = {
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
    static ref PARENT: Regex = { Regex::new(r"^(.+)\^$").unwrap() };
    static ref ANCESTOR: Regex = { Regex::new(r"^(.+)~(\d+)$").unwrap() };
    static ref REF_ALIASES: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("@", "HEAD");
        m
    };
}

#[derive(Debug, Clone)]
pub enum Rev {
    Ref { name: String },
    Parent { rev: Box<Rev> },
    Ancestor { rev: Box<Rev>, n: i32 },
}

pub struct Revision<'a> {
    repo: &'a mut Repository,
    query: Rev,
    expr: String,
}

impl<'a> Revision<'a> {
    pub fn new(repo: &'a mut Repository, expr: &str) -> Revision<'a> {
        Revision {
            repo,
            expr: expr.to_string(),
            query: Self::parse(expr).expect("Revision parse failed"),
        }
    }

    pub fn parse(revision: &str) -> Option<Rev> {
        if let Some(caps) = PARENT.captures(revision) {
            let rev = Revision::parse(&caps[1]).expect("parsing parent rev failed");
            return Some(Rev::Parent { rev: Box::new(rev) });
        } else if let Some(caps) = ANCESTOR.captures(revision) {
            let rev = Revision::parse(&caps[1]).expect("parsing ancestor rev failed");
            return Some(Rev::Ancestor {
                rev: Box::new(rev),
                n: i32::from_str_radix(&caps[2], 10).expect("could not parse ancestor number"),
            });
        } else if Revision::is_valid_ref(revision) {
            let rev = REF_ALIASES.get(revision).unwrap_or(&revision);
            Some(Rev::Ref {
                name: rev.to_string(),
            })
        } else {
            None
        }
    }

    fn is_valid_ref(revision: &str) -> bool {
        INVALID_NAME.matches(revision).into_iter().count() == 0
    }

    pub fn resolve(&mut self) -> Option<String> {
        self.resolve_query(self.query.clone())
    }

    /// Resolve Revision to commit object ID.
    pub fn resolve_query(&mut self, query: Rev) -> Option<String> {
        match query {
            Rev::Ref { name } => self.read_ref(&name),
            Rev::Parent { rev } => {
                let oid = self.resolve_query(*rev).expect("Invalid parent rev");
                self.commit_parent(&oid)
            }
            Rev::Ancestor { rev, n } => {
                let mut oid = self.resolve_query(*rev).expect("Invalid ancestor rev");
                for _ in 0..n {
                    if let Some(parent_oid) = self.commit_parent(&oid) {
                        oid = parent_oid
                    } else {
                        break;
                    }
                }
                Some(oid)
            }
        }
    }

    fn read_ref(&self, name: &str) -> Option<String> {
        if let Some(path) = self.path_for_name(name) {
            Some(Self::read_ref_file(&path))
        } else {
            None
        }
    }

    fn path_for_name(&self, name: &str) -> Option<PathBuf> {
        let git_path = self.repo.root_path.join(".git");
        let refs_path = git_path.join("refs");
        let heads_path = git_path.join("heads");

        let prefixes = [git_path, refs_path, heads_path];
        for prefix in &prefixes {
            if prefix.join(name).exists() {
                return Some(prefix.join(name));
            }
        }
        None
    }

    fn read_ref_file(path: &Path) -> String {
        let mut ref_file = File::open(&path).expect("failed to open ref file");
        let mut contents = String::new();
        ref_file.read_to_string(&mut contents).unwrap();
        contents.trim().to_string()
    }

    fn commit_parent(&mut self, oid: &str) -> Option<String> {
        match self.repo.database.load(oid) {
            ParsedObject::Commit(commit) => commit.parent.clone(),
            _ => None,
        }
    }
}
