use crate::commands::CommandContext;
use crate::database::tree::TreeEntry;
use crate::database::tree_diff::TreeDiff;
use crate::repository::Repository;
use crate::revision::Revision;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;

pub struct Checkout<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    repo: Repository,
    ctx: CommandContext<'a, I, O, E>,
}

impl<'a, I, O, E> Checkout<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub fn new(ctx: CommandContext<'a, I, O, E>) -> Checkout<'a, I, O, E> {
        let working_dir = &ctx.dir;
        let root_path = working_dir.as_path();
        let repo = Repository::new(&root_path);

        Checkout { repo, ctx }
    }

    pub fn run(&mut self) -> Result<(), String> {
        assert!(self.ctx.args.len() > 2, "no target provided");
        let target = &self.ctx.args[2];
        let current_oid = self.repo.refs.read_head().expect("failed to read HEAD");

        let mut revision = Revision::new(&mut self.repo, target);
        let target_oid = match revision.resolve() {
            Ok(oid) => oid,
            Err(errors) => {
                let mut v = vec![];
                for error in errors {
                    v.push(format!("error: {}", error.message));
                    for h in error.hint {
                        v.push(format!("hint: {}", h));
                    }
                }

                v.push("\n".to_string());

                return Err(v.join("\n"));
            }
        };

        let tree_diff = self.tree_diff(&current_oid, &target_oid);
        let mut migration = self.repo.migration(tree_diff);
        migration.apply_changes();

        Ok(())
    }

    fn tree_diff(
        &mut self,
        a: &str,
        b: &str,
    ) -> HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)> {
        let mut td = TreeDiff::new(&mut self.repo.database);
        td.compare_oids(
            Some(a.to_string()),
            Some(b.to_string()),
            std::path::Path::new(""),
        );
        td.changes
    }
}
