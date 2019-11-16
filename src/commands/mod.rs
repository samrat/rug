use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;

mod add;
use add::add_command;
mod init;
use init::init_command;
mod commit;
use commit::commit_command;
mod status;
use status::Status;
mod diff;
use diff::Diff;
mod branch;
use branch::Branch;
mod checkout;
use checkout::Checkout;

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
        "status" => {
            let mut cmd = Status::new(ctx);
            cmd.run()
        }
        "diff" => {
            let mut cmd = Diff::new(ctx);
            cmd.run()
        }
        "branch" => {
            let mut cmd = Branch::new(ctx);
            cmd.run()
        }
        "checkout" => {
            let mut cmd = Checkout::new(ctx);
            cmd.run()
        }
        _ => Err(format!("invalid command: {}\n", command)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Repository;
    use crate::util::*;
    use filetime::FileTime;
    use std::env;
    use std::fmt::Debug;
    use std::fs::{self, File, OpenOptions};
    use std::hash::Hash;
    use std::io::Cursor;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::str;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn gen_repo_path() -> PathBuf {
        let mut temp_dir = generate_temp_name();
        temp_dir.push_str("_rug_test");
        env::temp_dir()
            .canonicalize()
            .expect("canonicalization failed")
            .join(temp_dir)
    }

    pub fn repo(repo_path: &Path) -> Repository {
        Repository::new(&repo_path)
    }

    pub struct CommandHelper {
        repo_path: PathBuf,
        stdin: String,
        stdout: Cursor<Vec<u8>>,
        stderr: Cursor<Vec<u8>>,
        env: HashMap<String, String>,
    }

    impl CommandHelper {
        pub fn new() -> CommandHelper {
            CommandHelper {
                repo_path: gen_repo_path(),
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

        pub fn jit_cmd(&mut self, args: &[&str]) -> Result<(String, String), String> {
            let mut args = args.iter().map(|a| a.to_string()).collect::<Vec<String>>();
            // Command handler assumes first arg is executable name
            args.insert(0, "".to_string());
            let ctx = CommandContext {
                dir: Path::new(&self.repo_path).to_path_buf(),
                env: &self.env,
                args,
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

        pub fn commit(&mut self, msg: &str) {
            self.set_env("GIT_AUTHOR_NAME", "A. U. Thor");
            self.set_env("GIT_AUTHOR_EMAIL", "author@example.com");
            self.set_stdin(msg);
            self.jit_cmd(&["commit"]).unwrap();
        }

        pub fn write_file(&self, file_name: &str, contents: &[u8]) -> Result<(), std::io::Error> {
            let path = Path::new(&self.repo_path).join(file_name);
            fs::create_dir_all(path.parent().unwrap())?;
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)?;
            file.write_all(contents)?;

            Ok(())
        }

        pub fn mkdir(&self, dir_name: &str) -> Result<(), std::io::Error> {
            fs::create_dir_all(self.repo_path.join(dir_name))
        }

        pub fn touch(&self, file_name: &str) -> Result<(), std::io::Error> {
            let path = Path::new(&self.repo_path).join(file_name);
            let now = FileTime::from_unix_time(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("time is broken")
                    .as_secs() as i64,
                0,
            );
            filetime::set_file_times(path, now, now)
        }

        pub fn delete(&self, pathname: &str) -> Result<(), std::io::Error> {
            let path = Path::new(&self.repo_path).join(pathname);

            if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
        }

        pub fn make_executable(&self, file_name: &str) -> Result<(), std::io::Error> {
            let path = self.repo_path.join(file_name);
            let file = File::open(&path)?;
            let metadata = file.metadata()?;
            let mut permissions = metadata.permissions();

            permissions.set_mode(0o744);
            fs::set_permissions(path, permissions)?;
            Ok(())
        }

        pub fn make_unreadable(&self, file_name: &str) -> Result<(), std::io::Error> {
            let path = self.repo_path.join(file_name);
            let file = File::open(&path)?;
            let metadata = file.metadata()?;
            let mut permissions = metadata.permissions();

            permissions.set_mode(0o044);
            fs::set_permissions(path, permissions)?;
            Ok(())
        }

        pub fn assert_index(&self, expected: Vec<(u32, String)>) -> Result<(), std::io::Error> {
            let mut repo = repo(&self.repo_path);
            repo.index.load()?;

            let actual: Vec<(u32, String)> = repo
                .index
                .entries
                .iter()
                .map(|(_, entry)| (entry.mode, entry.path.clone()))
                .collect();

            assert_eq!(expected, actual);

            Ok(())
        }

        pub fn clear_stdout(&mut self) {
            self.stdout = Cursor::new(vec![]);
        }

        pub fn assert_status(&mut self, expected: &str) {
            if let Ok((stdout, _stderr)) = self.jit_cmd(&["status", "--porcelain"]) {
                assert_output(&stdout, expected)
            } else {
                assert!(false);
            }
        }

        pub fn assert_workspace(&self, expected_contents: HashMap<&str, &str>) {
            let mut files = HashMap::new();
            for file in repo(&self.repo_path)
                .workspace
                .list_files(&self.repo_path)
                .unwrap()
            {
                let file_contents = repo(&self.repo_path).workspace.read_file(&file).unwrap();
                files.insert(file, file_contents);
            }

            assert_maps_equal(expected_contents, files);
        }
    }

    impl Drop for CommandHelper {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.repo_path).unwrap();
        }
    }

    pub fn assert_output(stream: &str, expected: &str) {
        assert_eq!(stream, expected);
    }

    fn assert_maps_equal(a: HashMap<&str, &str>, b: HashMap<String, String>) {
        assert_eq!(a.len(), b.len());
        for (k, v) in a {
            if let Some(bv) = b.get(k) {
                assert_eq!(v, *bv);
            }
        }
    }

}
