use crate::commands::CommandContext;
use crate::repository::Repository;
use crate::revision::Revision;

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

        Branch { repo, ctx }
    }

    pub fn run(&mut self) -> Result<(), String> {
        let options = self.ctx.options.as_ref().unwrap().clone();
        let args: Vec<_> = if let Some(args) = options.values_of("args") {
            args.collect()
        } else {
            vec![]
        };

        let branch_name = args.get(0).expect("no branch name provided");
        let start_point = args.get(1);
        self.create_branch(branch_name, start_point)?;

        Ok(())
    }

    fn create_branch(
        &mut self,
        branch_name: &str,
        start_point: Option<&&str>,
    ) -> Result<(), String> {
        let start_point = if start_point.is_none() {
            self.repo.refs.read_head().expect("empty HEAD")
        } else {
            match Revision::new(&mut self.repo, start_point.unwrap()).resolve() {
                Ok(rev) => rev,
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
            }
        };

        self.repo.refs.create_branch(branch_name, &start_point)?;

        Ok(())
    }
}
