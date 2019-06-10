extern crate crypto;
extern crate flate2;
extern crate rand;
extern crate chrono;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::io::{self, BufReader};
use std::num::ParseIntError;
use std::os::unix::fs::PermissionsExt;
use std::collections::BTreeMap;

use crypto::digest::Digest;
use crypto::sha1::Sha1;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

use chrono::Utc;

mod lockfile;
use lockfile::Lockfile;

fn decode_hex(s: &str) -> Result<Vec<u8>, ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}

struct Workspace {
    path: PathBuf,
}

impl Workspace {
    fn new(path: &Path) -> Workspace {
        Workspace { path: path.to_path_buf() }
    }

    fn list_dir_files(&self, dir: &Path) -> Result<Vec<String>, std::io::Error> {
        let ignore_paths = [".git", "target"];
        if ignore_paths.contains(&dir.file_name().unwrap().to_str().unwrap()) {
            return Ok(vec![]);
        }
        
        let mut files = vec![];
        for file in fs::read_dir(dir)? {
            let path = file?.path();
            if File::open(&path)?.metadata()?.is_dir() {
                files.extend_from_slice(&self.list_dir_files(&path)?);
                continue;
            } else {
                let file_name = path.file_name().unwrap();
                let file_name_str = file_name.to_str()
                    .expect("invalid filename");
                if !ignore_paths.contains(&file_name_str) {
                    files.push(dir.join(file_name_str.to_string())
                               .strip_prefix(self.path.clone())
                               .unwrap()
                               .to_str()
                               .unwrap()
                               .to_string());
                }
            }
        }
        Ok(files)
    }

    fn list_files(&self) -> Result<Vec<String>, std::io::Error> {
        self.list_dir_files(&self.path)
    }

    fn read_file(&self, file_name: &str) -> Result<String, std::io::Error> {
        let file = File::open(self.path.as_path().join(file_name))?;
        let mut buf_reader = BufReader::new(file);
        let mut contents = String::new();
        
        buf_reader.read_to_string(&mut contents)?;
        Ok(contents)
    }

    fn file_mode(&self, file_name: &str) -> Result<u32, std::io::Error> {
        let file = File::open(self.path.join(file_name))?;
        Ok(file.metadata()?.permissions().mode())
    }
}

trait Object {
    fn r#type(&self) -> String;
    fn to_string(&self) -> Vec<u8>;

    fn get_oid(&self) -> String {
        let mut hasher = Sha1::new();
        hasher.input(&self.get_content());
        hasher.result_str()
    }

    fn get_content(&self) -> Vec<u8> {
        // TODO: need to do something to force ASCII encoding?
        let string = self.to_string();
        let mut content : Vec<u8> = self.r#type().as_bytes().to_vec();
        
        content.push(0x20);
        content.extend_from_slice(format!("{}", string.len()).as_bytes());
        content.push(0x0);
        content.extend_from_slice(&string);

        content
    }
}

struct Blob {
    data: Vec<u8>,
}

impl Blob {
    fn new(data: &[u8]) -> Blob {
        Blob { data: data.to_vec() }
    }

}

