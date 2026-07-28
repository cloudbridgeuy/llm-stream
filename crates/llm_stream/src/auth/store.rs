use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::auth::token::TokenSet;
use crate::prelude::Result;

/// Where credentials live: `<config_dir>/auth.json`.
pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join("auth.json")
}

/// Reads the stored credentials.
///
/// An absent file is `Ok(None)` — a fresh install is not an error. A file we
/// cannot parse is `Err`, deliberately: silently discarding credentials would
/// send the operator into a re-login loop with nothing explaining why.
pub fn load(config_dir: &Path) -> Result<Option<TokenSet>> {
    let file = path(config_dir);
    if !file.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&file)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

/// Writes the credentials with owner-only permissions.
pub fn save(config_dir: &Path, tokens: &TokenSet) -> Result<()> {
    let file_path = path(config_dir);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&file_path)?;

    // `.mode()` applies only when the file is created. Set permissions
    // explicitly so overwriting an existing, looser file still tightens it.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(serde_json::to_string_pretty(tokens)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Removes the stored credentials. Absent is success — `--logout` twice in a
/// row should not fail the second time.
pub fn clear(config_dir: &Path) -> Result<()> {
    match std::fs::remove_file(path(config_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn sample() -> TokenSet {
        TokenSet {
            access_token: "a".into(),
            refresh_token: "r".into(),
            id_token: "i".into(),
            account_id: Some("acct_1".into()),
        }
    }

    #[test]
    fn load_returns_none_when_the_file_is_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        assert!(load(dir.path()).expect("load").is_none());
    }

    #[test]
    fn save_then_load_round_trips_every_field() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        save(dir.path(), &sample()).expect("save");
        let got = load(dir.path()).expect("load").expect("some");
        assert_eq!(got.access_token, "a");
        assert_eq!(got.refresh_token, "r");
        assert_eq!(got.id_token, "i");
        assert_eq!(got.account_id.as_deref(), Some("acct_1"));
    }

    #[test]
    fn saved_file_is_owner_read_write_only() {
        // Credentials must never be group- or world-readable.
        let dir = tempfile::TempDir::new().expect("tempdir");
        save(dir.path(), &sample()).expect("save");
        let mode = std::fs::metadata(path(dir.path()))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
    }

    #[test]
    fn save_tightens_permissions_on_a_preexisting_loose_file() {
        // `OpenOptions::mode` only applies at creation, so overwriting a file
        // that is already 0644 would silently leave it world-readable.
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(path(dir.path()), "{}").expect("seed");
        std::fs::set_permissions(path(dir.path()), std::fs::Permissions::from_mode(0o644))
            .expect("chmod");
        save(dir.path(), &sample()).expect("save");
        let mode = std::fs::metadata(path(dir.path()))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
    }

    #[test]
    fn clear_removes_the_file_and_is_idempotent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        save(dir.path(), &sample()).expect("save");
        clear(dir.path()).expect("first clear");
        assert!(!path(dir.path()).exists());
        clear(dir.path()).expect("second clear must not error");
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_a_silent_none() {
        // Returning Ok(None) here would drop the user into a re-login loop
        // with no explanation of why.
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(path(dir.path()), "{").expect("seed");
        assert!(load(dir.path()).is_err());
    }
}
