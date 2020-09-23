use crate::commands::CommandContext;
use crate::database::commit::Commit;
use crate::database::object::Object;
use crate::database::ParsedObject;
use crate::pager::Pager;
use crate::repository::Repository;
use colored::*;
use std::io::{Read, Write};

pub struct Log<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    // FIXME: This is inconsistent with the struct for every
    // other command.
    // repo: Repository,
    ctx: CommandContext<'a, I, O, E>,
    commits: CommitsLog,
}

impl<'a, I, O, E> Log<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub fn new(ctx: CommandContext<'a, I, O, E>) -> Log<'a, I, O, E> {
        let working_dir = &ctx.dir;
        let root_path = working_dir.as_path();
        let repo = Repository::new(&root_path);
        let current_oid = repo.refs.read_head();
        let commits = CommitsLog::new(current_oid, repo);

        Log { ctx, commits }
    }

    pub fn run(&mut self) -> Result<(), String> {
        Pager::setup_pager();

        self.each_commit(Self::show_commit)?;
        Ok(())
    }

    pub fn each_commit<F>(&mut self, f: F) -> Result<(), String>
    where
        F: Fn(&Commit) -> Result<(), String>,
    {
        for c in &mut self.commits {
            f(&c)?;
        }

        Ok(())
    }

    fn show_commit(commit: &Commit) -> Result<(), String> {
        let author = &commit.author;
        println!();
        println!("commit {}", commit.get_oid().yellow());
        println!("Author: {} <{}>", author.name, author.email);
        println!("Date: {}", author.readable_time());
        println!();

        for line in commit.message.lines() {
            println!("    {}", line);
        }
        Ok(())
    }
}

struct CommitsLog {
    current_oid: Option<String>,
    repo: Repository,
}

impl CommitsLog {
    pub fn new(current_oid: Option<String>, repo: Repository) -> CommitsLog {
        CommitsLog { current_oid, repo }
    }
}

impl Iterator for CommitsLog {
    type Item = Commit;

    fn next(&mut self) -> Option<Commit> {
        if let Some(current_oid) = &self.current_oid {
            if let ParsedObject::Commit(commit) = self.repo.database.load(&current_oid) {
                self.current_oid = commit.parent.clone();
                Some(commit.clone())
            } else {
                None
            }
        } else {
            None
        }
    }
}
