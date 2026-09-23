//! Sessions, CSRF and account binding (PLAN.md § Authentication).
//!
//! - `wordfall_session`: PASETO v4.local, `httpOnly`, `SameSite=Lax`, `Secure`
//!   in production, `SESSION_TTL_SECONDS` long.
//! - `wordfall_csrf`: readable by scripts, same TTL, double-submitted in the
//!   `X-CSRF-Token` header on every cookie-authenticated write (PQ-002: names).
//! - `X-Wordfall-User`: the account the tab runs as; a mismatch with the
//!   session, or no header, is `401` before anything is applied or read.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method};
use axum::response::{IntoResponseParts, ResponseParts};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand::RngExt;
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::tokens::SessionClaims;
use crate::error::ApiError;

pub const SESSION_COOKIE: &str = "wordfall_session";
pub const CSRF_COOKIE: &str = "wordfall_csrf";
pub const CSRF_HEADER: &str = "x-csrf-token";
pub const USER_HEADER: &str = "x-wordfall-user";

/// A sync in the last seven days of a cookie's life is answered with a fresh one.
pub const RENEW_WINDOW_DAYS: i64 = 7;

/// The user row, re-read on every request.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    pub username: String,
    pub is_admin: bool,
    pub session_generation: i32,
    pub claims: SessionClaims,
}

pub fn random_token() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn cookie_base(state: &AppState, name: &'static str, value: String) -> Cookie<'static> {
    let mut c = Cookie::new(name, value);
    c.set_path("/");
    c.set_same_site(SameSite::Lax);
    c.set_secure(state.config.secure_cookies);
    c
}

fn max_age_until(expires: DateTime<Utc>, now: DateTime<Utc>) -> cookie::time::Duration {
    cookie::time::Duration::seconds((expires.timestamp() - now.timestamp()).max(0))
}

/// Set-Cookie headers for a session and its CSRF token. The CSRF cookie
/// carries the same lifetime as the session cookie, never a browser-session one.
#[derive(Clone)]
pub struct SessionCookies {
    pub session: Option<Cookie<'static>>,
    pub csrf: Cookie<'static>,
}

impl SessionCookies {
    /// A fresh session of the full TTL for `uid` at `generation`, keeping `csrf` if given.
    pub fn issue(state: &AppState, uid: Uuid, generation: i32, csrf: Option<String>) -> Self {
        let now = state.clock.now();
        let claims = SessionClaims::new(uid, generation, now, state.config.session_ttl_seconds);
        let token = state.keys.seal_session(&claims);
        let mut session = cookie_base(state, SESSION_COOKIE, token);
        session.set_http_only(true);
        session.set_max_age(max_age_until(claims.expires_at(), now));
        let mut csrf_c = cookie_base(state, CSRF_COOKIE, csrf.unwrap_or_else(random_token));
        csrf_c.set_http_only(false);
        csrf_c.set_max_age(max_age_until(claims.expires_at(), now));
        SessionCookies {
            session: Some(session),
            csrf: csrf_c,
        }
    }

    /// Re-sets only the CSRF cookie, for the session the request carried.
    pub fn csrf_for(state: &AppState, claims: &SessionClaims, csrf: Option<String>) -> Self {
        let now = state.clock.now();
        let mut c = cookie_base(state, CSRF_COOKIE, csrf.unwrap_or_else(random_token));
        c.set_http_only(false);
        c.set_max_age(max_age_until(claims.expires_at(), now));
        SessionCookies {
            session: None,
            csrf: c,
        }
    }
}

impl IntoResponseParts for SessionCookies {
    type Error = std::convert::Infallible;
    fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
        if let Some(s) = self.session {
            res.headers_mut().append(
                axum::http::header::SET_COOKIE,
                HeaderValue::from_str(&s.encoded().to_string()).expect("cookie header"),
            );
        }
        res.headers_mut().append(
            axum::http::header::SET_COOKIE,
            HeaderValue::from_str(&self.csrf.encoded().to_string()).expect("cookie header"),
        );
        Ok(res)
    }
}

/// Set-Cookie headers clearing both cookies.
pub struct ClearCookies(pub bool);

