//! Auth and account endpoints (PLAN.md § API → Auth and account, § Authentication).

use std::net::IpAddr;
use std::sync::OnceLock;

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::mail::{self, Email};
use crate::auth::password;
use crate::auth::session::{
    self, CSRF_COOKIE, ClearCookies, CurrentUser, SESSION_COOKIE, Session, SessionCookies,
    UnboundSession, csrf_matches,
};
use crate::error::{ApiError, ApiResult, FieldError};
use crate::extract::ApiJson;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/confirm-email", post(confirm_email))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/reset-password", post(reset_password))
        .route(
            "/api/auth/reset-password/confirm",
            post(reset_password_confirm),
        )
        .route("/api/auth/me", get(me))
        .route(
            "/api/account/sign-out-everywhere",
            post(sign_out_everywhere),
        )
        .route("/api/account/password", post(change_password))
        .route("/api/account", delete(delete_account))
}

/// The client address per-IP limits key on.
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(crate::net::client_ip(
            parts,
            state.config.trusted_proxy_hops,
        )))
    }
}

fn sha256(s: &str) -> Vec<u8> {
    Sha256::digest(s.as_bytes()).to_vec()
}

/// Queues an email after the response, if the address's caps allow it.
fn queue_mail(state: &AppState, ip: IpAddr, email: Email) {
    if !state.email_caps.try_reserve(&email.to, ip) {
        tracing::info!(kind = ?std::mem::discriminant(&email.kind), "email cap reached; not sent");
        return;
    }
    let mailer = state.mailer.clone();
    tokio::spawn(async move {
        if let Err(e) = mailer.send(email).await {
            tracing::error!(error = %e, "sending email failed");
        }
    });
}

fn is_valid_username(u: &str) -> bool {
    (3..=32).contains(&u.len()) && u.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_valid_email(e: &str) -> bool {
    let n = e.chars().count();
    if !(3..=254).contains(&n) || e.chars().any(char::is_whitespace) {
        return false;
    }
    match e.rsplit_once('@') {
        Some((local, domain)) => !local.is_empty() && !domain.is_empty() && !domain.contains('@'),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Register
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RegisterBody {
    username: String,
    email: String,
    password: String,
}

/// The same answer for a new account and an address already in use.
fn registered() -> Response {
    (StatusCode::ACCEPTED, Json(json!({}))).into_response()
}

async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<RegisterBody>,
) -> ApiResult<Response> {
    state.limits.auth_ip.check(&ip)?;
    let username = body.username.trim().to_owned();
    let email = body.email.trim().to_owned();

    let mut errors = Vec::new();
    let username_ok = is_valid_username(&username);
    if !username_ok {
        errors.push(FieldError::new(
            "username",
            "Use 3–32 letters, digits or underscores.",
        ));
    }
    if !is_valid_email(&email) {
        errors.push(FieldError::new("email", "Enter a valid email address."));
    }
    errors.extend(password::strength_errors(
        "password",
        &body.password,
        &[&username, &email],
    ));
    // A taken username is a field error, checked before anything that sends
    // email. One held by an unconfirmed account is taken while that account's
    // latest code is valid.
    if username_ok && username_taken(&state, &username).await? {
        errors.push(FieldError::new("username", "That username is taken."));
    }
    if !errors.is_empty() {
        return Err(ApiError::Fields(errors));
    }

    // Hashed on every path, stored or not, so every branch takes the same time.
    let hash = password::hash(body.password).await?;

    let mut tx = state.db.begin().await?;
    // An unconfirmed account whose codes have all expired no longer holds its
    // username or address: it is deleted and its dependents go by cascade.
    sqlx::query!(
        "DELETE FROM users u
         WHERE (u.username = $1 OR u.email = $2)
           AND u.email_confirmed_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM email_confirmations c
                           WHERE c.user_id = u.id AND c.expires_at > now())",
        &username as &str,
        &email as &str,
    )
    .execute(&mut *tx)
    .await?;

    let existing = sqlx::query!(
        "SELECT id, email::text AS \"email!\", email_confirmed_at FROM users
         WHERE email = $1 FOR UPDATE",
        &email as &str,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let mail = match existing {
        Some(u) if u.email_confirmed_at.is_some() => {
            mail::account_exists_email(&state.config.public_url, &u.email)
        }
        Some(u) => {
            // Never confirmed and a code still valid: a fresh code to the same
            // address, both valid until they expire.
            let code = session::random_token();
            insert_confirmation(&mut tx, u.id, &code).await?;
            mail::confirmation_email(&state.config.public_url, &u.email, &code)
        }
        None => {
            let user_id = match create_user(&mut tx, &username, &email, &hash).await {
                Ok(id) => id,
                Err(e) if is_unique_violation(&e, "users_username_key") => {
                    return Err(ApiError::Fields(vec![FieldError::new(
                        "username",
                        "That username is taken.",
                    )]));
                }
                Err(e) if is_unique_violation(&e, "users_email_key") => {
                    // Lost a race with another registration of the address.
                    return Ok(registered());
                }
                Err(e) => return Err(e.into()),
            };
            let code = session::random_token();
            insert_confirmation(&mut tx, user_id, &code).await?;
            mail::confirmation_email(&state.config.public_url, &email, &code)
        }
    };
    tx.commit().await?;
    queue_mail(&state, ip, mail);
    Ok(registered())
}

async fn username_taken(state: &AppState, username: &str) -> ApiResult<bool> {
    let taken = sqlx::query_scalar!(
        "SELECT EXISTS (
            SELECT 1 FROM users u
            WHERE u.username = $1
              AND (u.email_confirmed_at IS NOT NULL
                   OR EXISTS (SELECT 1 FROM email_confirmations c
                              WHERE c.user_id = u.id AND c.expires_at > now()))
         ) AS \"taken!\"",
        username as &str,
    )
    .fetch_one(&state.db)
    .await?;
    Ok(taken)
}

fn is_unique_violation(e: &sqlx::Error, constraint: &str) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.constraint() == Some(constraint))
}

