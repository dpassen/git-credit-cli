use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, bail};

#[derive(Debug, Eq, PartialEq)]
pub struct Contributor {
    pub name: String,
    pub email: String,
    pub commits: u64,
}

struct ContributorGroup {
    contributor: Contributor,
    preferred_identity_commits: u64,
}

fn git_output(dir: &Path, args: &[&str], failure: &str) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();

        if stderr.is_empty() {
            bail!("{failure}");
        }

        bail!("{failure}: {stderr}");
    }

    String::from_utf8(output.stdout)
        .with_context(|| format!("git {} returned invalid UTF-8", args.join(" ")))
}

fn git_stdout(dir: &Path, args: &[&str], failure: &str) -> anyhow::Result<String> {
    git_output(dir, args, failure).map(|stdout| stdout.trim().to_owned())
}

pub fn inspect_head(dir: &Path) -> anyhow::Result<String> {
    let inside_work_tree = git_stdout(
        dir,
        &["rev-parse", "--is-inside-work-tree"],
        "not inside a Git repository",
    )?;

    if inside_work_tree != "true" {
        bail!("not inside a Git working tree");
    }

    git_stdout(
        dir,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        "HEAD is detached",
    )?;

    let oid = git_stdout(
        dir,
        &["rev-parse", "--verify", "HEAD^{commit}"],
        "could not resolve HEAD to a commit",
    )?;

    Ok(oid)
}

pub fn ensure_head_unchanged(dir: &Path, expected_oid: &str) -> anyhow::Result<()> {
    let current_oid = git_stdout(
        dir,
        &["rev-parse", "--verify", "HEAD^{commit}"],
        "HEAD changed while picker was open",
    )?;

    if current_oid != expected_oid {
        bail!("HEAD changed while picker was open");
    }

    Ok(())
}

pub fn ensure_safe_state(dir: &Path, head_oid: &str) -> anyhow::Result<()> {
    const OPERATION_MARKERS: [&str; 6] = [
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "sequencer",
    ];

    for marker in OPERATION_MARKERS {
        if git_path(dir, marker)?.exists() {
            bail!("another Git operation is in progress");
        }
    }

    let output = Command::new("git")
        .args(["diff", "--cached", "--quiet", head_oid, "--"])
        .current_dir(dir)
        .output()
        .context("failed to inspect staged changes")?;

    match output.status.code() {
        Some(0) => Ok(()),
        Some(1) => bail!("staged changes are present"),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();

            if stderr.is_empty() {
                bail!("could not inspect staged changes");
            }

            bail!("could not inspect staged changes: {stderr}");
        }
    }
}

fn git_path(dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(git_stdout(
        dir,
        &["rev-parse", "--git-path", name],
        "could not inspect Git operation state",
    )?);

    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(dir.join(path))
    }
}

pub fn discover_contributors(dir: &Path, head_oid: &str) -> anyhow::Result<Vec<Contributor>> {
    let head_author_email = git_stdout(
        dir,
        &["show", "--no-patch", "--format=%aE", head_oid],
        "could not read HEAD author",
    )?;
    let output = git_stdout(
        dir,
        &["shortlog", "--summary", "--numbered", "--email", "--all"],
        "could not discover contributors",
    )?;

    let mut groups: HashMap<String, ContributorGroup> = HashMap::new();
    for identity in output
        .lines()
        .filter_map(parse_shortlog_line)
        .filter(|identity| !identity.email.eq_ignore_ascii_case(&head_author_email))
    {
        let normalized_email = identity.email.to_ascii_lowercase();
        match groups.get_mut(&normalized_email) {
            Some(group) => group.add(identity),
            None => {
                let preferred_identity_commits = identity.commits;
                groups.insert(
                    normalized_email,
                    ContributorGroup {
                        contributor: identity,
                        preferred_identity_commits,
                    },
                );
            }
        }
    }

    let mut contributors: Vec<_> = groups
        .into_values()
        .map(|group| group.contributor)
        .collect();
    contributors.sort_by(contributor_order);
    Ok(contributors)
}

pub fn read_message(dir: &Path, oid: &str) -> anyhow::Result<String> {
    git_output(
        dir,
        &["show", "--no-patch", "--format=format:%B", oid],
        "could not read commit message",
    )
}

pub fn prepare_message(
    dir: &Path,
    message: &str,
    contributors: &[&Contributor],
) -> anyhow::Result<String> {
    if contributors.is_empty() {
        return Ok(message.to_owned());
    }

    let mut command = Command::new("git");
    command
        .arg("interpret-trailers")
        .arg("--no-in-place")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for contributor in contributors {
        command.arg("--trailer").arg(format!(
            "Co-authored-by: {} <{}>",
            contributor.name, contributor.email
        ));
    }

    let mut child = command
        .spawn()
        .context("failed to run git interpret-trailers")?;
    let write_result = child
        .stdin
        .take()
        .context("failed to open git interpret-trailers stdin")?
        .write_all(message.as_bytes());
    let output = child
        .wait_with_output()
        .context("failed to wait for git interpret-trailers")?;
    write_result.context("failed to write commit message to git interpret-trailers")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();

        if stderr.is_empty() {
            bail!("could not prepare commit message");
        }

        bail!("could not prepare commit message: {stderr}");
    }

    String::from_utf8(output.stdout).context("git interpret-trailers returned invalid UTF-8")
}

impl ContributorGroup {
    fn add(&mut self, identity: Contributor) {
        let is_preferred = identity.commits > self.preferred_identity_commits
            || (identity.commits == self.preferred_identity_commits
                && (&identity.name, &identity.email)
                    < (&self.contributor.name, &self.contributor.email));

        self.contributor.commits += identity.commits;

        if is_preferred {
            self.contributor.name = identity.name;
            self.contributor.email = identity.email;
            self.preferred_identity_commits = identity.commits;
        }
    }
}

fn parse_shortlog_line(line: &str) -> Option<Contributor> {
    let (commits, identity) = line.split_once('\t')?;
    let commits = commits.trim().parse().ok()?;
    let (name, email) = identity.rsplit_once(" <")?;
    let email = email.strip_suffix('>')?;
    let name = name.trim();
    let email = email.trim();

    if name.is_empty() || email.is_empty() {
        return None;
    }

    Some(Contributor {
        name: name.to_owned(),
        email: email.to_owned(),
        commits,
    })
}

fn contributor_order(left: &Contributor, right: &Contributor) -> std::cmp::Ordering {
    right
        .commits
        .cmp(&left.commits)
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.email.cmp(&right.email))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
    };

    use tempfile::TempDir;

    use super::{
        Contributor, discover_contributors, ensure_head_unchanged, ensure_safe_state, inspect_head,
        prepare_message, read_message,
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

        let error = ensure_head_unchanged(repo.path(), &old_oid)
            .expect_err("changed HEAD should be rejected");

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
    fn reads_a_multiline_unicode_commit_message() {
        let repo = init_repository();
        commit_as(
            repo.path(),
            "Current Author",
            "current@example.com",
            "Subject\n\nBody with café.",
        );
        let oid = git(repo.path(), &["rev-parse", "HEAD"]);

        let message = read_message(repo.path(), &oid).expect("message should be read");

        assert_eq!(message, "Subject\n\nBody with café.\n");
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

        let prepared = prepare_message(repo.path(), message, &[&alice, &bob])
            .expect("message should be prepared");

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
}
