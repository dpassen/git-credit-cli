mod git;
mod git_app;
mod jj;
mod jj_app;
mod model;
mod summary;
mod trailers;
mod tui;

pub use git_app::run;
pub use jj_app::run as run_jj;
