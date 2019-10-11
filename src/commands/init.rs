use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::commands::CommandContext;

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

    for d in ["objects", "refs"].iter() {
        fs::create_dir_all(git_path.join(d)).expect("failed to create dir");
    }

    writeln!(
        ctx.stdout,
        "Initialized empty Jit repository in {:?}\n",
        git_path
    );

    Ok(())
}
