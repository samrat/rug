use crate::database::tree::TreeEntry;
use crate::database::{Database, ParsedObject, Tree};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub struct TreeDiff<'a> {
    database: &'a mut Database,
    pub changes: HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)>,
}

impl<'a> TreeDiff<'a> {
    pub fn new(database: &mut Database) -> TreeDiff {
        TreeDiff {
            database,
            changes: HashMap::new(),
        }
    }

    pub fn compare_oids(&mut self, a: Option<String>, b: Option<String>, prefix: &Path) {
        if a == b {
            return;
        }

        let a_entries = if let Some(a_oid) = a {
            self.oid_to_tree(&a_oid).entries
        } else {
            BTreeMap::new()
        };

        let b_entries = if let Some(b_oid) = b {
            self.oid_to_tree(&b_oid).entries
        } else {
            BTreeMap::new()
        };

        self.detect_deletions(&a_entries, &b_entries, prefix);
        self.detect_additions(&a_entries, &b_entries, prefix);
    }

    fn detect_deletions(
        &mut self,
        a_entries: &BTreeMap<String, TreeEntry>,
        b_entries: &BTreeMap<String, TreeEntry>,
        prefix: &Path,
    ) {
        for (name, entry) in a_entries {
            let path = prefix.join(name);
            let other = b_entries.get(name);

            let tree_b = if let Some(b_entry) = other {
                if b_entry == entry {
                    continue;
                }

                if b_entry.is_tree() {
                    Some(b_entry.get_oid())
                } else {
                    None
                }
            } else {
                None
            };

            let tree_a = if entry.is_tree() {
                Some(entry.get_oid())
            } else {
                None
            };

            self.compare_oids(tree_a, tree_b, &path);

            let blobs = match (!entry.is_tree(), other.map(|e| !e.is_tree()).unwrap_or(false)) {
                (true, true) => (Some(entry.clone()), other.cloned()),
                (true, false) => (Some(entry.clone()), None),
                (false, true) => (None, other.cloned()),
                (false, false) => continue,
            };
            self.changes.insert(path, blobs);
        }
    }

    fn detect_additions(
        &mut self,
        a_entries: &BTreeMap<String, TreeEntry>,
        b_entries: &BTreeMap<String, TreeEntry>,
        prefix: &Path,
    ) {
        for (name, entry) in b_entries {
            let path = prefix.join(name);
            let other = a_entries.get(name);

            if other.is_some() {
                continue;
            }

            if entry.is_tree() {
                self.compare_oids(None, Some(entry.get_oid()), &path);
            } else {
                self.changes.insert(path, (None, Some(entry.clone())));
            }
        }
    }

    fn oid_to_tree(&mut self, oid: &str) -> Tree {
        let tree_oid = match self.database.load(oid) {
            ParsedObject::Tree(tree) => return tree.clone(),
            ParsedObject::Commit(commit) => commit.tree_oid.clone(),
            _ => panic!("oid not a commit or tree"),
        };

        match self.database.load(&tree_oid) {
            ParsedObject::Tree(tree) => tree.clone(),
            _ => panic!("oid not a tree"),
        }
    }
}
