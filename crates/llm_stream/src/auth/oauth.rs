/// OAuth client registration used by OpenAI's Codex CLI.
///
/// Every value below was read out of the Codex binary and cross-checked: the
/// scope string matches the `scp` claim of a live access token exactly. Do not
/// alter them — a mismatch produces an opaque `invalid_request` from the server.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const SCOPE: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";

/// Free-form client identifier. The server does not check it, so we identify
/// ourselves honestly rather than impersonating Codex.
pub const ORIGINATOR: &str = "llm_stream";

/// Percent-encodes per RFC 3986: everything outside `A-Za-z0-9-._~` becomes `%XX`.
pub(crate) fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// Decodes `%XX` triplets and `+` as space. A malformed escape is passed
/// through literally rather than dropped: we would rather show the operator a
/// stray `%` than silently corrupt an authorization code.
pub(crate) fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 3 <= bytes.len() => {
                let decoded = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match decoded {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The inputs to the authorization request that vary per login.
pub struct AuthorizeParams {
    pub redirect_uri: String,
    pub state: String,
    pub challenge: String,
}

/// Builds the URL the operator opens in their browser.
pub fn authorize_url(params: &AuthorizeParams) -> String {
    let query = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", params.redirect_uri.as_str()),
        ("scope", SCOPE),
        ("code_challenge", params.challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", params.state.as_str()),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", ORIGINATOR),
    ]
    .iter()
    .map(|(k, v)| format!("{k}={}", percent_encode(v)))
    .collect::<Vec<_>>()
    .join("&");

    format!("{AUTHORIZE_URL}?{query}")
}

/// An authorization code that survived the `state` check.
///
/// The field is private and the only constructor is `parse_callback`, so no
/// code path can hold a code that skipped CSRF validation. This is
/// Parse-Don't-Validate: the type carries the proof.
#[derive(Debug, Clone)]
pub struct AuthCode(String);

impl AuthCode {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    #[error("the sign-in callback did not match this login attempt")]
    StateMismatch,
    #[error("the authorization server refused the request: {0}")]
    Denied(String),
    #[error("the sign-in callback carried no authorization code")]
    MissingCode,
    #[error("the sign-in callback was not a well-formed request")]
    Malformed,
}

/// Parses the query string of the loopback callback.
///
/// Order is load-bearing: an explicit `error=` is reported first so the
/// operator sees *why* they were refused, then `state` is checked, and only
/// then is a code produced. A missing `state` falls to `StateMismatch`.
pub fn parse_callback(
    query: &str,
    expected_state: &str,
) -> std::result::Result<AuthCode, CallbackError> {
    let mut code = None;
    let mut state = None;
    let mut error = None;

    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "code" => code = Some(percent_decode(value)),
            "state" => state = Some(percent_decode(value)),
            "error" => error = Some(percent_decode(value)),
            _ => {}
        }
    }

    if let Some(reason) = error {
        return Err(CallbackError::Denied(reason));
    }

    match state.as_deref() {
        Some(s) if s == expected_state => {}
        _ => return Err(CallbackError::StateMismatch),
    }

    code.map(AuthCode).ok_or(CallbackError::MissingCode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_characters_and_leaves_unreserved_alone() {
        assert_eq!(percent_encode("abcXYZ019-._~"), "abcXYZ019-._~");
        assert_eq!(
            percent_encode("http://localhost:1455/auth/callback"),
            "http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"
        );
        assert_eq!(percent_encode("a b"), "a%20b");
    }

    #[test]
    fn decodes_percent_triplets_and_plus() {
        assert_eq!(percent_decode("a%2Bb%3Dc"), "a+b=c");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn decode_leaves_truncated_or_invalid_escapes_intact() {
        // A malformed callback must not panic or silently eat characters.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%2"), "%2");
    }

    #[test]
    fn authorize_url_contains_every_required_param() {
        let url = authorize_url(&AuthorizeParams {
            redirect_uri: "http://localhost:1455/auth/callback".into(),
            state: "st4te".into(),
            challenge: "chall".into(),
        });
        for expected in [
            "response_type=code",
            "client_id=app_EMoamEEZ73f0CkXaXp7hrann",
            "redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback",
            "code_challenge=chall",
            "code_challenge_method=S256",
            "state=st4te",
            "id_token_add_organizations=true",
            "codex_cli_simplified_flow=true",
            "originator=llm_stream",
            "offline_access",
        ] {
            assert!(url.contains(expected), "missing `{expected}` in {url}");
        }
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
    }

    #[test]
    fn parse_callback_accepts_matching_state() {
        let code = parse_callback("code=abc123&state=st4te", "st4te").expect("should parse");
        assert_eq!(code.as_str(), "abc123");
    }

    #[test]
    fn parse_callback_rejects_mismatched_state() {
        // Security-critical: any process on this machine can hit our loopback
        // port. A forged callback must never yield a usable code.
        assert!(matches!(
            parse_callback("code=abc123&state=attacker", "st4te"),
            Err(CallbackError::StateMismatch)
        ));
    }

    #[test]
    fn parse_callback_rejects_absent_state() {
        // Absence is a mismatch, not a pass. Getting this wrong reopens the
        // exact hole the previous test closes.
        assert!(matches!(
            parse_callback("code=abc123", "st4te"),
            Err(CallbackError::StateMismatch)
        ));
    }

    #[test]
    fn parse_callback_reports_provider_denial() {
        assert!(matches!(
            parse_callback("error=access_denied&state=st4te", "st4te"),
            Err(CallbackError::Denied(ref s)) if s == "access_denied"
        ));
    }

    #[test]
    fn parse_callback_rejects_missing_code() {
        assert!(matches!(
            parse_callback("state=st4te", "st4te"),
            Err(CallbackError::MissingCode)
        ));
    }

    #[test]
    fn parse_callback_percent_decodes_the_code() {
        let code = parse_callback("code=a%2Bb%3Dc&state=st4te", "st4te").expect("should parse");
        assert_eq!(code.as_str(), "a+b=c");
    }

    #[test]
    fn parse_callback_ignores_unknown_parameters() {
        let code =
            parse_callback("iss=https%3A%2F%2Fx&code=abc&state=st4te", "st4te").expect("should parse");
        assert_eq!(code.as_str(), "abc");
    }
}
