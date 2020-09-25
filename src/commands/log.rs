use crate::commands::CommandContext;
use crate::database::commit::Commit;
use crate::database::object::Object;
use crate::database::{Database, ParsedObject};
use crate::pager::Pager;
use crate::refs::Ref;
use crate::repository::Repository;
use colored::*;
use std::collections::HashMap;
use std::io::{Read, Write};

#[derive(Clone, Copy)]
enum FormatOption {
    Medium,
    OneLine,
}

#[derive(Clone, Copy)]
enum DecorateOption {
    Auto,
    Short,
    Full,
    No,
}

struct Options {
    abbrev: bool,
    format: FormatOption,
    decorate: DecorateOption,
}

pub struct Log<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    current_oid: Option<String>,
    repo: Repository,
    ctx: CommandContext<'a, I, O, E>,
    options: Options,
    reverse_refs: Option<HashMap<String, Vec<Ref>>>,
    current_ref: Option<Ref>,
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
        let ctx_options = ctx.options.as_ref().unwrap().clone();
        let options = Self::define_options(ctx_options);

        Log {
            ctx,
            repo,
            current_oid,
            options,
            reverse_refs: None,
            current_ref: None,
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

        let mut decorate = DecorateOption::Short;

        if options.is_present("decorate") {
            decorate = match options.value_of("decorate").unwrap() {
                "full" => DecorateOption::Full,
                "short" => DecorateOption::Short,
                "no" => DecorateOption::No,
                _ => unimplemented!(),
            }
        }

        if options.is_present("no-decorate") {
            decorate = DecorateOption::No;
        }

        Options {
            abbrev: abbrev.unwrap_or(false),
            format,
            decorate,
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        Pager::setup_pager();

        self.reverse_refs = Some(self.repo.refs.reverse_refs());
        self.current_ref = Some(self.repo.refs.current_ref("HEAD"));

        // FIXME: Print commits as they are returned by the iterator
        // instead of collecting into a Vec.
        let mut commits = vec![];
        for c in &mut self.into_iter() {
            commits.push(c);
        }

        commits
            .iter()
            .for_each(|commit| self.show_commit(commit).unwrap());
        Ok(())
    }

    fn show_commit(&self, commit: &Commit) -> Result<(), String> {
        match self.options.format {
            FormatOption::Medium => {
                self.show_commit_medium(commit)?; // , abbrev, decorate, reverse_refs, current_ref)
            }
            FormatOption::OneLine => {
                self.show_commit_oneline(commit)?; // , abbrev, decorate, reverse_refs, current_ref)
            }
        }

        Ok(())
    }

    fn abbrev(&self, commit: &Commit) -> String {
        if self.options.abbrev {
            let oid = commit.get_oid();
            Database::short_oid(&oid).to_string()
        } else {
            commit.get_oid()
        }
    }

    fn show_commit_medium(&self, commit: &Commit) -> Result<(), String> {
        let author = &commit.author;
        println!();
        println!(
            "commit {} {}",
            self.abbrev(commit).yellow(),
            self.decorate(commit)
        );
        println!("Author: {} <{}>", author.name, author.email);
        println!("Date: {}", author.readable_time());
        println!();

        for line in commit.message.lines() {
            println!("    {}", line);
        }
        Ok(())
    }

    fn show_commit_oneline(&self, commit: &Commit) -> Result<(), String> {
        println!(
            "{} {} {}",
            self.abbrev(commit).yellow(),
            self.decorate(commit),
            commit.title_line()
        );

        Ok(())
    }

    fn decorate(&self, commit: &Commit) -> String {
        match self.options.decorate {
            DecorateOption::No | DecorateOption::Auto => return "".to_string(), // TODO: check isatty
            _ => (),
        }

        let refs = self.reverse_refs.as_ref().unwrap().get(&commit.get_oid());
        if let Some(refs) = refs {
            let (head, refs): (Vec<&Ref>, Vec<&Ref>) = refs.into_iter().partition(|r#ref| {
                r#ref.is_head() && !self.current_ref.as_ref().unwrap().is_head()
            });
            let names: Vec<_> = refs
                .iter()
                .map(|r#ref| self.decoration_name(head.get(0), r#ref))
                .collect();

            format!(
                " {}{}{}",
                "(".yellow(),
                names.join(&", ".yellow()),
                ")".yellow()
            )
        } else {
            "".to_string()
        }
    }

    fn decoration_name(&self, head: Option<&&Ref>, r#ref: &Ref) -> String {
        let mut name = match self.options.decorate {
            DecorateOption::Short | DecorateOption::Auto => self.repo.refs.ref_short_name(r#ref),
            DecorateOption::Full => r#ref.path().to_string(),
            _ => unimplemented!(),
        };

        name = name.bold().color(Self::ref_color(&r#ref)).to_string();

        if let Some(head) = head {
            if r#ref == self.current_ref.as_ref().unwrap() {
                name = format!("{} -> {}", "HEAD", name)
                    .color(Self::ref_color(head))
                    .to_string();
            }
        }

        name
    }

    fn ref_color(r#ref: &Ref) -> &str {
        if r#ref.is_head() {
            "cyan"
        } else {
            "green"
        }
    }
}

impl<'a, I, O, E> Iterator for Log<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
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
