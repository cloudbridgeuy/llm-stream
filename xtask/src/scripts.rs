use crate::cli;
use crate::cli::GithubArgs;
use color_eyre::eyre::{eyre, Result};
use duct::cmd;
use std::io::Write;

pub fn build(args: &cli::BuildArgs) -> Result<()> {
    let mut arguments = vec!["build", "--verbose"];

    if let Some(bin) = &args.bin {
        println!("Building {bin}");
        arguments.push("--bin");
        arguments.push(bin);
    }

    if args.release {
        println!("Building in release mode");
        arguments.push("--release");
    }

    println!("Building...");
    cmd("cargo", arguments).read()?;

    Ok(())
}

fn release(bin: Option<String>) -> Result<()> {
    let build_args = cli::BuildArgs { release: true, bin };

    build(&build_args)?;

    Ok(())
}

pub fn install(args: &cli::InstallArgs) -> Result<()> {
    release(Some(args.name.clone()))?;

    let target_path = "target/release/".to_string() + &args.name;

    cmd!("cp", &target_path, &args.path).run()?;
    cmd!("chmod", "+x", &args.path).run()?;

    Ok(())
}

pub fn changelog(args: &cli::ChangelogArgs) -> Result<()> {
    let prev_version = &args.prev_version;

    println!("Generating changelog");
    let log = cmd(
        "git",
        [
            "log",
            &(format!("{prev_version}..HEAD")),
            "--pretty=format:'%h %ad %B'",
            "--date=short",
        ],
    )
    .stdout_capture()
    .run()?
    .stdout;

    println!("Creating changelog entry");
    let changelog = String::from_utf8(
        cmd(
            "llm-stream",
            [
                "--preset",
                "sonnet",
                "--template",
                "changelog",
                "--vars",
                serde_json::json!({"prev_version": &args.prev_version.clone(), "next_version":  &args.next_version.clone()}).to_string().as_ref(),
            ],
        )
        .stdout_capture()
        .stdin_bytes(log)
        .run()?
        .stdout,
    )?;

    println!("Updating CHANGELOG.md");
    std::fs::OpenOptions::new()
        .append(true)
        .open("CHANGELOG.md")?
        .write_all(changelog.as_bytes())?;

    println!("Opening CHANGELOG.md");
    cmd(std::env::var("EDITOR")?, ["CHANGELOG.md"]).run()?;

    Ok(())
}

pub fn publish(args: &cli::PublishArgs) -> Result<()> {
    let version = &args.next_version;

    if args.no_changelog {
        println!("Skipping the changelog command");
    } else {
        println!("Running the changelog command");
        changelog(&cli::ChangelogArgs {
            prev_version: args
                .prev_version
                .clone()
                .ok_or_else(|| eyre!("prev_version is required unless --no-changelog is used"))?,
            next_version: version.clone(),
        })?;
    }

    println!("Publishing {version} to GitHub");
    github(&GithubArgs {
        version: version.clone(),
        bin: args.bin.clone(),
    })?;

    let mut arguments = vec!["publish", "--package", "llm_stream"];

    if args.dry_run {
        arguments.push("--dry-run");
    }

    cmd("cargo", arguments).read()?;

    Ok(())
}

pub fn github(args: &cli::GithubArgs) -> Result<()> {
    release(args.bin.clone())?;

    let version = &args.version;
    let notes = "Release notes for ".to_string() + version;

    println!("Creating {version} tag");
    let git_committer_date = String::from_utf8(
        cmd("git", ["log", "-n1", "--pretty=%aD"])
            .stdout_capture()
            .run()?
            .stdout,
    )?;

    cmd(
        "git",
        ["tag", "-a", "-m", &(format!("Release {version}")), version],
    )
    .env("GIT_COMMITTER_DATE", git_committer_date)
    .run()?;

    println!("Pushing {version} tag");
    cmd!("git", "push", "origin", &version).run()?;

    println!("Logging into GitHub");
    cmd("gh", ["auth", "login", "--with-token"])
        .stdin_bytes(std::env::var("GITHUB_PAT_CLOUDBRIDGEUY")?)
        .run()?;

    println!("Creating {version} release");
    cmd!("gh", "release", "create", &version, "--title", &version, "--notes", &notes).run()?;

    println!("Uploading {version} release binary");
    if let Some(bin) = &args.bin {
        let target_path = "target/release/".to_string() + bin;

        println!("Uploading {version} release binary");
        cmd(
            "gh",
            ["release", "upload", version, &target_path, "--clobber"],
        )
        .run()?;
    }

    Ok(())
}
