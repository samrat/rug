extern crate crypto;
extern crate flate2;
extern crate rand;
extern crate chrono;
#[macro_use]
extern crate lazy_static;

use std::env;
use std::path::Path;
use std::fs;
use std::io::prelude::*;
use std::io;

mod lockfile;

mod database;
use database::{Object, Blob, Tree, Entry};

mod workspace;
mod index;
mod refs;

mod util;

mod commit;
use commit::{Author, Commit};

mod repository;
use repository::Repository;

fn main() -> std::io::Result<()> {
    let args : Vec<String> = env::args().collect();
    let command = &args[1];
    match &command[..] {
        "init" => {
            let working_dir = env::current_dir()?;
            let root_path = if args.len() > 2 {
                Path::new(&args[2])
            } else {
                working_dir.as_path()
            };
            let git_path = root_path.join(".git");

            for d in ["objects", "refs"].iter() {
                fs::create_dir_all(git_path.join(d))?;
            }

            println!("Initialized empty Jit repository in {:?}", git_path);
            
            Ok(())
        },
        "commit" => {
            let working_dir = env::current_dir()?;
            let root_path = working_dir.as_path();
            let mut repo = Repository::new(&root_path.join(".git"));

            repo.index.load()?;
            let entries : Vec<Entry> = repo.index.entries.iter()
                .map(|(_path, idx_entry)| Entry::from(idx_entry))
                .collect();
            let root = Tree::build(&entries);
            root.traverse(&repo.database)?;

            let parent = repo.refs.read_head();
            let author_name = env::var("GIT_AUTHOR_NAME")
                .expect("GIT_AUTHOR_NAME not set");
            let author_email = env::var("GIT_AUTHOR_EMAIL")
                .expect("GIT_AUTHOR_EMAIL not set");

            let author = Author { name: author_name,
                                  email: author_email };

            let mut commit_message = String::new();
            io::stdin().read_to_string(&mut commit_message)?;

            let commit = Commit::new(&parent, root.get_oid(), author, commit_message);
            repo.database.store(&commit)?;
            repo.refs.update_head(&commit.get_oid())?;
            repo.refs.update_master_ref(&commit.get_oid())?;

            let commit_prefix = if parent.is_some() {
                ""
            } else {
                "(root-commit) "
            };

            println!("[{}{}] {}", commit_prefix, commit.get_oid(), commit.message);
            
            Ok(())
        },
        "add" => {
            let working_dir = env::current_dir()?;
            let root_path = working_dir.as_path();
            let mut repo = Repository::new(&root_path.join(".git"));
            
            match repo.index.load_for_update() {
                Ok(_) => (),
                Err(ref e)
                    if e.kind() == io::ErrorKind::AlreadyExists => {
                    eprintln!("fatal: {}

Another jit process seems to be running in this repository. Please make sure all processes are terminated then try again.

If it still fails, a jit process may have crashed in this repository earlier: remove the .git/index.lock file manually to continue.",
                              e);
                    std::process::exit(128);
                    },
                Err(_) => {
                    eprintln!("fatal: could not create/load .git/index");
                    std::process::exit(128);
                },
            }

            let mut paths = vec![];
            for arg in &args[2..] {
                let path = match Path::new(arg).canonicalize() {
                    Ok(path) => path,
                    Err(_) => {
                        eprintln!("fatal: pathspec '{:}' did not match any files", arg);
                        repo.index.release_lock()?;
                        std::process::exit(1);
                    },
                };

                for pathname in repo.workspace.list_files(&path)? {
                    paths.push(pathname);
                }
            }

            for pathname in paths {
                let data = match repo.workspace.read_file(&pathname) {
                    Ok(data) => data,
                    Err(ref err)
                        if err.kind() == io::ErrorKind::PermissionDenied => {
                        eprintln!("{}", err);
                        eprintln!("fatal: adding files failed");

                        repo.index.release_lock()?;
                        std::process::exit(128);
                    },
                    _ => {
                        panic!("fatal: adding files failed");
                    },
                };
                let stat = repo.workspace.stat_file(&pathname)?;

                let blob = Blob::new(data.as_bytes());
                repo.database.store(&blob)?;
                
                repo.index.add(&pathname, &blob.get_oid(), &stat);
            }

            repo.index.write_updates()?;
            
            Ok(())

        },
        _ => panic!("invalid command: {}", command),
    }
}
