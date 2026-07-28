use color_eyre::eyre::{eyre, Context, Result};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::{Command, Output};

mod hooks;

#[derive(Debug, Clone, clap::Args)]
pub struct LintArgs {
    #[arg(long)]
    pub fix: bool,
    #[arg(long, hide = true)]
    pub staged_only: bool,
    #[arg(long)]
    pub verbose: bool,
    #[arg(long = "no-fmt")]
    pub no_fmt: bool,
    #[arg(long = "no-check")]
    pub no_check: bool,
    #[arg(long = "no-clippy")]
    pub no_clippy: bool,
    #[arg(long = "no-test")]
    pub no_test: bool,
    #[arg(long = "no-rail")]
    pub no_rail: bool,
    #[arg(long, conflicts_with_all = ["uninstall_hooks", "hooks_status"])]
    pub install_hooks: bool,
    #[arg(long, conflicts_with_all = ["install_hooks", "hooks_status"])]
    pub uninstall_hooks: bool,
    #[arg(long, conflicts_with_all = ["install_hooks", "uninstall_hooks"])]
    pub hooks_status: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckId {
    Fmt,
    Check,
    Clippy,
    Test,
    Rail,
}

impl CheckId {
    const ALL: [Self; 5] = [Self::Fmt, Self::Check, Self::Clippy, Self::Test, Self::Rail];

    const fn name(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::Check => "check",
            Self::Clippy => "clippy",
            Self::Test => "test",
            Self::Rail => "rail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckOutcome {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CheckResult {
    id: CheckId,
    outcome: CheckOutcome,
    output: String,
}

fn should_skip(id: CheckId, args: &LintArgs) -> bool {
    match id {
        CheckId::Fmt => args.no_fmt,
        CheckId::Check => args.no_check,
        CheckId::Clippy => args.no_clippy,
        CheckId::Test => args.no_test,
        CheckId::Rail => args.no_rail,
    }
}

fn effective_args(id: CheckId, fix: bool) -> Vec<&'static str> {
    match (id, fix) {
        (CheckId::Fmt, true) => vec!["fmt"],
        (CheckId::Fmt, false) => vec!["fmt", "--", "--check"],
        (CheckId::Check, _) => vec!["check", "--workspace", "--all-targets"],
        (CheckId::Clippy, true) => vec![
            "clippy",
            "--workspace",
            "--all-targets",
            "--fix",
            "--allow-dirty",
            "--",
            "-D",
            "warnings",
        ],
        (CheckId::Clippy, false) => {
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ]
        }
        (CheckId::Test, _) => vec!["test", "--workspace", "--all-targets"],
        (CheckId::Rail, true) => vec!["rail", "unify"],
        (CheckId::Rail, false) => vec!["rail", "unify", "--check"],
    }
}

fn determine_outcome(success: bool, output: String, optional: bool) -> CheckOutcome {
    if success {
        CheckOutcome::Passed
    } else {
        let lower = output.to_ascii_lowercase();
        if optional
            && (lower.contains("no such command")
                || lower.contains("could not find")
                || lower.contains("cargo-rail"))
        {
            CheckOutcome::Skipped
        } else {
            CheckOutcome::Failed
        }
    }
}

fn format_log_entry(result: &CheckResult) -> String {
    let status = match result.outcome {
        CheckOutcome::Passed => "passed",
        CheckOutcome::Failed => "failed",
        CheckOutcome::Skipped => "skipped",
    };
    format!("[{}] {status}\n{}\n", result.id.name(), result.output)
}

fn run_command(id: CheckId, fix: bool) -> Result<Output> {
    Command::new("cargo")
        .args(effective_args(id, fix))
        .output()
        .with_context(|| format!("failed to start cargo {}", id.name()))
}