async fn insert_confirmation(
    tx: &mut sqlx::PgConnection,
    user_id: Uuid,
    code: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO email_confirmations (user_id, code_hash, expires_at)
         VALUES ($1, $2, now() + interval '24 hours')",
        user_id,
        sha256(code),
    )
    .execute(tx)
    .await?;
    Ok(())
}

/// The user, its preferences row stamped with the account's first sync
/// sequence, and the default bindings, in one transaction.
async fn create_user(
    tx: &mut sqlx::PgConnection,
    username: &str,
    email: &str,
    hash: &str,
) -> Result<Uuid, sqlx::Error> {
    let user = sqlx::query!(
        "INSERT INTO users (username, email, password_hash, sync_seq)
         VALUES ($1, $2, $3, 1) RETURNING id, sync_seq",
        username as &str,
        email as &str,
        hash,
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO user_preferences (user_id, updated_seq) VALUES ($1, $2)",
        user.id,
        user.sync_seq,
    )
    .execute(&mut *tx)
    .await?;
    // Show / Next: left click or Space. Toggle grade: right click or X.
    // Previous: middle click or Backspace (PLAN.md § Preferences).
    sqlx::query!(
        "INSERT INTO user_input_bindings (user_id, action, slot, kind, code) VALUES
            ($1, 'show_next',    0, 'mouse_button', 'left'),
            ($1, 'show_next',    1, 'key',          'Space'),
            ($1, 'toggle_grade', 0, 'mouse_button', 'right'),
            ($1, 'toggle_grade', 1, 'key',          'KeyX'),
            ($1, 'previous',     0, 'mouse_button', 'middle'),
            ($1, 'previous',     1, 'key',          'Backspace')",
        user.id,
    )
    .execute(&mut *tx)
    .await?;
    Ok(user.id)
}

// ---------------------------------------------------------------------------
// Confirm email
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ConfirmBody {
    code: String,
}

