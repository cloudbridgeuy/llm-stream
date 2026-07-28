use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    forwarded: Vec<String>,
    force_rebuild: bool,
}

impl Dispatch {
    pub fn forwarded(&self) -> &[String] {
        &self.forwarded
    }

    pub fn force_rebuild(&self) -> bool {
        self.force_rebuild
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgsError {
    InvalidDispatch(String),
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDispatch(value) => {
                write!(f, "invalid cargo-xtask dispatch token: {value}")
            }
        }
    }
}

impl std::error::Error for ArgsError {}

pub fn dispatch(argv: &[String]) -> Result<Dispatch, ArgsError> {
    let mut tokens = argv.iter();
    if let Some(first) = tokens.next() {
        if first != "xtask" && !first.starts_with('-') {
            return Err(ArgsError::InvalidDispatch(first.clone()));
        }
        if first.starts_with('-') {
            tokens = argv.iter();
        }
    }

    let mut forwarded = Vec::new();
    let mut force_rebuild = false;
    for token in tokens {
        if token == "--rebuild" {
            force_rebuild = true;
        } else {
            forwarded.push(token.clone());
        }
    }
    Ok(Dispatch {
        forwarded,
        force_rebuild,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn parses_cargo_dispatch_and_bare_dispatch() {
        let parsed = dispatch(&values(&["xtask", "lint", "--verbose"])).expect("valid args");
        assert_eq!(
            parsed.forwarded(),
            values(&["lint", "--verbose"]).as_slice()
        );
        let bare = dispatch(&values(&["--help"])).expect("valid bare args");
        assert_eq!(bare.forwarded(), values(&["--help"]).as_slice());
    }

    #[test]
    fn rejects_invalid_dispatch_and_accepts_missing_dispatch() {
        assert!(dispatch(&values(&["other", "--help"])).is_err());
        assert!(dispatch(&[]).is_ok());
        assert!(dispatch(&values(&["xtask"])).is_ok());
    }

    #[test]
    fn consumes_rebuild_flags_in_any_tail_position() {
        let parsed = dispatch(&values(&[
            "xtask",
            "--rebuild",
            "lint",
            "--rebuild",
            "--verbose",
        ]))
        .expect("valid args");
        assert!(parsed.force_rebuild());
        assert_eq!(
            parsed.forwarded(),
            values(&["lint", "--verbose"]).as_slice()
        );
    }

    #[test]
    fn duplicate_rebuild_flags_are_consumed() {
        let parsed = dispatch(&values(&["xtask", "--rebuild", "--rebuild"])).expect("valid args");
        assert!(parsed.force_rebuild());
        assert!(parsed.forwarded().is_empty());
    }
}
