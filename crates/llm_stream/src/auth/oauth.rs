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
}
