//! See <https://github.com/matklad/cargo-xtask/>
//!
//! This binary defines various auxiliary build commands, which are not
//! expressible with just `cargo`.
//!
//! The binary is integrated into the `cargo` command line by using an
//! alias in `.cargo/config`.

#![deny(clippy::unwrap_used, clippy::expect_used)]

mod cli;
mod lint;
mod scripts;

use clap::Parser;

fn main() -> color_eyre::eyre::Result<()> {
    color_eyre::install()?;
    let cli = cli::App::parse();

    match cli.command {
        cli::Commands::Build(args) => scripts::build(&args)?,
        cli::Commands::Publish(args) => scripts::publish(&args)?,
        cli::Commands::Github(args) => scripts::github(&args)?,
        cli::Commands::Install(args) => scripts::install(&args)?,
        cli::Commands::Changelog(args) => scripts::changelog(&args)?,
        cli::Commands::Lint(args) => lint::run(&args)?,
    }

    Ok(())
}
