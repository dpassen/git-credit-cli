use std::{
    collections::HashMap,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use anyhow::{Context, bail};

use crate::model::{CommitInfo, Contributor, consolidate_contributors};

const REVISION_TEMPLATE: &str = r#"commit_id ++ "\0" ++ change_id ++ "\0" ++ if(immutable, "true", "false") ++ "\0" ++ description ++ "\0" ++ author.name() ++ "\0" ++ author.email() ++ "\0""#;
const CONTRIBUTORS_TEMPLATE: &str = r#"author.name() ++ "\0" ++ author.email() ++ "\0" ++ committer.name() ++ "\0" ++ committer.email() ++ "\0""#;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Target {
    pub(crate) selector: String,
    pub(crate) commit_id: String,
    pub(crate) change_id: String,
    pub(crate) author_email: String,
    author_name: String,
    description: String,
}

#[derive(Debug, Eq, PartialEq)]
struct Revision {
    commit_id: String,
    change_id: String,
    is_immutable: bool,
    description: String,
    author_name: String,
    author_email: String,
}

#[derive(Default)]
struct Participation {
    authored: u64,
    committed_only: u64,
}

pub(crate) fn inspect_target(dir: &Path, selector: &str) -> anyhow::Result<Target> {
    let revision = inspect_revision(dir, selector)?;
    validate_target(&revision)?;
    Ok(Target {
        selector: selector.to_owned(),
        commit_id: revision.commit_id,
        change_id: revision.change_id,
        author_email: revision.author_email,
        author_name: revision.author_name,
        description: revision.description,
    })
}

pub(crate) fn ensure_target_unchanged(dir: &Path, expected: &Target) -> anyhow::Result<Target> {
    let current = inspect_revision(dir, &expected.selector)
        .context("target changed while picker was open")?;
    validate_target(&current)?;

    let commit_changed = expected.selector != "@" && current.commit_id != expected.commit_id;
    if current.change_id != expected.change_id
        || current.description != expected.description
        || current.author_name != expected.author_name
        || current.author_email != expected.author_email
        || commit_changed
    {
        bail!("target changed while picker was open");
    }

    Ok(Target {
        selector: expected.selector.clone(),
        commit_id: current.commit_id,
        change_id: current.change_id,
        author_email: current.author_email,
        author_name: current.author_name,
        description: current.description,
    })
}

