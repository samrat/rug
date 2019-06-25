use crate::commands::CommandContext;
use crate::database::blob::Blob;
use crate::database::commit::Commit;
use crate::database::object::Object;
use crate::database::tree::TreeEntry;
use crate::database::ParsedObject;
use crate::index;
use crate::repository::Repository;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
enum ChangeType {
    WorkspaceDeleted,
    WorkspaceModified,
    IndexAdded,
    IndexModified,
    IndexDeleted,
}

pub struct Status<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    root_path: PathBuf,
    repo: Repository,
    stats: HashMap<String, fs::Metadata>,
    ctx: CommandContext<'a, I, O, E>,
    untracked: BTreeSet<String>,
    changed: BTreeSet<String>,
    changes: HashMap<String, HashSet<ChangeType>>,
    head_tree: HashMap<String, TreeEntry>,
}

impl<'a, I, O, E> Status<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub fn new(ctx: CommandContext<'a, I, O, E>) -> Status<'a, I, O, E>
    where
        I: Read,
        O: Write,
        E: Write,
    {
        let working_dir = &ctx.dir;
        let root_path = working_dir.as_path();
        let repo = Repository::new(&root_path.join(".git"));

        Status {
            root_path: working_dir.to_path_buf(),
            repo,
            stats: HashMap::new(),
            ctx: ctx,
            untracked: BTreeSet::new(),
            changed: BTreeSet::new(),
            changes: HashMap::new(),
            head_tree: HashMap::new(),
        }
    }

    fn status_for(&self, path: &str) -> String {
        let mut left = " ";
        let mut right = " ";
        match self.changes.get(path) {
            None => (),
            Some(change_types) => {
                if change_types.contains(&ChangeType::IndexAdded) {
                    left = "A";
                }

                if change_types.contains(&ChangeType::IndexModified) {
                    left = "M";
                }

                if change_types.contains(&ChangeType::IndexDeleted) {
                    left = "D";
                }

                if change_types.contains(&ChangeType::WorkspaceDeleted) {
                    right = "D";
                } else if change_types.contains(&ChangeType::WorkspaceModified) {
                    right = "M"
                } else {
                    right = " ";
                }
            }
        };

        format!("{}{}", left, right)
    }

    pub fn print_results(&mut self) -> Result<(), std::io::Error> {
        for file in &self.changed {
            self.ctx
                .stdout
                .write(format!("{} {}\n", self.status_for(file), file).as_bytes())?;
        }

        for file in &self.untracked {
            self.ctx.stdout.write(format!("?? {}\n", file).as_bytes())?;
        }

        Ok(())
    }

    pub fn run(&mut self) -> Result<(), String> {
        self.repo
            .index
            .load_for_update()
            .expect("failed to load index");

        self.scan_workspace(&self.root_path.clone()).unwrap();
        self.load_head_tree();
        self.check_index_entries();
        self.collect_deleted_head_files();

        self.repo
            .index
            .write_updates()
            .expect("failed to write index");

        self.print_results()
            .expect("printing status results failed");

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
            if !self.repo.index.is_tracked_path(&path) {
                self.record_change(&path, ChangeType::IndexDeleted);
            }
        }
    }

    fn load_head_tree(&mut self) {
        let head_oid = self.repo.refs.read_head();
        if let Some(head_oid) = head_oid {
            let commit: Commit = {
                if let ParsedObject::Commit(commit) = self.repo.database.load(&head_oid) {
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
            if let ParsedObject::Tree(tree) = self.repo.database.load(tree_oid) {
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
        for (mut path, stat) in self.repo.workspace.list_dir(prefix)? {
            if self.repo.index.is_tracked(&path) {
                if self.repo.workspace.is_dir(&path) {
                    self.scan_workspace(&self.repo.workspace.abs_path(&path))?;
                } else {
                    // path is file
                    self.stats.insert(path.to_string(), stat);
                }
            } else if self.is_trackable_path(&path, &stat)? {
                if self.repo.workspace.is_dir(&path) {
                    path.push('/');
                }
                self.untracked.insert(path);
            }
        }

        Ok(())
    }

    fn check_index_entries(&mut self) -> Result<(), std::io::Error> {
        let entries: Vec<index::Entry> = self
            .repo
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

    fn record_change(&mut self, path: &str, change_type: ChangeType) {
        self.changed.insert(path.to_string());

        match self.changes.get_mut(path) {
            Some(change_types) => {
                change_types.insert(change_type);
            }
            None => {
                let mut change_types = HashSet::new();
                change_types.insert(change_type);
                self.changes.insert(path.to_string(), change_types);
            }
        }
    }

    /// Adds modified entries to self.changed
    fn check_index_against_workspace(&mut self, mut entry: &mut index::Entry) {
        if let Some(stat) = self.stats.get(&entry.path) {
            if !entry.stat_match(&stat) {
                return self.record_change(&entry.path, ChangeType::WorkspaceModified);
            }
            if entry.times_match(&stat) {
                return;
            }

            let data = self
                .repo
                .workspace
                .read_file(&entry.path)
                .expect("failed to read file");
            let blob = Blob::new(data.as_bytes());
            let oid = blob.get_oid();

            if entry.oid == oid {
                self.repo.index.update_entry_stat(&mut entry, &stat);
            } else {
                self.record_change(&entry.path, ChangeType::WorkspaceModified);
            }
        } else {
            self.record_change(&entry.path, ChangeType::WorkspaceDeleted)
        }
    }

    fn check_index_against_head_tree(&mut self, entry: &mut index::Entry) {
        let item = self.head_tree.get(&entry.path);
        if let Some(item) = item {
            if !(item.mode() == entry.mode && item.get_oid() == entry.oid) {
                self.record_change(&entry.path, ChangeType::IndexModified);
            }
        } else {
            self.record_change(&entry.path, ChangeType::IndexAdded);
        }
    }

    /// Check if path is trackable but not currently tracked
    fn is_trackable_path(&self, path: &str, stat: &fs::Metadata) -> Result<bool, std::io::Error> {
        if stat.is_file() {
            return Ok(!self.repo.index.is_tracked(path));
        }

        let items = self
            .repo
            .workspace
            .list_dir(&self.repo.workspace.abs_path(path))?;
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
}

#[cfg(test)]
mod tests {
    use crate::commands::tests::*;
    use std::{thread, time};

    #[test]
    fn list_untracked_files_in_name_order() {
        let mut cmd_helper = CommandHelper::new();

        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.write_file("file.txt", b"hello").unwrap();
        cmd_helper.write_file("another.txt", b"hello").unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(
            "?? another.txt
?? file.txt\n",
        );
    }

    #[test]
    fn list_files_as_untracked_if_not_in_index() {
        let mut cmd_helper = CommandHelper::new();

        cmd_helper.write_file("committed.txt", b"").unwrap();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.commit("commit message");

        cmd_helper.write_file("file.txt", b"").unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("?? file.txt\n");
    }

    #[test]
    fn list_untracked_dir_not_contents() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.write_file("file.txt", b"").unwrap();
        cmd_helper.write_file("dir/another.txt", b"").unwrap();
        cmd_helper.assert_status(
            "?? dir/
?? file.txt\n",
        );
    }

    #[test]
    fn list_untracked_files_inside_tracked_dir() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper.write_file("a/b/inner.txt", b"").unwrap();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.commit("commit message");

        cmd_helper.write_file("a/outer.txt", b"").unwrap();
        cmd_helper.write_file("a/b/c/file.txt", b"").unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(
            "?? a/b/c/
?? a/outer.txt\n",
        );
    }

    #[test]
    fn does_not_list_empty_untracked_dirs() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper.mkdir("outer").unwrap();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn list_untracked_dirs_that_indirectly_contain_files() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper.write_file("outer/inner/file.txt", b"").unwrap();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("?? outer/\n");
    }

    fn create_and_commit(cmd_helper: &mut CommandHelper) {
        cmd_helper.write_file("1.txt", b"one").unwrap();
        cmd_helper.write_file("a/2.txt", b"two").unwrap();
        cmd_helper.write_file("a/b/3.txt", b"three").unwrap();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.commit("commit message");
    }

    #[test]
    fn prints_nothing_when_no_files_changed() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn reports_files_with_changed_contents() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);

        cmd_helper.clear_stdout();
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        cmd_helper.write_file("a/2.txt", b"modified").unwrap();
        cmd_helper.assert_status(
            " M 1.txt
 M a/2.txt\n",
        );
    }

    #[test]
    fn reports_files_with_changed_modes() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);

        cmd_helper.make_executable("a/2.txt").unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.assert_status(" M a/2.txt\n");
    }

    #[test]
    fn reports_modified_files_with_unchanged_size() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);

        // Sleep so that mtime is slightly different from what is in
        // index
        let ten_millis = time::Duration::from_millis(2);
        thread::sleep(ten_millis);

        cmd_helper.write_file("a/b/3.txt", b"hello").unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.assert_status(" M a/b/3.txt\n");
    }

    #[test]
    fn prints_nothing_if_file_is_touched() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);
        cmd_helper.touch("1.txt").unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn reports_deleted_files() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);
        cmd_helper.delete("a/2.txt").unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(" D a/2.txt\n");
    }

    #[test]
    fn reports_files_in_deleted_dir() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);
        cmd_helper.delete("a").unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(
            " D a/2.txt
 D a/b/3.txt\n",
        );
    }

    #[test]
    fn reports_file_added_to_tracked_dir() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);
        cmd_helper.write_file("a/4.txt", b"four").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("A  a/4.txt\n");
    }

    #[test]
    fn reports_file_added_to_untracked_dir() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);
        cmd_helper.write_file("d/e/5.txt", b"five").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("A  d/e/5.txt\n");
    }

    #[test]
    fn reports_files_with_modes_modified_between_head_and_index() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);

        cmd_helper.make_executable("1.txt").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("M  1.txt\n");
    }

    #[test]
    fn reports_files_with_contents_modified_between_head_and_index() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);

        cmd_helper.write_file("a/b/3.txt", b"modified").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("M  a/b/3.txt\n");
    }

    #[test]
    fn reports_files_deleted_in_index() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);
        cmd_helper.delete("1.txt").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("D  1.txt\n");
    }

    #[test]
    fn reports_all_deleted_files_in_dir() {
        let mut cmd_helper = CommandHelper::new();
        create_and_commit(&mut cmd_helper);
        cmd_helper.delete("a").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(
            "D  a/2.txt
D  a/b/3.txt\n",
        );
    }
}
