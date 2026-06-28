//! Client-side PKCE + CSRF helpers for the web (authorization-code) login flow.
//!
//! The client (CLI or Flutter bridge) generates a high-entropy `code_verifier`,
//! sends only its [`challenge`] to the backend at `/auth/web/start`, and later
//! presents the raw verifier at `/auth/web/exchange`. This binds the one-time
//! exchange code to the client that started the flow, so a party that merely
//! intercepts the redirect (e.g. a custom-scheme hijacker on mobile) cannot
//! redeem it. All pure (no I/O) so it stays usable from the mobile bridge and
//! the static-musl CLI alike.

use base64::Engine as _;
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};

/// Generate a PKCE code verifier (RFC 7636 §4.1): 32 random bytes,
/// base64url-no-pad (43 chars). Keep it in memory only — never persist it.
pub fn gen_verifier() -> String {
    gen_token(32)
}

/// Generate a random, URL-safe CSRF state token the client echoes through the
/// flow and checks on the final redirect.
pub fn gen_state() -> String {
    gen_token(24)
}

/// `base64url(n random bytes)`, no padding.
fn gen_token(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The PKCE challenge for a verifier (RFC 7636 §4.2, `S256`):
/// `base64url(SHA-256(code_verifier))`, no padding.
pub fn challenge(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_rfc7636_vector() {
        // RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(challenge(verifier), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn verifier_is_high_entropy_and_unique() {
        let a = gen_verifier();
        let b = gen_verifier();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43); // 32 bytes → 43 base64url chars (no pad)
    }
}
