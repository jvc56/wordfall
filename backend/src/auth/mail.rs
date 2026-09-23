//! Outgoing email and the per-address caps (PLAN.md § Authentication → Register).
//!
//! At most three confirmation, notice or reset emails go to one address in
//! 24 hours per requesting IP, and twenty per address in all. Further requests
//! get the same response and send nothing.

use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};

use crate::clock::Clock;

pub const PER_IP_PER_ADDRESS: usize = 3;
pub const PER_ADDRESS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailKind {
    /// A new registration's confirmation code.
    Confirmation { code: String },
    /// Sent instead of a code when the address already has a confirmed account.
    AccountExists,
    /// A password reset token.
    PasswordReset { token: String },
}

#[derive(Debug, Clone)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub kind: MailKind,
}

pub type SendFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

pub trait Mailer: Send + Sync {
    fn send<'a>(&'a self, email: Email) -> SendFuture<'a>;
}

/// `MAIL_BACKEND=console`: the email is written to the log, where
/// `docker compose logs backend` (and `scripts/stack.py`) can read it.
pub struct ConsoleMailer;

impl Mailer for ConsoleMailer {
    fn send<'a>(&'a self, email: Email) -> SendFuture<'a> {
        Box::pin(async move {
            match &email.kind {
                MailKind::Confirmation { code } => tracing::info!(
                    mail_to = %email.to, confirmation_code = %code,
                    subject = %email.subject, body = %email.body, "mail"
                ),
                MailKind::PasswordReset { token } => tracing::info!(
                    mail_to = %email.to, reset_token = %token,
                    subject = %email.subject, body = %email.body, "mail"
                ),
                MailKind::AccountExists => tracing::info!(
                    mail_to = %email.to, subject = %email.subject, body = %email.body, "mail"
                ),
            }
            Ok(())
        })
    }
}

/// Keeps every email, for tests.
#[derive(Default, Clone)]
pub struct RecordingMailer {
    pub sent: Arc<Mutex<Vec<Email>>>,
}

impl RecordingMailer {
    pub fn to(&self, address: &str) -> Vec<Email> {
        let sent = self.sent.lock().expect("mail lock");
        sent.iter()
            .filter(|e| e.to.eq_ignore_ascii_case(address))
            .cloned()
            .collect()
    }
}

impl Mailer for RecordingMailer {
    fn send<'a>(&'a self, email: Email) -> SendFuture<'a> {
        Box::pin(async move {
            self.sent.lock().expect("mail lock").push(email);
            Ok(())
        })
    }
}

/// The 24-hour caps. In memory, per instance, like every limit in the plan.
pub struct EmailCaps {
    clock: Arc<Clock>,
    sent: Mutex<HashMap<String, Vec<(DateTime<Utc>, IpAddr)>>>,
}

impl EmailCaps {
    pub fn new(clock: Arc<Clock>) -> Self {
        EmailCaps {
            clock,
            sent: Mutex::new(HashMap::new()),
        }
    }

    /// Takes a slot for one email to `address` requested from `ip`, or
    /// returns false when either cap is reached.
    pub fn try_reserve(&self, address: &str, ip: IpAddr) -> bool {
        let now = self.clock.now();
        let window_start = now - Duration::hours(24);
        let key = address.to_lowercase();
        let mut sent = self.sent.lock().expect("caps lock");
        let entries = sent.entry(key).or_default();
        entries.retain(|(at, _)| *at > window_start);
        let from_ip = entries.iter().filter(|(_, i)| *i == ip).count();
        if from_ip >= PER_IP_PER_ADDRESS || entries.len() >= PER_ADDRESS {
            return false;
        }
        entries.push((now, ip));
        true
    }
}

pub fn confirmation_email(public_url: &str, to: &str, code: &str) -> Email {
    let link = format!(
        "{}/confirm-email?code={}",
        public_url.trim_end_matches('/'),
        code
    );
    Email {
        to: to.into(),
        subject: "Confirm your Wordfall account".into(),
        body: format!(
            "Welcome to Wordfall.\n\nConfirm your email address within 24 hours:\n{link}\n"
        ),
        kind: MailKind::Confirmation { code: code.into() },
    }
}

pub fn account_exists_email(public_url: &str, to: &str) -> Email {
    let base = public_url.trim_end_matches('/');
    Email {
        to: to.into(),
        subject: "Your Wordfall account".into(),
        body: format!(
            "Someone tried to register a Wordfall account with this address, which already \
             has one. If it was you, log in at {base}/login, or reset your password at \
             {base}/reset-password. Otherwise you can ignore this email.\n"
        ),
        kind: MailKind::AccountExists,
    }
}

pub fn reset_email(public_url: &str, to: &str, token: &str) -> Email {
    let link = format!(
        "{}/reset-password/confirm?token={}",
        public_url.trim_end_matches('/'),
        token
    );
    Email {
        to: to.into(),
        subject: "Reset your Wordfall password".into(),
        body: format!(
            "Set a new password within 30 minutes:\n{link}\n\nIf you did not ask for this, \
             you can ignore this email.\n"
        ),
        kind: MailKind::PasswordReset {
            token: token.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_per_ip_and_per_address() {
        let clock = Arc::new(Clock::system());
        let caps = EmailCaps::new(clock.clone());
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..3 {
            assert!(caps.try_reserve("x@y.z", a));
        }
        assert!(!caps.try_reserve("X@y.z", a));
        // Twenty in all, whatever the IPs.
        for i in 0..17 {
            let ip: IpAddr = format!("10.0.1.{i}").parse().unwrap();
            assert!(caps.try_reserve("x@y.z", ip));
        }
        assert!(!caps.try_reserve("x@y.z", "10.0.2.1".parse().unwrap()));
        clock.advance(Duration::hours(24) + Duration::seconds(1));
        assert!(caps.try_reserve("x@y.z", a));
    }
}