impl Object for Blob {
    fn r#type(&self) -> String {
        "blob".to_string()
    }

    fn to_string(&self) -> Vec<u8> {
        self.data.clone()
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
struct Entry {
    name: String,
    oid: String,
    mode: u32,
}

impl Entry {
    fn new(name: &str, oid: &str, mode: u32) -> Entry {
        Entry { name: name.to_string(),
                oid: oid.to_string(),
                mode,}
    }

    // if user is allowed to executable, set mode to Executable,
    // else Regular
    fn is_executable(&self) -> bool {
        (self.mode >> 6) & 0b1 == 1
    }

    fn mode(&self) -> &str {
        if self.is_executable() {
            "100755"
        } else {
            "100644"
        }
    }
}

#[derive(Clone, Debug)]
enum TreeEntry {
    Entry(Entry),
    Tree(Tree),
}

impl TreeEntry {
    fn mode(&self) -> &str {
        match self {
            TreeEntry::Entry(e) => e.mode(),
            _ => "40000",
        }
    }

    fn get_oid(&self) -> String {
        match self {
            TreeEntry::Entry(e) => e.oid.clone(),
            TreeEntry::Tree(t) => t.get_oid(),
        }
    }
}

#[derive(Clone, Debug)]
struct Tree {
    entries: BTreeMap<String, TreeEntry>,
}

impl Tree {
    fn new() -> Tree {
        Tree { entries: BTreeMap::new() }
    }

    fn build(entries: &[Entry]) -> Tree {
        let mut sorted_entries = entries.to_vec();
        sorted_entries.sort();

        let mut root = Tree::new();
        for entry in sorted_entries.iter() {
            let mut path : Vec<String> = Path::new(&entry.name)
                .iter()
                .map(|c| c.to_str().unwrap().to_string())
                .collect();
            let name = path.pop().expect("file path has zero components");
            root.add_entry(&path, name, entry.clone());
        }

        root
    }

    fn add_entry(&mut self, path: &[String], name: String, entry: Entry) {
        if path.is_empty() {
            self.entries.insert(name, TreeEntry::Entry(entry));
        } else if let Some(TreeEntry::Tree(tree)) = self.entries.get_mut(&path[0]) {
            tree.add_entry(&path[1..], name, entry);
        } else {
            let mut tree = Tree::new();
            tree.add_entry(&path[1..], name, entry);
            self.entries.insert(path[0].clone(),
                                TreeEntry::Tree(tree));

        };
    }

    // TODO: Take closure that calls `database.store` as arg instead
    // of taking `database`
    fn traverse(&self, database: &Database) -> Result<(), std::io::Error> {
        // Do a postorder traversal(visit all children first, then
        // process `self`
        for (_name, entry) in self.entries.clone() {
            if let TreeEntry::Tree(tree) = entry {
                tree.traverse(database)?;
            }
        }

        database.store(self)?;

        Ok(())
    }
}

impl Object for Tree {
    fn r#type(&self) -> String {
        "tree".to_string()
    }
    
    fn to_string(&self) -> Vec<u8> {
        let mut tree_vec = Vec::new();
        for (name, entry) in self.entries.iter() {
            let mut entry_vec : Vec<u8> = format!("{} {}\0",
                                                  entry.mode(),
                                                  name).as_bytes().to_vec();
            entry_vec.extend_from_slice(&decode_hex(&entry.get_oid()).expect("invalid oid"));
            tree_vec.extend_from_slice(&entry_vec);
        }
        tree_vec
    }
}

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

struct Database {
    path: PathBuf,
}

impl Database {
    fn new(path: &Path) -> Database {
        Database { path: path.to_path_buf() }
    }

    fn store<T>(&self, obj: &T) -> Result<(), std::io::Error>
    where T: Object {
        let oid = obj.get_oid();
        let content = obj.get_content();

        self.write_object(oid, content)
    }

    fn generate_temp_name() -> String {
        thread_rng()
            .sample_iter(&Alphanumeric)
            .take(6)
            .collect()
    }

    fn write_object(&self, oid: String, content: Vec<u8>) -> Result<(), std::io::Error> {
        let dir : &str = &oid[0..2];
        let filename : &str = &oid[2..];
        let object_path = self.path.as_path().join(dir).join(filename);

        // If object already exists, we are certain that the contents
        // have not changed. So there is no need to write it again.
        if object_path.exists() {
            return Ok(())
        }

        let dir_path = object_path.parent().expect("invalid parent path");
        fs::create_dir_all(dir_path)?;
        let mut temp_file_name = String::from("tmp_obj_");
        temp_file_name.push_str(&Self::generate_temp_name());
        let temp_path = dir_path.join(temp_file_name);

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(temp_path.clone())?;

        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(&content)?;
        let compressed_bytes = e.finish()?;

        file.write_all(&compressed_bytes)?;
        fs::rename(temp_path, object_path)?;
        Ok(())
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
        _ => panic!("invalid command: {}", command),
    }
}
