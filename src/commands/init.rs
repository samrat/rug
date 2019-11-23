use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use crate::refs::Refs;

use crate::commands::CommandContext;

const DEFAULT_BRANCH: &'static str = "master";

pub fn init_command<I, O, E>(mut ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    let working_dir = ctx.dir;
    let root_path = if ctx.args.len() > 2 {
        Path::new(&ctx.args[2])
    } else {
        working_dir.as_path()
    };
    let git_path = root_path.join(".git");

    for d in ["objects", "refs/heads"].iter() {
        fs::create_dir_all(git_path.join(d)).expect("failed to create dir");
    }

    let refs = Refs::new(&git_path);
    let path = Path::new("refs/heads").join(DEFAULT_BRANCH);
    refs.update_head(&format!("ref: {}", path.to_str().expect("failed to convert path to str"))).map_err(|e| e.to_string())?;

    writeln!(
        ctx.stdout,
        "Initialized empty Jit repository in {:?}\n",
        git_path
    ).map_err(|e| e.to_string())
}
