#[cfg(test)]
mod tests {
    use crate::commands::tests::*;

    #[test]
    fn list_untracked_files_in_name_order() {
        let repo_path = gen_repo_path();
        let mut repo = repo(&repo_path);

        write_file(&repo_path, "file.txt", "hello".as_bytes()).unwrap();
        write_file(&repo_path, "another.txt", "hello".as_bytes()).unwrap();

        if let Ok((stdout, stderr)) = jit_cmd(&repo_path, vec!["status"]) {
            assert_eq!(
                stdout,
                "?? another.txt
?? hello.txt"
            );
        } else {
            assert!(false);
        }
    }
}
