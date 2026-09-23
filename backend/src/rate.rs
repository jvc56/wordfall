//! Rate limits (PLAN.md § Authentication → Security generally).
//!
//! In-memory token buckets, per backend instance. Every limit is a bucket of
//! `n` tokens refilled at `n` per minute; a `429` carries `Retry-After`.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use governor::clock::{Clock, DefaultClock};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use uuid::Uuid;

use crate::config::Config;
use crate::error::ApiError;

/// A keyed `governor` limiter that answers `429` with `Retry-After`.
pub struct Keyed<K: Hash + Eq + Clone> {
    limiter: DefaultKeyedRateLimiter<K>,
    clock: DefaultClock,
}

impl<K: Hash + Eq + Clone> Keyed<K> {
    pub fn per_minute(n: u32) -> Self {
        let n = NonZeroU32::new(n).expect("rates are validated as >= 1");
        Keyed {
            limiter: RateLimiter::keyed(Quota::per_minute(n)),
            clock: DefaultClock::default(),
        }
    }

    pub fn check(&self, key: &K) -> Result<(), ApiError> {
        self.limiter
            .check_key(key)
            .map_err(|not_until| ApiError::TooManyRequests {
                retry_after_secs: ceil_secs(not_until.wait_time_from(self.clock.now())),
            })
    }
}

fn ceil_secs(d: Duration) -> u64 {
    let s = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    s.max(1)
}

/// A bucket that can be inspected before it is spent, for the login-failure
/// limits: a request is refused before any Argon2 verify when the bucket is
/// empty, and a token is taken only when the verify fails. `governor` can only
/// check-and-take, so it cannot express "a successful login spends nothing"
/// (PQ-004). Same refill rule as `governor`'s quota: `n` per minute, burst `n`.
pub struct FailureBuckets<K: Hash + Eq + Clone> {
    capacity: f64,
    per_sec: f64,
    buckets: Mutex<HashMap<K, (f64, Instant)>>,
}

impl<K: Hash + Eq + Clone> FailureBuckets<K> {
    pub fn per_minute(n: u32) -> Self {
        FailureBuckets {
            capacity: f64::from(n),
            per_sec: f64::from(n) / 60.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn level(&self, map: &mut HashMap<K, (f64, Instant)>, key: &K, now: Instant) -> f64 {
        match map.get(key) {
            None => self.capacity,
            Some(&(tokens, at)) => {
                (tokens + now.duration_since(at).as_secs_f64() * self.per_sec).min(self.capacity)
            }
        }
    }

    /// `Some(retry_after)` when the bucket has no whole token left.
    pub fn empty(&self, key: &K) -> Option<u64> {
        let now = Instant::now();
        let mut map = self.buckets.lock().expect("bucket lock");
        let level = self.level(&mut map, key, now);
        if level >= 1.0 {
            None
        } else {
            Some(((1.0 - level) / self.per_sec).ceil().max(1.0) as u64)
        }
    }

    pub fn take(&self, key: &K) {
        let now = Instant::now();
        let mut map = self.buckets.lock().expect("bucket lock");
        let level = self.level(&mut map, key, now);
        map.insert(key.clone(), ((level - 1.0).max(0.0), now));
        // Keep the map from growing without bound: full buckets carry no state.
        if map.len() > 100_000 {
            let cap = self.capacity;
            let per_sec = self.per_sec;
            map.retain(|_, (t, at)| *t + now.duration_since(*at).as_secs_f64() * per_sec < cap);
        }
    }
}

/// Every bucket, each with its named refill rate from Configuration.
pub struct Limiters {
    pub auth_ip: Keyed<IpAddr>,
    pub catalog_ip: Keyed<IpAddr>,
    pub login_fail_ip: FailureBuckets<IpAddr>,
    pub login_fail_username: FailureBuckets<String>,
    pub sync: Keyed<Uuid>,
    pub download: Keyed<Uuid>,
    pub search: Keyed<Uuid>,
    pub export: Keyed<Uuid>,
    pub admin_upload: Keyed<Uuid>,
}

impl Limiters {
    pub fn new(c: &Config) -> Self {
        Limiters {
            auth_ip: Keyed::per_minute(c.auth_rate_per_ip_per_minute),
            catalog_ip: Keyed::per_minute(c.catalog_rate_per_minute),
            login_fail_ip: FailureBuckets::per_minute(c.login_failures_per_ip_per_minute),
            login_fail_username: FailureBuckets::per_minute(
                c.login_failures_per_username_per_minute,
            ),
            sync: Keyed::per_minute(c.sync_rate_per_minute),
            download: Keyed::per_minute(c.download_rate_per_minute),
            search: Keyed::per_minute(c.search_rate_per_minute),
            export: Keyed::per_minute(c.export_rate_per_minute),
            admin_upload: Keyed::per_minute(c.admin_upload_rate_per_minute),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_bucket_peeks_without_spending() {
        let b = FailureBuckets::<u8>::per_minute(2);
        for _ in 0..10 {
            assert!(b.empty(&1).is_none());
        }
        b.take(&1);
        assert!(b.empty(&1).is_none());
        b.take(&1);
        assert!(b.empty(&1).is_some());
        assert!(b.empty(&2).is_none());
    }

    #[test]
    fn keyed_limits_per_key() {
        let k = Keyed::<u8>::per_minute(1);
        assert!(k.check(&1).is_ok());
        assert!(matches!(k.check(&1), Err(ApiError::TooManyRequests { .. })));
        assert!(k.check(&2).is_ok());
    }
}
