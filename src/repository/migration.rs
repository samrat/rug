use crate::database::tree::TreeEntry;
use crate::repository::Repository;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub struct Migration<'a> {
    repo: &'a mut Repository,
    diff: HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)>,
    // changes: Vec<>,
    // mkdirs: HashSet<>,
    // rmdirs: HashSet<>
}

impl<'a> Migration<'a> {
    pub fn new(
        repo: &'a mut Repository,
        tree_diff: HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)>,
    ) -> Migration {
        Migration {
            repo,
            diff: tree_diff,
        }
    }
    pub fn apply_changes(&self) {}
}
