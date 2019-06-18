use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::str;

mod add;
use add::add_command;
mod init;
use init::init_command;
mod commit;
use commit::commit_command;
// mod status;

pub struct CommandContext<I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub dir: PathBuf,
    pub env: HashMap<String, String>,
    pub args: Vec<String>,
    pub stdin: I,
    pub stdout: O,
    pub stderr: E,
}

pub fn execute<I, O, E>(ctx: CommandContext<I, O, E>) -> Result<(), String>
where
    I: Read,
    O: Write,
    E: Write,
{
    if ctx.args.len() < 2 {
        return Err("No command provided\n".to_string());
    }
    let command = &ctx.args[1];
    match &command[..] {
        "init" => init_command(ctx),
        "commit" => commit_command(ctx),
        "add" => add_command(ctx),
        _ => Err(format!("invalid command: {}\n", command)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Repository;
    use crate::util::*;
    use std::env;
    use std::fs::{self, OpenOptions};
    use std::io::Cursor;
    use std::path::Path;

    pub fn gen_repo_path() -> PathBuf {
        let mut temp_dir = generate_temp_name();
        temp_dir.push_str("_rug_test");
        let repo_path = env::temp_dir()
            .canonicalize()
            .expect("canonicalization failed")
            .join(temp_dir);
        repo_path.to_path_buf()
    }

    pub fn repo(repo_path: &Path) -> Repository {
        Repository::new(&repo_path.join(".git"))
    }

    pub fn jit_cmd(repo_path: &Path, args: Vec<&str>) -> Result<(String, String), String> {
        let stdin = String::new();
        let mut stdout = Cursor::new(vec![]);
        let mut stderr = Cursor::new(vec![]);

        let ctx = CommandContext {
            dir: Path::new(repo_path).to_path_buf(),
            env: HashMap::new(),
            args: args.iter().map(|a| a.to_string()).collect::<Vec<String>>(),
            stdin: stdin.as_bytes(),
            stdout: &mut stdout,
            stderr: &mut stderr,
        };

        match execute(ctx) {
            Ok(_) => Ok((
                str::from_utf8(&stdout.into_inner())
                    .expect("invalid stdout")
                    .to_string(),
                str::from_utf8(&stderr.into_inner())
                    .expect("invalid stderr")
                    .to_string(),
            )),
            Err(e) => Err(e),
        }
    }

    pub fn write_file(
        repo_path: &Path,
        file_name: &str,
        contents: &[u8],
    ) -> Result<(), std::io::Error> {
        let path = Path::new(repo_path).join(file_name);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .truncate(true)
            .open(&path)?;
        file.write_all(contents)?;

        Ok(())
    }

    pub fn assert_output(stream: &str, expected: &str) {
        assert_eq!(stream, expected);
    }
}
