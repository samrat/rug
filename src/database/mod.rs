use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::str;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::index;
use crate::util::*;

pub mod blob;
pub mod commit;
pub mod object;
pub mod tree;
pub mod tree_diff;

use blob::Blob;
use commit::Commit;
use object::Object;
use tree::{Tree, TREE_MODE};

#[derive(Debug)]
pub enum ParsedObject {
    Commit(Commit),
    Blob(Blob),
    Tree(Tree),
}

impl ParsedObject {
    pub fn obj_type(&self) -> &str {
        match self {
            &ParsedObject::Commit(_) => "commit",
            &ParsedObject::Blob(_) => "blob",
            &ParsedObject::Tree(_) => "tree",
        }
    }

    pub fn get_oid(&self) -> String {
        match self {
            ParsedObject::Commit(obj) => obj.get_oid(),
            ParsedObject::Blob(obj) => obj.get_oid(),
            ParsedObject::Tree(obj) => obj.get_oid(),
        }
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

    fn mode(&self) -> u32 {
        if self.mode == TREE_MODE {
            return TREE_MODE;
        }
        if self.is_executable() {
            return 0o100755;
        } else {
            return 0o100644;
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
            .expect(&format!("failed to open file: {:?}", self.object_path(oid)));
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

    pub fn short_oid(oid: &str) -> &str {
        &oid[0..6]
    }

    pub fn prefix_match(&self, name: &str) -> Vec<String> {
        let object_path = self.object_path(name);
        let dirname = object_path
            .parent()
            .expect("Could not get parent from object_path");

        let oids: Vec<_> = fs::read_dir(&dirname)
            .expect("read_dir call failed")
            .map(|f| {
                format!(
                    "{}{}",
                    dirname
                        .file_name()
                        .expect("could not get filename")
                        .to_str()
                        .expect("conversion from OsStr to str failed"),
                    f.unwrap()
                        .file_name()
                        .to_str()
                        .expect("conversion from OsStr to str failed")
                )
            })
            .filter(|o| o.starts_with(name))
            .collect();

        oids
    }
}
