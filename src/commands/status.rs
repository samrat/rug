use crate::commands::CommandContext;
use crate::repository::Repository;
use std::io::{self, Read, Write};

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
        ctx.stdout.write(format!("?? {}\n", file).as_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::commands::tests::*;

    #[test]
    fn list_untracked_files_in_name_order() {
        let repo_path = gen_repo_path();
        let mut repo = repo(&repo_path);

        write_file(&repo_path, "file.txt", "hello".as_bytes()).unwrap();
        write_file(&repo_path, "another.txt", "hello".as_bytes()).unwrap();

        if let Ok((stdout, stderr)) = jit_cmd(&repo_path, vec!["", "status"]) {
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
        let repo_path = gen_repo_path();
        write_file(&repo_path, "committed.txt", "".as_bytes()).unwrap();
        jit_cmd(&repo_path, vec!["", "init", repo_path.to_str().unwrap()]).unwrap();
        jit_cmd(&repo_path, vec!["", "add", "."]);
        commit(&repo_path, "commit message");

        write_file(&repo_path, "file.txt", "".as_bytes()).unwrap();
        if let Ok((stdout, stderr)) = jit_cmd(&repo_path, vec!["", "status"]) {
            assert_output(&stdout, "?? file.txt\n")
        } else {
            assert!(false);
        }
    }
}
