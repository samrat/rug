use std::io::{self, Read, Write};

use crate::commands::CommandContext;
use crate::database::{Blob, Object};
use crate::repository::Repository;

static INDEX_LOAD_OR_CREATE_FAILED: &'static str = "fatal: could not create/load .git/index\n";

fn locked_index_message(e: &std::io::Error) -> String {
    format!("fatal: {}

Another jit process seems to be running in this repository. Please make sure all processes are terminated then try again.

If it still fails, a jit process may have crashed in this repository earlier: remove the .git/index.lock file manually to continue.\n",
            e)
}

fn add_failed_message(e: &std::io::Error) -> String {
    format!(
        "{}

fatal: adding files failed\n",
        e
    )
}

fn add_to_index(repo: &mut Repository, pathname: &str) -> Result<(), String> {
    let data = match repo.workspace.read_file(&pathname) {
        Ok(data) => data,
        Err(ref err) if err.kind() == io::ErrorKind::PermissionDenied => {
            repo.index.release_lock().unwrap();
            return Err(add_failed_message(&err));
        }
        _ => {
            panic!("fatal: adding files failed");
        }
    };

    let stat = repo
        .workspace
        .stat_file(&pathname)
        .expect("could not stat file");
    let blob = Blob::new(data.as_bytes());
    repo.database.store(&blob).expect("storing blob failed");

    repo.index.add(&pathname, &blob.get_oid(), &stat);

    Ok(())
}

pub fn add_command<I, O, E>(ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    let working_dir = ctx.dir;
    let root_path = working_dir.as_path();
    let mut repo = Repository::new(&root_path.join(".git"));

    match repo.index.load_for_update() {
        Ok(_) => (),
        Err(ref e) if e.kind() == io::ErrorKind::AlreadyExists => {
            return Err(locked_index_message(e));
        }
        Err(_) => {
            return Err(INDEX_LOAD_OR_CREATE_FAILED.to_string());
        }
    }

    let mut paths = vec![];
    for arg in &ctx.args[2..] {
        let path = match working_dir.join(arg).canonicalize() {
            Ok(canon_path) => canon_path,
            Err(_) => {
                repo.index.release_lock().unwrap();
                return Err(format!(
                    "fatal: pathspec '{:}' did not match any files\n",
                    arg
                ));
            }
        };

        for pathname in repo.workspace.list_files(&path).unwrap() {
            paths.push(pathname);
        }
    }

    for pathname in paths {
        add_to_index(&mut repo, &pathname)?;
    }

    repo.index
        .write_updates()
        .expect("writing updates to index failed");

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::commands::tests::*;

    #[test]
    fn add_regular_file_to_index() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("hello.txt", "hello".as_bytes())
            .unwrap();
        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        cmd_helper.jit_cmd(vec!["", "add", "hello.txt"]).unwrap();
        cmd_helper
            .assert_index(vec![(0o100644, "hello.txt".to_string())])
            .unwrap();
    }

    #[test]
    fn add_executable_file_to_index() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("hello.txt", "hello".as_bytes())
            .unwrap();
        cmd_helper.make_executable("hello.txt").unwrap();

        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        cmd_helper.jit_cmd(vec!["", "add", "hello.txt"]).unwrap();
        cmd_helper
            .assert_index(vec![(0o100755, "hello.txt".to_string())])
            .unwrap();
    }

    #[test]
    fn add_multiple_files_to_index() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("hello.txt", "hello".as_bytes())
            .unwrap();
        cmd_helper
            .write_file("world.txt", "world".as_bytes())
            .unwrap();

        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        cmd_helper
            .jit_cmd(vec!["", "add", "hello.txt", "world.txt"])
            .unwrap();

        cmd_helper
            .assert_index(vec![
                (0o100644, "hello.txt".to_string()),
                (0o100644, "world.txt".to_string()),
            ])
            .unwrap();
    }

    #[test]
    fn incrementally_add_files_to_index() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("hello.txt", "hello".as_bytes())
            .unwrap();
        cmd_helper
            .write_file("world.txt", "world".as_bytes())
            .unwrap();

        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        cmd_helper.jit_cmd(vec!["", "add", "hello.txt"]).unwrap();

        cmd_helper
            .assert_index(vec![(0o100644, "hello.txt".to_string())])
            .unwrap();

        cmd_helper.jit_cmd(vec!["", "add", "world.txt"]).unwrap();
        cmd_helper
            .assert_index(vec![
                (0o100644, "hello.txt".to_string()),
                (0o100644, "world.txt".to_string()),
            ])
            .unwrap();
    }

    #[test]
    fn add_a_directory_to_index() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("a-dir/nested.txt", "hello".as_bytes())
            .unwrap();
        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();

        cmd_helper.jit_cmd(vec!["", "add", "a-dir"]).unwrap();
        cmd_helper
            .assert_index(vec![(0o100644, "a-dir/nested.txt".to_string())])
            .unwrap();
    }

    #[test]
    fn add_repository_root_to_index() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("a/b/c/hello.txt", "hello".as_bytes())
            .unwrap();

        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        cmd_helper.jit_cmd(vec!["", "add", "."]).unwrap();

        cmd_helper
            .assert_index(vec![(0o100644, "a/b/c/hello.txt".to_string())])
            .unwrap();
    }

    #[test]
    fn add_fails_for_non_existent_files() {
        let mut cmd_helper = CommandHelper::new();

        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        assert!(cmd_helper.jit_cmd(vec!["", "add", "hello.txt"]).is_err());
    }

    #[test]
    fn add_fails_for_unreadable_files() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("hello.txt", "hello".as_bytes())
            .unwrap();
        cmd_helper.make_unreadable("hello.txt").unwrap();

        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        assert!(cmd_helper.jit_cmd(vec!["", "add", "hello.txt"]).is_err());
    }

    #[test]
    fn add_fails_if_index_is_locked() {
        let mut cmd_helper = CommandHelper::new();
        cmd_helper
            .write_file("hello.txt", "hello".as_bytes())
            .unwrap();
        cmd_helper
            .write_file(".git/index.lock", "hello".as_bytes())
            .unwrap();

        cmd_helper.jit_cmd(vec!["", "init"]).unwrap();
        assert!(cmd_helper.jit_cmd(vec!["", "add", "hello.txt"]).is_err());
    }
}
