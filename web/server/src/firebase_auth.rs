//! Firebase ID-token verifier — the relay's REMOTE-session authz.
//!
//! Remote access requires login (`/darkrun:darkrun-login`); local sessions stay
//! loopback + unauthed. A logged-in client/host presents a **Firebase ID token**
//! (a JWT Firebase Auth issues after GitHub/GitLab sign-in). This verifier
//! implements [`RelayAuth`](crate::RelayAuth): it checks the token's signature
//! against Google's public keys and its `iss`/`aud`/`exp`, then returns the
//! account id (`sub`, the Firebase `uid`).
//!
//! Keys are cached in memory (`kid -> DecodingKey`) and refreshed from Google's
//! published certs, so verification is synchronous (the [`RelayAuth`] contract).
//! The signing algorithm is injectable so the unit tests can drive the
//! claim-validation logic with a symmetric key instead of provisioning RSA.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

use crate::RelayAuth;

/// The Google endpoint publishing the public x509 certs (keyed by `kid`) that
/// sign Firebase ID tokens.
pub const FIREBASE_CERTS_URL: &str =
    "https://www.googleapis.com/robot/v1/metadata/x509/securetoken@system.gserviceaccount.com";

/// The subset of an ID token's claims we need. Firebase puts the account id in
/// `sub` (the stable Firebase `uid`), and the linked provider identities under
/// `firebase.identities` (a map of provider → array of provider subject ids).
#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    /// The Firebase-specific claim block (`firebase.identities`, `firebase.sign_in_provider`).
    #[serde(default)]
    firebase: FirebaseClaim,
}

/// The `firebase` claim block on a Firebase ID token.
#[derive(Debug, Default, Deserialize)]
struct FirebaseClaim {
    /// Linked provider identities, keyed by provider (`github.com`, `oidc.gitlab`,
    /// …); each value is the array of that provider's subject ids for the user.
    #[serde(default)]
    identities: std::collections::HashMap<String, Vec<serde_json::Value>>,
}

/// The Firebase claim key GitHub identities live under.
const GITHUB_IDENTITY_KEY: &str = "github.com";

/// A verified Firebase token: the account `uid` plus any linked GitHub identity.
///
/// The `uid` is the durable web-app session key; the GitHub identity (the numeric
/// user id from `firebase.identities["github.com"][0]`) is what the GitHub-App
/// workspace path keys installation selection on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedClaims {
    /// The Firebase account id (`sub` / `uid`).
    pub uid: String,
    /// The GitHub numeric user id, if this account has a linked GitHub identity.
    pub github_user_id: Option<String>,
}

/// Verifies Firebase ID tokens for a project, against a refreshable key cache.
pub struct FirebaseTokenAuth {
    /// The Firebase project id — the token `aud`, and the tail of its `iss`.
    project_id: String,
    /// The JWT algorithm to validate with (RS256 for real Firebase tokens).
    algorithm: Algorithm,
    /// `kid -> DecodingKey`, refreshed from Google's certs.
    keys: Arc<RwLock<HashMap<String, DecodingKey>>>,
}

impl FirebaseTokenAuth {
    /// A verifier for `project_id` validating RS256 tokens, with an empty key
    /// cache (call [`refresh_from_google`](Self::refresh_from_google) before it
    /// can verify anything).
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            algorithm: Algorithm::RS256,
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// The expected token issuer for this project.
    fn issuer(&self) -> String {
        format!("https://securetoken.google.com/{}", self.project_id)
    }

    /// Load `kid -> PEM` cert entries (Google's cert JSON shape) into the cache,
    /// replacing the prior set. Returns how many keys were loaded.
    pub fn load_certs(&self, certs: &HashMap<String, String>) -> usize {
        let mut loaded = HashMap::new();
        for (kid, pem) in certs {
            if let Ok(key) = DecodingKey::from_rsa_pem(pem.as_bytes()) {
                loaded.insert(kid.clone(), key);
            }
        }
        let n = loaded.len();
        *self.keys.write().unwrap() = loaded;
        n
    }

    /// Fetch Google's current signing certs and load them. Best-effort networking
    /// — returns the number of keys loaded, or an error string.
    pub async fn refresh_from_google(&self) -> Result<usize, String> {
        let certs: HashMap<String, String> = reqwest::Client::new()
            .get(FIREBASE_CERTS_URL)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(self.load_certs(&certs))
    }

    /// Build a [`Validation`] pinned to this project's issuer + audience.
    fn validation(&self) -> Validation {
        let mut v = Validation::new(self.algorithm);
        v.set_issuer(&[self.issuer()]);
        v.set_audience(std::slice::from_ref(&self.project_id));
        v.validate_exp = true;
        v
    }

    /// Verify `token` and return its account `uid` plus any linked GitHub
    /// identity, or `None` if verification fails. This is the richer form of
    /// [`RelayAuth::account_for`] the workspace endpoints use: they need the
    /// GitHub identity (to select the App installation), not just the `uid`.
    pub fn verify(&self, token: &str) -> Option<VerifiedClaims> {
        let claims = self.decode_claims(token)?;
        let github_user_id = github_user_id(&claims);
        Some(VerifiedClaims {
            uid: claims.sub,
            github_user_id,
        })
    }

    /// Decode + validate `token`, returning its claims (or `None`). Shared by
    /// [`RelayAuth::account_for`] and [`verify`](Self::verify).
    fn decode_claims(&self, token: &str) -> Option<Claims> {
        let header = decode_header(token).ok()?;
        let kid = header.kid?;
        let key = {
            let keys = self.keys.read().unwrap();
            keys.get(&kid)?.clone()
        };
        let data = decode::<Claims>(token, &key, &self.validation()).ok()?;
        (!data.claims.sub.is_empty()).then_some(data.claims)
    }

