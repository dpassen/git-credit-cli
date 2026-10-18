use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use anyhow::{Context, bail};

const REVISION_TEMPLATE: &str = r#"commit_id ++ "\0" ++ change_id ++ "\0" ++ if(immutable, "true", "false") ++ "\0" ++ description ++ "\0""#;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Target {
    pub(crate) selector: String,
    pub(crate) commit_id: String,
    pub(crate) change_id: String,
    description: String,
}

#[derive(Debug, Eq, PartialEq)]
struct Revision {
    commit_id: String,
    change_id: String,
    is_immutable: bool,
    description: String,
}

pub(crate) fn inspect_target(dir: &Path, selector: &str) -> anyhow::Result<Target> {
    ensure_colocated(dir)?;

    let revision = inspect_revision(dir, selector)?;
    validate_target(&revision)?;
    Ok(Target {
        selector: selector.to_owned(),
        commit_id: revision.commit_id,
        change_id: revision.change_id,
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
        || commit_changed
    {
        bail!("target changed while picker was open");
    }

    Ok(Target {
        selector: expected.selector.clone(),
        commit_id: current.commit_id,
        change_id: current.change_id,
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

fn ensure_colocated(dir: &Path) -> anyhow::Result<()> {
    let output = run_jj(
        dir,
        &["git", "colocation", "status"],
        "could not inspect Jujutsu colocation",
    )?;
    let stdout = String::from_utf8(output.stdout)
        .context("jj git colocation status returned invalid UTF-8")?;

    if !stdout.contains(" is currently colocated with Git.") {
        bail!("jj-credit requires a colocated Jujutsu/Git workspace");
    }

    Ok(())
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
        4 => {}
        count if count % 4 == 0 => bail!("revision {selector} resolved to multiple commits"),
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
    })
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