impl IntoResponseParts for ClearCookies {
    type Error = std::convert::Infallible;
    fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
        for name in [SESSION_COOKIE, CSRF_COOKIE] {
            let mut c = Cookie::new(name, "");
            c.set_path("/");
            c.set_same_site(SameSite::Lax);
            c.set_secure(self.0);
            c.set_http_only(name == SESSION_COOKIE);
            c.set_max_age(cookie::time::Duration::ZERO);
            res.headers_mut().append(
                axum::http::header::SET_COOKIE,
                HeaderValue::from_str(&c.encoded().to_string()).expect("cookie header"),
            );
        }
        Ok(res)
    }
}

pub fn csrf_matches(jar: &CookieJar, headers: &HeaderMap) -> bool {
    let cookie = jar
        .get(CSRF_COOKIE)
        .map(|c| c.value().to_owned())
        .unwrap_or_default();
    let header = headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    !cookie.is_empty() && constant_time_eq(cookie.as_bytes(), header.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn is_write(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Loads the session's user, refusing a missing, expired or revoked session.
pub async fn load_session(state: &AppState, jar: &CookieJar) -> Result<CurrentUser, ApiError> {
    let token = jar.get(SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
    let claims = state
        .keys
        .open_session(token.value())
        .ok_or(ApiError::Unauthorized)?;
    if !claims.is_live(state.clock.now()) {
        return Err(ApiError::Unauthorized);
    }
    let row = sqlx::query!(
        "SELECT id, username::text AS \"username!\", is_admin, session_generation
         FROM users WHERE id = $1",
        claims.uid
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::Unauthorized)?;
    if row.session_generation != claims.generation {
        return Err(ApiError::Unauthorized);
    }
    Ok(CurrentUser {
        id: row.id,
        username: row.username,
        is_admin: row.is_admin,
        session_generation: row.session_generation,
        claims,
    })
}

fn bound_user(headers: &HeaderMap) -> Option<Uuid> {
    headers.get(USER_HEADER)?.to_str().ok()?.trim().parse().ok()
}

/// An authenticated request: a live session, `X-Wordfall-User` naming its
/// user, and for writes a matching CSRF token.
pub struct Session(pub CurrentUser);

impl FromRequestParts<AppState> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let jar = CookieJar::from_headers(&parts.headers);
        let user = load_session(state, &jar).await?;
        if bound_user(&parts.headers) != Some(user.id) {
            return Err(ApiError::Unauthorized);
        }
        if is_write(&parts.method) && !csrf_matches(&jar, &parts.headers) {
            return Err(ApiError::Forbidden("csrf"));
        }
        Ok(Session(user))
    }
}

/// An authenticated request exempt from the account binding: `GET /api/auth/me`.
pub struct UnboundSession(pub CurrentUser);

impl FromRequestParts<AppState> for UnboundSession {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let jar = CookieJar::from_headers(&parts.headers);
        let user = load_session(state, &jar).await?;
        if is_write(&parts.method) && !csrf_matches(&jar, &parts.headers) {
            return Err(ApiError::Forbidden("csrf"));
        }
        Ok(UnboundSession(user))
    }
}

/// An admin request. Non-admins get `404`, so the admin surface is not advertised.
pub struct Admin(pub CurrentUser);

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let Session(user) = Session::from_request_parts(parts, state).await?;
        if !user.is_admin {
            return Err(ApiError::NotFound);
        }
        Ok(Admin(user))
    }
}

/// The sliding TTL: in the last `RENEW_WINDOW_DAYS` of a cookie's life a
/// sync is answered with fresh cookies of the full TTL.
pub fn renewal(state: &AppState, user: &CurrentUser, jar: &CookieJar) -> Option<SessionCookies> {
    let now = state.clock.now();
    if user.claims.expires_at() - now < Duration::days(RENEW_WINDOW_DAYS) {
        let csrf = jar.get(CSRF_COOKIE).map(|c| c.value().to_owned());
        Some(SessionCookies::issue(
            state,
            user.id,
            user.session_generation,
            csrf,
        ))
    } else {
        None
    }
}
