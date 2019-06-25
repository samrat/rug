use crate::database::object::Object;
use crate::database::{Entry, ParsedObject};
use crate::util::*;

use std::collections::{BTreeMap};
use std::path::{Path};
use std::str;

pub const TREE_MODE: u32 = 0o40000;

#[derive(Clone, Debug)]
pub enum TreeEntry {
    Entry(Entry),
    Tree(Tree),
}

impl TreeEntry {
    pub fn mode(&self) -> u32 {
        match self {
            TreeEntry::Entry(e) => e.mode(),
            _ => TREE_MODE,
        }
    }

    pub fn get_oid(&self) -> String {
        match self {
            TreeEntry::Entry(e) => e.oid.clone(),
            TreeEntry::Tree(t) => t.get_oid(),
        }
    }

    pub fn is_tree(&self) -> bool {
        match self {
            TreeEntry::Entry(e) => e.mode() == TREE_MODE,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub entries: BTreeMap<String, TreeEntry>,
}

impl Tree {
    pub fn new() -> Tree {
        Tree {
            entries: BTreeMap::new(),
        }
    }

    pub fn build(entries: &[Entry]) -> Tree {
        let mut sorted_entries = entries.to_vec();
        sorted_entries.sort();

        let mut root = Tree::new();
        for entry in sorted_entries.iter() {
            let mut path: Vec<String> = Path::new(&entry.name)
                .iter()
                .map(|c| c.to_str().unwrap().to_string())
                .collect();
            let name = path.pop().expect("file path has zero components");
            root.add_entry(&path, name, entry.clone());
        }

        root
    }

    pub fn add_entry(&mut self, path: &[String], name: String, entry: Entry) {
        if path.is_empty() {
            self.entries.insert(name, TreeEntry::Entry(entry));
        } else if let Some(TreeEntry::Tree(tree)) = self.entries.get_mut(&path[0]) {
            tree.add_entry(&path[1..], name, entry);
        } else {
            let mut tree = Tree::new();
            tree.add_entry(&path[1..], name, entry);
            self.entries.insert(path[0].clone(), TreeEntry::Tree(tree));
        };
    }

    pub fn traverse<F>(&self, f: &F)
    where
        F: Fn(&Tree) -> (),
    {
        // Do a postorder traversal(visit all children first, then
        // process `self`
        for (_name, entry) in self.entries.clone() {
            if let TreeEntry::Tree(tree) = entry {
                tree.traverse(f);
            }
        }

        f(self);
    }
}

impl Object for Tree {
    fn r#type(&self) -> String {
        "tree".to_string()
    }

    fn to_string(&self) -> Vec<u8> {
        let mut tree_vec = Vec::new();
        for (name, entry) in self.entries.iter() {
            let mut entry_vec: Vec<u8> =
                format!("{:o} {}\0", entry.mode(), name).as_bytes().to_vec();
            entry_vec.extend_from_slice(&decode_hex(&entry.get_oid()).expect("invalid oid"));
            tree_vec.extend_from_slice(&entry_vec);
        }
        tree_vec
    }

    fn parse(v: &[u8]) -> ParsedObject {
        let mut entries: Vec<Entry> = vec![];

        let mut vs = v;

        while vs.len() > 0 {
            let (mode, rest): (u32, &[u8]) = match vs
                .splitn(2, |c| *c as char == ' ')
                .collect::<Vec<&[u8]>>()
                .as_slice()
            {
                &[mode, rest] => (
                    u32::from_str_radix(str::from_utf8(mode).expect("invalid utf8"), 8)
                        .expect("parsing mode failed"),
                    rest,
                ),
                _ => panic!("EOF while parsing mode"),
            };
            vs = rest;

            let (name, rest) = match vs
                .splitn(2, |c| *c as char == '\u{0}')
                .collect::<Vec<&[u8]>>()
                .as_slice()
            {
                &[name_bytes, rest] => (str::from_utf8(name_bytes).expect("invalid utf8"), rest),
                _ => panic!("EOF while parsing name"),
            };
            vs = rest;

            let (oid_bytes, rest) = vs.split_at(20);
            vs = rest;

            let oid = encode_hex(&oid_bytes);

            entries.push(Entry::new(name, &oid, mode));
        }
        ParsedObject::Tree(Tree::build(&entries))
    }
}
