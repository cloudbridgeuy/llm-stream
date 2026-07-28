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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use std::time::{Duration, UNIX_EPOCH};

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
}
