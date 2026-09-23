//! Argon2 hashing and the registration strength rule (zxcvbn score ≥ 3 and length).

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};

use crate::error::FieldError;

/// PQ-002: the plan names a length check without bounds; these are the bounds.
pub const MIN_PASSWORD_CHARS: usize = 8;
pub const MAX_PASSWORD_CHARS: usize = 128;

pub fn strength_errors(field: &str, password: &str, user_inputs: &[&str]) -> Vec<FieldError> {
    let n = password.chars().count();
    if n < MIN_PASSWORD_CHARS {
        return vec![FieldError::new(
            field,
            format!("Use at least {MIN_PASSWORD_CHARS} characters."),
        )];
    }
    if n > MAX_PASSWORD_CHARS {
        return vec![FieldError::new(
            field,
            format!("Use at most {MAX_PASSWORD_CHARS} characters."),
        )];
    }
    let score = zxcvbn::zxcvbn(password, user_inputs).score();
    if (score as u8) < 3 {
        return vec![FieldError::new(
            field,
            "This password is too easy to guess. Try a longer phrase of unrelated words.",
        )];
    }
    Vec::new()
}

/// Hashes on a blocking thread, since Argon2 is deliberately slow.
pub async fn hash(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!("argon2: {e}"))
    })
    .await?
}

pub async fn verify(password: String, phc: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&phc)
            .map(|h| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &h)
                    .is_ok()
            })
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dev_passphrase_is_strong_enough() {
        assert!(strength_errors("password", "correct-tile-rack-bingo", &[]).is_empty());
        assert!(!strength_errors("password", "password1", &[]).is_empty());
        assert!(!strength_errors("password", "short", &[]).is_empty());
    }

    #[tokio::test]
    async fn hash_verifies() {
        let h = hash("correct-tile-rack-bingo".into()).await.unwrap();
        assert!(verify("correct-tile-rack-bingo".into(), h.clone()).await);
        assert!(!verify("wrong".into(), h).await);
    }
}
