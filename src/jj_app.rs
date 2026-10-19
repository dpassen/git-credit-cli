use std::path::Path;

use anyhow::bail;

use crate::{jj, model::Contributor, summary::amendment_summary, trailers, tui};

pub fn run(dir: &Path, revision: &str) -> anyhow::Result<()> {
    let target = jj::inspect_target(dir, revision)?;
    let contributors = jj::discover_contributors(dir, &target.author_email)?;

    if contributors.is_empty() {
        bail!("no usable contributors found");
    }

    let commit = jj::commit_info(&target);
    let rewrite_warning = format!("Rewriting {} will change its commit ID.", target.selector);
    let tui_target = tui::Target::new(&target.selector, &target.commit_id, &rewrite_warning);
    let Some(selected_contributors) = tui::run("jj-credit", &contributors, &tui_target, &commit)?
    else {
        return Ok(());
    };

    if selected_contributors.is_empty() {
        return Ok(());
    }

    let Some(new_id) =
        rewrite_target_safely(dir, &target, &commit.message, &selected_contributors)?
    else {
        println!("No new co-author trailers to add.");
        return Ok(());
    };
    let summary = amendment_summary(selected_contributors.len(), &target.commit_id, &new_id);
    println!("{summary}");

    Ok(())
}

fn rewrite_target_safely(
    dir: &Path,
    expected_target: &jj::Target,
    message: &[u8],
    contributors: &[&Contributor],
) -> anyhow::Result<Option<String>> {
    let prepared_message = trailers::prepare_message(dir, message, contributors)?;
    if prepared_message == message {
        return Ok(None);
    }

    let current_target = jj::ensure_target_unchanged(dir, expected_target)?;
    jj::describe(dir, &current_target, &prepared_message).map(Some)
}
