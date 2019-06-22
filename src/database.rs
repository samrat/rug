use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::str;

use crypto::digest::Digest;
use crypto::sha1::Sha1;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::commit::Commit;
use crate::index;
use crate::util::*;

const TREE_MODE: &'static str = "40000";

#[derive(Debug)]
pub enum ParsedObject {
    Commit(Commit),
    Blob(Blob),
    Tree(Tree),
}

pub trait Object {
    fn r#type(&self) -> String;
    fn to_string(&self) -> Vec<u8>;

    fn parse(s: &[u8]) -> ParsedObject;

    fn get_oid(&self) -> String {
        let mut hasher = Sha1::new();
        hasher.input(&self.get_content());
        hasher.result_str()
    }

    fn get_content(&self) -> Vec<u8> {
        // TODO: need to do something to force ASCII encoding?
        let string = self.to_string();
        let mut content: Vec<u8> = self.r#type().as_bytes().to_vec();

        content.push(0x20);
        content.extend_from_slice(format!("{}", string.len()).as_bytes());
        content.push(0x0);
        content.extend_from_slice(&string);

        content
    }
}

#[derive(Debug)]
pub struct Blob {
    data: Vec<u8>,
}

impl Blob {
    pub fn new(data: &[u8]) -> Blob {
        Blob {
            data: data.to_vec(),
        }
    }
}

impl Object for Blob {
    fn r#type(&self) -> String {
        "blob".to_string()
    }

    fn to_string(&self) -> Vec<u8> {
        self.data.clone()
    }

    fn parse(s: &[u8]) -> ParsedObject {
        ParsedObject::Blob(Blob::new(s))
    }
}

#[derive(Clone, Debug)]
pub enum TreeEntry {
    Entry(Entry),
    Tree(Tree),
}

impl TreeEntry {
    pub fn mode(&self) -> &str {
        match self {
            TreeEntry::Entry(e) => e.mode(),
            _ => TREE_MODE,
        }
    }

    pub fn get_oid(&self) -> String {
        match self {
            TreeEntry::Entry(e) => e.oid.clone(),
            TreeEntry::Tree(t) => t.get_oid(),
        }
    }