async fn confirm_email(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ConfirmBody>,
) -> ApiResult<StatusCode> {
    state.limits.auth_ip.check(&ip)?;
    let mut tx = state.db.begin().await?;
    let used = sqlx::query_scalar!(
        "UPDATE email_confirmations SET used_at = now()
         WHERE code_hash = $1 AND used_at IS NULL AND expires_at > now()
         RETURNING user_id",
        sha256(body.code.trim()),
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(user_id) = used else {
        return Err(ApiError::bad_request("invalid_code"));
    };
    sqlx::query!(
        "UPDATE users SET email_confirmed_at = coalesce(email_confirmed_at, now()) WHERE id = $1",
        user_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Login, me, logout
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct LoginBody {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct Me {
    user_id: Uuid,
    username: String,
    is_admin: bool,
    trash_retention_days: u32,
    max_quiz_questions: u32,
}

fn me_body(state: &AppState, id: Uuid, username: String, is_admin: bool) -> Me {
    Me {
        user_id: id,
        username,
        is_admin,
        trash_retention_days: state.config.trash_retention_days,
        max_quiz_questions: state.config.max_quiz_questions,
    }
}

/// A hash to verify against when the username does not exist.
fn dummy_hash() -> &'static str {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| {
        use argon2::password_hash::PasswordHasher;
        argon2::Argon2::default()
            .hash_password(b"wordfall-dummy-password")
            .expect("hash")
            .to_string()
    })
}

async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<LoginBody>,
) -> ApiResult<Response> {
    let username_key = body.username.trim().to_lowercase();
    // Refused before any Argon2 verify when either failure bucket is empty.
    if let Some(retry_after_secs) = state.limits.login_fail_ip.empty(&ip) {
        return Err(ApiError::TooManyRequests { retry_after_secs });
    }
    if let Some(retry_after_secs) = state.limits.login_fail_username.empty(&username_key) {
        return Err(ApiError::TooManyRequests { retry_after_secs });
    }
    state
        .argon2_verifies
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let row = sqlx::query!(
        "SELECT id, username::text AS \"username!\", password_hash, email_confirmed_at,
                is_admin, session_generation
         FROM users WHERE username = $1",
        body.username.trim() as &str,
    )
    .fetch_optional(&state.db)
    .await?;
    let ok = match &row {
        Some(r) => password::verify(body.password, r.password_hash.clone()).await,
        None => {
            password::verify(body.password, dummy_hash().to_owned()).await;
            false
        }
    };
    let Some(user) = row.filter(|_| ok) else {
        // A token is taken from both buckets only when the verify fails.
        state.limits.login_fail_ip.take(&ip);
        state.limits.login_fail_username.take(&username_key);
        return Err(ApiError::Unauthorized);
    };
    if user.email_confirmed_at.is_none() {
        return Err(ApiError::Forbidden("email_unconfirmed"));
    }
    let cookies = SessionCookies::issue(&state, user.id, user.session_generation, None);
    Ok((
        cookies,
        Json(me_body(&state, user.id, user.username, user.is_admin)),
    )
        .into_response())
}

/// Exempt from the account binding: it reports whose cookie the browser
/// holds, and re-sets the CSRF cookie so a client holding only the session
/// cookie recovers a usable token.
async fn me(
    State(state): State<AppState>,
    jar: CookieJar,
    UnboundSession(user): UnboundSession,
) -> Response {
    let csrf = jar
        .get(CSRF_COOKIE)
        .map(|c| c.value().to_owned())
        .filter(|v| !v.is_empty());
    let cookies = SessionCookies::csrf_for(&state, &user.claims, csrf);
    (
        cookies,
        Json(me_body(&state, user.id, user.username, user.is_admin)),
    )
        .into_response()
}

/// Clears both cookies. Needs neither a session nor a CSRF token when no
/// session cookie arrives; with one, it is CSRF-checked like any other write.
/// Exempt from the account binding. Never `401`.
async fn logout(State(state): State<AppState>, jar: CookieJar, headers: HeaderMap) -> Response {
    let has_session_cookie = jar
        .get(SESSION_COOKIE)
        .is_some_and(|c| !c.value().is_empty());
    if has_session_cookie && !csrf_matches(&jar, &headers) {
        return ApiError::Forbidden("csrf").into_response();
    }
    (
        ClearCookies(state.config.secure_cookies),
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Password reset
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ResetBody {
    email: String,
}

/// Always the same answer; the lookup and the email happen after the
/// response, so both branches take the same time.
async fn reset_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ResetBody>,
) -> ApiResult<Response> {
    state.limits.auth_ip.check(&ip)?;
    let email = body.email.trim().to_owned();
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = send_reset(&st, ip, &email).await {
            tracing::error!(error = %e, "password reset failed");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(json!({}))).into_response())
}

