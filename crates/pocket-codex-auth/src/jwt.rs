//! Stateless HS256 session JWTs.

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// Claims carried in a session token. Verified statelessly by the HTTP API and
/// the broker, so neither hits the database on the hot path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Internal user id (the relay-key namespace owner).
    pub sub: String,
    /// `pcxu:<user_id>` — the relay-key namespace prefix (informational; the
    /// broker derives keys from [`Claims::sub`]).
    pub ns: String,
    /// GitHub login/handle (display).
    pub login: String,
    /// GitHub numeric account id.
    pub gh_id: i64,
    /// Issued-at (unix seconds).
    pub iat: i64,
    /// Expiry (unix seconds).
    pub exp: i64,
    /// Unique token id.
    pub jti: String,
}

/// HS256 signer/verifier over a shared secret.
pub(crate) struct Jwt {
    enc: EncodingKey,
    dec: DecodingKey,
    validation: Validation,
}

impl Jwt {
    /// Build from the raw HS256 secret bytes.
    pub(crate) fn new(secret: &[u8]) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        // We don't use the `aud` claim; don't require/validate it.
        validation.validate_aud = false;
        Self {
            enc: EncodingKey::from_secret(secret),
            dec: DecodingKey::from_secret(secret),
            validation,
        }
    }

    /// Sign `claims` into a compact JWT.
    pub(crate) fn issue(&self, claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
        encode(&Header::new(Algorithm::HS256), claims, &self.enc)
    }

    /// Verify a token's signature and expiry, returning its claims.
    pub(crate) fn verify(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        Ok(decode::<Claims>(token, &self.dec, &self.validation)?.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(exp: i64) -> Claims {
        Claims {
            sub: "u1".to_string(),
            ns: "pcxu:u1".to_string(),
            login: "octocat".to_string(),
            gh_id: 42,
            iat: 0,
            exp,
            jti: "j1".to_string(),
        }
    }

    #[test]
    fn round_trips_a_valid_token() {
        let jwt = Jwt::new(b"secret");
        let token = jwt.issue(&claims(99_999_999_999)).expect("issue");
        let got = jwt.verify(&token).expect("verify");
        assert_eq!(got.sub, "u1");
        assert_eq!(got.gh_id, 42);
        assert_eq!(got.ns, "pcxu:u1");
    }

    #[test]
    fn rejects_a_token_signed_with_a_different_secret() {
        let token = Jwt::new(b"secret")
            .issue(&claims(99_999_999_999))
            .expect("issue");
        assert!(Jwt::new(b"other-secret").verify(&token).is_err());
    }

    #[test]
    fn rejects_an_expired_token() {
        let jwt = Jwt::new(b"secret");
        let token = jwt.issue(&claims(1)).expect("issue"); // exp in 1970
        assert!(jwt.verify(&token).is_err());
    }
}
