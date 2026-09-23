//! Client address for per-IP limits (PLAN.md § Configuration, `TRUSTED_PROXY_HOPS`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::request::Parts;

/// With `hops` = 0 the peer address; otherwise the `hops`-th `X-Forwarded-For`
/// entry from the right, which the trusted proxy (the ALB, or the compose
/// Nginx) wrote. Falls back to the peer when the header is short.
pub fn client_ip(parts: &Parts, hops: u32) -> IpAddr {
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    if hops == 0 {
        return peer;
    }
    let entries: Vec<&str> = parts
        .headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let idx = entries.len().checked_sub(hops as usize);
    idx.and_then(|i| entries.get(i))
        .and_then(|s| s.parse().ok())
        .unwrap_or(peer)
}
