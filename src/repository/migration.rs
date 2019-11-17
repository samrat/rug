use crate::database::tree::TreeEntry;
use crate::index::Entry;
use crate::repository::{ChangeType, Repository};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

lazy_static! {
    static ref MESSAGES: HashMap<ConflictType, (&'static str, &'static str)> = {
        let mut m = HashMap::new();
        m.insert(
            ConflictType::StaleFile,
            (
                "Your local changes to the following files would be overwritten by checkout:",
                "Please commit your changes to stash them before you switch branches",
            ),
        );
        m.insert(
            ConflictType::StaleDirectory,
            (
                "Updating the following directories would lose untracekdd files in them:",
                "\n",
            ),
        );
        m.insert(
            ConflictType::UntrackedOverwritten,
            (
                "The following untracked working tree files would be overwritten by checkout:",
                "Please move or remove them before you switch branches",
            ),
        );
        m.insert(
            ConflictType::UntrackedRemoved,
            (
                "The following untracked working tree files would be removed by checkout:",
                "Please commit your changes to stash them before you switch branches",
            ),
        );
        m
    };
}

pub struct Migration<'a> {
    repo: &'a mut Repository,
    diff: HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)>,
    pub changes: HashMap<Action, Vec<(PathBuf, Option<TreeEntry>)>>,
    pub mkdirs: BTreeSet<PathBuf>,
    pub rmdirs: BTreeSet<PathBuf>,
    pub errors: Vec<String>,
    pub conflicts: HashMap<ConflictType, HashSet<PathBuf>>,
}

#[derive(Hash, PartialEq, Eq)]
pub enum ConflictType {
    StaleFile,
    StaleDirectory,
    UntrackedOverwritten,
    UntrackedRemoved,
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

        let conflicts = {
            let mut m = HashMap::new();
            m.insert(ConflictType::StaleFile, HashSet::new());
            m.insert(ConflictType::StaleDirectory, HashSet::new());
            m.insert(ConflictType::UntrackedOverwritten, HashSet::new());
            m.insert(ConflictType::UntrackedRemoved, HashSet::new());
            m
        };

        Migration {
            repo,
            diff: tree_diff,
            changes,
            mkdirs: BTreeSet::new(),
            rmdirs: BTreeSet::new(),
            errors: vec![],
            conflicts,
        }
    }
    pub fn apply_changes(&mut self) -> Result<(), String> {
        match self.plan_changes() {
            Ok(_) => (),
            Err(errors) => return Err(errors.join("\n")),
        }
        self.update_workspace()?;
        self.update_index();

        Ok(())
    }

    fn plan_changes(&mut self) -> Result<(), Vec<String>> {
        for (path, (old_item, new_item)) in self.diff.clone() {
            self.check_for_conflict(&path, &old_item, &new_item);
            self.record_change(&path, old_item, new_item);
        }

        self.collect_errors()
    }

    fn insert_conflict(&mut self, conflict_type: &ConflictType, path: &Path) {
        if let Some(conflicts) = self.conflicts.get_mut(conflict_type) {
            conflicts.insert(path.to_path_buf());
        }
    }

    fn check_for_conflict(
        &mut self,
        path: &Path,
        old_item: &Option<TreeEntry>,
        new_item: &Option<TreeEntry>,
    ) {
        let path_str = path.to_str().unwrap();
        let entry = self.repo.index.entry_for_path(path_str).cloned();
        if self.index_differs_from_trees(entry.as_ref(), old_item.as_ref(), new_item.as_ref()) {
            self.insert_conflict(&ConflictType::StaleFile, &path);
            return;
        }

        let stat = self.repo.workspace.stat_file(path_str).ok();
        let error_type = self.get_error_type(&stat, &entry.as_ref(), new_item);

        if stat.is_none() {
            let parent = self.untracked_parent(path);
            if parent.is_some() {
                let parent = parent.unwrap();
                let conflict_path = if entry.is_some() { path } else { &parent };
                self.insert_conflict(&error_type, conflict_path);
            }
        } else if Self::stat_is_file(&stat) {
            let changed = self
                .repo
                .compare_index_to_workspace(entry.as_ref(), stat.as_ref());
            if changed != ChangeType::NoChange {
                self.insert_conflict(&error_type, path);
            }
        } else if Self::stat_is_dir(&stat) {
            let trackable = self
                .repo
                .is_trackable_path(path_str, &stat.unwrap())
                .ok()
                .unwrap_or(false);
            if trackable {
                self.insert_conflict(&error_type, path);
            }
        }
    }

    fn untracked_parent(&self, path: &'a Path) -> Option<PathBuf> {
        let dirname = path.parent().expect("failed to get dirname");
        for parent in dirname.ancestors() {
            let parent_path_str = parent.to_str().unwrap();
            if parent_path_str == "" {
                continue;
            }

            if let Ok(parent_stat) = self.repo.workspace.stat_file(parent_path_str) {
                if parent_stat.is_dir() {
                    continue;
                }

                if self
                    .repo
                    .is_trackable_path(parent_path_str, &parent_stat)
                    .unwrap_or(false)
                {
                    return Some(parent.to_path_buf());
                }
            }
        }
        None
    }

    fn stat_is_dir(stat: &Option<fs::Metadata>) -> bool {
        match stat {
            None => false,
            Some(stat) => stat.is_dir(),
        }
    }

    fn stat_is_file(stat: &Option<fs::Metadata>) -> bool {
        match stat {
            None => false,
            Some(stat) => stat.is_file(),
        }
    }

    fn get_error_type(
        &self,
        stat: &Option<fs::Metadata>,
        entry: &Option<&Entry>,
        item: &Option<TreeEntry>,
    ) -> ConflictType {
        if entry.is_some() {
            ConflictType::StaleFile
        } else if Self::stat_is_dir(&stat) {
            ConflictType::StaleDirectory
        } else if item.is_some() {
            ConflictType::UntrackedOverwritten
        } else {
            ConflictType::UntrackedRemoved
        }
    }

    fn index_differs_from_trees(
        &self,
        entry: Option<&Entry>,
        old_item: Option<&TreeEntry>,
        new_item: Option<&TreeEntry>,
    ) -> bool {
        self.repo.compare_tree_to_index(old_item, entry) != ChangeType::NoChange
            && self.repo.compare_tree_to_index(new_item, entry) != ChangeType::NoChange
    }

    fn collect_errors(&mut self) -> Result<(), Vec<String>> {
        for (conflict_type, paths) in &self.conflicts {
            if paths.is_empty() {
                continue;
            }

            let (header, footer) = MESSAGES.get(&conflict_type).unwrap();
            let mut error = vec![header.to_string()];

            for p in paths {
                error.push(format!("\t{}", p.to_str().unwrap()));
            }

            error.push(footer.to_string());
            error.push("\n".to_string());

            self.errors.push(error[..].join("\n"));
        }

        if !self.errors.is_empty() {
            return Err(self.errors.clone());
        }
        Ok(())
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
            .filter(|p| p.parent().is_some()) // filter out root path
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

    fn update_workspace(&mut self) -> Result<(), String> {
        self.repo.workspace.apply_migration(
            &mut self.repo.database,
            &self.changes,
            &self.rmdirs,
            &self.mkdirs,
        )
    }

    fn update_index(&mut self) {
        for (path, _) in self.changes.get(&Action::Delete).unwrap() {
            self.repo
                .index
                .remove(path.to_str().expect("failed to convert path to str"));
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
                self.repo.index.add(path, &entry_oid, &stat);
            }
        }
    }
}
