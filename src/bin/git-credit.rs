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
    git_credit::run(&std::env::current_dir()?)
}
