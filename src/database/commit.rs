use chrono::prelude::*;
use std::collections::HashMap;
use std::str;

use crate::database::{Object, ParsedObject};

#[derive(Debug, Clone)]
pub struct Author {
    pub name: String,
    pub email: String,
    pub time: DateTime<FixedOffset>,
}

impl Author {
    fn to_string(&self) -> String {
        format!(
            "{} <{}> {}",
            self.name,
            self.email,
            self.time.format("%s %z")
        )
    }

    pub fn short_date(&self) -> String {
        self.time.format("%Y-%m-%d").to_string()
    }

    pub fn readable_time(&self) -> String {
        self.time.format("%a %b %-d  %H:%M:%S %Y %Z").to_string()
    }

    pub fn parse(s: &str) -> Author {
        let split_author_str = s
            .split(&['<', '>'][..])
            .map(|s| s.trim())
            .collect::<Vec<_>>();

        let name = split_author_str[0].to_string();
        let email = split_author_str[1].to_string();
        let time = DateTime::parse_from_str(split_author_str[2], "%s %z")
            .expect("could not parse datetime");

        Author { name, email, time }
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

    pub fn title_line(&self) -> String {
        self.message
            .lines()
            .next()
            .expect("could not get first line of commit")
            .to_string()
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
            Author::parse(headers.get("author").expect("no author found in commit")),
            s.to_string(),
        ))
    }
}
