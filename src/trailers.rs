use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, bail};

use crate::model::Contributor;

pub(crate) fn prepare_message(
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::prepare_message;
    use crate::model::Contributor;

    #[test]
    fn prepares_a_message_outside_a_git_repository() {
        let dir = tempdir().expect("temporary directory should be created");
        let alice = Contributor {
            name: "Alice".to_owned(),
            email: "alice@example.com".to_owned(),
            commits: 1,
        };

        let prepared = prepare_message(dir.path(), b"Subject\n", &[&alice])
            .expect("message should be prepared");

        assert_eq!(
            prepared,
            b"Subject\n\nCo-authored-by: Alice <alice@example.com>\n"
        );
    }
}