    /// Test constructor: a verifier whose cache holds one key for `kid` under the
    /// given `algorithm` — so the claim-validation path is exercised without
    /// provisioning RSA.
    #[cfg(test)]
    fn with_key(project_id: &str, algorithm: Algorithm, kid: &str, key: DecodingKey) -> Self {
        let mut keys = HashMap::new();
        keys.insert(kid.to_string(), key);
        Self {
            project_id: project_id.to_string(),
            algorithm,
            keys: Arc::new(RwLock::new(keys)),
        }
    }
}

impl RelayAuth for FirebaseTokenAuth {
    fn account_for(&self, token: &str) -> Option<String> {
        self.decode_claims(token).map(|c| c.sub)
    }
}

/// Extract the GitHub numeric user id from a token's `firebase.identities`
/// (`firebase.identities["github.com"][0]`). Firebase stores the value as either
/// a JSON string or number; both are normalized to the string form. `None` when
/// no GitHub identity is linked.
fn github_user_id(claims: &Claims) -> Option<String> {
    let first = claims.firebase.identities.get(GITHUB_IDENTITY_KEY)?.first()?;
    match first {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::Serialize;

    #[derive(Serialize)]
    struct TestClaims {
        sub: String,
        iss: String,
        aud: String,
        exp: usize,
        /// The `firebase` claim block — omitted from the wire when `None` so the
        /// existing tokens exercise the default (no linked identities).
        #[serde(skip_serializing_if = "Option::is_none")]
        firebase: Option<serde_json::Value>,
    }

    const SECRET: &[u8] = b"test-signing-secret";
    const KID: &str = "test-kid";
    const PROJECT: &str = "darkrun-app";

    fn token(claims: &TestClaims) -> String {
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(KID.to_string());
        encode(&header, claims, &EncodingKey::from_secret(SECRET)).unwrap()
    }

    fn verifier() -> FirebaseTokenAuth {
        FirebaseTokenAuth::with_key(
            PROJECT,
            Algorithm::HS256,
            KID,
            DecodingKey::from_secret(SECRET),
        )
    }

    fn future_exp() -> usize {
        // A fixed far-future expiry (the suite is offline; no wall-clock needed
        // beyond "not expired").
        4_102_444_800 // 2100-01-01
    }

    #[test]
    fn valid_token_resolves_to_its_uid() {
        let t = token(&TestClaims {
            sub: "uid-123".into(),
            iss: format!("https://securetoken.google.com/{PROJECT}"),
            aud: PROJECT.into(),
            exp: future_exp(),
            firebase: None,
        });
        assert_eq!(verifier().account_for(&t), Some("uid-123".to_string()));
    }

    #[test]
    fn verify_extracts_the_github_identity() {
        // A token carrying a linked GitHub identity resolves both uid + github id.
        let t = token(&TestClaims {
            sub: "uid-123".into(),
            iss: format!("https://securetoken.google.com/{PROJECT}"),
            aud: PROJECT.into(),
            exp: future_exp(),
            firebase: Some(serde_json::json!({
                "identities": { "github.com": ["583231"], "email": ["x@y.z"] },
                "sign_in_provider": "github.com",
            })),
        });
        let claims = verifier().verify(&t).unwrap();
        assert_eq!(claims.uid, "uid-123");
        assert_eq!(claims.github_user_id.as_deref(), Some("583231"));
    }

    #[test]
    fn verify_without_github_identity_yields_none_for_that_field() {
        let t = token(&TestClaims {
            sub: "uid-123".into(),
            iss: format!("https://securetoken.google.com/{PROJECT}"),
            aud: PROJECT.into(),
            exp: future_exp(),
            firebase: None,
        });
        let claims = verifier().verify(&t).unwrap();
        assert_eq!(claims.uid, "uid-123");
        assert!(claims.github_user_id.is_none());
    }

    #[test]
    fn wrong_audience_is_rejected() {
        let t = token(&TestClaims {
            sub: "uid-123".into(),
            iss: format!("https://securetoken.google.com/{PROJECT}"),
            aud: "some-other-project".into(),
            exp: future_exp(),
            firebase: None,
        });
        assert_eq!(verifier().account_for(&t), None);
    }

    #[test]
    fn wrong_issuer_is_rejected() {
        let t = token(&TestClaims {
            sub: "uid-123".into(),
            iss: "https://evil.example.com".into(),
            aud: PROJECT.into(),
            exp: future_exp(),
            firebase: None,
        });
        assert_eq!(verifier().account_for(&t), None);
    }

    #[test]
    fn expired_token_is_rejected() {
        let t = token(&TestClaims {
            sub: "uid-123".into(),
            iss: format!("https://securetoken.google.com/{PROJECT}"),
            aud: PROJECT.into(),
            exp: 1, // 1970 — long expired
            firebase: None,
        });
        assert_eq!(verifier().account_for(&t), None);
    }

    #[test]
    fn unknown_signing_key_is_rejected() {
        // A token whose header kid isn't in the cache can't be verified.
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("unknown-kid".into());
        let t = encode(
            &header,
            &TestClaims {
                sub: "uid-123".into(),
                iss: format!("https://securetoken.google.com/{PROJECT}"),
                aud: PROJECT.into(),
                exp: future_exp(),
                firebase: None,
            },
            &EncodingKey::from_secret(SECRET),
        )
        .unwrap();
        assert_eq!(verifier().account_for(&t), None);
    }

    #[test]
    fn load_certs_replaces_the_cache() {
        let auth = FirebaseTokenAuth::new(PROJECT);
        // A bogus PEM is skipped, so nothing loads.
        let mut certs = HashMap::new();
        certs.insert("k1".to_string(), "not a pem".to_string());
        assert_eq!(auth.load_certs(&certs), 0);
    }
}