async fn send_reset(state: &AppState, ip: IpAddr, email: &str) -> anyhow::Result<()> {
    let user = sqlx::query!(
        "SELECT id, email::text AS \"email!\" FROM users
         WHERE email = $1 AND email_confirmed_at IS NOT NULL",
        email as &str,
    )
    .fetch_optional(&state.db)
    .await?;
    let Some(user) = user else { return Ok(()) };
    if !state.email_caps.try_reserve(&user.email, ip) {
        return Ok(());
    }
    let token = session::random_token();
    sqlx::query!(
        "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at)
         VALUES ($1, $2, now() + interval '30 minutes')",
        user.id,
        sha256(&token),
    )
    .execute(&state.db)
    .await?;
    state
        .mailer
        .send(mail::reset_email(
            &state.config.public_url,
            &user.email,
            &token,
        ))
        .await
}

#[derive(Deserialize)]
pub struct ResetConfirmBody {
    token: String,
    password: String,
}

async fn reset_password_confirm(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ResetConfirmBody>,
) -> ApiResult<StatusCode> {
    state.limits.auth_ip.check(&ip)?;
    let errors = password::strength_errors("password", &body.password, &[]);
    if !errors.is_empty() {
        return Err(ApiError::Fields(errors));
    }
    let hash = password::hash(body.password).await?;
    let mut tx = state.db.begin().await?;
    let user_id = sqlx::query_scalar!(
        "UPDATE password_reset_tokens SET used_at = now()
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
         RETURNING user_id",
        sha256(body.token.trim()),
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::bad_request("invalid_token"))?;
    // Completing a reset spends every other outstanding token and signs out
    // every session.
    sqlx::query!(
        "UPDATE password_reset_tokens SET used_at = now() WHERE user_id = $1 AND used_at IS NULL",
        user_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE users SET password_hash = $2, session_generation = session_generation + 1
         WHERE id = $1",
        user_id,
        hash,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

async fn check_password(state: &AppState, user: &CurrentUser, password: String) -> ApiResult<()> {
    let hash = sqlx::query_scalar!("SELECT password_hash FROM users WHERE id = $1", user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if password::verify(password, hash).await {
        Ok(())
    } else {
        Err(ApiError::Fields(vec![FieldError::new(
            "password",
            "That password is not right.",
        )]))
    }
}

#[derive(Deserialize)]
pub struct PasswordBody {
    password: String,
}

/// Bumps `session_generation` and re-issues this device's cookies, so every
/// other session is refused while this one keeps working.
async fn sign_out_everywhere(
    State(state): State<AppState>,
    Session(user): Session,
    ApiJson(body): ApiJson<PasswordBody>,
) -> ApiResult<Response> {
    check_password(&state, &user, body.password).await?;
    let generation = sqlx::query_scalar!(
        "UPDATE users SET session_generation = session_generation + 1 WHERE id = $1
         RETURNING session_generation",
        user.id
    )
    .fetch_one(&state.db)
    .await?;
    Ok((
        SessionCookies::issue(&state, user.id, generation, None),
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

async fn change_password(
    State(state): State<AppState>,
    Session(user): Session,
    ApiJson(body): ApiJson<ChangePasswordBody>,
) -> ApiResult<Response> {
    let errors = password::strength_errors("new_password", &body.new_password, &[&user.username]);
    if !errors.is_empty() {
        return Err(ApiError::Fields(errors));
    }
    check_password(&state, &user, body.current_password)
        .await
        .map_err(|e| match e {
            ApiError::Fields(_) => ApiError::Fields(vec![FieldError::new(
                "current_password",
                "That password is not right.",
            )]),
            other => other,
        })?;
    let hash = password::hash(body.new_password).await?;
    let generation = sqlx::query_scalar!(
        "UPDATE users SET password_hash = $2, session_generation = session_generation + 1
         WHERE id = $1 RETURNING session_generation",
        user.id,
        hash,
    )
    .fetch_one(&state.db)
    .await?;
    Ok((
        SessionCookies::issue(&state, user.id, generation, None),
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

/// A single `DELETE FROM users`: everything the user owns cascades.
async fn delete_account(
    State(state): State<AppState>,
    Session(user): Session,
    ApiJson(body): ApiJson<PasswordBody>,
) -> ApiResult<Response> {
    check_password(&state, &user, body.password).await?;
    sqlx::query!("DELETE FROM users WHERE id = $1", user.id)
        .execute(&state.db)
        .await?;
    Ok((
        ClearCookies(state.config.secure_cookies),
        StatusCode::NO_CONTENT,
    )
        .into_response())
}
