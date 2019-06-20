use crate::commands::CommandContext;
use crate::database::{Blob, Object};
use crate::index;
use crate::repository::Repository;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
    changed: BTreeSet<String>,
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
            changed: BTreeSet::new(),
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        self.repo
            .index
            .load_for_update()
            .expect("failed to load index");

        let mut untracked_files = self.scan_workspace(&self.root_path.clone()).unwrap();
        untracked_files.sort();

        self.detect_workspace_changes().unwrap();

        self.repo
            .index
            .write_updates()
            .expect("failed to write index");

        for file in &self.changed {
            self.ctx
                .stdout
                .write(format!(" M {}\n", file).as_bytes())
                .unwrap();
        }

        for file in untracked_files {
            self.ctx
                .stdout
                .write(format!("?? {}\n", file).as_bytes())
                .unwrap();
        }

        Ok(())
    }

    fn scan_workspace(&mut self, prefix: &Path) -> Result<Vec<String>, std::io::Error> {
        let mut untracked = vec![];
        for (mut path, stat) in self.repo.workspace.list_dir(prefix)? {
            if self.repo.index.is_tracked_path(&path) {
                if self.repo.workspace.is_dir(&path) {
                    untracked.extend_from_slice(
                        &self.scan_workspace(&self.repo.workspace.abs_path(&path))?,
                    );
                } else {
                    // path is file
                    self.stats.insert(path.to_string(), stat);
                }
            } else if self.is_trackable_path(&path, &stat)? {
                if self.repo.workspace.is_dir(&path) {
                    path.push('/');
                }
                untracked.push(path);
            }
        }

        Ok(untracked)
    }

    fn detect_workspace_changes(&mut self) -> Result<(), std::io::Error> {
        let entries: Vec<index::Entry> = self
            .repo
            .index
            .entries
            .iter()
            .map(|(_, entry)| entry.clone())
            .collect();
        for mut entry in entries {
            self.check_index_entry(&mut entry);
        }

        Ok(())
    }

    /// Adds modified entries to self.changed
    fn check_index_entry(&mut self, mut entry: &mut index::Entry) {
        let stat = self
            .stats
            .get(&entry.path)
            .expect("didn't find cached stat");
        if !entry.stat_match(stat) {
            self.changed.insert(entry.path.clone());
        }

        if entry.times_match(stat) {
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
            self.repo.index.update_entry_stat(&mut entry, stat);
        } else {
            self.changed.insert(entry.path.clone());
        }
    }

    /// Check if path is trackable but not currently tracked
    fn is_trackable_path(&self, path: &str, stat: &fs::Metadata) -> Result<bool, std::io::Error> {
        if stat.is_file() {
            return Ok(!self.repo.index.is_tracked_path(path));
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

}
