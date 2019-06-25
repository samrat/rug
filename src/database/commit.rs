use chrono::Utc;
use std::collections::HashMap;
use std::str;

use crate::database::{Object, ParsedObject};

#[derive(Debug, Clone)]
pub struct Author {
    pub name: String,
    pub email: String,
}

impl Author {
    fn to_string(&self) -> String {
        format!(
            "{} <{}> {}",
            self.name,
            self.email,
            Utc::now().format("%s %z")
        )
    }
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub parent: Option<String>,
    pub tree_oid: String,
    pub author: Author,
    pub message: String,
}

impl Commit {
    pub fn new(
        parent: &Option<String>,
        tree_oid: String,
        author: Author,
        message: String,
    ) -> Commit {
        Commit {
            parent: parent.clone(),
            tree_oid,
            author,
            message,
        }
    }
}

impl Object for Commit {
    fn r#type(&self) -> String {
        "commit".to_string()
    }

    fn to_string(&self) -> Vec<u8> {
        let author_str = self.author.to_string();
        let mut lines = String::new();
        lines.push_str(&format!("tree {}\n", self.tree_oid));
        if let Some(parent_oid) = &self.parent {
            lines.push_str(&format!("parent {}\n", parent_oid));
        }
        lines.push_str(&format!("author {}\n", author_str));
        lines.push_str(&format!("committer {}\n", author_str));
        lines.push_str("\n");
        lines.push_str(&self.message);

        lines.as_bytes().to_vec()
    }

    fn parse(s: &[u8]) -> ParsedObject {
        let mut s = str::from_utf8(s).expect("invalid utf-8");
        let mut headers = HashMap::new();
        // Parse headers
        loop {
            if let Some(newline) = s.find('\n') {
                let line = &s[..newline];
                s = &s[newline + 1..];

                // Headers and commit message is separated by empty
                // line
                if line == "" {
                    break;
                }

                let v: Vec<&str> = line.splitn(2, ' ').collect();
                headers.insert(v[0], v[1]);
            } else {
                panic!("no body in commit");
            }
        }

        ParsedObject::Commit(Commit::new(
            &headers.get("parent").map(|s| s.to_string()),
            headers.get("tree").expect("no tree header").to_string(),
            Author {
                name: "TODO".to_string(),
                email: "TODO".to_string(),
            },
            s.to_string(),
        ))
    }
}
