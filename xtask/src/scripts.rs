use crate::cli;
use crate::cli::GithubArgs;
use bunt::println;
use duct::cmd;
use std::error::Error;
use std::io::Write;

pub fn build(args: &cli::BuildArgs) -> Result<(), Box<dyn Error>> {
    if !std::path::Path::new("lib/bat/assets/themes/tokyonight").exists() {
        println!(
            "{$red}Error: {[yellow]} does not exist.{/$}",
            "lib/bat/assets/themes/tokyonight"
        );

        println!("{$magenta}Copying {[yellow]} theme{/$}", "tokyonight");
        cmd(
            "cp",
            [
                "-Rp",
                "./crates/llm_stream/assets/themes/tokyonight",
                "./lib/bat/assets/themes/",
            ],
        )
        .read()?;
    }

    let mut arguments = vec!["build", "--verbose"];

    if let Some(bin) = &args.bin {
        println!("{$magenta}Building {[yellow]}{/$}", bin);
        arguments.push("--bin");
        arguments.push(bin);
    }

    if args.release {
        println!("{$magenta}Building in release mode{/$}");
        arguments.push("--release");
    }

    println!("{$magenta}Building...{/$}");
    cmd("cargo", arguments).read()?;

    Ok(())
}

fn release(bin: Option<String>) -> Result<(), Box<dyn Error>> {
    let build_args = cli::BuildArgs { release: true, bin };

    build(&build_args)?;

    Ok(())
}

pub fn install(args: &cli::InstallArgs) -> Result<(), Box<dyn Error>> {
    release(Some(args.name.clone()))?;

    let target_path = "target/release/".to_string() + &args.name;

    cmd!("cp", &target_path, &args.path).run()?;
    cmd!("chmod", "+x", &args.path).run()?;

    Ok(())
}

pub fn changelog(args: &cli::ChangelogArgs) -> Result<(), Box<dyn Error>> {
    let prev_version = &args.prev_version;

    println!("{$magenta}Generating changelog{/$}");
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

    println!("{$magenta}Creating changelog entry{/$}");
    let changelog = String::from_utf8(
        cmd(
            "e",
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

    println!("{$magenta}Updating CHANGELOG.md{/$}");
    std::fs::OpenOptions::new()
        .append(true)
        .open("CHANGELOG.md")?
        .write_all(changelog.as_bytes())?;

    println!("{$magenta}Opening CHANGELOG.md in editor{/$}");
    cmd(std::env::var("EDITOR")?, ["CHANGELOG.md"]).run()?;

    Ok(())
}

pub fn publish(args: &cli::PublishArgs) -> Result<(), Box<dyn Error>> {
    let version = &args.next_version;

    println!("{$magenta}Running the changelog command{/$}");
    changelog(&cli::ChangelogArgs {
        prev_version: args.prev_version.clone(),
        next_version: version.clone(),
    })?;

    println!("{$magenta}Publishing {[yellow]} to GitHub{/$}", &version);
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

pub fn github(args: &cli::GithubArgs) -> Result<(), Box<dyn Error>> {
    release(args.bin.clone())?;

    let version = &args.version;
    let notes = "Release notes for ".to_string() + version;

    println!("{$magenta}Creating {[yellow]} tag{/$}", &version);
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

    println!("{$magenta}Pusing {[yellow]} tag{/$}", &version);
    cmd!("git", "push", "origin", &version).run()?;

    println!("{$magenta}Logging into GitHub{/$}");
    cmd("gh", ["auth", "login", "--with-token"])
        .stdin_bytes(std::env::var("GITHUB_PAT_CLOUDBRIDGEUY")?)
        .run()?;

    println!("{$magenta}Creating {[yellow]} release{/$}", &version);
    cmd!("gh", "release", "create", &version, "--title", &version, "--notes", &notes).run()?;

    println!(
        "{$magenta}Uploading {[yellow]} release binary{/$}",
        &version
    );
    if let Some(bin) = &args.bin {
        let target_path = "target/release/".to_string() + bin;

        println!(
            "{$magenta}Uploading {[yellow]} release binary{/$}",
            &version
        );
        cmd(
            "gh",
            ["release", "upload", version, &target_path, "--clobber"],
        )
        .run()?;
    }

    Ok(())
}
