mod app;
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
    app::run(&std::env::current_dir()?)
}
