use crate::commands::CommandContext;
use crate::database::commit::Commit;
use crate::database::object::Object;
use crate::database::{Database, ParsedObject};
use crate::pager::Pager;
use crate::repository::Repository;
use colored::*;
use std::io::{Read, Write};

#[derive(Clone, Copy)]
enum FormatOption {
    Medium,
    OneLine,
}

struct Options {
    abbrev: bool,
    format: FormatOption,
}

pub struct Log<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    // FIXME: This is inconsistent with the struct for every
    // other command.
    // repo: Rc<Repository>,
    ctx: CommandContext<'a, I, O, E>,
    options: Options,
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

        let ctx_options = ctx.options.as_ref().unwrap().clone();
        let options = Self::define_options(ctx_options);

        Log {
            ctx,
            commits,
            options,
        }
    }

    fn define_options(options: clap::ArgMatches) -> Options {
        let mut abbrev = None;

        if options.is_present("abbrev-commit") {
            abbrev = Some(true);
        }

        if options.is_present("no-abbrev-commit") {
            abbrev = Some(false);
        }

        let mut format = FormatOption::Medium;
        if options.is_present("format") || options.is_present("pretty") {
            match options.value_of("format").unwrap() {
                "oneline" => {
                    format = FormatOption::OneLine;
                }
                "medium" => {
                    format = FormatOption::Medium;
                }
                _ => (),
            };
        }

        if options.is_present("oneline") {
            format = FormatOption::OneLine;
            if abbrev == None {
                abbrev = Some(true);
            }
        }

        Options {
            abbrev: abbrev.unwrap_or(false),
            format,
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        Pager::setup_pager();

        let abbrev = self.options.abbrev;
        let log_format = self.options.format;
        self.each_commit(|commit| match log_format {
            FormatOption::Medium => Self::show_commit_medium(commit, abbrev),
            FormatOption::OneLine => Self::show_commit_oneline(commit, abbrev),
        })?;
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

    fn abbrev(commit: &Commit, abbrev: bool) -> String {
        if abbrev {
            let oid = commit.get_oid();
            Database::short_oid(&oid).to_string()
        } else {
            commit.get_oid()
        }
    }

    fn show_commit_medium(commit: &Commit, abbrev: bool) -> Result<(), String> {
        let author = &commit.author;
        println!();
        println!("commit {}", Self::abbrev(commit, abbrev).yellow());
        println!("Author: {} <{}>", author.name, author.email);
        println!("Date: {}", author.readable_time());
        println!();

        for line in commit.message.lines() {
            println!("    {}", line);
        }
        Ok(())
    }

    fn show_commit_oneline(commit: &Commit, abbrev: bool) -> Result<(), String> {
        println!(
            "{} {}",
            Self::abbrev(commit, abbrev).yellow(),
            commit.title_line()
        );

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
