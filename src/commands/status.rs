use crate::commands::CommandContext;
use crate::repository::Repository;
use std::io::{Read, Write};
use std::path::Path;

fn scan_workspace(repo: &Repository, prefix: &Path) -> Result<Vec<String>, std::io::Error> {
    let mut untracked = vec![];
    for (mut path, _stat) in repo.workspace.list_dir(prefix)? {
        if repo.index.is_tracked_path(&path) {
            if repo.workspace.is_dir(&path) {
                untracked
                    .extend_from_slice(&scan_workspace(repo, &repo.workspace.abs_path(&path))?);
            }
        } else {
            if repo.workspace.is_dir(&path) {
                path.push('/');
            }
            untracked.push(path);
        }
    }

    Ok(untracked)
}

pub fn status_command<I, O, E>(mut ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    let working_dir = ctx.dir;
    let root_path = working_dir.as_path();
    let mut repo = Repository::new(&root_path.join(".git"));

    repo.index.load().expect("failed to load index");

    let mut untracked_files = scan_workspace(&repo, &root_path).unwrap();
    untracked_files.sort();

    for file in untracked_files {
        ctx.stdout
            .write(format!("?? {}\n", file).as_bytes())
            .unwrap();
    }
    Ok(())
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
        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        cmd_helper.jit_cmd(vec!["", "add", "."]).unwrap();
        cmd_helper.commit("commit message");

        cmd_helper.write_file("file.txt", "".as_bytes()).unwrap();

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("?? file.txt\n");
    }

    #[test]
    fn list_untracked_dir_not_contents() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
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
        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        cmd_helper.jit_cmd(vec!["", "add", "."]).unwrap();
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
}