fn combined_output(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

pub fn run(args: &LintArgs) -> Result<()> {
    if args.install_hooks {
        return hooks::install_hooks();
    }
    if args.uninstall_hooks {
        return hooks::uninstall_hooks();
    }
    if args.hooks_status {
        return hooks::show_status();
    }

    let staged_paths = if args.staged_only {
        Some(collect_staged_rust_paths()?)
    } else {
        None
    };
    let fix = args.fix || args.staged_only;
    fs::create_dir_all("target").context("failed to create target directory")?;
    let mut log = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open("target/xtask-lint.log")
        .context("failed to open target/xtask-lint.log")?;

    for id in CheckId::ALL {
        if should_skip(id, args) {
            let result = CheckResult {
                id,
                outcome: CheckOutcome::Skipped,
                output: String::new(),
            };
            write!(log, "{}", format_log_entry(&result))?;
            continue;
        }
        let output = run_command(id, fix)?;
        let text = combined_output(&output);
        let outcome = determine_outcome(output.status.success(), text.clone(), id == CheckId::Rail);
        let result = CheckResult {
            id,
            outcome,
            output: text.clone(),
        };
        write!(log, "{}", format_log_entry(&result))?;
        if args.verbose && !text.is_empty() {
            print!("{text}");
        }
        if outcome == CheckOutcome::Failed {
            return Err(eyre!(
                "lint check '{}' failed; see target/xtask-lint.log",
                id.name()
            ));
        }
    }

    if let Some(paths) = staged_paths {
        restage_paths(&paths)?;
    }
    Ok(())
}

fn collect_staged_rust_paths() -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--cached", "--name-only", "--diff-filter=ACM"])
        .output()
        .context("failed to inspect staged files")?;
    if !output.status.success() {
        return Err(eyre!("git could not list staged files"));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|path| path.ends_with(".rs"))
        .map(str::to_owned)
        .collect())
}

fn restage_paths(paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let status = Command::new("git").arg("add").args(paths).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(eyre!("git failed to re-stage Rust files"))
    }
}

impl fmt::Display for CheckId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> LintArgs {
        LintArgs {
            fix: false,
            staged_only: false,
            verbose: false,
            no_fmt: false,
            no_check: false,
            no_clippy: false,
            no_test: false,
            no_rail: false,
            install_hooks: false,
            uninstall_hooks: false,
            hooks_status: false,
        }
    }

    #[test]
    fn every_skip_flag_skips_only_its_check() {
        for id in CheckId::ALL {
            let mut value = args();
            match id {
                CheckId::Fmt => value.no_fmt = true,
                CheckId::Check => value.no_check = true,
                CheckId::Clippy => value.no_clippy = true,
                CheckId::Test => value.no_test = true,
                CheckId::Rail => value.no_rail = true,
            }
            assert!(should_skip(id, &value));
            assert_eq!(
                CheckId::ALL
                    .into_iter()
                    .filter(|other| should_skip(*other, &value))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn fix_commands_are_exact() {
        assert_eq!(effective_args(CheckId::Fmt, true), ["fmt"]);
        assert_eq!(
            effective_args(CheckId::Clippy, true),
            [
                "clippy",
                "--workspace",
                "--all-targets",
                "--fix",
                "--allow-dirty",
                "--",
                "-D",
                "warnings"
            ]
        );
        assert_eq!(effective_args(CheckId::Rail, true), ["rail", "unify"]);
    }

    #[test]
    fn outcomes_classify_pass_fail_and_missing_optional_tool() {
        assert_eq!(
            determine_outcome(true, "ok".into(), false),
            CheckOutcome::Passed
        );
        assert_eq!(
            determine_outcome(false, "error".into(), false),
            CheckOutcome::Failed
        );
        assert_eq!(
            determine_outcome(false, "no such command: rail".into(), true),
            CheckOutcome::Skipped
        );
    }

    #[test]
    fn log_entries_are_stable() {
        for (outcome, status) in [
            (CheckOutcome::Passed, "passed"),
            (CheckOutcome::Failed, "failed"),
            (CheckOutcome::Skipped, "skipped"),
        ] {
            let result = CheckResult {
                id: CheckId::Fmt,
                outcome,
                output: "details\n".into(),
            };
            assert_eq!(
                format_log_entry(&result),
                format!("[fmt] {status}\ndetails\n\n")
            );
        }
    }
}
