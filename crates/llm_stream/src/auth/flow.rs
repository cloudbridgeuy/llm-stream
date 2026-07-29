use std::path::Path;
use std::time::SystemTime;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::Rng;

use crate::auth::listener;
use crate::auth::oauth::{self, percent_encode, AuthorizeParams};
use crate::auth::pkce::PkceVerifier;
use crate::auth::store;
use crate::auth::token::{self, AccountInfo, AuthState, TokenSet};
use crate::error::Error;
use crate::prelude::Result;

/// The one sentence a signed-out operator should ever see. It names the exact
/// command that fixes the problem.
pub const NOT_SIGNED_IN: &str = "not signed in — run: llm-stream --login";

/// Runs the full browser sign-in and persists the result.
pub fn login(config_dir: &Path) -> Result<TokenSet> {
    let verifier = PkceVerifier::generate();
    let state = random_state();

    let listener = listener::bind()?;
    if let Some(notice) = listener::fallback_notice(listener.port()?) {
        eprintln!("{notice}");
    }
    let redirect_uri = listener.redirect_uri()?;

    let url = oauth::authorize_url(&AuthorizeParams {
        redirect_uri: redirect_uri.clone(),
        state: state.clone(),
        challenge: verifier.challenge(),
    });

    // Print before opening. On a headless box or over SSH the browser will not
    // open, and the printed URL is the whole fallback.
    eprintln!("Open this URL to sign in:\n\n{url}\n");
    open_browser(&url);
    eprintln!("Waiting for the browser to complete sign-in...");

    let code = listener.await_code(&state)?;

    let body = form_encode(&[
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", &redirect_uri),
        ("client_id", oauth::CLIENT_ID),
        ("code_verifier", verifier.as_str()),
    ]);

    let response = post_form(oauth::TOKEN_URL, &body)?;
    let tokens = token::parse_token_response(&response).map_err(|e| Error::Auth(e.to_string()))?;
    store::save(config_dir, &tokens)?;
    Ok(tokens)
}

/// Exchanges the refresh token for a fresh access token and persists it.
pub fn refresh(config_dir: &Path, tokens: &TokenSet) -> Result<TokenSet> {
    if tokens.refresh_token.is_empty() {
        return Err(Error::Auth(NOT_SIGNED_IN.to_string()));
    }

    let body = form_encode(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &tokens.refresh_token),
        ("client_id", oauth::CLIENT_ID),
        ("scope", oauth::SCOPE),
    ]);

    let response = post_form(oauth::TOKEN_URL, &body)?;
    let mut fresh =
        token::parse_token_response(&response).map_err(|e| Error::Auth(e.to_string()))?;

    // A refresh response may legitimately omit the refresh token and the ID
    // token. Dropping them would force a full re-login at the next expiry and
    // would make `--login-status` forget who you are.
    if fresh.refresh_token.is_empty() {
        fresh.refresh_token.clone_from(&tokens.refresh_token);
    }
    if fresh.id_token.is_empty() {
        fresh.id_token.clone_from(&tokens.id_token);
        fresh.account_id.clone_from(&tokens.account_id);
    }

    store::save(config_dir, &fresh)?;
    Ok(fresh)
}

/// Returns credentials usable right now, refreshing if needed.
pub fn ensure_valid(config_dir: &Path) -> Result<TokenSet> {
    match token::classify(store::load(config_dir)?, SystemTime::now()) {
        AuthState::Valid(set) => Ok(set),
        AuthState::Expired(set) => refresh(config_dir, &set),
        AuthState::Missing => Err(Error::Auth(NOT_SIGNED_IN.to_string())),
    }
}

/// Reports who is signed in and when the access token expires, without
/// refreshing anything. `Ok(None)` means no credentials are stored.
pub fn status(config_dir: &Path) -> Result<Option<(AccountInfo, SystemTime)>> {
    let Some(set) = store::load(config_dir)? else {
        return Ok(None);
    };
    let info = token::account_info(&set.id_token).unwrap_or_default();
    let expiry = token::expiry_of(&set.access_token).unwrap_or(SystemTime::UNIX_EPOCH);
    Ok(Some((info, expiry)))
}

/// A 32-byte random CSRF nonce, base64url-encoded.
fn random_state() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Best-effort browser launch. A non-zero exit or a missing opener is not an
/// error: headless and SSH sessions are normal, and the URL was already printed.
fn open_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(url).status();
}

fn post_form(url: &str, body: &str) -> Result<String> {
    match ureq::post(url)
        .set("content-type", "application/x-www-form-urlencoded")
        .send_string(body)
    {
        Ok(response) => response.into_string().map_err(Into::into),
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_string().unwrap_or_default();
            Err(Error::Http(format!("{url} returned {code}: {detail}")))
        }
        Err(e) => Err(Error::Http(e.to_string())),
    }
}
