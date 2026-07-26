mod git;
mod tui;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "git-credit",
    version,
    about = "Easily add 'Co-authored-by' trailers to HEAD"
)]
struct Args {}

fn main() -> anyhow::Result<()> {
    Args::parse();

    let cwd = std::env::current_dir()?;
    let head_oid = git::inspect_head(&cwd)?;
    let contributors = git::discover_contributors(&cwd, &head_oid)?;

    if contributors.is_empty() {
        anyhow::bail!("no usable contributors found");
    }

    let Some(selected) = tui::run(&contributors)? else {
        return Ok(());
    };

    if selected.is_empty() {
        return Ok(());
    }

    let message = git::read_message(&cwd, &head_oid)?;
    let selected_contributors: Vec<_> = selected
        .into_iter()
        .map(|index| &contributors[index])
        .collect();
    let prepared = git::prepare_message(&cwd, &message, &selected_contributors)?;
    print!("{prepared}");

    Ok(())
}
