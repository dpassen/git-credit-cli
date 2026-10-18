use std::path::Path;

use anyhow::bail;

use crate::{
    git::{self, Head},
    model::{CommitInfo, Contributor},
    summary::amendment_summary,
    tui,
};

pub fn run(dir: &Path) -> anyhow::Result<()> {
    let (head, contributors, commit) = load_repository(dir)?;
    let target = tui::Target::new(
        "HEAD",
        &head.oid,
        "Amending HEAD will change its commit ID.",
    );
    let Some(selected_contributors) = tui::run("git-credit", &contributors, &target, &commit)?
    else {
        return Ok(());
    };

    if selected_contributors.is_empty() {
        return Ok(());
    }

    let Some(new_oid) = amend_head_safely(dir, &head, &commit.message, &selected_contributors)?
    else {
        println!("No new co-author trailers to add.");
        return Ok(());
    };
    let summary = amendment_summary(selected_contributors.len(), &head.oid, &new_oid);
    println!("{summary}");

    Ok(())
}

fn load_repository(dir: &Path) -> anyhow::Result<(Head, Vec<Contributor>, CommitInfo)> {
    let head = git::inspect_head(dir)?;
    git::ensure_safe_state(dir, &head.oid)?;
    let contributors = git::discover_contributors(dir, &head.oid)?;

    if contributors.is_empty() {
        bail!("no usable contributors found");
    }

    let commit = git::read_commit_info(dir, &head.oid)?;
    Ok((head, contributors, commit))
}

fn amend_head_safely(
    dir: &Path,
    expected_head: &Head,
    message: &[u8],
    contributors: &[&Contributor],
) -> anyhow::Result<Option<String>> {
    let prepared_message = git::prepare_message(dir, message, contributors)?;
    if prepared_message == message {
        return Ok(None);
    }

    git::ensure_head_unchanged(dir, expected_head)?;
    git::ensure_safe_state(dir, &expected_head.oid)?;
    git::amend_head(dir, &prepared_message).map(Some)
}

#[cfg(test)]
mod tests;
