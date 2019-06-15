use chrono::Utc;

use crate::database::Object;

pub struct Author {
    pub name: String,
    pub email: String,
}

impl Author {
    fn to_string(&self) -> String {
        format!("{} <{}> {}",
                self.name,
                self.email,
                Utc::now().format("%s %z"))
    }
}

pub struct Commit {
    pub parent: Option<String>,
    pub tree_oid: String,
    pub author: Author,
    pub message: String,
}

impl Commit {
    pub fn new(parent: &Option<String>, tree_oid: String, author: Author, message: String) -> Commit {
        Commit { parent: parent.clone(), tree_oid, author, message}
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
}
