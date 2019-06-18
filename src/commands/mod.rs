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
mod status;
use status::status_command;

#[derive(Debug)]
pub struct CommandContext<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub dir: PathBuf,
    pub env: &'a HashMap<String, String>,
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
        "status" => status_command(ctx),
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

    struct CommandHelper {
        stdin: String,
        stdout: Cursor<Vec<u8>>,
        stderr: Cursor<Vec<u8>>,
        env: HashMap<String, String>,
    }

    impl CommandHelper {
        fn new() -> CommandHelper {
            CommandHelper {
                stdin: String::new(),
                stdout: Cursor::new(vec![]),
                stderr: Cursor::new(vec![]),
                env: HashMap::new(),
            }
        }

        fn set_env(&mut self, key: &str, value: &str) {
            self.env.insert(key.to_string(), value.to_string());
        }

        fn set_stdin(&mut self, s: &str) {
            self.stdin = s.to_string();
        }

        pub fn jit_cmd(
            &mut self,
            repo_path: &Path,
            args: Vec<&str>,
        ) -> Result<(String, String), String> {
            let ctx = CommandContext {
                dir: Path::new(repo_path).to_path_buf(),
                env: &self.env,
                args: args.iter().map(|a| a.to_string()).collect::<Vec<String>>(),
                stdin: self.stdin.as_bytes(),
                stdout: &mut self.stdout,
                stderr: &mut self.stderr,
            };

            match execute(ctx) {
                Ok(_) => Ok((
                    str::from_utf8(&self.stdout.clone().into_inner())
                        .expect("invalid stdout")
                        .to_string(),
                    str::from_utf8(&self.stderr.clone().into_inner())
                        .expect("invalid stderr")
                        .to_string(),
                )),
                Err(e) => {
                    // eprintln!("execute failed: {:}", e);
                    Err(e)
                }
            }
        }
    }

    // TODO: Migrate to CommandHelper method. And remove this.
    pub fn jit_cmd(repo_path: &Path, args: Vec<&str>) -> Result<(String, String), String> {
        let stdin = String::new();
        let mut stdout = Cursor::new(vec![]);
        let mut stderr = Cursor::new(vec![]);

        let ctx = CommandContext {
            dir: Path::new(repo_path).to_path_buf(),
            env: &HashMap::new(),
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
            Err(e) => {
                // eprintln!("execute failed: {:}", e);
                Err(e)
            }
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

    pub fn commit(repo_path: &Path, msg: &str) {
        let repo = repo(&repo_path);
        let mut cmd_helper = CommandHelper::new();

        cmd_helper.set_env("GIT_AUTHOR_NAME", "A. U. Thor");
        cmd_helper.set_env("GIT_AUTHOR_EMAIL", "author@example.com");
        cmd_helper.set_stdin("message");
        cmd_helper.jit_cmd(&repo_path, vec!["", "commit"]);
    }
}
