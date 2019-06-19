use crate::commands::CommandContext;
use crate::repository::Repository;
use std::collections::HashMap;
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
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        self.repo.index.load().expect("failed to load index");

        let mut untracked_files = self.scan_workspace(&self.root_path.clone()).unwrap();
        untracked_files.sort();

        let mut changed = self.detect_workspace_changes().unwrap();
        changed.sort();

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

    fn detect_workspace_changes(&self) -> Result<Vec<String>, std::io::Error> {
        Ok(vec![])
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

    #[test]
    fn list_untracked_files_in_name_order() {
        let mut cmd_helper = CommandHelper::new();

        cmd_helper
            .write_file("file.txt", "hello".as_bytes())
            .unwrap();
        cmd_helper
            .write_file("another.txt", "hello".as_bytes())
            .unwrap();

        cmd_helper.assert_status(
            "?? another.txt
?? file.txt\n",
        );
    }

    #[test]
    fn list_files_as_untracked_if_not_in_index() {
        let mut cmd_helper = CommandHelper::new();

        cmd_helper
            .write_file("committed.txt", "".as_bytes())
            .unwrap();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.commit("commit message");

        cmd_helper.write_file("file.txt", "".as_bytes()).unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("?? file.txt\n");
    }

    #[test]
    fn list_untracked_dir_not_contents() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.write_file("file.txt", "".as_bytes()).unwrap();
        cmd_helper
            .write_file("dir/another.txt", "".as_bytes())
            .unwrap();
        cmd_helper.assert_status(
            "?? dir/
?? file.txt\n",
        );
    }

    #[test]
    fn list_untracked_files_inside_tracked_dir() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("a/b/inner.txt", "".as_bytes())
            .unwrap();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.commit("commit message");

        cmd_helper.write_file("a/outer.txt", "".as_bytes()).unwrap();
        cmd_helper
            .write_file("a/b/c/file.txt", "".as_bytes())
            .unwrap();

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
        cmd_helper
            .write_file("outer/inner/file.txt", "".as_bytes())
            .unwrap();
        cmd_helper.jit_cmd(&["init"]).unwrap();
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("?? outer/\n");
    }
}
