use crate::cli;
use crate::cli::GithubArgs;
use bunt::println;
use duct::cmd;
use std::error::Error;

pub fn build(args: &cli::BuildArgs) -> Result<(), Box<dyn Error>> {
    if !std::path::Path::new("lib/bat/assets/themes/tokyonight").exists() {
        println!(
            "{$red}Error: {[yellow]} does not exist.{/$}",
            "lib/bat/assets/themes/tokyonight"
        );

        println!("{$magenta}Cleaning lib/bat directory{/$}");
        cmd("rm", ["-Rf", "lib/bat"]).read()?;

        println!("{$magenta}Cloning {[yellow]}{/$}", "bat");
        cmd(
            "git",
            [
                "clone",
                "--depth",
                "1",
                "https://github.com/sharkdp/bat.git",
                "lib/bat",
            ],
        )
        .read()?;

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

    if args.release {
        arguments.push("--release");
    }

    println!("{$magenta}Building...{/$}");
    cmd("cargo", arguments).read()?;

    Ok(())
}

fn release() -> Result<(), Box<dyn Error>> {
    let build_args = cli::BuildArgs { release: true };

    build(&build_args)?;

    Ok(())
}

pub fn install(args: &cli::InstallArgs) -> Result<(), Box<dyn Error>> {
    release()?;

    let target_path = "target/release/".to_string() + &args.name;

    cmd!("cp", &target_path, &args.path).run()?;
    cmd!("chmod", "+x", &args.path).run()?;

    Ok(())
}

pub fn publish(args: &cli::PublishArgs) -> Result<(), Box<dyn Error>> {
    let version = &args.version;

    println!("{$magenta}Publishing {[yellow]}{/$}", &version);
    github(&GithubArgs {
        version: version.clone(),
    })?;

    let mut arguments = vec!["publish", "--package", "llm_stream"];

    if args.dry_run {
        arguments.push("--dry-run");
    }

    cmd("cargo", arguments).read()?;

    Ok(())
}

pub fn github(args: &cli::GithubArgs) -> Result<(), Box<dyn Error>> {
    release()?;

    let version = &args.version;
    let target_path = "target/release/".to_string() + version;
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
    println!("{$magenta}Creating {[yellow]} release{/$}", &version);
    cmd!("gh", "release", "create", &version, "--title", &version, "--notes", &notes).run()?;
    println!(
        "{$magenta}Uploading {[yellow]} release binary{/$}",
        &version
    );
    cmd("gh", ["gh", "auth", "login", "--with-token"])
        .stdin_bytes(std::env::var("GITHUB_PAT_CLOUDBRIDGEUY")?)
        .run()?;
    cmd(
        "gh",
        ["release", "upload", version, &target_path, "--clobber"],
    )
    .run()?;

    Ok(())
}
