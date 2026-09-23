//! PASETO v4.local tokens: the session cookie and, under a key derived with
//! HKDF (label `wordfall export token`), the export download token. Neither
//! decrypts under the other's key (PLAN.md § Configuration).

use chrono::{DateTime, TimeZone, Utc};
use hkdf::Hkdf;
use pasetors::Local;
use pasetors::keys::SymmetricKey;
use pasetors::token::UntrustedToken;
use pasetors::version4::{LocalToken, V4};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha256;
use uuid::Uuid;

pub const EXPORT_KEY_LABEL: &[u8] = b"wordfall export token";

#[derive(Clone)]
pub struct Keys {
    session: [u8; 32],
    export: [u8; 32],
}

impl Keys {
    pub fn new(session_signing_key: [u8; 32]) -> Self {
        let hk = Hkdf::<Sha256>::new(None, &session_signing_key);
        let mut export = [0u8; 32];
        hk.expand(EXPORT_KEY_LABEL, &mut export)
            .expect("32 bytes is a valid HKDF length");
        Keys {
            session: session_signing_key,
            export,
        }
    }

    pub fn seal_session(&self, claims: &SessionClaims) -> String {
        seal(&self.session, claims)
    }

    pub fn open_session(&self, token: &str) -> Option<SessionClaims> {
        open(&self.session, token)
    }

    pub fn seal_export<T: Serialize>(&self, claims: &T) -> String {
        seal(&self.export, claims)
    }

    pub fn open_export<T: DeserializeOwned>(&self, token: &str) -> Option<T> {
        open(&self.export, token)
    }
}

fn seal<T: Serialize>(key: &[u8; 32], claims: &T) -> String {
    let sk = SymmetricKey::<V4>::from(key).expect("32-byte key");
    let payload = serde_json::to_vec(claims).expect("claims serialise");
    LocalToken::encrypt(&sk, &payload, None, None).expect("encrypt")
}

fn open<T: DeserializeOwned>(key: &[u8; 32], token: &str) -> Option<T> {
    let sk = SymmetricKey::<V4>::from(key).ok()?;
    let untrusted = UntrustedToken::<Local, V4>::try_from(token).ok()?;
    let trusted = LocalToken::decrypt(&sk, &untrusted, None, None).ok()?;
    serde_json::from_str(trusted.payload()).ok()
}

/// The session token carries the user id and the account's
/// `session_generation` (PLAN.md § Authentication → Login).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionClaims {
    pub kind: String,
    pub uid: Uuid,
    #[serde(rename = "gen")]
    pub generation: i32,
    pub iat: i64,
    pub exp: i64,
}

impl SessionClaims {
    pub fn new(uid: Uuid, generation: i32, issued: DateTime<Utc>, ttl_seconds: u64) -> Self {
        let iat = issued.timestamp();
        SessionClaims {
            kind: "session".into(),
            uid,
            generation,
            iat,
            exp: iat + ttl_seconds as i64,
        }
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        Utc.timestamp_opt(self.exp, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }

    pub fn is_live(&self, now: DateTime<Utc>) -> bool {
        self.kind == "session" && now.timestamp() < self.exp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trip_and_key_separation() {
        let keys = Keys::new([7u8; 32]);
        let c = SessionClaims::new(Uuid::new_v4(), 3, Utc::now(), 60);
        let t = keys.seal_session(&c);
        assert!(t.starts_with("v4.local."));
        assert_eq!(keys.open_session(&t), Some(c.clone()));
        // A session token is not an export token, and the reverse.
        assert!(keys.open_export::<SessionClaims>(&t).is_none());
        let e = keys.seal_export(&c);
        assert!(keys.open_session(&e).is_none());
        // Another key opens nothing.
        assert!(Keys::new([8u8; 32]).open_session(&t).is_none());
    }
}
