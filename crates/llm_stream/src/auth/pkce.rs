use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::Rng;
use sha2::{Digest, Sha256};

/// A PKCE code verifier.
///
/// The inner `String` is private so a caller cannot substitute an arbitrary
/// value for the one whose challenge we actually sent to the authorization
/// server. Constructing one that never derived a challenge is not expressible.
#[derive(Debug, Clone)]
pub struct PkceVerifier(String);

impl PkceVerifier {
    /// Generates a 32-byte random verifier, base64url-encoded to 43 characters.
    pub fn generate() -> Self {
        let bytes: [u8; 32] = rand::thread_rng().gen();
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Test seam: builds a verifier from a known string so the RFC vector can
    /// be checked. Not used outside tests.
    pub fn from_raw(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derives the S256 challenge: `BASE64URL(SHA256(ASCII(verifier)))`.
    pub fn challenge(&self) -> String {
        let digest = Sha256::digest(self.0.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636 Appendix B publishes this exact verifier/challenge pair.
    #[test]
    fn challenge_matches_rfc7636_vector() {
        let verifier = PkceVerifier::from_raw("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(
            verifier.challenge(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_verifier_is_url_safe_and_long_enough() {
        let v = PkceVerifier::generate();
        assert!(
            v.as_str().len() >= 43 && v.as_str().len() <= 128,
            "RFC 7636 section 4.1 requires 43..=128 characters, got {}",
            v.as_str().len()
        );
        assert!(v
            .as_str()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
    }

    #[test]
    fn generated_verifiers_differ() {
        assert_ne!(
            PkceVerifier::generate().as_str(),
            PkceVerifier::generate().as_str()
        );
    }
}
