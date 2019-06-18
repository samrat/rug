use crate::commands::CommandContext;
use crate::repository::Repository;
use std::io::{Read, Write};

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

    let mut untracked_files: Vec<String> = repo
        .workspace
        .list_files(&working_dir)
        .expect("list files failed")
        .iter()
        .filter(|path| !repo.index.is_tracked_path(path))
        .cloned()
        .collect();
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

        if let Ok((stdout, _stderr)) = cmd_helper.jit_cmd(vec!["", "status"]) {
            assert_output(
                &stdout,
                "?? another.txt
?? file.txt\n",
            );
        } else {
            assert!(false);
        }
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
        if let Ok((stdout, _stderr)) = cmd_helper.jit_cmd(vec!["", "status"]) {
            assert_output(&stdout, "?? file.txt\n")
        } else {
            assert!(false);
        }
    }
}
