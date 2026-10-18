use clap::Parser;

#[derive(Parser)]
#[command(
    name = "jj-credit",
    version,
    about = "Easily add 'Co-authored-by' trailers to a Jujutsu change"
)]
struct Args {
    /// Jujutsu revision to rewrite
    #[arg(short, long, value_name = "REVSET")]
    revision: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    git_credit::run_jj(&std::env::current_dir()?, &args.revision)
}
