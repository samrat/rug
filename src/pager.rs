use std::collections::HashMap;
use std::ffi::OsString;

const PAGER_CMD: &str = "less";

lazy_static! {
    static ref PAGER_ENV: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("LESS", "FRX");
        m.insert("LV", "-c");
        m
    };
}

mod utils {
    use std::ffi::{CString, OsString};
    use std::os::unix::ffi::OsStringExt;
    use std::ptr;

    use errno;
    use libc;

    fn split_string(s: &OsString) -> Vec<OsString> {
        match s.clone().into_string() {
            Ok(cmd) => cmd.split_whitespace().map(OsString::from).collect(),
            Err(cmd) => vec![cmd],
        }
    }

    pub fn pipe() -> (i32, i32) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        (fds[0], fds[1])
    }

    pub fn close(fd: i32) {
        assert_eq!(unsafe { libc::close(fd) }, 0);
    }

    pub fn dup2(fd1: i32, fd2: i32) {
        assert!(unsafe { libc::dup2(fd1, fd2) } > -1);
    }

    fn osstring2cstring(s: OsString) -> CString {
        unsafe { CString::from_vec_unchecked(s.into_vec()) }
    }

    pub fn execvp(cmd: &OsString) {
        let cstrings = split_string(cmd)
            .into_iter()
            .map(osstring2cstring)
            .collect::<Vec<_>>();
        let mut args = cstrings.iter().map(|c| c.as_ptr()).collect::<Vec<_>>();
        args.push(ptr::null());
        errno::set_errno(errno::Errno(0));
        unsafe { libc::execvp(args[0], args.as_ptr()) };
    }

    // Helper wrappers around libc::* API
    pub fn fork() -> libc::pid_t {
        unsafe { libc::fork() }
    }
}

pub struct Pager;

impl Pager {
    pub fn setup_pager() {
        let (git_pager, pager) = (std::env::var("GIT_PAGER"), std::env::var("PAGER"));

        let cmd = match (git_pager, pager) {
            (Ok(git_pager), _) => git_pager,
            (_, Ok(pager)) => pager,
            _ => PAGER_CMD.to_string(),
        };

        let pager_cmd = OsString::from(cmd);

        for (k, v) in PAGER_ENV.iter() {
            std::env::set_var(k, v);
        }

        let (pager_stdin, main_stdout) = utils::pipe();
        let pager_pid = utils::fork();
        match pager_pid {
            -1 => {
                // Fork failed
                utils::close(pager_stdin);
                utils::close(main_stdout);
            }
            0 => {
                // Child
                utils::dup2(main_stdout, libc::STDOUT_FILENO);
                utils::close(pager_stdin);
            }
            _ => {
                // Parent-- executes pager
                utils::dup2(pager_stdin, libc::STDIN_FILENO);
                utils::close(main_stdout);
                utils::execvp(&pager_cmd);
            }
        }
    }
}
