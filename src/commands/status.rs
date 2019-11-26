use crate::commands::CommandContext;
use crate::repository::{ChangeType, Repository};
use colored::*;
use std::collections::HashMap;
use std::io::{Read, Write};

static LABEL_WIDTH: usize = 12;

lazy_static! {
    static ref SHORT_STATUS: HashMap<ChangeType, &'static str> = {
        let mut m = HashMap::new();
        m.insert(ChangeType::Added, "A");
        m.insert(ChangeType::Modified, "M");
        m.insert(ChangeType::Deleted, "D");
        m
    };
    static ref LONG_STATUS: HashMap<ChangeType, &'static str> = {
        let mut m = HashMap::new();
        m.insert(ChangeType::Added, "new file:");
        m.insert(ChangeType::Modified, "modified:");
        m.insert(ChangeType::Deleted, "deleted:");
        m
    };
}

pub struct Status<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    repo: Repository,
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
        let repo = Repository::new(&root_path);

        Status { repo, ctx }
    }

    fn status_for(&self, path: &str) -> String {
        let left = if let Some(index_change) = self.repo.index_changes.get(path) {
            SHORT_STATUS.get(index_change).unwrap_or(&" ")
        } else {
            " "
        };
        let right = if let Some(workspace_change) = self.repo.workspace_changes.get(path) {
            SHORT_STATUS.get(workspace_change).unwrap_or(&" ")
        } else {
            " "
        };
        format!("{}{}", left, right)
    }

    fn print_porcelain_format(&mut self) -> Result<(), String> {
        for file in &self.repo.changed {
            writeln!(self.ctx.stdout, "{} {}", self.status_for(file), file)
                .map_err(|e| e.to_string())?;
        }

        for file in &self.repo.untracked {
            writeln!(self.ctx.stdout, "?? {}", file).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    fn print_long_format(&mut self) -> Result<(), String> {
        self.print_index_changes("Changes to be committed", "green")?;
        self.print_workspace_changes("Changes not staged for commit", "red")?;
        self.print_untracked_files("Untracked files", "red")?;

        self.print_commit_status()?;

        Ok(())
    }

    fn print_index_changes(&mut self, message: &str, style: &str) -> Result<(), String> {
        writeln!(self.ctx.stdout, "{}\n", message).map_err(|e| e.to_string())?;

        for (path, change_type) in &self.repo.index_changes {
            if let Some(status) = LONG_STATUS.get(change_type) {
                writeln!(
                    self.ctx.stdout,
                    "{}",
                    format!("\t{:width$}{}", status, path, width = LABEL_WIDTH).color(style)
                )
                .map_err(|e| e.to_string())?;
            }
        }

        writeln!(self.ctx.stdout).map_err(|e| e.to_string())
    }

    fn print_workspace_changes(&mut self, message: &str, style: &str) -> Result<(), String> {
        writeln!(self.ctx.stdout, "{}\n", message).map_err(|e| e.to_string())?;

        for (path, change_type) in &self.repo.workspace_changes {
            if let Some(status) = LONG_STATUS.get(change_type) {
                writeln!(
                    self.ctx.stdout,
                    "{}",
                    format!("\t{:width$}{}", status, path, width = LABEL_WIDTH).color(style)
                )
                .map_err(|e| e.to_string())?;
            }
        }

        writeln!(self.ctx.stdout).map_err(|e| e.to_string())
    }

    fn print_untracked_files(&mut self, message: &str, style: &str) -> Result<(), String> {
        writeln!(self.ctx.stdout, "{}\n", message).map_err(|e| e.to_string())?;

        for path in &self.repo.untracked {
            writeln!(self.ctx.stdout, "{}", format!("\t{}", path).color(style))
                .map_err(|e| e.to_string())?;
        }
        writeln!(self.ctx.stdout).map_err(|e| e.to_string())
    }

    pub fn print_results(&mut self) -> Result<(), String> {
        if self
            .ctx
            .options
            .as_ref()
            .map(|o| o.is_present("porcelain"))
            .unwrap_or(false)
        {
            self.print_porcelain_format()?;
        } else {
            self.print_long_format()?;
        }

        Ok(())
    }

    fn print_commit_status(&mut self) -> Result<(), String> {
        if !self.repo.index_changes.is_empty() {
            return Ok(());
        }

        if !self.repo.workspace_changes.is_empty() {
            writeln!(self.ctx.stdout, "no changes added to commit").map_err(|e| e.to_string())
        } else if !self.repo.untracked.is_empty() {
            writeln!(
                self.ctx.stdout,
                "nothing added to commit but untracked files present"
            )
            .map_err(|e| e.to_string())
        } else {
            writeln!(self.ctx.stdout, "nothing to commit, working tree clean")
                .map_err(|e| e.to_string())
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        self.repo
            .index
            .load_for_update()
            .expect("failed to load index");

        self.repo.initialize_status()?;

        self.repo
            .index
            .write_updates()
            .expect("failed to write index");

        self.print_results()
            .expect("printing status results failed");

        Ok(())
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
