use std::{path::Path, process::Command};

use tempfile::TempDir;

use super::{amend_head_safely, amendment_summary};
use crate::git::{self, Contributor};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Git should be installed for tests");

    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
}

fn repository_with_commit(message: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temporary directory should be created");
    git(dir.path(), &["init", "--quiet"]);
    git(dir.path(), &["config", "user.name", "Test Author"]);
    git(dir.path(), &["config", "user.email", "author@example.com"]);
    git(dir.path(), &["config", "commit.gpgSign", "false"]);
    git(
        dir.path(),
        &[
            "commit",
            "--allow-empty",
            "--quiet",
            "--no-verify",
            "-m",
            message,
        ],
    );
    dir
}

#[test]
fn formats_amendment_summaries() {
    assert_eq!(
        amendment_summary(1, "0123456789abcdef", "fedcba9876543210"),
        "Added 1 co-author: 01234567 -> fedcba98"
    );
    assert_eq!(
        amendment_summary(2, "old", "new"),
        "Added 2 co-authors: old -> new"
    );
}

#[test]
fn skips_an_amendment_when_the_message_is_unchanged() {
    let message = "Subject\n\nCo-authored-by: Alice <alice@example.com>";
    let repo = repository_with_commit(message);
    let head = git::inspect_head(repo.path()).expect("HEAD should be inspected");
    let commit =
        git::read_commit_info(repo.path(), &head.oid).expect("commit information should be read");
    let contributor = Contributor {
        name: "Alice".to_owned(),
        email: "alice@example.com".to_owned(),
        commits: 1,
    };

    let result = amend_head_safely(repo.path(), &head, &commit.message, &[&contributor])
        .expect("an unchanged amendment should be skipped");

    assert_eq!(result, None);
    assert_eq!(
        git::inspect_head(repo.path())
            .expect("HEAD should still be inspected")
            .oid,
        head.oid
    );
}
