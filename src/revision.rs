use crate::database::{commit, Database, ParsedObject};
use crate::repository::Repository;
use regex::{Regex, RegexSet};
use std::collections::HashMap;
use std::fmt;

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
pub struct HintedError {
    pub message: String,
    pub hint: Vec<String>,
}

impl fmt::Display for HintedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.message)?;
        for line in &self.hint {
            writeln!(f, "hint: {}", line)?;
        }

        Ok(())
    }
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
    errors: Vec<HintedError>,
}

impl<'a> Revision<'a> {
    pub fn new(repo: &'a mut Repository, expr: &str) -> Revision<'a> {
        Revision {
            repo,
            expr: expr.to_string(),
            query: Self::parse(expr).expect("Revision parse failed"),
            errors: vec![],
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

    pub fn resolve(&mut self) -> Result<String, Vec<HintedError>> {
        match self.resolve_query(self.query.clone()) {
            Some(revision) => {
                if self.load_commit(&revision).is_some() {
                    Ok(revision)
                } else {
                    Err(self.errors.clone())
                }
            }
            None => Err(self.errors.clone()),
        }
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

    fn read_ref(&mut self, name: &str) -> Option<String> {
        let symref = self.repo.refs.read_ref(name);
        if symref.is_some() {
            symref
        } else {
            let candidates = self.repo.database.prefix_match(name);
            if candidates.len() == 1 {
                Some(candidates[0].to_string())
            } else {
                if candidates.len() > 1 {
                    self.log_ambiguous_sha1(name, candidates);
                }
                None
            }
        }
    }

    fn log_ambiguous_sha1(&mut self, name: &str, mut candidates: Vec<String>) {
        candidates.sort();
        let message = format!("short SHA1 {} is ambiguous", name);
        let mut hint = vec!["The candidates are:".to_string()];

        for oid in candidates {
            let object = self.repo.database.load(&oid);
            let long_oid = object.get_oid();
            let short = Database::short_oid(&long_oid);
            let info = format!(" {} {}", short, object.obj_type());

            let obj_message = if let ParsedObject::Commit(commit) = object {
                format!(
                    "{} {} - {}",
                    info,
                    commit.author.short_date(),
                    commit.title_line()
                )
            } else {
                info
            };
            hint.push(obj_message);
        }
        self.errors.push(HintedError { message, hint });
    }

    fn commit_parent(&mut self, oid: &str) -> Option<String> {
        match self.load_commit(oid) {
            Some(commit) => commit.parent.clone(),
            None => None,
        }
    }

    fn load_commit(&mut self, oid: &str) -> Option<&commit::Commit> {
        match self.repo.database.load(oid) {
            ParsedObject::Commit(commit) => Some(commit),
            object => {
                let message = format!("object {} is a {}, not a commit", oid, object.obj_type());
                self.errors.push(HintedError {
                    message,
                    hint: vec![],
                });
                None
            }
        }
    }
}
