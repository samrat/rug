extern crate chrono;
extern crate crypto;
extern crate flate2;
extern crate rand;
#[macro_use]
extern crate lazy_static;
extern crate regex;

use std::collections::HashMap;
use std::env;
use std::io::{self, Write};

mod lockfile;

mod database;
mod index;
mod refs;
mod repository;
mod util;
mod workspace;
mod diff;
mod pager;
mod revision;

mod commands;
use commands::{execute, CommandContext};

use database::commit::Author;

fn main() {
    let args: Vec<String> = env::args().collect();
    let ctx = CommandContext {
        dir: env::current_dir().unwrap(),
        env: &env::vars().collect::<HashMap<String, String>>(),
        args,
        stdin: io::stdin(),
        stdout: io::stdout(),
        stderr: io::stderr(),
    };

    match execute(ctx) {
        Ok(_) => (),
        Err(msg) => {
            io::stderr().write_all(msg.as_bytes()).unwrap();
            std::process::exit(128);
        }
    }
}
