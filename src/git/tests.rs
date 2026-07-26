use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::TempDir;

use super::{
    CommitInfo, Contributor, amend_head, discover_contributors, ensure_head_unchanged,
    ensure_safe_state, inspect_head, prepare_message, read_commit_info,
};

fn git(dir: &Path, args: &[&str]) -> String {
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

    String::from_utf8(output.stdout)
        .expect("Git output should be UTF-8")
        .trim()
        .to_owned()
}

fn init_repository() -> TempDir {
    let dir = tempfile::tempdir().expect("temporary directory should be created");
    git(dir.path(), &["init", "--quiet"]);
    git(dir.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(dir.path(), &["config", "user.name", "Test Author"]);
    git(dir.path(), &["config", "user.email", "author@example.com"]);
    git(dir.path(), &["config", "commit.gpgSign", "false"]);
    dir
}

fn commit_file(dir: &Path) -> String {
    fs::write(dir.join("file.txt"), "initial contents\n").expect("test file should be written");
    git(dir, &["add", "file.txt"]);
    git(dir, &["commit", "--quiet", "--no-verify", "-m", "Initial"]);
    git(dir, &["rev-parse", "HEAD"])
}

fn commit_as(dir: &Path, name: &str, email: &str, message: &str) {
    git(
        dir,
        &[
            "-c",
            &format!("user.name={name}"),
            "-c",
            &format!("user.email={email}"),
            "commit",
            "--allow-empty",
            "--quiet",
            "--no-verify",
            "-m",
            message,
        ],
    );
}

#[test]
fn inspects_head_from_a_subdirectory() {
    let repo = init_repository();
    let oid = commit_file(repo.path());
    let subdirectory = repo.path().join("nested");
    fs::create_dir(&subdirectory).expect("subdirectory should be created");

    let head = inspect_head(&subdirectory).expect("HEAD should be inspected");

    assert_eq!(head, oid);
}

#[test]
fn discovers_contributors_by_commit_count() {
    let repo = init_repository();
    commit_as(repo.path(), "Alice", "alice@example.com", "One");
    commit_as(repo.path(), "Bob", "bob@example.com", "Two");
    commit_as(repo.path(), "Alice", "alice@example.com", "Three");
    commit_as(repo.path(), "Current Author", "current@example.com", "HEAD");
    let head = inspect_head(repo.path()).expect("HEAD should be inspected");

    let contributors =
        discover_contributors(repo.path(), &head).expect("contributors should be discovered");

    assert_eq!(
        contributors,
        vec![
            Contributor {
                name: "Alice".to_owned(),
                email: "alice@example.com".to_owned(),
                commits: 2,
            },
            Contributor {
                name: "Bob".to_owned(),
                email: "bob@example.com".to_owned(),
                commits: 1,
            },
        ]
    );
}

#[test]
fn contributor_discovery_honors_mailmap() {
    let repo = init_repository();
    commit_as(repo.path(), "Old Name", "old@example.com", "One");
    commit_as(repo.path(), "Current Author", "current@example.com", "HEAD");
    fs::write(
        repo.path().join(".mailmap"),
        "Canonical Name <canonical@example.com> Old Name <old@example.com>\n",
    )
    .expect("mailmap should be written");

    let head = inspect_head(repo.path()).expect("HEAD should be inspected");
    let contributors =
        discover_contributors(repo.path(), &head).expect("contributors should be discovered");

    assert_eq!(
        contributors,
        vec![Contributor {
            name: "Canonical Name".to_owned(),
            email: "canonical@example.com".to_owned(),
            commits: 1,
        }]
    );
}

#[test]
fn contributor_discovery_groups_normalized_emails() {
    let repo = init_repository();
    commit_as(repo.path(), "Alice", "ALICE@example.com", "One");
    commit_as(repo.path(), "Alicia", "alice@example.com", "Two");
    commit_as(repo.path(), "Alicia", "alice@example.com", "Three");
    commit_as(repo.path(), "Current Author", "current@example.com", "HEAD");
    let head = inspect_head(repo.path()).expect("HEAD should be inspected");

    let contributors =
        discover_contributors(repo.path(), &head).expect("contributors should be discovered");

    assert_eq!(
        contributors,
        vec![Contributor {
            name: "Alicia".to_owned(),
            email: "alice@example.com".to_owned(),
            commits: 3,
        }]
    );
}

#[test]
fn rejects_a_changed_head() {
    let repo = init_repository();
    let old_oid = commit_file(repo.path());
    commit_as(
        repo.path(),
        "Current Author",
        "current@example.com",
        "New HEAD",
    );

    let error =
        ensure_head_unchanged(repo.path(), &old_oid).expect_err("changed HEAD should be rejected");

    assert_eq!(error.to_string(), "HEAD changed while picker was open");
}

#[test]
fn rejects_staged_changes() {
    let repo = init_repository();
    let oid = commit_file(repo.path());
    fs::write(repo.path().join("file.txt"), "staged contents\n")
        .expect("test file should be written");
    git(repo.path(), &["add", "file.txt"]);

    let error =
        ensure_safe_state(repo.path(), &oid).expect_err("staged changes should be rejected");

    assert_eq!(error.to_string(), "staged changes are present");
}

#[test]
fn allows_unstaged_and_untracked_changes() {
    let repo = init_repository();
    let oid = commit_file(repo.path());
    fs::write(repo.path().join("file.txt"), "unstaged contents\n")
        .expect("tracked file should be written");
    fs::write(repo.path().join("untracked.txt"), "untracked contents\n")
        .expect("untracked file should be written");

    ensure_safe_state(repo.path(), &oid).expect("working tree changes should be allowed");
}

#[test]
fn rejects_in_progress_git_operations() {
    let repo = init_repository();
    let oid = commit_file(repo.path());
    let markers = [
        ("MERGE_HEAD", false),
        ("rebase-merge", true),
        ("rebase-apply", true),
        ("CHERRY_PICK_HEAD", false),
        ("REVERT_HEAD", false),
        ("sequencer", true),
    ];

    for (marker, is_directory) in markers {
        let path = PathBuf::from(git(repo.path(), &["rev-parse", "--git-path", marker]));
        let path = if path.is_absolute() {
            path
        } else {
            repo.path().join(path)
        };

        if is_directory {
            fs::create_dir_all(&path).expect("operation directory should be created");
        } else {
            fs::write(&path, "operation state\n").expect("operation file should be written");
        }

        let error = ensure_safe_state(repo.path(), &oid)
            .expect_err("in-progress operation should be rejected");
        assert_eq!(error.to_string(), "another Git operation is in progress");

        if is_directory {
            fs::remove_dir_all(path).expect("operation directory should be removed");
        } else {
            fs::remove_file(path).expect("operation file should be removed");
        }
    }
}

#[test]
fn reads_mailmapped_author_and_multiline_unicode_message() {
    let repo = init_repository();
    commit_as(
        repo.path(),
        "Current Author",
        "current@example.com",
        "Subject\n\nBody with café.",
    );
    let oid = git(repo.path(), &["rev-parse", "HEAD"]);
    fs::write(
        repo.path().join(".mailmap"),
        "Canonical Author <canonical@example.com> Current Author <current@example.com>\n",
    )
    .expect("mailmap should be written");

    let info = read_commit_info(repo.path(), &oid).expect("commit information should be read");

    assert_eq!(
        info,
        CommitInfo {
            author_name: "Canonical Author".to_owned(),
            author_email: "canonical@example.com".to_owned(),
            message: "Subject\n\nBody with café.\n".to_owned(),
        }
    );
}

#[test]
fn prepares_a_message_with_multiple_co_authors() {
    let repo = init_repository();
    commit_as(repo.path(), "Current Author", "current@example.com", "HEAD");
    let old_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let alice = Contributor {
        name: "Alice".to_owned(),
        email: "alice@example.com".to_owned(),
        commits: 2,
    };
    let bob = Contributor {
        name: "Bob".to_owned(),
        email: "bob@example.com".to_owned(),
        commits: 1,
    };
    let message = "Handle Unicode\n\nPreserve café text.\n\nSigned-off-by: Maintainer <maintainer@example.com>\n";

    let prepared =
        prepare_message(repo.path(), message, &[&alice, &bob]).expect("message should be prepared");

    assert_eq!(
        prepared,
        "Handle Unicode\n\nPreserve café text.\n\nSigned-off-by: Maintainer <maintainer@example.com>\nCo-authored-by: Alice <alice@example.com>\nCo-authored-by: Bob <bob@example.com>\n"
    );
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), old_head);
}

