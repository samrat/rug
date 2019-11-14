use crate::database::blob::Blob;
use crate::database::commit::Commit;
use crate::database::object::Object;
use crate::database::tree::TreeEntry;
use crate::database::Database;
use crate::database::ParsedObject;
use crate::index;
use crate::index::Index;
use crate::refs::Refs;
use crate::workspace::Workspace;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

pub mod migration;
use migration::Migration;

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
enum ChangeKind {
    Workspace,
    Index,
}

pub struct Repository {
    pub database: Database,
    pub index: Index,
    pub refs: Refs,
    pub workspace: Workspace,

    // status fields
    pub root_path: PathBuf,
    pub stats: HashMap<String, fs::Metadata>,
    pub untracked: BTreeSet<String>,
    pub changed: BTreeSet<String>,
    pub workspace_changes: BTreeMap<String, ChangeType>,
    pub index_changes: BTreeMap<String, ChangeType>,
    pub head_tree: HashMap<String, TreeEntry>,
}

impl Repository {
    pub fn new(root_path: &Path) -> Repository {
        let git_path = root_path.join(".git");
        let db_path = git_path.join("objects");

        Repository {
            database: Database::new(&db_path),
            index: Index::new(&git_path.join("index")),
            refs: Refs::new(&git_path),
            workspace: Workspace::new(git_path.parent().unwrap()),

            root_path: root_path.to_path_buf(),
            stats: HashMap::new(),
            untracked: BTreeSet::new(),
            changed: BTreeSet::new(),
            workspace_changes: BTreeMap::new(),
            index_changes: BTreeMap::new(),
            head_tree: HashMap::new(),
        }
    }

    pub fn initialize_status(&mut self) -> Result<(), String> {
        self.scan_workspace(&self.root_path.clone()).unwrap();
        self.load_head_tree();
        self.check_index_entries().map_err(|e| e.to_string())?;
        self.collect_deleted_head_files();

        Ok(())
    }

    fn collect_deleted_head_files(&mut self) {
        let paths: Vec<String> = {
            self.head_tree
                .iter()
                .map(|(path, _)| path.clone())
                .collect()
        };
        for path in paths {
            if !self.index.is_tracked_path(&path) {
                self.record_change(&path, ChangeKind::Index, ChangeType::Deleted);
            }
        }
    }

    fn load_head_tree(&mut self) {
        let head_oid = self.refs.read_head();
        if let Some(head_oid) = head_oid {
            let commit: Commit = {
                if let ParsedObject::Commit(commit) = self.database.load(&head_oid) {
                    commit.clone()
                } else {
                    panic!("HEAD points to a non-commit");
                }
            };
            self.read_tree(&commit.tree_oid, Path::new(""));
        }
    }

    fn read_tree(&mut self, tree_oid: &str, prefix: &Path) {
        let entries = {
            if let ParsedObject::Tree(tree) = self.database.load(tree_oid) {
                tree.entries.clone()
            } else {
                BTreeMap::new()
            }
        };

        for (name, entry) in entries {
            let path = prefix.join(name);

            if entry.is_tree() {
                self.read_tree(&entry.get_oid(), &path);
            } else {
                self.head_tree
                    .insert(path.to_str().unwrap().to_string(), entry);
            }
        }
    }

    fn scan_workspace(&mut self, prefix: &Path) -> Result<(), std::io::Error> {
        for (mut path, stat) in self.workspace.list_dir(prefix)? {
            if self.index.is_tracked(&path) {
                if self.workspace.is_dir(&path) {
                    self.scan_workspace(&self.workspace.abs_path(&path))?;
                } else {
                    // path is file
                    self.stats.insert(path.to_string(), stat);
                }
            } else if self.is_trackable_path(&path, &stat)? {
                if self.workspace.is_dir(&path) {
                    path.push('/');
                }
                self.untracked.insert(path);
            }
        }

        Ok(())
    }

    fn check_index_entries(&mut self) -> Result<(), std::io::Error> {
        let entries: Vec<index::Entry> = self
            .index
            .entries
            .iter()
            .map(|(_, entry)| entry.clone())
            .collect();
        for mut entry in entries {
            self.check_index_against_workspace(&mut entry);
            self.check_index_against_head_tree(&mut entry);
        }

        Ok(())
    }

    fn record_change(&mut self, path: &str, change_kind: ChangeKind, change_type: ChangeType) {
        self.changed.insert(path.to_string());

        let changes_map = match change_kind {
            ChangeKind::Index => &mut self.index_changes,
            ChangeKind::Workspace => &mut self.workspace_changes,
        };

        changes_map.insert(path.to_string(), change_type);
    }

    /// Adds modified entries to self.changed
    fn check_index_against_workspace(&mut self, mut entry: &mut index::Entry) {
        if let Some(stat) = self.stats.get(&entry.path) {
            if !entry.stat_match(&stat) {
                return self.record_change(
                    &entry.path,
                    ChangeKind::Workspace,
                    ChangeType::Modified,
                );
            }
            if entry.times_match(&stat) {
                return;
            }

            let data = self
                .workspace
                .read_file(&entry.path)
                .expect("failed to read file");
            let blob = Blob::new(data.as_bytes());
            let oid = blob.get_oid();

            if entry.oid == oid {
                self.index.update_entry_stat(&mut entry, &stat);
            } else {
                self.record_change(&entry.path, ChangeKind::Workspace, ChangeType::Modified);
            }
        } else {
            self.record_change(&entry.path, ChangeKind::Workspace, ChangeType::Deleted)
        }
    }

    fn check_index_against_head_tree(&mut self, entry: &mut index::Entry) {
        let item = self.head_tree.get(&entry.path);
        if let Some(item) = item {
            if !(item.mode() == entry.mode && item.get_oid() == entry.oid) {
                self.record_change(&entry.path, ChangeKind::Index, ChangeType::Modified);
            }
        } else {
            self.record_change(&entry.path, ChangeKind::Index, ChangeType::Added);
        }
    }

    /// Check if path is trackable but not currently tracked
    fn is_trackable_path(&self, path: &str, stat: &fs::Metadata) -> Result<bool, std::io::Error> {
        if stat.is_file() {
            return Ok(!self.index.is_tracked(path));
        }

        let items = self.workspace.list_dir(&self.workspace.abs_path(path))?;
        let (files, dirs): (Vec<(&String, &fs::Metadata)>, Vec<(&String, &fs::Metadata)>) =
            items.iter().partition(|(_path, stat)| stat.is_file());

        for (file_path, file_stat) in files.iter() {
            if self.is_trackable_path(file_path, file_stat)? {
                return Ok(true);
            }
        }

        for (dir_path, dir_stat) in dirs.iter() {
            if self.is_trackable_path(dir_path, dir_stat)? {
                return Ok(true);
            }
        }

        return Ok(false);
    }

    pub fn migration(
        &mut self,
        tree_diff: HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)>,
    ) -> Migration {
        Migration::new(self, tree_diff)
    }
}
