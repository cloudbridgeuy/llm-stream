use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The claim namespace OpenAI uses for ChatGPT-specific fields in the ID token.
const AUTH_CLAIM: &str = "https://api.openai.com/auth";

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("the token was not a well-formed JWT")]
    MalformedJwt,
    #[error("the token is missing the `{0}` claim")]
    MissingClaim(&'static str),
    #[error("could not read the token response: {0}")]
    Json(#[from] serde_json::Error),
}

/// Who the stored credentials belong to, as far as the ID token says.
#[derive(Debug, Default, Clone)]
pub struct AccountInfo {
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

/// Decodes a JWT's payload segment to JSON.
///
/// The signature is deliberately **not** verified. We received these tokens
/// over TLS straight from the issuer and we use the claims only to schedule a
/// refresh and to print who is signed in — never to make an authorization
/// decision. Verifying would mean fetching and caching a JWKS for no gain.
fn decode_payload(jwt: &str) -> std::result::Result<serde_json::Value, TokenError> {
    let mut parts = jwt.split('.');
    let payload = match (parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(p), Some(_)) if !h.is_empty() && !p.is_empty() => p,
        _ => return Err(TokenError::MalformedJwt),
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| TokenError::MalformedJwt)?;
    serde_json::from_slice(&bytes).map_err(|_| TokenError::MalformedJwt)
}

/// Reads the `exp` claim as an absolute time.
pub fn expiry_of(access_token: &str) -> std::result::Result<SystemTime, TokenError> {
    let seconds = decode_payload(access_token)?
        .get("exp")
        .and_then(serde_json::Value::as_u64)
        .ok_or(TokenError::MissingClaim("exp"))?;
    Ok(UNIX_EPOCH + Duration::from_secs(seconds))
}

/// Reads the operator-facing identity claims from the ID token.
pub fn account_info(id_token: &str) -> std::result::Result<AccountInfo, TokenError> {
    let payload = decode_payload(id_token)?;
    Ok(AccountInfo {
        email: payload
            .get("email")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        plan_type: payload
            .get(AUTH_CLAIM)
            .and_then(|a| a.get("chatgpt_plan_type"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}

/// Reads the ChatGPT account id, which a later slice sends as a request header.
/// Absence is normal, so this returns `Option` rather than `Result`.
pub fn account_id_of(id_token: &str) -> Option<String> {
    decode_payload(id_token)
        .ok()?
        .get(AUTH_CLAIM)?
        .get("chatgpt_account_id")?
        .as_str()
        .map(str::to_owned)
}

/// The credentials we persist. `refresh_token` and `id_token` are `String`
/// rather than `Option<String>` because an empty one means the same thing as an
/// absent one to every caller, and one representation of "not there" is enough.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub account_id: Option<String>,
}

/// What we know about the stored credentials, as one closed set of cases.
///
/// Callers `match` on this instead of juggling `Option<TokenSet>` alongside an
/// `is_expired` boolean — a pair that can represent "expired but absent".
#[derive(Debug)]
pub enum AuthState {
    Missing,
    Expired(TokenSet),
    Valid(TokenSet),
}

/// How long before real expiry we start treating a token as expired.
pub const EXPIRY_SKEW: Duration = Duration::from_secs(60);

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

/// Parses an OAuth token endpoint response into a `TokenSet`.
pub fn parse_token_response(json: &str) -> std::result::Result<TokenSet, TokenError> {
    let raw: TokenResponse = serde_json::from_str(json)?;
    let id_token = raw.id_token.unwrap_or_default();
    let account_id = account_id_of(&id_token);
    Ok(TokenSet {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token.unwrap_or_default(),
        id_token,
        account_id,
    })
}

/// Decides whether stored credentials are usable as of `now`.
///
/// `now` is a parameter, not a call to `SystemTime::now()`. That is what makes
/// this a pure function and lets the expiry-boundary cases be tested without
/// sleeping or mocking a clock.
pub fn classify(tokens: Option<TokenSet>, now: SystemTime) -> AuthState {
    let Some(set) = tokens else {
        return AuthState::Missing;
    };
    match expiry_of(&set.access_token) {
        Ok(expiry) if expiry > now + EXPIRY_SKEW => AuthState::Valid(set),
        _ => AuthState::Expired(set),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// Builds a syntactically valid JWT around an arbitrary payload. The
    /// header and signature are placeholders — nothing verifies them.
    fn jwt(payload: &str) -> String {
        format!("h.{}.s", URL_SAFE_NO_PAD.encode(payload))
    }

    #[test]
    fn expiry_reads_the_exp_claim() {
        assert_eq!(
            expiry_of(&jwt(r#"{"exp":1785966321}"#)).expect("should parse"),
            UNIX_EPOCH + Duration::from_secs(1_785_966_321)
        );
    }

    #[test]
    fn expiry_rejects_malformed_jwts() {
        assert!(matches!(expiry_of("not-a-jwt"), Err(TokenError::MalformedJwt)));
        assert!(matches!(expiry_of("a.b"), Err(TokenError::MalformedJwt)));
        assert!(matches!(
            expiry_of("a.!!!notbase64!!!.c"),
            Err(TokenError::MalformedJwt)
        ));
        assert!(matches!(expiry_of(""), Err(TokenError::MalformedJwt)));
    }

    #[test]
    fn expiry_rejects_a_missing_exp() {
        assert!(matches!(
            expiry_of(&jwt(r#"{"sub":"x"}"#)),
            Err(TokenError::MissingClaim("exp"))
        ));
    }

    #[test]
    fn account_info_reads_email_and_plan() {
        let t = jwt(
            r#"{"email":"x@y.z","https://api.openai.com/auth":{"chatgpt_plan_type":"plus"}}"#,
        );
        let info = account_info(&t).expect("should parse");
        assert_eq!(info.email.as_deref(), Some("x@y.z"));
        assert_eq!(info.plan_type.as_deref(), Some("plus"));
    }

    #[test]
    fn account_info_tolerates_absent_claims() {
        let info = account_info(&jwt("{}")).expect("should parse");
        assert!(info.email.is_none());
        assert!(info.plan_type.is_none());
    }

    #[test]
    fn account_id_is_read_from_the_nested_auth_claim() {
        let t = jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct_1"}}"#);
        assert_eq!(account_id_of(&t).as_deref(), Some("acct_1"));
        assert!(account_id_of(&jwt("{}")).is_none());
        assert!(account_id_of("garbage").is_none());
    }

    fn set_with_exp(exp: u64) -> TokenSet {
        TokenSet {
            access_token: jwt(&format!(r#"{{"exp":{exp}}}"#)),
            refresh_token: "r".into(),
            id_token: "i".into(),
            account_id: None,
        }
    }

    #[test]
    fn parse_token_response_extracts_all_fields() {
        let json = r#"{"access_token":"a","refresh_token":"r","id_token":"i"}"#;
        let t = parse_token_response(json).expect("should parse");
        assert_eq!(t.access_token, "a");
        assert_eq!(t.refresh_token, "r");
        assert_eq!(t.id_token, "i");
    }

    #[test]
    fn parse_token_response_populates_account_id_from_the_id_token() {
        let id = jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct_9"}}"#);
        let json = format!(r#"{{"access_token":"a","refresh_token":"r","id_token":"{id}"}}"#);
        assert_eq!(
            parse_token_response(&json).expect("should parse").account_id.as_deref(),
            Some("acct_9")
        );
    }

    #[test]
    fn parse_token_response_tolerates_a_response_without_a_refresh_token() {
        // A refresh grant may omit it. Missing must not be an error here;
        // `flow::refresh` carries the previous one forward.
        let t = parse_token_response(r#"{"access_token":"a"}"#).expect("should parse");
        assert!(t.refresh_token.is_empty());
        assert!(t.id_token.is_empty());
    }

    #[test]
    fn parse_token_response_rejects_a_response_without_an_access_token() {
        assert!(parse_token_response(r#"{"refresh_token":"r"}"#).is_err());
    }

    #[test]
    fn classify_none_is_missing() {
        assert!(matches!(classify(None, SystemTime::now()), AuthState::Missing));
    }

    #[test]
    fn classify_future_expiry_is_valid() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        assert!(matches!(
            classify(Some(set_with_exp(2_000_000_000)), now),
            AuthState::Valid(_)
        ));
    }

    #[test]
    fn classify_past_expiry_is_expired() {
        let now = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        assert!(matches!(
            classify(Some(set_with_exp(1_000_000_000)), now),
            AuthState::Expired(_)
        ));
    }

    #[test]
    fn classify_treats_imminent_expiry_as_expired() {
        // A token with 10 seconds left must refresh now, not die mid-stream.
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        assert!(matches!(
            classify(Some(set_with_exp(1_000_000_010)), now),
            AuthState::Expired(_)
        ));
    }

    #[test]
    fn classify_treats_an_unreadable_token_as_expired_not_valid() {
        // Fail toward refreshing, never toward using a token we cannot reason
        // about. The opposite default would produce a confusing mid-stream 401.
        let set = TokenSet {
            access_token: "garbage".into(),
            refresh_token: "r".into(),
            id_token: "i".into(),
            account_id: None,
        };
        assert!(matches!(
            classify(Some(set), SystemTime::now()),
            AuthState::Expired(_)
        ));
    }
}
