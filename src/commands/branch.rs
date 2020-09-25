use crate::commands::CommandContext;
use crate::database::object::Object;
use crate::database::{Database, ParsedObject};
use crate::pager::Pager;
use crate::refs::Ref;
use crate::repository::Repository;
use crate::revision::Revision;
use colored::*;
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

        if options.is_present("delete") || options.is_present("force_delete") {
            self.delete_branches(args)?;
        } else if args.is_empty() {
            self.list_branches()?;
        } else {
            let branch_name = args.get(0).expect("no branch name provided");
            let start_point = args.get(1);
            self.create_branch(branch_name, start_point)?;
        }
        Ok(())
    }

    fn list_branches(&mut self) -> Result<(), String> {
        let current = self.repo.refs.current_ref("HEAD");
        let mut branches = self.repo.refs.list_branches();
        branches.sort();

        let max_width = branches
            .iter()
            .map(|b| self.repo.refs.ref_short_name(b).len())
            .max()
            .unwrap_or(0);

        Pager::setup_pager();

        for r#ref in branches {
            let info = self.format_ref(&r#ref, &current);
            let extended_info = self.extended_branch_info(&r#ref, max_width);
            println!("{}{}", info, extended_info);
        }

        Ok(())
    }

    fn format_ref(&self, r#ref: &Ref, current: &Ref) -> String {
        if r#ref == current {
            format!("* {}", self.repo.refs.ref_short_name(r#ref).green())
        } else {
            format!("  {}", self.repo.refs.ref_short_name(r#ref))
        }
    }

    fn extended_branch_info(&mut self, r#ref: &Ref, max_width: usize) -> String {
        if self
            .ctx
            .options
            .as_ref()
            .map(|o| o.is_present("verbose"))
            .unwrap_or(false)
        {
            let oid = self
                .repo
                .refs
                .read_oid(r#ref)
                .expect("unable to resolve branch to oid");
            let commit = if let ParsedObject::Commit(commit) = self.repo.database.load(&oid) {
                commit
            } else {
                panic!("branch ref was not pointing to commit");
            };
            let oid = commit.get_oid();
            let short = Database::short_oid(&oid);
            let ref_short_name = self.repo.refs.ref_short_name(r#ref);
            format!(
                "{:width$}{} {}",
                " ",
                short,
                commit.title_line(),
                width = (max_width - ref_short_name.len() + 1)
            )
        } else {
            "".to_string()
        }
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

    fn delete_branches(&mut self, branch_names: Vec<&str>) -> Result<(), String> {
        for branch in branch_names {
            self.delete_branch(branch)?;
        }
        Ok(())
    }

    fn delete_branch(&mut self, branch_name: &str) -> Result<(), String> {
        let force = self
            .ctx
            .options
            .as_ref()
            .map(|o| o.is_present("force") || o.is_present("force_delete"))
            .unwrap_or(false);
        if !force {
            return Ok(());
        }

        let oid = self.repo.refs.delete_branch(branch_name)?;
        let short = Database::short_oid(&oid);

        println!("Deleted branch {} (was {})", branch_name, short);
        Ok(())
    }
}
