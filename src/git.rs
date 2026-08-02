use std::{
    collections::{HashMap, hash_map::Entry},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use anyhow::{Context, bail};

#[derive(Debug, Eq, PartialEq)]
pub struct CommitInfo {
    pub author_name: String,
    pub author_email: String,
    pub message: Vec<u8>,
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

fn run_git(dir: &Path, args: &[&str], failure: &str) -> anyhow::Result<Output> {
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

    Ok(output)
}

fn git_output_bytes(dir: &Path, args: &[&str], failure: &str) -> anyhow::Result<Vec<u8>> {
    Ok(run_git(dir, args, failure)?.stdout)
}

fn git_output(dir: &Path, args: &[&str], failure: &str) -> anyhow::Result<String> {
    let output = git_output_bytes(dir, args, failure)?;
    String::from_utf8(output)
        .with_context(|| format!("git {} returned invalid UTF-8", args.join(" ")))
}

fn git_output_lossy(dir: &Path, args: &[&str], failure: &str) -> anyhow::Result<String> {
    let output = git_output_bytes(dir, args, failure)?;
    Ok(String::from_utf8_lossy(&output).into_owned())
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
    let head_author_email = git_output_lossy(
        dir,
        &["show", "--no-patch", "--format=%aE", head_oid],
        "could not read HEAD author",
    )?;
    let head_author_email = head_author_email.trim();
    let output = git_output_lossy(
        dir,
        &["shortlog", "--summary", "--numbered", "--email", "--all"],
        "could not discover contributors",
    )?;

    let mut groups: HashMap<String, ContributorGroup> = HashMap::new();
    for identity in output
        .lines()
        .filter_map(parse_shortlog_line)
        .filter(|identity| !identity.email.eq_ignore_ascii_case(head_author_email))
    {
        let normalized_email = identity.email.to_ascii_lowercase();
        match groups.entry(normalized_email) {
            Entry::Occupied(mut entry) => entry.get_mut().add(identity),
            Entry::Vacant(entry) => {
                let preferred_identity_commits = identity.commits;
                entry.insert(ContributorGroup {
                    contributor: identity,
                    preferred_identity_commits,
                });
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

pub fn read_commit_info(dir: &Path, oid: &str) -> anyhow::Result<CommitInfo> {
    let output = git_output_bytes(
        dir,
        &[
            "show",
            "--no-patch",
            "--format=format:%aN%x00%aE%x00%B",
            oid,
        ],
        "could not read commit information",
    )?;
    let mut fields = output.splitn(3, |byte| *byte == b'\0');
    let author_name = fields.next().unwrap_or_default();
    let author_email = fields.next().unwrap_or_default();
    let Some(message) = fields.next() else {
        bail!("commit information is malformed");
    };

    if author_name.is_empty() || author_email.is_empty() {
        bail!("commit information is malformed");
    }

    Ok(CommitInfo {
        author_name: String::from_utf8_lossy(author_name).into_owned(),
        author_email: String::from_utf8_lossy(author_email).into_owned(),
        message: message.to_owned(),
    })
}

pub fn prepare_message(
    dir: &Path,
    message: &[u8],
    contributors: &[&Contributor],
) -> anyhow::Result<Vec<u8>> {
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
        .write_all(message);
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

    Ok(output.stdout)
}

pub fn amend_head(dir: &Path, message: &[u8]) -> anyhow::Result<String> {
    let mut child = Command::new("git")
        .args([
            "commit",
            "--amend",
            "--only",
            "--allow-empty",
            "--file=-",
            "--cleanup=verbatim",
        ])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to run git commit --amend")?;
    let write_result = child
        .stdin
        .take()
        .context("failed to open git commit --amend stdin")?
        .write_all(message);
    let output = child
        .wait_with_output()
        .context("failed to wait for git commit --amend")?;
    write_result.context("failed to write commit message to git commit --amend")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };

        if detail.is_empty() {
            bail!("could not amend HEAD");
        }

        bail!("could not amend HEAD: {detail}");
    }

    git_stdout(
        dir,
        &["rev-parse", "--verify", "HEAD^{commit}"],
        "could not resolve amended HEAD",
    )
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
mod tests;
