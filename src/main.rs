mod git;
mod tui;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "git-credit",
    version,
    about = "Easily add 'Co-authored-by' trailers to HEAD"
)]
struct Args;

fn main() -> anyhow::Result<()> {
    Args::parse();

    let cwd = std::env::current_dir()?;
    let head_oid = git::inspect_head(&cwd)?;
    git::ensure_safe_state(&cwd, &head_oid)?;
    let contributors = git::discover_contributors(&cwd, &head_oid)?;

    if contributors.is_empty() {
        anyhow::bail!("no usable contributors found");
    }

    let commit = git::read_commit_info(&cwd, &head_oid)?;
    let Some(selected_contributors) = tui::run(&contributors, &head_oid, &commit)? else {
        return Ok(());
    };

    if selected_contributors.is_empty() {
        return Ok(());
    }

    git::ensure_head_unchanged(&cwd, &head_oid)?;
    git::ensure_safe_state(&cwd, &head_oid)?;
    let prepared = git::prepare_message(&cwd, &commit.message, &selected_contributors)?;
    let new_oid = git::amend_head(&cwd, &prepared)?;
    let count = selected_contributors.len();
    let label = if count == 1 {
        "co-author"
    } else {
        "co-authors"
    };

    println!(
        "Added {count} {label}: {} -> {}",
        abbreviate_oid(&head_oid),
        abbreviate_oid(&new_oid)
    );

    Ok(())
}

fn abbreviate_oid(oid: &str) -> &str {
    oid.get(..8).unwrap_or(oid)
}
