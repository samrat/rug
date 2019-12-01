use crate::refs::Refs;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::commands::CommandContext;

const DEFAULT_BRANCH: &str = "master";

pub fn init_command<I, O, E>(ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    let working_dir = ctx.dir;
    let options = ctx.options.as_ref().unwrap();
    let args: Vec<_> = if let Some(args) = options.values_of("args") {
        args.collect()
    } else {
        vec![]
    };
    let root_path = if !args.is_empty() {
        Path::new(args[0])
    } else {
        working_dir.as_path()
    };
    let git_path = root_path.join(".git");

    for d in ["objects", "refs/heads"].iter() {
        fs::create_dir_all(git_path.join(d)).expect("failed to create dir");
    }

    let refs = Refs::new(&git_path);
    let path = Path::new("refs/heads").join(DEFAULT_BRANCH);
    refs.update_head(&format!(
        "ref: {}",
        path.to_str().expect("failed to convert path to str")
    ))
    .map_err(|e| e.to_string())?;

    println!("Initialized empty Jit repository in {:?}\n", git_path);
    Ok(())
}
