use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtasks")]
#[command(about = "Run project tasks using rust instead of scripts")]
pub struct App {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Builds one of the project binaries
    Build(BuildArgs),
    /// Builds a binary and installs it at the given path
    Install(InstallArgs),
    /// Publishes a package to crates.io
    Publish(PublishArgs),
    /// Creates a new GitHub release
    Github(GithubArgs),
    /// Creates a new Changelog entry using `git` and `e`.
    Changelog(ChangelogArgs),
}

#[derive(Args, Debug)]
pub struct BuildArgs {
    /// Release flag
    #[arg(short, long)]
    pub release: bool,
}

#[derive(Args, Debug)]
pub struct PublishArgs {
    /// The previous version of the library.
    #[arg(short, long)]
    pub prev_version: String,

    /// The next version of the library.
    #[arg(short, long)]
    pub next_version: String,

    /// Dry run flag.
    #[arg(short, long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Name of the binary to run.
    #[arg(short, long)]
    pub name: String,

    /// Path to install the binary to.
    #[arg(short, long)]
    pub path: String,
}

#[derive(Args, Debug)]
pub struct ChangelogArgs {
    /// The previous version of the library.
    #[arg(short, long)]
    pub prev_version: String,

    /// The next version of the library.
    #[arg(short, long)]
    pub next_version: String,
}

#[derive(Args, Debug)]
pub struct GithubArgs {
    /// Version to be published.
    #[arg(short, long)]
    pub version: String,
}
