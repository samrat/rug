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

struct Target {
    path: String,
    oid: String,
    mode: Option<u32>,
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
                ChangeType::Modified => {
                    self.print_diff(self.from_index(path), self.from_file(path))
                }
                ChangeType::Deleted => {
                    self.print_diff(self.from_index(path), self.from_nothing(path))
                }
                _ => panic!("NYI"),
            }
        }
        Ok(())
    }

    fn print_diff(&mut self, mut a: Target, mut b: Target) {
        if a.oid == b.oid && a.mode == b.mode {
            return;
        }

        a.path = format!("a/{}", a.path);
        b.path = format!("b/{}", b.path);

        writeln!(self.ctx.stdout, "diff --git {} {}", a.path, b.path);
        self.print_diff_mode(&a, &b);
        self.print_diff_content(&a, &b);
    }

    fn print_diff_mode(&mut self, a: &Target, b: &Target) {
        if b.mode == None {
            writeln!(
                self.ctx.stdout,
                "deleted file mode {:o}",
                a.mode.expect("missing mode")
            );
        } else if a.mode != b.mode {
            writeln!(
                self.ctx.stdout,
                "old mode {:o}",
                a.mode.expect("missing mode")
            );
            writeln!(
                self.ctx.stdout,
                "new mode {:o}",
                b.mode.expect("missing mode")
            );
        }
    }

    fn print_diff_content(&mut self, a: &Target, b: &Target) {
        if a.oid == b.oid {
            return;
        }

        writeln!(
            self.ctx.stdout,
            "index {} {}{}",
            short(&a.oid),
            short(&b.oid),
            if a.mode == b.mode {
                format!(" {:o}", a.mode.expect("Missing mode"))
            } else {
                format!("")
            }
        );
        writeln!(self.ctx.stdout, "--- {}", a.path);
        writeln!(self.ctx.stdout, "+++ {}", b.path);
    }

    fn from_index(&self, path: &str) -> Target {
        let entry = self
            .repo
            .index
            .entry_for_path(path)
            .expect("Path not found in index");
        Target {
            path: path.to_string(),
            oid: entry.oid.clone(),
            mode: Some(entry.mode),
        }
    }

    fn from_file(&self, path: &str) -> Target {
        let blob = Blob::new(
            self.repo
                .workspace
                .read_file(path)
                .expect("Failed to read file")
                .as_bytes(),
        );
        let oid = blob.get_oid();
        let mode = self.repo.stats.get(path).unwrap().mode();
        Target {
            path: path.to_string(),
            oid,
            mode: Some(mode),
        }
    }

    fn from_nothing(&self, path: &str) -> Target {
        Target {
            path: path.to_string(),
            oid: NULL_OID.to_string(),
            mode: None,
        }
    }
}

fn short(oid: &str) -> &str {
    Database::short_oid(oid)
}
