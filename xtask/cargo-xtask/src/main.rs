mod args;
mod staleness;

use args::Dispatch;
use staleness::Staleness;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

fn main() {
    if let Err(error) = run() {
        eprintln!("cargo-xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let raw: Vec<String> = env::args().skip(1).collect();
    let dispatch =
        args::dispatch(&raw).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let workspace = workspace_root();
    let binary = workspace.join("target/debug/xtask");
    let mut sources = Vec::new();
    collect_mtimes(&workspace.join("xtask/src"), &mut sources)?;
    sources.push(fs::metadata(workspace.join("xtask/Cargo.toml"))?.modified()?);

    if dispatch.force_rebuild()
        || matches!(
            staleness::is_stale(file_mtime(&binary), &sources),
            Staleness::Stale
        )
    {
        let status = Command::new("cargo")
            .args(["build", "--manifest-path", "xtask/Cargo.toml", "--quiet"])
            .current_dir(&workspace)
            .status()?;
        if !status.success() {
            return Err(io::Error::other("failed to build xtask"));
        }
    }

    replace_with_xtask(&binary, &dispatch)
}

fn workspace_root() -> PathBuf {
    if let Ok(value) = env::var("CARGO_WORKSPACE_DIR") {
        let candidate = PathBuf::from(value);
        if candidate.join("Cargo.toml").is_file() {
            return candidate;
        }
    }
    let mut current = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        if current.join("Cargo.toml").is_file() {
            return current;
        }
        if !current.pop() {
            return PathBuf::from(".");
        }
    }
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn collect_mtimes(path: &Path, output: &mut Vec<SystemTime>) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_mtimes(&entry?.path(), output)?;
        }
    } else if path.extension().is_some_and(|extension| extension == "rs") {
        output.push(fs::metadata(path)?.modified()?);
    }
    Ok(())
}

fn replace_with_xtask(binary: &Path, dispatch: &Dispatch) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = Command::new(binary)
            .args(dispatch.forwarded())
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .exec();
        Err(error)
    }
    #[cfg(not(unix))]
    {
        let status = Command::new(binary).args(dispatch.forwarded()).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other("xtask failed"))
        }
    }
}
