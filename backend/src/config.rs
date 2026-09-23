//! Configuration from environment variables (PLAN.md § Configuration).
//!
//! A malformed value fails startup rather than falling back to a default.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;

/// The hard ceiling on questions per quiz; `MAX_QUIZ_QUESTIONS` may only lower it.
pub const QUESTION_CEILING: u32 = 300_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailBackend {
    Console,
    Ses,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub session_signing_key: [u8; 32],
    pub bind_addr: SocketAddr,
    pub session_ttl_seconds: u64,
    pub secure_cookies: bool,
    pub mail_backend: MailBackend,
    pub mail_from: String,
    pub public_url: String,
    pub max_quiz_questions: u32,
    pub max_cascades_per_user: u32,
    pub max_saved_searches_per_user: u32,
    pub search_timeout_ms: u64,
    pub search_concurrency: u32,
    pub trash_retention_days: u32,
    pub sync_retention_days: u32,
    pub sync_max_ops: u32,
    pub min_app_version: u64,
    pub purge_interval_seconds: u64,
    pub purge_max_quizzes_per_user_per_run: u32,
    pub admin_upload_max_bytes: u64,
    pub api_max_body_bytes: u64,
    pub sync_rate_per_minute: u32,
    pub download_rate_per_minute: u32,
    pub search_rate_per_minute: u32,
    pub export_rate_per_minute: u32,
    pub admin_upload_rate_per_minute: u32,
    pub catalog_rate_per_minute: u32,
    pub login_failures_per_ip_per_minute: u32,
    pub login_failures_per_username_per_minute: u32,
    pub auth_rate_per_ip_per_minute: u32,
    pub catalog_reconcile_seconds: u64,
    pub trusted_proxy_hops: u32,
}

#[derive(Debug, thiserror::Error)]
#[error("configuration error: {0}")]
pub struct ConfigError(pub String);

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(env: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let r = Reader { env };
        let key_hex = r.required("SESSION_SIGNING_KEY")?;
        let session_signing_key = parse_key(&key_hex)
            .ok_or_else(|| ConfigError("SESSION_SIGNING_KEY must be 32 bytes as hex".into()))?;
        let mail_backend = match r.string("MAIL_BACKEND", "console").as_str() {
            "console" => MailBackend::Console,
            "ses" => MailBackend::Ses,
            other => {
                return Err(ConfigError(format!(
                    "MAIL_BACKEND must be console or ses, not {other:?}"
                )))
            }
        };
        let max_quiz_questions: u32 = r.parse("MAX_QUIZ_QUESTIONS", QUESTION_CEILING)?;
        if max_quiz_questions > QUESTION_CEILING || max_quiz_questions == 0 {
            return Err(ConfigError(format!(
                "MAX_QUIZ_QUESTIONS must be 1..={QUESTION_CEILING}"
            )));
        }
        let search_concurrency: u32 = r.parse("SEARCH_CONCURRENCY", 2)?;
        if search_concurrency == 0 {
            return Err(ConfigError("SEARCH_CONCURRENCY must be at least 1".into()));
        }
        Ok(Config {
            database_url: r.required("DATABASE_URL")?,
            session_signing_key,
            bind_addr: r.parse("BIND_ADDR", "0.0.0.0:8080".parse().unwrap())?,
            session_ttl_seconds: r.parse("SESSION_TTL_SECONDS", 2_592_000)?,
            secure_cookies: r.parse_bool("SECURE_COOKIES", false)?,
            mail_backend,
            mail_from: r.string("MAIL_FROM", "no-reply@wordfall.local"),
            public_url: r.string("PUBLIC_URL", "http://localhost:5173"),
            max_quiz_questions,
            max_cascades_per_user: r.parse("MAX_CASCADES_PER_USER", 100)?,
            max_saved_searches_per_user: r.parse("MAX_SAVED_SEARCHES_PER_USER", 200)?,
            search_timeout_ms: r.parse("SEARCH_TIMEOUT_MS", 2000)?,
            search_concurrency,
            trash_retention_days: r.parse("TRASH_RETENTION_DAYS", 30)?,
            sync_retention_days: r.parse("SYNC_RETENTION_DAYS", 90)?,
            sync_max_ops: r.parse("SYNC_MAX_OPS", 500)?,
            min_app_version: r.parse("MIN_APP_VERSION", 0)?,
            purge_interval_seconds: r.parse("PURGE_INTERVAL_SECONDS", 3600)?,
            purge_max_quizzes_per_user_per_run: r
                .parse("PURGE_MAX_QUIZZES_PER_USER_PER_RUN", 5000)?,
            admin_upload_max_bytes: r.parse("ADMIN_UPLOAD_MAX_BYTES", 104_857_600)?,
            api_max_body_bytes: r.parse("API_MAX_BODY_BYTES", 16_777_216)?,
            sync_rate_per_minute: r.parse_rate("SYNC_RATE_PER_MINUTE", 120)?,
            download_rate_per_minute: r.parse_rate("DOWNLOAD_RATE_PER_MINUTE", 120)?,
            search_rate_per_minute: r.parse_rate("SEARCH_RATE_PER_MINUTE", 30)?,
            export_rate_per_minute: r.parse_rate("EXPORT_RATE_PER_MINUTE", 10)?,
            admin_upload_rate_per_minute: r.parse_rate("ADMIN_UPLOAD_RATE_PER_MINUTE", 10)?,
            catalog_rate_per_minute: r.parse_rate("CATALOG_RATE_PER_MINUTE", 300)?,
            login_failures_per_ip_per_minute: r
                .parse_rate("LOGIN_FAILURES_PER_IP_PER_MINUTE", 10)?,
            login_failures_per_username_per_minute: r
                .parse_rate("LOGIN_FAILURES_PER_USERNAME_PER_MINUTE", 10)?,
            auth_rate_per_ip_per_minute: r.parse_rate("AUTH_RATE_PER_IP_PER_MINUTE", 30)?,
            catalog_reconcile_seconds: r.parse("CATALOG_RECONCILE_SECONDS", 60)?,
            trusted_proxy_hops: r.parse("TRUSTED_PROXY_HOPS", 0)?,
        })
    }
}

