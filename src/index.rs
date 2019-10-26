use crypto::digest::Digest;
use crypto::sha1::Sha1;
use std::cmp;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::str;

use crate::lockfile::Lockfile;
use crate::util::*;

const MAX_PATH_SIZE: u16 = 0xfff;
const CHECKSUM_SIZE: u64 = 20;

const HEADER_SIZE: usize = 12; // bytes
const MIN_ENTRY_SIZE: usize = 64;

#[derive(Debug, Clone)]
pub struct Entry {
    ctime: i64,
    ctime_nsec: i64,
    mtime: i64,
    mtime_nsec: i64,
    dev: u64,
    ino: u64,
    uid: u32,
    gid: u32,
    size: u64,
    flags: u16,
    pub mode: u32,
    pub oid: String,
    pub path: String,
}

impl Entry {
    fn is_executable(mode: u32) -> bool {
        (mode >> 6) & 0b1 == 1
    }

    fn mode(mode: u32) -> u32 {
        if Entry::is_executable(mode) {
            0o100755u32
        } else {
            0o100644u32
        }
    }

    fn new(pathname: &str, oid: &str, metadata: &fs::Metadata) -> Entry {
        let path = pathname.to_string();
        Entry {
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            mode: Entry::mode(metadata.mode()),
            uid: metadata.uid(),
            gid: metadata.gid(),
            size: metadata.size(),
            oid: oid.to_string(),
            flags: cmp::min(path.len() as u16, MAX_PATH_SIZE),
            path,
        }
    }

