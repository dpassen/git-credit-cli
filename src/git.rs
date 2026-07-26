use std::{collections::HashMap, path::Path, process::Command};

use anyhow::{Context, bail};

#[derive(Debug)]
pub struct Head {
    pub branch: String,
    pub oid: String,
}

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

fn git_stdout(dir: &Path, args: &[&str], failure: &str) -> anyhow::Result<String> {
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
        .map(|stdout| stdout.trim().to_owned())
}

pub fn inspect_head(dir: &Path) -> anyhow::Result<Head> {
    let inside_work_tree = git_stdout(
        dir,
        &["rev-parse", "--is-inside-work-tree"],
        "not inside a Git repository",
    )?;

    if inside_work_tree != "true" {
        bail!("not inside a Git working tree");
    }

    let branch = git_stdout(
        dir,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        "HEAD is detached",
    )?;

    let oid = git_stdout(
        dir,
        &["rev-parse", "--verify", "HEAD^{commit}"],
        "could not resolve HEAD to a commit",
    )?;

    Ok(Head { branch, oid })
}

pub fn discover_contributors(dir: &Path) -> anyhow::Result<Vec<Contributor>> {
    let output = git_stdout(
        dir,
        &["shortlog", "--summary", "--numbered", "--email", "--all"],
        "could not discover contributors",
    )?;

    let mut groups: HashMap<String, ContributorGroup> = HashMap::new();
    for identity in output.lines().filter_map(parse_shortlog_line) {
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
    use std::{fs, path::Path, process::Command};

    use tempfile::TempDir;

    use super::{Contributor, discover_contributors, inspect_head};

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

        assert_eq!(head.branch, "main");
        assert_eq!(head.oid, oid);
    }

    #[test]
    fn discovers_contributors_by_commit_count() {
        let repo = init_repository();
        commit_as(repo.path(), "Alice", "alice@example.com", "One");
        commit_as(repo.path(), "Bob", "bob@example.com", "Two");
        commit_as(repo.path(), "Alice", "alice@example.com", "Three");

        let contributors =
            discover_contributors(repo.path()).expect("contributors should be discovered");

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
        fs::write(
            repo.path().join(".mailmap"),
            "Canonical Name <canonical@example.com> Old Name <old@example.com>\n",
        )
        .expect("mailmap should be written");

        let contributors =
            discover_contributors(repo.path()).expect("contributors should be discovered");

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

        let contributors =
            discover_contributors(repo.path()).expect("contributors should be discovered");

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
