use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::commit::{Author, Commit};
use crate::database::{Blob, Entry, Object, Tree};
use crate::repository::Repository;

pub struct CommandContext<I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub dir: PathBuf,
    pub env: HashMap<String, String>,
    pub args: Vec<String>,
    pub stdin: I,
    pub stdout: O,
    pub stderr: E,
}

pub fn execute<I, O, E>(ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    if ctx.args.len() < 2 {
        return Err("No command provided\n".to_string());
    }
    let command = &ctx.args[1];
    match &command[..] {
        "init" => init_command(ctx),
        "commit" => commit_command(ctx),
        "add" => add_command(ctx),
        _ => Err(format!("invalid command: {}\n", command)),
    }
}

pub fn init_command<I, O, E>(mut ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    let working_dir = ctx.dir;
    let root_path = if ctx.args.len() > 2 {
        Path::new(&ctx.args[2])
    } else {
        working_dir.as_path()
    };
    let git_path = root_path.join(".git");

    for d in ["objects", "refs"].iter() {
        fs::create_dir_all(git_path.join(d)).expect("failed to create dir");
    }

    ctx.stdout
        .write_all(format!("Initialized empty Jit repository in {:?}\n", git_path).as_bytes())
        .unwrap();

    Ok(())
}

pub fn commit_command<I, O, E>(mut ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    let working_dir = ctx.dir;
    let root_path = working_dir.as_path();
    let mut repo = Repository::new(&root_path.join(".git"));

    repo.index.load().expect("loading .git/index failed");
    let entries: Vec<Entry> = repo
        .index
        .entries
        .iter()
        .map(|(_path, idx_entry)| Entry::from(idx_entry))
        .collect();
    let root = Tree::build(&entries);
    root.traverse(&repo.database)
        .expect("Traversing tree to write to database failed");

    let parent = repo.refs.read_head();
    let author_name = ctx
        .env
        .get("GIT_AUTHOR_NAME")
        .expect("GIT_AUTHOR_NAME not set");
    let author_email = ctx
        .env
        .get("GIT_AUTHOR_EMAIL")
        .expect("GIT_AUTHOR_EMAIL not set");

    let author = Author {
        name: author_name.to_string(),
        email: author_email.to_string(),
    };

    let mut commit_message = String::new();
    ctx.stdin
        .read_to_string(&mut commit_message)
        .expect("reading commit from STDIN failed");

    let commit = Commit::new(&parent, root.get_oid(), author, commit_message);
    repo.database.store(&commit).expect("writing commit failed");
    repo.refs
        .update_head(&commit.get_oid())
        .expect("updating HEAD failed");
    repo.refs
        .update_master_ref(&commit.get_oid())
        .expect("updating master ref failed");

    let commit_prefix = if parent.is_some() {
        ""
    } else {
        "(root-commit) "
    };

    println!("[{}{}] {}", commit_prefix, commit.get_oid(), commit.message);

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
            return Err(
                    format!("fatal: {}

Another jit process seems to be running in this repository. Please make sure all processes are terminated then try again.

If it still fails, a jit process may have crashed in this repository earlier: remove the .git/index.lock file manually to continue.\n",
                        e)
                );
        }
        Err(_) => {
            return Err("fatal: could not create/load .git/index\n".to_string());
        }
    }

    let mut paths = vec![];
    for arg in &ctx.args[2..] {
        let path = match Path::new(arg).canonicalize() {
            Ok(path) => path,
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
        let data = match repo.workspace.read_file(&pathname) {
            Ok(data) => data,
            Err(ref err) if err.kind() == io::ErrorKind::PermissionDenied => {
                repo.index.release_lock().unwrap();
                return Err(format!(
                    "{}

fatal: adding files failed\n",
                    err
                ));
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
    }

    repo.index
        .write_updates()
        .expect("writing updates to index failed");

    Ok(())
}
