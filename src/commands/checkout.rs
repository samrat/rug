use crate::commands::CommandContext;
use crate::database::object::Object;
use crate::database::tree::TreeEntry;
use crate::database::tree_diff::TreeDiff;
use crate::database::{Database, ParsedObject};
use crate::refs::Ref;
use crate::repository::Repository;
use crate::revision::Revision;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;

const DETACHED_HEAD_MESSAGE: &str =
    "You are in 'detached HEAD' state. You can look around, make experimental 
changes and commit them, and you can discard any commits you make in this
 state without impacting any branches by performing another checkout.

If you want to create a new branch to retain commits you create, you may
do so (now or later) by using the branch command. Example:

  rug branch <new-branch-name>
";

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

    fn read_ref(&self, r#ref: &Ref) -> Option<String> {
        match r#ref {
            Ref::Ref { oid } => Some(oid.to_string()),
            Ref::SymRef { path } => self.repo.refs.read_ref(&path),
        }
    }

    fn print_head_position(&mut self, message: &str, oid: &str) -> Result<(), String> {
        let commit = match self.repo.database.load(oid) {
            ParsedObject::Commit(commit) => commit,
            _ => panic!("oid not a commit"),
        };
        let oid = commit.get_oid();
        let short = Database::short_oid(&oid);

        writeln!(
            self.ctx.stderr,
            "{}",
            format!("{} {} {}", message, short, commit.title_line())
        ).map_err(|e| e.to_string())
    }

    fn print_previous_head(&mut self, current_ref: &Ref, current_oid: &str, target_oid: &str) -> Result<(), String> {
        if current_ref.is_head() && current_oid != target_oid {
            return self.print_head_position("Previous HEAD position was", current_oid);
        }
        Ok(())
    }

    fn print_detachment_notice(&mut self, current_ref: &Ref, target: &str, new_ref: &Ref) -> Result<(), String> {
        if new_ref.is_head() && !current_ref.is_head() {
            return writeln!(
                self.ctx.stderr,
                "{}

{}
",
                format!("Note: checking out '{}'.", target),
                DETACHED_HEAD_MESSAGE
            ).map_err(|e| e.to_string())
        }
        Ok(())
    }

    fn print_new_head(&mut self, current_ref: &Ref, new_ref: &Ref, target: &str, target_oid: &str) -> Result<(), String> {
        if new_ref.is_head() {
            self.print_head_position("HEAD is now at", target_oid)
        } else if new_ref == current_ref {
            writeln!(
                self.ctx.stderr,
                "{}",
                format!("Already on {}", target)).map_err(|e| e.to_string())
        } else {
            writeln!(
                self.ctx.stderr,
                "{}",
                format!("Switched to branch {}", target)).map_err(|e| e.to_string())
        }
    }

    pub fn run(&mut self) -> Result<(), String> {
        assert!(self.ctx.args.len() > 2, "no target provided");
        self.repo
            .index
            .load_for_update()
            .map_err(|e| e.to_string())?;

        let current_ref = self.repo.refs.current_ref("HEAD");
        let current_oid = self
            .read_ref(&current_ref)
            .unwrap_or_else(|| panic!("failed to read ref: {:?}", current_ref));

        let target = &self.ctx.args[2].clone();

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
            .set_head(&target, &target_oid)
            .map_err(|e| e.to_string())?;

        let new_ref = self.repo.refs.current_ref("HEAD");
        self.print_previous_head(&current_ref, &current_oid, &target_oid)?;
        self.print_detachment_notice(&current_ref, &target, &new_ref)?;
        self.print_new_head(&current_ref, &new_ref, &target, &target_oid)?;

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

    fn assert_stale_directory(error: Result<(String, String), String>, filename: &str) {
        if let Err(error) = error {
            assert_eq!(error,
                       format!("Updating the following directories would lose untracked files in them:\n\t{}\n\n\n\n", filename));
        } else {
            assert!(false, format!("Expected Err but got {:?}", error));
        }
    }

    fn assert_remove_conflict(error: Result<(String, String), String>, filename: &str) {
        if let Err(error) = error {
            assert_eq!(error,
                       format!("The following untracked working tree files would be removed by checkout:\n\t{}\nPlease commit your changes to stash them before you switch branches\n\n", filename));
        } else {
            assert!(false, format!("Expected Err but got {:?}", error));
        }
    }

    fn assert_overwrite_conflict(error: Result<(String, String), String>, filename: &str) {
        if let Err(error) = error {
            assert_eq!(error,
                       format!("The following untracked working tree files would be overwritten by checkout:\n\t{}\nPlease move or remove them before you switch branches\n\n", filename));
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

    #[test]
    fn removes_a_file_from_an_existing_directory() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn removes_a_file_from_a_new_directory() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("new/94.txt", b"94").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.assert_noent("new");
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn removes_a_file_from_a_new_nested_directory() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("new/inner/94.txt", b"94").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.assert_noent("new");
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn fails_to_remove_a_modified_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("outer/94.txt", b"conflict").unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/94.txt");
    }

    #[test]
    fn fails_to_remove_a_changed_mode_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.make_executable("outer/94.txt").unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/94.txt");
    }

    #[test]
    fn leaves_a_deleted_file_deleted() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/94.txt").unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn leaves_a_deleted_directory_deleted() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/inner/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        let mut expected_workspace = BASE_FILES.clone();
        expected_workspace.remove("outer/inner/3.txt").unwrap();

        cmd_helper.assert_workspace(expected_workspace);
        cmd_helper.clear_stdout();
        cmd_helper.assert_status(" D outer/inner/3.txt\n");
    }

    #[test]
    fn fails_to_remove_a_staged_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("outer/94.txt", b"conflict").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/94.txt");
    }

    #[test]
    fn fails_to_remove_a_staged_changed_mode_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.make_executable("outer/94.txt").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/94.txt");
    }

    #[test]
    fn leaves_an_unindexed_file_deleted() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/94.txt").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn fails_to_remove_an_unindexed_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/94.txt").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.write_file("outer/94.txt", b"conflict").unwrap();

        assert_remove_conflict(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/94.txt");
    }

    #[test]
    fn leaves_an_unindexed_directory_deleted() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/inner/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        let mut expected_workspace = BASE_FILES.clone();
        expected_workspace.remove("outer/inner/3.txt").unwrap();

        cmd_helper.assert_workspace(expected_workspace);
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("D  outer/inner/3.txt\n");
    }

    #[test]
    fn fails_to_remove_with_a_file_at_a_parent_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/inner/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.write_file("outer/inner", b"conflict").unwrap();

        assert_stale_file(
            cmd_helper.jit_cmd(&["checkout", "@^"]),
            "outer/inner/94.txt",
        );
    }

    #[test]
    fn removes_a_file_with_a_staged_file_at_a_parent_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/inner/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.write_file("outer/inner", b"conflict").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        let mut expected_workspace = BASE_FILES.clone();
        expected_workspace.remove("outer/inner/3.txt").unwrap();
        expected_workspace.insert("outer/inner", "conflict");

        cmd_helper.assert_workspace(expected_workspace);

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(
            "A  outer/inner
D  outer/inner/3.txt\n",
        );
    }

    #[test]
    fn fails_to_remove_with_an_unstaged_file_at_a_parent_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/inner/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.delete(".git/index").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        cmd_helper.write_file("outer/inner", b"conflict").unwrap();

        assert_remove_conflict(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/inner");
    }

    #[test]
    fn fails_to_remove_with_a_file_at_a_child_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/94.txt").unwrap();
        cmd_helper
            .write_file("outer/94.txt/extra.log", b"conflict")
            .unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/94.txt");
    }

    #[test]
    fn removes_a_file_with_a_staged_file_at_a_child_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("outer/94.txt", b"94").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/94.txt").unwrap();
        cmd_helper
            .write_file("outer/94.txt/extra.log", b"conflict")
            .unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();
        cmd_helper.assert_workspace(BASE_FILES.clone());

        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn adds_a_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("1.txt").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn adds_a_file_to_a_directory() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/2.txt").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn adds_a_directory() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn fails_to_add_an_untracked_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/2.txt").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("outer/2.txt", b"conflict").unwrap();
        assert_overwrite_conflict(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/2.txt");
    }

    #[test]
    fn fails_to_add_a_staged_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/2.txt").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("outer/2.txt", b"conflict").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        assert_stale_file(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/2.txt");
    }

    #[test]
    fn adds_a_staged_equal_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/2.txt").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("outer/2.txt", b"2").unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn fails_to_add_with_an_untracked_file_at_a_parent_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/inner/3.txt").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.write_file("outer/inner", b"conflict").unwrap();

        assert_overwrite_conflict(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/inner");
    }

    #[test]
    fn adds_a_file_with_a_staged_file_at_a_parent_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/inner/3.txt").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.write_file("outer/inner", b"conflict").unwrap();

        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn fails_to_add_with_an_untracked_file_at_a_child_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/2.txt").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper
            .write_file("outer/2.txt/extra.log", b"conflict")
            .unwrap();

        assert_stale_directory(cmd_helper.jit_cmd(&["checkout", "@^"]), "outer/2.txt");
    }

    #[test]
    fn adds_a_file_with_a_staged_file_at_a_child_path() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/2.txt").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper
            .write_file("outer/2.txt/extra.log", b"conflict")
            .unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn replaces_a_file_with_a_directory() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.write_file("outer/inner", b"in").unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn replaces_a_directory_with_a_file() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.delete("outer/2.txt").unwrap();
        cmd_helper
            .write_file("outer/2.txt/nested.log", b"nested")
            .unwrap();
        commit_and_checkout(&mut cmd_helper, "@^");

        cmd_helper.assert_workspace(BASE_FILES.clone());
        cmd_helper.clear_stdout();
        cmd_helper.assert_status("");
    }

    #[test]
    fn maintains_workspace_modifications() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("outer/2.txt", b"hello").unwrap();
        cmd_helper.delete("outer/inner").unwrap();
        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        let mut expected_workspace = HashMap::new();
        expected_workspace.insert("1.txt", "1");
        expected_workspace.insert("outer/2.txt", "hello");

        cmd_helper.assert_workspace(expected_workspace);

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(
            " M outer/2.txt
 D outer/inner/3.txt\n",
        );
    }

    #[test]
    fn maintains_index_modifications() {
        let mut cmd_helper = CommandHelper::new();
        before(&mut cmd_helper);
        cmd_helper.write_file("1.txt", b"changed").unwrap();
        commit_all(&mut cmd_helper);

        cmd_helper.write_file("outer/2.txt", b"hello").unwrap();
        cmd_helper
            .write_file("outer/inner/4.txt", b"world")
            .unwrap();
        cmd_helper.jit_cmd(&["add", "."]).unwrap();

        cmd_helper.jit_cmd(&["checkout", "@^"]).unwrap();

        let mut expected_workspace = BASE_FILES.clone();
        expected_workspace.insert("outer/2.txt", "hello");
        expected_workspace.insert("outer/inner/4.txt", "world");

        cmd_helper.assert_workspace(expected_workspace);

        cmd_helper.clear_stdout();
        cmd_helper.assert_status(
            "M  outer/2.txt
A  outer/inner/4.txt\n",
        );
    }

}