#[test]
fn message_preparation_uses_gits_default_duplicate_handling() {
    let repo = init_repository();
    let alice = Contributor {
        name: "Alice".to_owned(),
        email: "alice@example.com".to_owned(),
        commits: 1,
    };
    let message = "Subject\n\nCo-authored-by: Alice <alice@example.com>\n";

    let prepared =
        prepare_message(repo.path(), message, &[&alice]).expect("message should be prepared");

    assert_eq!(prepared, message);
}

#[test]
fn amendment_preserves_commit_and_working_tree_state() {
    let repo = init_repository();
    commit_file(repo.path());
    commit_as(
        repo.path(),
        "Original Author",
        "original@example.com",
        "Original message",
    );
    let old_oid = git(repo.path(), &["rev-parse", "HEAD"]);
    let old_tree = git(repo.path(), &["show", "-s", "--format=%T", "HEAD"]);
    let old_parents = git(repo.path(), &["show", "-s", "--format=%P", "HEAD"]);
    let old_author = git(repo.path(), &["show", "-s", "--format=%an <%ae>", "HEAD"]);
    fs::write(repo.path().join("file.txt"), "unstaged contents\n")
        .expect("tracked file should be written");
    fs::write(repo.path().join("untracked.txt"), "untracked contents\n")
        .expect("untracked file should be written");
    let message = "Updated message\n\nCo-authored-by: Alice <alice@example.com>\n";

    let new_oid = amend_head(repo.path(), message).expect("HEAD should be amended");

    assert_ne!(new_oid, old_oid);
    assert_eq!(
        git(repo.path(), &["show", "-s", "--format=%T", "HEAD"]),
        old_tree
    );
    assert_eq!(
        git(repo.path(), &["show", "-s", "--format=%P", "HEAD"]),
        old_parents
    );
    assert_eq!(
        git(repo.path(), &["show", "-s", "--format=%an <%ae>", "HEAD"]),
        old_author
    );
    assert_eq!(
        read_commit_info(repo.path(), &new_oid).unwrap().message,
        message
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("file.txt")).unwrap(),
        "unstaged contents\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("untracked.txt")).unwrap(),
        "untracked contents\n"
    );
}

#[test]
fn rejects_a_directory_outside_a_repository() {
    let dir = tempfile::tempdir().expect("temporary directory should be created");

    let error = inspect_head(dir.path()).expect_err("directory should be rejected");

    assert!(error.to_string().starts_with("not inside a Git repository"));
}

#[test]
fn rejects_a_bare_repository() {
    let dir = tempfile::tempdir().expect("temporary directory should be created");
    git(dir.path(), &["init", "--quiet", "--bare"]);

    let error = inspect_head(dir.path()).expect_err("bare repository should be rejected");

    assert_eq!(error.to_string(), "not inside a Git working tree");
}

#[test]
fn rejects_an_unborn_head() {
    let repo = init_repository();

    let error = inspect_head(repo.path()).expect_err("unborn HEAD should be rejected");

    assert!(
        error
            .to_string()
            .starts_with("could not resolve HEAD to a commit")
    );
}

#[test]
fn rejects_a_detached_head() {
    let repo = init_repository();
    commit_file(repo.path());
    git(repo.path(), &["checkout", "--quiet", "--detach", "HEAD"]);

    let error = inspect_head(repo.path()).expect_err("detached HEAD should be rejected");

    assert_eq!(error.to_string(), "HEAD is detached");
}