pub(crate) fn describe(dir: &Path, target: &Target, message: &[u8]) -> anyhow::Result<String> {
    let mut command = jj_command(dir);
    command
        .args(["describe", "--stdin", "-r"])
        .arg(&target.commit_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().context("failed to run jj describe")?;
    let write_result = child
        .stdin
        .take()
        .context("failed to open jj describe stdin")?
        .write_all(message);
    let output = child
        .wait_with_output()
        .context("failed to wait for jj describe")?;
    write_result.context("failed to write commit message to jj describe")?;
    ensure_success(output, "could not describe target")?;

    let rewritten =
        inspect_revision(dir, &target.change_id).context("could not resolve rewritten target")?;
    Ok(rewritten.commit_id)
}

pub(crate) fn commit_info(target: &Target) -> CommitInfo {
    CommitInfo {
        author_name: target.author_name.clone(),
        author_email: target.author_email.clone(),
        message: target.description.as_bytes().to_owned(),
    }
}

pub(crate) fn discover_contributors(
    dir: &Path,
    excluded_email: &str,
) -> anyhow::Result<Vec<Contributor>> {
    let output = run_jj(
        dir,
        &[
            "log",
            "--no-graph",
            "-r",
            "all()",
            "-T",
            CONTRIBUTORS_TEMPLATE,
        ],
        "could not discover contributors",
    )?;
    parse_contributors_output(&output.stdout, excluded_email)
}

fn inspect_revision(dir: &Path, selector: &str) -> anyhow::Result<Revision> {
    let output = run_jj(
        dir,
        &["log", "--no-graph", "-r", selector, "-T", REVISION_TEMPLATE],
        "could not inspect Jujutsu revision",
    )?;
    parse_revision_output(selector, &output.stdout)
}

fn parse_revision_output(selector: &str, output: &[u8]) -> anyhow::Result<Revision> {
    let mut fields: Vec<_> = output.split(|byte| *byte == b'\0').collect();
    if fields.last() == Some(&&[][..]) {
        fields.pop();
    }

    match fields.len() {
        0 => bail!("revision {selector} did not resolve to a commit"),
        6 => {}
        count if count % 6 == 0 => bail!("revision {selector} resolved to multiple commits"),
        _ => bail!("jj returned malformed revision information"),
    }

    let text = |field: &[u8]| -> anyhow::Result<String> {
        String::from_utf8(field.to_owned()).context("jj returned invalid UTF-8")
    };
    let boolean = |field: &[u8]| -> anyhow::Result<bool> {
        match field {
            b"true" => Ok(true),
            b"false" => Ok(false),
            _ => bail!("jj returned malformed revision information"),
        }
    };

    Ok(Revision {
        commit_id: text(fields[0])?,
        change_id: text(fields[1])?,
        is_immutable: boolean(fields[2])?,
        description: text(fields[3])?,
        author_name: text(fields[4])?,
        author_email: text(fields[5])?,
    })
}

fn parse_contributors_output(
    output: &[u8],
    excluded_email: &str,
) -> anyhow::Result<Vec<Contributor>> {
    let mut fields: Vec<_> = output.split(|byte| *byte == b'\0').collect();
    if fields.last() == Some(&&[][..]) {
        fields.pop();
    }

    if fields.len() % 4 != 0 {
        bail!("jj returned malformed contributor information");
    }

    let (signatures, remainder) = fields.as_chunks::<4>();
    debug_assert!(remainder.is_empty());

    let mut identities = Vec::new();
    let mut participation: HashMap<String, Participation> = HashMap::new();
    for [author_name, author_email, committer_name, committer_email] in signatures {
        let author = parse_identity(author_name, author_email)?;
        let committer = parse_identity(committer_name, committer_email)?;
        let author_email = author
            .as_ref()
            .map(|identity| identity.email.to_ascii_lowercase());

        if let Some(author) = author
            && !author.email.eq_ignore_ascii_case(excluded_email)
        {
            participation
                .entry(author.email.to_ascii_lowercase())
                .or_default()
                .authored += 1;
            identities.push(author);
        }

        if let Some(committer) = committer {
            let normalized_email = committer.email.to_ascii_lowercase();
            if !committer.email.eq_ignore_ascii_case(excluded_email)
                && author_email.as_ref() != Some(&normalized_email)
            {
                participation
                    .entry(normalized_email)
                    .or_default()
                    .committed_only += 1;
                identities.push(committer);
            }
        }
    }

    let mut contributors = consolidate_contributors(identities);
    contributors.sort_by(|left, right| {
        let left_participation = &participation[&left.email.to_ascii_lowercase()];
        let right_participation = &participation[&right.email.to_ascii_lowercase()];

        // This is equivalent to sorting by authored + committed / (committed + 1),
        // without using floating-point arithmetic.
        right_participation
            .authored
            .cmp(&left_participation.authored)
            .then_with(|| {
                right_participation
                    .committed_only
                    .cmp(&left_participation.committed_only)
            })
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.email.cmp(&right.email))
    });
    Ok(contributors)
}

fn parse_identity(name: &[u8], email: &[u8]) -> anyhow::Result<Option<Contributor>> {
    if name.is_empty() || email.is_empty() {
        return Ok(None);
    }

    let name = String::from_utf8(name.to_owned()).context("jj returned invalid UTF-8")?;
    let email = String::from_utf8(email.to_owned()).context("jj returned invalid UTF-8")?;
    Ok(Some(Contributor {
        name,
        email,
        commits: 1,
    }))
}

fn validate_target(revision: &Revision) -> anyhow::Result<()> {
    if revision.commit_id.chars().all(|character| character == '0') {
        bail!("cannot add co-authors to Jujutsu's root commit");
    }

    if revision.is_immutable {
        bail!("target commit is immutable");
    }

    Ok(())
}

fn jj_command(dir: &Path) -> Command {
    let mut command = Command::new("jj");
    command
        .args(["--no-pager", "--color=never", "-R"])
        .arg(dir)
        .current_dir(dir);
    command
}

fn run_jj(dir: &Path, args: &[&str], failure: &str) -> anyhow::Result<Output> {
    let output = jj_command(dir)
        .args(args)
        .output()
        .with_context(|| format!("failed to run jj {}", args.join(" ")))?;
    ensure_success(output, failure)
}

fn ensure_success(output: Output, failure: &str) -> anyhow::Result<Output> {
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

#[cfg(test)]
mod tests;
