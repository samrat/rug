use crate::commands::CommandContext;
use crate::database::blob::Blob;
use crate::database::object::Object;
use crate::database::Database;
use crate::repository::{ChangeType, Repository};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const NULL_OID: &str = "0000000";
const NULL_PATH: &str = "/dev/null";

pub struct Diff<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    repo: Repository,
    ctx: CommandContext<'a, I, O, E>,
}

impl<'a, I, O, E> Diff<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub fn new(ctx: CommandContext<'a, I, O, E>) -> Diff<'a, I, O, E> {
        let working_dir = &ctx.dir;
        let root_path = working_dir.as_path();
        let repo = Repository::new(&root_path);

        Diff { ctx, repo }
    }

    pub fn run(&mut self) -> Result<(), String> {
        self.repo.index.load();
        self.repo.initialize_status();

        for (path, state) in &self.repo.workspace_changes.clone() {
            match state {
                ChangeType::Modified => self.diff_file_modified(&path),
                ChangeType::Deleted => self.diff_file_deleted(&path),
                _ => panic!("NYI"),
            }
        }
        Ok(())
    }

    fn diff_file_modified(&mut self, path: &str) {
        let entry = self
            .repo
            .index
            .entry_for_path(path)
            .expect("Path not found in index");
        let a_oid = &entry.oid;
        let a_mode = &entry.mode;
        let a_path = format!("a/{}", path);

        let blob = Blob::new(
            self.repo
                .workspace
                .read_file(path)
                .expect("Failed to read file")
                .as_bytes(),
        );
        let b_oid = blob.get_oid();
        let b_path = format!("b/{}", path);
        let b_mode = self.repo.stats.get(path).unwrap().mode();

        writeln!(self.ctx.stdout, "diff --git {} {}", a_path, b_path);

        if a_mode != &b_mode {
            writeln!(self.ctx.stdout, "old mode {:o}", a_mode);
            writeln!(self.ctx.stdout, "mew mode {:o}", b_mode);
        }

        if a_oid == &b_oid {
            return;
        }

        writeln!(
            self.ctx.stdout,
            "index {} {}{}",
            short(&a_oid),
            short(&b_oid),
            if a_mode == &b_mode {
                format!(" {:o}", a_mode)
            } else {
                format!("")
            }
        );

        writeln!(self.ctx.stdout, "--- {}", a_path);
        writeln!(self.ctx.stdout, "+++ {}", b_path);
    }

    fn diff_file_deleted(&mut self, path: &str) {
        let entry = self
            .repo
            .index
            .entry_for_path(path)
            .expect("Path not found in index");
        let a_oid = &entry.oid;
        let a_mode = &entry.mode;
        let a_path = format!("a/{}", path);

        let b_oid = NULL_OID;
        let b_path = format!("b/{}", path);

        writeln!(self.ctx.stdout, "diff --git {} {}", a_path, b_path);
        writeln!(self.ctx.stdout, "deleted file mode {:o}", a_mode);
        writeln!(self.ctx.stdout, "index {} {}", short(&a_oid), short(&b_oid),);
        writeln!(self.ctx.stdout, "--- {}", a_path);
        writeln!(self.ctx.stdout, "+++ {}", NULL_PATH);
    }
}

fn short(oid: &str) -> &str {
    Database::short_oid(oid)
}
