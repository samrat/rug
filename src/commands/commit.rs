use std::io::{Read, Write};

use crate::commands::CommandContext;
use crate::commit::{Author, Commit};
use crate::database::{Entry, Object, Tree};
use crate::repository::Repository;

pub fn commit_command<I, O, E>(mut ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    let working_dir = ctx.dir;
    let root_path = working_dir.as_path();
    let mut repo = Repository::new(&root_path.join(".git"));

    repo.index.load().expect("loading .git/index failed");
    let entries: Vec<Entry> = repo
        .index
        .entries
        .iter()
        .map(|(_path, idx_entry)| Entry::from(idx_entry))
        .collect();
    let root = Tree::build(&entries);
    root.traverse(&|tree| {
        repo.database
            .store(tree)
            .expect("Traversing tree to write to database failed")
    });

    let parent = repo.refs.read_head();
    let author_name = ctx
        .env
        .get("GIT_AUTHOR_NAME")
        .expect("GIT_AUTHOR_NAME not set");
    let author_email = ctx
        .env
        .get("GIT_AUTHOR_EMAIL")
        .expect("GIT_AUTHOR_EMAIL not set");

    let author = Author {
        name: author_name.to_string(),
        email: author_email.to_string(),
    };

    let mut commit_message = String::new();
    ctx.stdin
        .read_to_string(&mut commit_message)
        .expect("reading commit from STDIN failed");

    let commit = Commit::new(&parent, root.get_oid(), author, commit_message);
    repo.database.store(&commit).expect("writing commit failed");
    repo.refs
        .update_head(&commit.get_oid())
        .expect("updating HEAD failed");
    repo.refs
        .update_master_ref(&commit.get_oid())
        .expect("updating master ref failed");

    let commit_prefix = if parent.is_some() {
        ""
    } else {
        "(root-commit) "
    };

    println!("[{}{}] {}", commit_prefix, commit.get_oid(), commit.message);

    Ok(())
}