    fn parse(bytes: &[u8]) -> Result<Entry, std::io::Error> {
        let mut metadata_ints: Vec<u32> = vec![];
        for i in 0..10 {
            metadata_ints.push(u32::from_be_bytes(
                bytes[i * 4..i * 4 + 4].try_into().unwrap(),
            ));
        }

        let oid = encode_hex(&bytes[40..60]);
        let flags = u16::from_be_bytes(bytes[60..62].try_into().unwrap());
        let path_bytes = bytes[62..].split(|b| b == &0u8).next().unwrap();
        let path = str::from_utf8(path_bytes).unwrap().to_string();

        Ok(Entry {
            ctime: i64::from(metadata_ints[0]),
            ctime_nsec: i64::from(metadata_ints[1]),
            mtime: i64::from(metadata_ints[2]),
            mtime_nsec: i64::from(metadata_ints[3]),
            dev: u64::from(metadata_ints[4]),
            ino: u64::from(metadata_ints[5]),
            mode: metadata_ints[6],
            uid: metadata_ints[7],
            gid: metadata_ints[8],
            size: u64::from(metadata_ints[9]),

            oid,
            flags,
            path,
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // 10 32-bit integers
        bytes.extend_from_slice(&(self.ctime as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.ctime_nsec as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.mtime as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.mtime_nsec as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.dev as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.ino as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.mode as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.uid as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.gid as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.size as u32).to_be_bytes());

        // 20 bytes (40-char hex-string)
        bytes.extend_from_slice(&decode_hex(&self.oid).expect("invalid oid"));

        // 16-bit
        bytes.extend_from_slice(&self.flags.to_be_bytes());

        bytes.extend_from_slice(self.path.as_bytes());
        bytes.push(0x0);

        // add padding
        while bytes.len() % 8 != 0 {
            bytes.push(0x0)
        }

        bytes
    }

    fn parent_dirs(&self) -> Vec<&str> {
        let path = Path::new(&self.path);
        let mut parent_dirs: Vec<_> = path
            .ancestors()
            .map(|d| d.to_str().expect("invalid filename"))
            .collect();
        parent_dirs.pop(); // drop root dir(always "")
        parent_dirs.reverse();
        parent_dirs.pop(); // drop filename

        parent_dirs
    }

    pub fn stat_match(&self, stat: &fs::Metadata) -> bool {
        (self.mode == Entry::mode(stat.mode())) && (self.size == 0 || self.size == stat.size())
    }

    pub fn times_match(&self, stat: &fs::Metadata) -> bool {
        self.ctime == stat.ctime()
            && self.ctime_nsec == stat.ctime_nsec()
            && self.mtime == stat.mtime()
            && self.mtime_nsec == stat.mtime_nsec()
    }

    pub fn update_stat(&mut self, stat: &fs::Metadata) {
        self.ctime = stat.ctime();
        self.ctime_nsec = stat.ctime_nsec();
        self.mtime = stat.mtime();
        self.mtime_nsec = stat.mtime_nsec();
        self.dev = stat.dev();
        self.ino = stat.ino();
        self.mode = Entry::mode(stat.mode());
        self.uid = stat.uid();
        self.gid = stat.gid();
        self.size = stat.size();
    }
}

pub struct Checksum<T>
where
    T: Read + Write,
{
    file: T,
    digest: Sha1,
}

impl<T> Checksum<T>
where
    T: Read + Write,
{
    fn new(file: T) -> Checksum<T> {
        Checksum {
            file,
            digest: Sha1::new(),
        }
    }

    fn read(&mut self, size: usize) -> Result<Vec<u8>, std::io::Error> {
        let mut buf = vec![0; size];
        self.file.read_exact(&mut buf)?;
        self.digest.input(&buf);

        Ok(buf)
    }

    fn write(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.file.write_all(data)?;
        self.digest.input(data);

        Ok(())
    }

    fn write_checksum(&mut self) -> Result<(), std::io::Error> {
        self.file
            .write_all(&decode_hex(&self.digest.result_str()).unwrap())?;

        Ok(())
    }

    fn verify_checksum(&mut self) -> Result<(), std::io::Error> {
        let hash = self.digest.result_str();

        let mut buf = vec![0; CHECKSUM_SIZE as usize];
        self.file.read_exact(&mut buf)?;

        let sum = encode_hex(&buf);

        if sum != hash {
            return Err(io::Error::new(
                ErrorKind::Other,
                "Checksum does not match value stored on disk",
            ));
        }

        Ok(())
    }
}

pub struct Index {
    pathname: PathBuf,
    pub entries: BTreeMap<String, Entry>,
    parents: HashMap<String, HashSet<String>>,
    lockfile: Lockfile,
    hasher: Option<Sha1>,
    changed: bool,
}

impl Index {
    pub fn new(path: &Path) -> Index {
        Index {
            pathname: path.to_path_buf(),
            entries: BTreeMap::new(),
            parents: HashMap::new(),
            lockfile: Lockfile::new(path),
            hasher: None,
            changed: false,
        }
    }

    pub fn write_updates(&mut self) -> Result<(), std::io::Error> {
        if !self.changed {
            return self.lockfile.rollback();
        }

        let lock = &mut self.lockfile;
        let mut writer: Checksum<&Lockfile> = Checksum::new(lock);

        let mut header_bytes: Vec<u8> = vec![];
        header_bytes.extend_from_slice(b"DIRC");
        header_bytes.extend_from_slice(&2u32.to_be_bytes()); // version no.
        header_bytes.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        writer.write(&header_bytes)?;
        for (_key, entry) in self.entries.clone().iter() {
            writer.write(&entry.to_bytes())?;
        }
        writer.write_checksum()?;
        lock.commit()?;
        Ok(())
    }

    /// Remove any entries whose name matches the name of one of the
    /// new entry's parent directories
    pub fn discard_conflicts(&mut self, entry: &Entry) {
        for parent in entry.parent_dirs() {
            self.remove_entry(parent);
        }

        let to_remove = {
            let mut children = vec![];
            if let Some(children_set) = self.parents.get(&entry.path) {
                for child in children_set {
                    children.push(child.clone())
                }
            }
            children
        };

        for child in to_remove {
            self.remove_entry(&child);
        }
    }

    fn remove_entry(&mut self, pathname: &str) {
        let entry = {
            if let Some(entry) = self.entries.get(pathname) {
                Some(entry.clone())
            } else {
                None
            }
        };

        if let Some(entry) = entry {
            self.entries.remove(pathname);

            for dirname in entry.parent_dirs() {
                if let Some(ref mut children_set) = self.parents.get_mut(dirname) {
                    children_set.remove(pathname);
                    if children_set.is_empty() {
                        self.parents.remove(dirname);
                    }
                }
            }
        }
    }

    pub fn add(&mut self, pathname: &str, oid: &str, metadata: &fs::Metadata) {
        let entry = Entry::new(pathname, oid, metadata);
        self.discard_conflicts(&entry);
        self.store_entry(entry);
        self.changed = true;
    }

    pub fn store_entry(&mut self, entry: Entry) {
        self.entries.insert(entry.path.clone(), entry.clone());

        for dirname in entry.parent_dirs() {
            if let Some(ref mut children_set) = self.parents.get_mut(dirname) {
                children_set.insert(entry.path.clone());
            } else {
                let mut h = HashSet::new();
                h.insert(entry.path.clone());
                self.parents.insert(dirname.to_string(), h);
            }
        }
    }

    pub fn load_for_update(&mut self) -> Result<(), std::io::Error> {
        self.lockfile.hold_for_update()?;
        self.load()?;

        Ok(())
    }

    fn clear(&mut self) {
        self.entries = BTreeMap::new();
        self.hasher = None;
        self.parents = HashMap::new();
        self.changed = false;
    }

    fn open_index_file(&self) -> Option<File> {
        if self.pathname.exists() {
            OpenOptions::new()
                .read(true)
                .open(self.pathname.clone())
                .ok()
        } else {
            None
        }
    }

    fn read_header(checksum: &mut Checksum<File>) -> usize {
        let data = checksum
            .read(HEADER_SIZE)
            .expect("could not read checksum header");
        let signature = str::from_utf8(&data[0..4]).expect("invalid signature");
        let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
        let count = u32::from_be_bytes(data[8..12].try_into().unwrap());

        if signature != "DIRC" {
            panic!("Signature: expected 'DIRC', but found {}", signature);
        }

        if version != 2 {
            panic!("Version: expected '2', but found {}", version);
        }

        count as usize
    }

    fn read_entries(
        &mut self,
        checksum: &mut Checksum<File>,
        count: usize,
    ) -> Result<(), std::io::Error> {
        for _i in 0..count {
            let mut entry = checksum.read(MIN_ENTRY_SIZE)?;
            while entry.last().unwrap() != &0u8 {
                entry.extend_from_slice(&checksum.read(8)?);
            }

            self.store_entry(Entry::parse(&entry)?);
        }

        Ok(())
    }

    pub fn load(&mut self) -> Result<(), std::io::Error> {
        self.clear();
        if let Some(file) = self.open_index_file() {
            let mut reader = Checksum::new(file);
            let count = Index::read_header(&mut reader);
            self.read_entries(&mut reader, count)?;
            reader.verify_checksum()?;
        }

        Ok(())
    }

    pub fn release_lock(&mut self) -> Result<(), std::io::Error> {
        self.lockfile.rollback()
    }

    pub fn is_tracked_path(&self, pathname: &str) -> bool {
        self.entries.contains_key(pathname)
    }

    pub fn is_tracked(&self, pathname: &str) -> bool {
        self.is_tracked_path(pathname) || self.parents.contains_key(pathname)
    }

    pub fn update_entry_stat(&mut self, entry: &mut Entry, stat: &fs::Metadata) {
        entry.update_stat(stat);
        self.changed = true;
    }

    pub fn entry_for_path(&self, path: &str) -> Option<&Entry> {
        self.entries.get(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::blob::Blob;
    use crate::database::object::Object;
    use crate::repository::Repository;
    use rand::random;
    use std::process::Command;

    #[test]
    fn add_files_to_index() -> Result<(), std::io::Error> {
        // Add a file to an index and check that it's there
        let mut temp_dir = generate_temp_name();
        temp_dir.push_str("_jit_test");

        let root_path = Path::new("/tmp").join(temp_dir);
        let mut repo = Repository::new(&root_path);
        fs::create_dir(&root_path)?;

        let oid = encode_hex(&(0..20).map(|_n| random::<u8>()).collect::<Vec<u8>>());

        let f1_filename = "alice.txt";
        let f1_path = root_path.join(f1_filename);
        File::create(&f1_path)?.write(b"file 1")?;
        let stat = repo.workspace.stat_file(f1_filename)?;

        {
            repo.index.clear();
            repo.index.add(f1_filename, &oid, &stat);

            let index_entry_paths: Vec<&String> =
                repo.index.entries.iter().map(|(path, _)| path).collect();

            assert_eq!(vec![f1_filename], index_entry_paths);
        }

        // Replace file with directory
        {
            repo.index.clear();
            repo.index.add("alice.txt", &oid, &stat);
            repo.index.add("alice.txt/nested.txt", &oid, &stat);
            repo.index.add("bob.txt", &oid, &stat);
            let index_entry_paths: Vec<&String> =
                repo.index.entries.iter().map(|(path, _)| path).collect();

            assert_eq!(vec!["alice.txt/nested.txt", "bob.txt"], index_entry_paths);
        }

        // Replace directory with file
        {
            repo.index.clear();
            repo.index.add("alice.txt", &oid, &stat);
            repo.index.add("nested/bob.txt", &oid, &stat);

            repo.index.add("nested", &oid, &stat);

            let index_entry_paths: Vec<&String> =
                repo.index.entries.iter().map(|(path, _)| path).collect();

            assert_eq!(vec!["alice.txt", "nested"], index_entry_paths);
        }

        // Replace directory(with subdirectories) with file
        {
            repo.index.clear();
            repo.index.add("alice.txt", &oid, &stat);
            repo.index.add("nested/bob.txt", &oid, &stat);
            repo.index.add("nested/inner/claire.txt", &oid, &stat);

            repo.index.add("nested", &oid, &stat);

            let index_entry_paths: Vec<&String> =
                repo.index.entries.iter().map(|(path, _)| path).collect();

            assert_eq!(vec!["alice.txt", "nested"], index_entry_paths);
        }

        // Cleanup
        fs::remove_dir_all(&root_path)?;

        Ok(())
    }

    #[test]
    fn emit_index_file_same_as_stock_git() -> Result<(), std::io::Error> {
        // Create index file, using "stock" git and our implementation and
        // check that they are byte-for-byte equal

        let mut temp_dir = generate_temp_name();
        temp_dir.push_str("_jit_test");

        let root_path = Path::new("/tmp").join(temp_dir);
        let mut repo = Repository::new(&root_path);
        fs::create_dir(&root_path)?;

        let git_path = root_path.join(".git");
        fs::create_dir(&git_path)?;

        repo.index.load_for_update()?;

        // Create some files
        File::create(root_path.join("f1.txt"))?.write(b"file 1")?;
        File::create(root_path.join("f2.txt"))?.write(b"file 2")?;

        // Create an index out of those files
        for pathname in repo.workspace.list_files(&root_path)? {
            let data = repo.workspace.read_file(&pathname)?;
            let stat = repo.workspace.stat_file(&pathname)?;

            let blob = Blob::new(data.as_bytes());
            repo.database.store(&blob)?;

            repo.index.add(&pathname, &blob.get_oid(), &stat);
        }

        repo.index.write_updates()?;

        // Store contents of our index file
        let mut our_index = File::open(&git_path.join("index"))?;
        let mut our_index_contents = Vec::new();
        our_index.read_to_end(&mut our_index_contents)?;

        // Remove .git dir that we created
        fs::remove_dir_all(&git_path)?;

        // Create index using "stock" git
        let _git_init_output = Command::new("git")
            .current_dir(&root_path)
            .arg("init")
            .arg(".")
            .output();
        let _git_output = Command::new("git")
            .current_dir(&root_path)
            .arg("add")
            .arg(".")
            .output();

        let mut git_index = File::open(&git_path.join("index"))?;
        let mut git_index_contents = Vec::new();
        git_index.read_to_end(&mut git_index_contents)?;

        assert_eq!(our_index_contents, git_index_contents);

        // Cleanup
        fs::remove_dir_all(&root_path)?;

        Ok(())
    }
}
