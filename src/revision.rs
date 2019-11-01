use regex::{Regex, RegexSet};
use std::collections::HashMap;

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

#[derive(Debug)]
pub enum Revision {
    Ref { name: String },
    Parent { rev: Box<Revision> },
    Ancestor { rev: Box<Revision>, n: i32 },
}

impl Revision {
    pub fn parse(revision: &str) -> Option<Revision> {
        if let Some(caps) = PARENT.captures(revision) {
            let rev = Self::parse(&caps[1]).expect("parsing parent rev failed");
            return Some(Revision::Parent { rev: Box::new(rev) });
        } else if let Some(caps) = ANCESTOR.captures(revision) {
            let rev = Self::parse(&caps[1]).expect("parsing ancestor rev failed");
            return Some(Revision::Ancestor {
                rev: Box::new(rev),
                n: i32::from_str_radix(&caps[2], 10).expect("could not parse ancestor number"),
            });
        } else if Self::is_valid_ref(revision) {
            let rev = REF_ALIASES.get(revision).unwrap_or(&revision);
            Some(Revision::Ref {
                name: rev.to_string(),
            })
        } else {
            None
        }
    }

    fn is_valid_ref(revision: &str) -> bool {
        INVALID_NAME.matches(revision).into_iter().count() == 0
    }
}