    pub fn is_tree(&self) -> bool {
        match self {
            TreeEntry::Entry(e) => e.mode() == TREE_MODE,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub entries: BTreeMap<String, TreeEntry>,
}

impl Tree {
    pub fn new() -> Tree {
        Tree {
            entries: BTreeMap::new(),
        }
    }

    pub fn build(entries: &[Entry]) -> Tree {
        let mut sorted_entries = entries.to_vec();
        sorted_entries.sort();

        let mut root = Tree::new();
        for entry in sorted_entries.iter() {
            let mut path: Vec<String> = Path::new(&entry.name)
                .iter()
                .map(|c| c.to_str().unwrap().to_string())
                .collect();
            let name = path.pop().expect("file path has zero components");
            root.add_entry(&path, name, entry.clone());
        }

        root
    }

    pub fn add_entry(&mut self, path: &[String], name: String, entry: Entry) {
        if path.is_empty() {
            self.entries.insert(name, TreeEntry::Entry(entry));
        } else if let Some(TreeEntry::Tree(tree)) = self.entries.get_mut(&path[0]) {
            tree.add_entry(&path[1..], name, entry);
        } else {
            let mut tree = Tree::new();
            tree.add_entry(&path[1..], name, entry);
            self.entries.insert(path[0].clone(), TreeEntry::Tree(tree));
        };
    }

    pub fn traverse<F>(&self, f: &F)
    where
        F: Fn(&Tree) -> (),
    {
        // Do a postorder traversal(visit all children first, then
        // process `self`
        for (_name, entry) in self.entries.clone() {
            if let TreeEntry::Tree(tree) = entry {
                tree.traverse(f);
            }
        }

        f(self);
    }
}

impl Object for Tree {
    fn r#type(&self) -> String {
        "tree".to_string()
    }

    fn to_string(&self) -> Vec<u8> {
        let mut tree_vec = Vec::new();
        for (name, entry) in self.entries.iter() {
            let mut entry_vec: Vec<u8> = format!("{} {}\0", entry.mode(), name).as_bytes().to_vec();
            entry_vec.extend_from_slice(&decode_hex(&entry.get_oid()).expect("invalid oid"));
            tree_vec.extend_from_slice(&entry_vec);
        }
        tree_vec
    }

    fn parse(v: &[u8]) -> ParsedObject {
        let mut entries: Vec<Entry> = vec![];

        let mut vs = v;

        while vs.len() > 0 {
            let (mode, rest): (u32, &[u8]) = match vs
                .splitn(2, |c| *c as char == ' ')
                .collect::<Vec<&[u8]>>()
                .as_slice()
            {
                &[mode, rest] => (
                    u32::from_str_radix(str::from_utf8(mode).expect("invalid utf8"), 8)
                        .expect("parsing mode failed"),
                    rest,
                ),
                _ => panic!("EOF while parsing mode"),
            };
            vs = rest;

            let (name, rest) = match vs
                .splitn(2, |c| *c as char == '\u{0}')
                .collect::<Vec<&[u8]>>()
                .as_slice()
            {
                &[name_bytes, rest] => (str::from_utf8(name_bytes).expect("invalid utf8"), rest),
                _ => panic!("EOF while parsing name")
            };
            vs = rest;

            let (oid_bytes, rest) = vs.split_at(20);
            vs = rest;

            let oid = encode_hex(&oid_bytes);

            entries.push(Entry::new(name, &oid, mode));
        }
        ParsedObject::Tree(Tree::build(&entries))
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct Entry {
    name: String,
    oid: String,
    mode: u32,
}

impl From<&index::Entry> for Entry {
    fn from(entry: &index::Entry) -> Entry {
        Entry {
            name: entry.path.clone(),
            oid: entry.oid.clone(),
            mode: entry.mode,
        }
    }
}

impl Entry {
    pub fn new(name: &str, oid: &str, mode: u32) -> Entry {
        Entry {
            name: name.to_string(),
            oid: oid.to_string(),
            mode,
        }
    }

    // if user is allowed to executable, set mode to Executable,
    // else Regular
    fn is_executable(&self) -> bool {
        (self.mode >> 6) & 0b1 == 1
    }

    fn mode(&self) -> &str {
        if self.mode == 0o40000 {
            return TREE_MODE;
        }
        if self.is_executable() {
            return "100755";
        } else {
            return "100644";
        }
    }
}

pub struct Database {
    path: PathBuf,
    objects: HashMap<String, ParsedObject>,
}

impl Database {
    pub fn new(path: &Path) -> Database {
        Database {
            path: path.to_path_buf(),
            objects: HashMap::new(),
        }
    }

    pub fn read_object(&self, oid: &str) -> Option<ParsedObject> {
        let mut contents = vec![];
        let mut file = OpenOptions::new()
            .read(true)
            .create(false)
            .open(self.object_path(oid))
            .expect("failed to open file");
        file.read_to_end(&mut contents)
            .expect("reading file failed");

        let mut z = ZlibDecoder::new(&contents[..]);
        let mut v = vec![];
        z.read_to_end(&mut v).unwrap();
        let mut vs = &v[..];

        let (obj_type, rest) = match vs
            .splitn(2, |c| *c as char == ' ')
            .collect::<Vec<&[u8]>>()
            .as_slice()
        {
            &[type_bytes, rest] => (
                str::from_utf8(type_bytes).expect("failed to parse type"),
                rest,
            ),
            _ => panic!("EOF while parsing type"),
        };
        vs = rest;

        let (_size, rest) = match vs
            .splitn(2, |c| *c as char == '\u{0}')
            .collect::<Vec<&[u8]>>()
            .as_slice()
        {
            &[size_bytes, rest] => (
                str::from_utf8(size_bytes).expect("failed to parse size"),
                rest,
            ),
            _ => panic!("EOF while parsing size"),
        };
        vs = rest;

        match obj_type {
            "commit" => return Some(Commit::parse(&rest)),
            "blob" => return Some(Blob::parse(&rest)),
            "tree" => return Some(Tree::parse(&rest)),
            _ => unimplemented!(),
        }
    }

    pub fn load(&mut self, oid: &str) -> &ParsedObject {
        let o = self.read_object(oid);
        self.objects.insert(oid.to_string(), o.unwrap());

        self.objects.get(oid).unwrap()
    }

    pub fn store<T>(&self, obj: &T) -> Result<(), std::io::Error>
    where
        T: Object,
    {
        let oid = obj.get_oid();
        let content = obj.get_content();

        self.write_object(oid, content)
    }

    fn object_path(&self, oid: &str) -> PathBuf {
        let dir: &str = &oid[0..2];
        let filename: &str = &oid[2..];

        self.path.as_path().join(dir).join(filename)
    }

    fn write_object(&self, oid: String, content: Vec<u8>) -> Result<(), std::io::Error> {
        let object_path = self.object_path(&oid);

        // If object already exists, we are certain that the contents
        // have not changed. So there is no need to write it again.
        if object_path.exists() {
            return Ok(());
        }

        let dir_path = object_path.parent().expect("invalid parent path");
        fs::create_dir_all(dir_path)?;
        let mut temp_file_name = String::from("tmp_obj_");
        temp_file_name.push_str(&generate_temp_name());
        let temp_path = dir_path.join(temp_file_name);

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&temp_path)?;

        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(&content)?;
        let compressed_bytes = e.finish()?;

        file.write_all(&compressed_bytes)?;
        fs::rename(temp_path, object_path)?;
        Ok(())
    }
}
