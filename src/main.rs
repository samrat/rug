extern crate crypto;
extern crate flate2;
extern crate rand;
extern crate chrono;

use std::env;
use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::io::prelude::*;
use std::io;

use chrono::Utc;

mod lockfile;
use lockfile::Lockfile;

mod database;
use database::{Object, Blob, Tree, Database, Entry};

mod workspace;
use workspace::Workspace;

mod index;
use index::Index;

mod util;

struct Refs {
    pathname: PathBuf,
}

impl Refs {
    fn new(pathname: &Path) -> Refs {
        Refs { pathname: pathname.to_path_buf() }
    }
    
    fn head_path(&self) -> PathBuf {
        self.pathname.as_path().join("HEAD").to_path_buf()
    }
    
    fn update_head(&self, oid: &str) -> Result<(), std::io::Error> {
        let mut lock = Lockfile::new(&self.head_path());
        lock.hold_for_update()?;
        lock.write(oid)?;
        lock.write("\n")?;
        lock.commit()
    }

    // NOTE: Jumping a bit ahead of the book so that we can have a
    // `master` branch
    fn update_master_ref(&self, oid: &str) -> Result<(), std::io::Error> {
        let master_ref_path = self.pathname.as_path().join("refs/heads/master");
        fs::create_dir_all(master_ref_path.parent().unwrap())?;
        
        let mut lock = Lockfile::new(&master_ref_path);
        lock.hold_for_update()?;
        lock.write(oid)?;
        lock.write("\n")?;
        lock.commit()
    }

    fn read_head(&self) -> Option<String> {
        if self.head_path().as_path().exists() {
            let mut head_file = File::open(self.head_path()).unwrap();
            let mut contents = String::new();
            head_file.read_to_string(&mut contents).unwrap();
            Some(contents.trim().to_string())
        } else {
            None
        }
    }
}

struct Author {
    name: String,
    email: String,
}

impl Author {
    fn to_string(&self) -> String {
        format!("{} <{}> {}",
                self.name,
                self.email,
                Utc::now().format("%s %z"))
    }
}

struct Commit {
    parent: Option<String>,
    tree_oid: String,
    author: Author,
    message: String,
}

impl Commit {
    fn new(parent: &Option<String>, tree_oid: String, author: Author, message: String) -> Commit {
        Commit { parent: parent.clone(), tree_oid, author, message}
    }
}

impl Object for Commit {
    fn r#type(&self) -> String {
        "commit".to_string()
    }

    fn to_string(&self) -> Vec<u8> {
        let author_str = self.author.to_string();
        let mut lines = String::new();
        lines.push_str(&format!("tree {}\n", self.tree_oid));
        if let Some(parent_oid) = &self.parent {
            lines.push_str(&format!("parent {}\n", parent_oid));
        }
        lines.push_str(&format!("author {}\n", author_str));
        lines.push_str(&format!("committer {}\n", author_str));
        lines.push_str("\n");
        lines.push_str(&self.message);

        lines.as_bytes().to_vec()
    }
}

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
            let git_path = root_path.join(".git");
            let db_path = git_path.join("objects");

            let workspace = Workspace::new(root_path);
            let database = Database::new(&db_path);
            let refs = Refs::new(git_path.as_path());

            let mut entries = Vec::new();
            
            for path in workspace.list_files()? {
                let blob = Blob::new(workspace.read_file(&path)?.as_bytes());
                database.store(&blob)?;
                let mode = workspace.file_mode(&path)?;

                entries.push(Entry::new(&path, &blob.get_oid(), mode));
            };

            let root = Tree::build(&entries);
            root.traverse(&database)?;

            let parent = refs.read_head();
            let author_name = env::var("GIT_AUTHOR_NAME")
                .expect("GIT_AUTHOR_NAME not set");
            let author_email = env::var("GIT_AUTHOR_EMAIL")
                .expect("GIT_AUTHOR_EMAIL not set");

            let author = Author { name: author_name,
                                  email: author_email };

            let mut commit_message = String::new();
            io::stdin().read_to_string(&mut commit_message)?;

            let commit = Commit::new(&parent, root.get_oid(), author, commit_message);
            database.store(&commit)?;
            refs.update_head(&commit.get_oid())?;
            refs.update_master_ref(&commit.get_oid())?;

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
            let git_path = root_path.join(".git");
            let db_path = git_path.join("objects");
            
            let workspace = Workspace::new(root_path);
            let database = Database::new(&db_path);
            let mut index = Index::new(&git_path.join("index"));

            let path = &args[2];
            let data = workspace.read_file(&path);
            let stat = workspace.stat_file(&path)?;

            let blob = Blob::new(workspace.read_file(&path)?.as_bytes());
            database.store(&blob)?;
            index.add(&path, &blob.get_oid(), stat);

            index.write_updates()?;
            
            Ok(())

        },
        _ => panic!("invalid command: {}", command),
    }
}
