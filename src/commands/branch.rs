use crate::commands::CommandContext;
use crate::repository::Repository;
use std::io::{Read, Write};

pub struct Branch<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    repo: Repository,
    ctx: CommandContext<'a, I, O, E>,
}

impl<'a, I, O, E> Branch<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub fn new(ctx: CommandContext<'a, I, O, E>) -> Branch<'a, I, O, E> {
        let working_dir = &ctx.dir;
        let root_path = working_dir.as_path();
        let repo = Repository::new(&root_path);

        Branch {
            repo,
            ctx
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        self.create_branch()?;

        Ok(())
    }

    fn create_branch(&self) -> Result<(), String> {
        let branch_name = &self.ctx.args[2];

        self.repo.refs.create_branch(branch_name)?;

        Ok(())
    }
}
