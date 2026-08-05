use std::path::Path;

use anyhow::bail;

use crate::{
    git::{self, CommitInfo, Contributor},
    tui,
};

pub fn run(dir: &Path) -> anyhow::Result<()> {
    let (head_oid, contributors, commit) = load_repository(dir)?;
    let Some(selected_contributors) = tui::run(&contributors, &head_oid, &commit)? else {
        return Ok(());
    };

    if selected_contributors.is_empty() {
        return Ok(());
    }

    let new_oid = amend_head_safely(dir, &head_oid, &commit.message, &selected_contributors)?;
    let summary = amendment_summary(selected_contributors.len(), &head_oid, &new_oid);
    println!("{summary}");

    Ok(())
}

fn load_repository(dir: &Path) -> anyhow::Result<(String, Vec<Contributor>, CommitInfo)> {
    let head_oid = git::inspect_head(dir)?;
    git::ensure_safe_state(dir, &head_oid)?;
    let contributors = git::discover_contributors(dir, &head_oid)?;

    if contributors.is_empty() {
        bail!("no usable contributors found");
    }

    let commit = git::read_commit_info(dir, &head_oid)?;
    Ok((head_oid, contributors, commit))
}

fn amend_head_safely(
    dir: &Path,
    expected_oid: &str,
    message: &[u8],
    contributors: &[&Contributor],
) -> anyhow::Result<String> {
    git::ensure_head_unchanged(dir, expected_oid)?;
    git::ensure_safe_state(dir, expected_oid)?;
    let message = git::prepare_message(dir, message, contributors)?;
    git::amend_head(dir, &message)
}

fn amendment_summary(count: usize, old_oid: &str, new_oid: &str) -> String {
    let label = if count == 1 {
        "co-author"
    } else {
        "co-authors"
    };

    format!(
        "Added {count} {label}: {} -> {}",
        abbreviate_oid(old_oid),
        abbreviate_oid(new_oid)
    )
}

fn abbreviate_oid(oid: &str) -> &str {
    oid.get(..8).unwrap_or(oid)
}

#[cfg(test)]
mod tests;
