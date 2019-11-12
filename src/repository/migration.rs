use crate::database::tree::TreeEntry;
use crate::database::ParsedObject;
use crate::repository::Repository;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub struct Migration<'a> {
    repo: &'a mut Repository,
    diff: HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)>,
    pub changes: HashMap<Action, Vec<(PathBuf, Option<TreeEntry>)>>,
    pub mkdirs: BTreeSet<PathBuf>,
    pub rmdirs: BTreeSet<PathBuf>,
}

#[derive(Hash, PartialEq, Eq, Debug)]
pub enum Action {
    Create,
    Delete,
    Update,
}

impl<'a> Migration<'a> {
    pub fn new(
        repo: &'a mut Repository,
        tree_diff: HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)>,
    ) -> Migration<'a> {
        // TODO: can be a struct instead(?)
        let mut changes = HashMap::new();
        changes.insert(Action::Create, vec![]);
        changes.insert(Action::Delete, vec![]);
        changes.insert(Action::Update, vec![]);

        Migration {
            repo,
            diff: tree_diff,
            changes,
            mkdirs: BTreeSet::new(),
            rmdirs: BTreeSet::new(),
        }
    }
    pub fn apply_changes(&mut self) {
        self.plan_changes();
        self.update_workspace();
        self.update_index();
    }

    fn plan_changes(&mut self) {
        for (path, (old_item, new_item)) in self.diff.clone() {
            self.record_change(&path, old_item, new_item);
        }
    }

    fn record_change(
        &mut self,
        path: &Path,
        old_item: Option<TreeEntry>,
        new_item: Option<TreeEntry>,
    ) {
        let path_ancestors: BTreeSet<_> = path
            .parent()
            .expect("could not find parent")
            .ancestors()
            .map(|p| p.to_path_buf())
            .collect();

        let action = if old_item.is_none() {
            self.mkdirs = self.mkdirs.union(&path_ancestors).cloned().collect();
            Action::Create
        } else if new_item.is_none() {
            self.rmdirs = self.rmdirs.union(&path_ancestors).cloned().collect();
            Action::Delete
        } else {
            self.mkdirs = self.mkdirs.union(&path_ancestors).cloned().collect();
            Action::Update
        };

        if let Some(action_changes) = self.changes.get_mut(&action) {
            action_changes.push((path.to_path_buf(), new_item));
        }
    }

    fn update_workspace(&mut self) {
        self.repo.workspace.apply_migration(
            &mut self.repo.database,
            &self.changes,
            &self.rmdirs,
            &self.mkdirs,
        );
    }

    fn update_index(&mut self) {
        for (path, _) in self.changes.get(&Action::Delete).unwrap() {
            self.repo.index.remove(path.to_str().expect("failed to convert path to str"));
        }

        for action in &[Action::Create, Action::Update] {
            for (path, entry) in self.changes.get(action).unwrap() {
                let path = path.to_str().expect("failed to convert path to str");
                let entry_oid = entry.clone().unwrap().get_oid();
                let stat = self
                    .repo
                    .workspace
                    .stat_file(path)
                    .expect("failed to stat file");
                self.repo
                    .index
                    .add(path, &entry_oid, &stat);
            }
        }
    }
}
