use std::path::Path;

use crate::database::Database;
use crate::index::Index;
use crate::refs::Refs;
use crate::workspace::Workspace;

pub struct Repository {
    pub database: Database,
    pub index: Index,
    pub refs: Refs,
    pub workspace: Workspace,
}

impl Repository {
    pub fn new(git_path: &Path) -> Repository {
        let db_path = git_path.join("objects");

        Repository {
            database: Database::new(&db_path),
            index: Index::new(&git_path.join("index")),
            refs: Refs::new(&git_path),
            workspace: Workspace::new(git_path.parent().unwrap()),
        }
    }
}
