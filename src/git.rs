use std::{path::Path, process::Command};

use anyhow::{Context, bail};

#[derive(Debug)]
pub struct Head {
    pub branch: String,
    pub oid: String,
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

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use tempfile::TempDir;

    use super::inspect_head;

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