struct Reader<'a> {
    env: &'a HashMap<String, String>,
}

impl Reader<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(String::as_str)
    }

    fn required(&self, key: &str) -> Result<String, ConfigError> {
        match self.get(key) {
            Some(v) if !v.is_empty() => Ok(v.to_owned()),
            _ => Err(ConfigError(format!("{key} is required"))),
        }
    }

    fn string(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_owned()
    }

    fn parse<T: FromStr>(&self, key: &str, default: T) -> Result<T, ConfigError> {
        match self.get(key) {
            None => Ok(default),
            Some(v) => v
                .trim()
                .parse()
                .map_err(|_| ConfigError(format!("{key} has a malformed value {v:?}"))),
        }
    }

    /// A rate of zero would make a bucket that never refills.
    fn parse_rate(&self, key: &str, default: u32) -> Result<u32, ConfigError> {
        let v: u32 = self.parse(key, default)?;
        if v == 0 {
            return Err(ConfigError(format!("{key} must be at least 1")));
        }
        Ok(v)
    }

    fn parse_bool(&self, key: &str, default: bool) -> Result<bool, ConfigError> {
        match self.get(key) {
            None => Ok(default),
            Some("true") => Ok(true),
            Some("false") => Ok(false),
            Some(v) => Err(ConfigError(format!(
                "{key} must be true or false, not {v:?}"
            ))),
        }
    }
}

fn parse_key(hex: &str) -> Option<[u8; 32]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> HashMap<String, String> {
        HashMap::from([
            ("DATABASE_URL".into(), "postgres://x".into()),
            ("SESSION_SIGNING_KEY".into(), "00".repeat(32)),
        ])
    }

    #[test]
    fn defaults_apply() {
        let c = Config::from_map(&base()).unwrap();
        assert_eq!(c.session_ttl_seconds, 2_592_000);
        assert_eq!(c.max_quiz_questions, 300_000);
        assert_eq!(c.search_concurrency, 2);
        assert_eq!(c.api_max_body_bytes, 16_777_216);
        assert_eq!(c.mail_backend, MailBackend::Console);
        assert_eq!(c.bind_addr.to_string(), "0.0.0.0:8080");
    }

    #[test]
    fn malformed_values_fail() {
        for (k, v) in [
            ("SESSION_TTL_SECONDS", "abc"),
            ("SECURE_COOKIES", "yes"),
            ("MAIL_BACKEND", "smtp"),
            ("MAX_QUIZ_QUESTIONS", "300001"),
            ("SESSION_SIGNING_KEY", "abcd"),
        ] {
            let mut env = base();
            env.insert(k.into(), v.into());
            assert!(Config::from_map(&env).is_err(), "{k}={v} accepted");
        }
    }

    #[test]
    fn required_values() {
        let mut env = base();
        env.remove("DATABASE_URL");
        assert!(Config::from_map(&env).is_err());
    }
}
