mod git;

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
    let head = git::inspect_head(&cwd)?;
    let contributors = git::discover_contributors(&cwd)?;

    dbg!(&head.branch, &head.oid, &contributors);
    Ok(())
}
