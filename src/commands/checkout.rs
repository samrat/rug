use crate::commands::CommandContext;
use crate::database::tree::TreeEntry;
use crate::database::tree_diff::TreeDiff;
use crate::repository::Repository;
use crate::revision::Revision;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;

pub struct Checkout<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    repo: Repository,
    ctx: CommandContext<'a, I, O, E>,
}

impl<'a, I, O, E> Checkout<'a, I, O, E>
where
    I: Read,
    O: Write,
    E: Write,
{
    pub fn new(ctx: CommandContext<'a, I, O, E>) -> Checkout<'a, I, O, E> {
        let working_dir = &ctx.dir;
        let root_path = working_dir.as_path();
        let repo = Repository::new(&root_path);

        Checkout { repo, ctx }
    }

    pub fn run(&mut self) -> Result<(), String> {
        assert!(self.ctx.args.len() > 2, "no target provided");
        self.repo
            .index
            .load_for_update()
            .map_err(|e| e.to_string())?;

        let target = &self.ctx.args[2];
        let current_oid = self.repo.refs.read_head().expect("failed to read HEAD");

        let mut revision = Revision::new(&mut self.repo, target);
        let target_oid = match revision.resolve() {
            Ok(oid) => oid,
            Err(errors) => {
                let mut v = vec![];
                for error in errors {
                    v.push(format!("error: {}", error.message));
                    for h in error.hint {
                        v.push(format!("hint: {}", h));
                    }
                }

                v.push("\n".to_string());

                return Err(v.join("\n"));
            }
        };

        let tree_diff = self.tree_diff(&current_oid, &target_oid);
        let mut migration = self.repo.migration(tree_diff);
        migration.apply_changes()?;

        self.repo.index.write_updates().map_err(|e| e.to_string())?;
        self.repo
            .refs
            .update_head(&target_oid)
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    fn tree_diff(
        &mut self,
        a: &str,
        b: &str,
    ) -> HashMap<PathBuf, (Option<TreeEntry>, Option<TreeEntry>)> {
        let mut td = TreeDiff::new(&mut self.repo.database);
        td.compare_oids(
            Some(a.to_string()),
            Some(b.to_string()),
            std::path::Path::new(""),
        );
        td.changes
    }
}

#[cfg(test)]
mod tests {
    use crate::commands::tests::*;
    use std::collections::HashMap;

    lazy_static! {
        static ref BASE_FILES: HashMap<&'static str, &'static str> = {
            let mut m = HashMap::new();
            m.insert("1.txt", "1");
            m.insert("outer/2.txt", "2");
            m.insert("outer/inner/3.txt", "3");
            m
        };
    }

    fn commit_all(cmd_helper: &mut CommandHelper) {
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.commit("change");
    }

    fn commit_and_checkout(cmd_helper: &mut CommandHelper, revision: &str) {
        commit_all(cmd_helper);
        cmd_helper.jit_cmd(&["checkout", revision]).unwrap();
    }

    fn before(cmd_helper: &mut CommandHelper) {
        cmd_helper.jit_cmd(&["init"]).unwrap();
        for (filename, contents) in BASE_FILES.iter() {
            cmd_helper
                .write_file(filename, contents.as_bytes())
                .unwrap();
        }
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.commit("first");
    }

    fn assert_stale_file(error: Result<(String, String), String>, filename: &str) {
        if let Err(error) = error {
            assert_eq!(error,
                       format!("Your local changes to the following files would be overwritten by checkout:\n\t{}\nPlease commit your changes to stash them before you switch branches\n\n", filename));
        } else {
            assert!(false, format!("Expected Err but got {:?}", error));
        }
    }

    #[test]
    fn updates_a_changed_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");
        cmd_helper.assert_workspace(BASE_FILES.clone());
    }

    #[test]
    fn fails_to_update_a_modified_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"conflict").unwrap();
        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "1.txt");
    }

    #[test]
    fn fails_to_update_a_modified_equal_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("1.txt", b"1").unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "1.txt");
    }

    #[test]
    fn fails_to_update_a_changed_mode_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.make_executable("1.txt").unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "1.txt");
    }

    #[test]
    fn restores_a_deleted_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("1.txt").unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();
        cmd_helper.assert_workspace(BASE_FILES.clone());
    }

    #[test]
    fn restores_files_from_a_deleted_directory() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper
            .write_file("outer/inner/3.txt", b"changed")
            .unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer").unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        let mut expected_workspace = BASE_FILES.clone();
        expected_workspace.remove("outer/2.txt");
        cmd_helper.assert_workspace(expected_workspace);

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(" D outer/2.txt\n");
    }

    #[test]
    fn fails_to_update_a_staged_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("1.txt", b"conflict").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "1.txt");
    }

    #[test]
    fn updates_a_staged_equal_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("1.txt", b"1").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        cmd_helper.assert_workspace(BASE_FILES.clone());
    }

    #[test]
    fn fails_to_update_a_staged_changed_mode_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.make_executable("1.txt").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "1.txt");
    }

    #[test]
    fn fails_to_update_an_unindexed_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("1.txt").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "1.txt");
    }

    #[test]
    fn fails_to_update_an_unindexed_and_untracked_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("1.txt").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.write_file("1.txt", b"conflict").unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "1.txt");
    }

    #[test]
    fn fails_to_update_an_unindexed_directory() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper
            .write_file("outer/inner/3.txt", b"changed")
            .unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/inner/3.txt");
    }

    #[test]
    fn fails_to_update_with_a_file_at_a_parent_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper
            .write_file("outer/inner/3.txt", b"changed")
            .unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.write_file("outer/inner", b"conflict").unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/inner/3.txt");
    }

    #[test]
    fn fails_to_update_with_a_staged_file_at_a_parent_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper
            .write_file("outer/inner/3.txt", b"changed")
            .unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.write_file("outer/inner", b"conflict").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/inner/3.txt");
    }

    #[test]
    fn fails_to_update_with_an_unstaged_file_at_a_parent_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper
            .write_file("outer/inner/3.txt", b"changed")
            .unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        cmd_helper.write_file("outer/inner", b"conflict").unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/inner/3.txt");
    }

    #[test]
    fn fails_to_update_with_a_file_at_a_child_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/2.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/2.txt").unwrap();
        cmd_helper
            .write_file("outer/2.txt/extra.log", b"conflict")
            .unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/2.txt");
    }

    #[test]
    fn fails_to_update_with_a_staged_file_at_a_child_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/2.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/2.txt").unwrap();
        cmd_helper
            .write_file("outer/2.txt/extra.log", b"conflict")
            .unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/2.txt");
    }

    #[test]
    fn removes_a_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("94.txt", b"94").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }
}
