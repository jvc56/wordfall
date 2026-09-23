//! `POST /api/cascades`, `POST /api/cascades/:id/start-over` and
//! `GET /api/cascades/:id/cards` (PLAN.md § API → Cascades and sync).

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::order::{questions_hash, shuffle, to_i64};
use super::rows::{self, Progression};
use crate::app::AppState;
use crate::auth::{CurrentUser, Session};
use crate::catalog::tiles::Tile;
use crate::error::{ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::search::routes::{path_errors, prepare, run, Prepared};
use crate::search::store::{copy_spec, insert_spec, load_spec, SpecKind};
use crate::search::wire::{tree_to_json, PathError, QuizType};

pub const MAX_CARDS_PER_PAGE: i64 = 10_000;
pub const MAX_KEYS_PER_PAGE: i64 = 100_000;
/// A device timestamp ahead of the server by more than this is stored at the limit.
pub const MAX_FUTURE: Duration = Duration::minutes(5);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/cascades", post(create))
        .route("/api/cascades/{id}/start-over", post(start_over))
        .route("/api/cascades/{id}/cards", get(cards))
}

/// Device time, clamped to the server's now + 5 minutes.
pub fn clamp_at(state: &AppState, at: DateTime<Utc>) -> DateTime<Utc> {
    at.min(state.clock.now() + MAX_FUTURE)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBody {
    id: Uuid,
    source_quiz_id: Uuid,
    device_id: Uuid,
    at: DateTime<Utc>,
    name: String,
    lexicon: String,
    quiz_type: QuizType,
    clear_threshold: i64,
    segment_size: i64,
    progression: Progression,
    require_alphabetical: bool,
    filters: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartOverBody {
    id: Uuid,
    source_quiz_id: Uuid,
    device_id: Uuid,
    at: DateTime<Utc>,
}

struct NewCascade {
    id: Uuid,
    source_quiz_id: Uuid,
    device_id: Uuid,
    at: DateTime<Utc>,
    name: String,
    clear_threshold: i16,
    segment_size: i32,
    progression: Progression,
    require_alphabetical: bool,
    /// Start over copies the original's spec; creation stores the request's tree.
    copy_of_spec: Option<Uuid>,
}

fn check_fields(state: &AppState, name: &str, threshold: i64, segment_size: i64) -> Result<(), ApiError> {
    let mut errors = Vec::new();
    let n = name.chars().count();
    if n == 0 || n > 200 {
        errors.push(PathError::new(&[], "name", "The name must be 1–200 characters."));
    }
    if !(1..=100).contains(&threshold) {
        errors.push(PathError::new(&[], "clear_threshold", "The clear threshold is 1–100."));
    }
    let cap = i64::from(state.config.max_quiz_questions);
    if segment_size != 0 && !(5..=cap).contains(&segment_size) {
        errors.push(PathError::new(&[], "segment_size", format!("The segment size is 0 (off) or 5–{cap}.")));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(path_errors(errors))
    }
}

async fn existing(state: &AppState, user: &CurrentUser, id: Uuid) -> ApiResult<Option<Response>> {
    let mut conn = state.db.acquire().await?;
    let owner = sqlx::query_scalar!("SELECT user_id FROM cascades WHERE id = $1", id)
        .fetch_optional(&mut *conn)
        .await?;
    match owner {
        None => Ok(None),
        Some(o) if o != user.id => Err(ApiError::conflict("invalid")),
        Some(_) => {
            let cascade = rows::cascade(&mut conn, user.id, id).await?.ok_or(ApiError::NotFound)?;
            let source = rows::source_quiz(&mut conn, id).await?.ok_or(ApiError::NotFound)?;
            let seq = sqlx::query_scalar!("SELECT sync_seq FROM users WHERE id = $1", user.id)
                .fetch_one(&mut *conn)
                .await?;
            Ok(Some(
                (StatusCode::OK, Json(json!({ "cascade": cascade, "source_quiz": source, "sync_seq": seq.to_string() })))
                    .into_response(),
            ))
        }
    }
}

async fn count_cascades(conn: &mut sqlx::PgConnection, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM cascades WHERE user_id = $1"#, user_id)
        .fetch_one(conn)
        .await
}

fn limit_error(state: &AppState, count: i64) -> ApiError {
    ApiError::Conflict(json!({ "error": "cascade_limit", "limit": state.config.max_cascades_per_user, "count": count }))
}

/// Runs the search before any transaction, then stores the cascade in one
/// short transaction that locks the user's row and re-checks the limit.
async fn store(state: &AppState, user: &CurrentUser, n: NewCascade, p: Prepared) -> ApiResult<Response> {
    if let Some(r) = existing(state, user, n.id).await? {
        return Ok(r);
    }
    let taken = sqlx::query_scalar!("SELECT 1 AS \"one!\" FROM quizzes WHERE id = $1", n.source_quiz_id)
        .fetch_optional(&state.db)
        .await?;
    if taken.is_some() {
        return Err(ApiError::conflict("invalid"));
    }
    // A cheap limit check refuses an over-limit request without spending a search.
    let count = count_cascades(&mut *state.db.acquire().await?, user.id).await?;
    if count >= i64::from(state.config.max_cascades_per_user) {
        return Err(limit_error(state, count));
    }
    let keys = run(state, &p).await?;
    if keys.is_empty() {
        return Err(ApiError::Unprocessable(json!({ "error": "empty", "message": "The search matched nothing." })));
    }
    if keys.len() > state.config.max_quiz_questions as usize {
        return Err(ApiError::Unprocessable(json!({ "error": "over_cap", "count": keys.len(),
            "limit": state.config.max_quiz_questions })));
    }
    let dist = &p.lexicon.distribution;
    let key_text: Vec<String> = keys.iter().map(|k| dist.to_magpie(k)).collect();
    let count_q = keys.len() as i32;
    let idx: Vec<i32> = (0..count_q).collect();
    let seed: u64 = rand::random();
    let order = shuffle(&(0..count_q as u32).collect::<Vec<_>>(), seed);
    let mut positions = vec![0i32; order.len()];
    for (pos, &i) in order.iter().enumerate() {
        positions[i as usize] = pos as i32;
    }
    let hash = questions_hash(&(0..count_q as u32).collect::<Vec<_>>());
    let at = clamp_at(state, n.at);

    let mut tx = state.db.begin().await?;
    sqlx::query!("SELECT id FROM users WHERE id = $1 FOR UPDATE", user.id).fetch_one(&mut *tx).await?;
    let count = count_cascades(&mut tx, user.id).await?;
    if count >= i64::from(state.config.max_cascades_per_user) {
        return Err(limit_error(state, count));
    }
    let race = sqlx::query_scalar!("SELECT user_id FROM cascades WHERE id = $1", n.id)
        .fetch_optional(&mut *tx)
        .await?;
    if race.is_some() {
        drop(tx);
        return existing(state, user, n.id).await?.ok_or(ApiError::conflict("invalid"));
    }
    let seq = sqlx::query_scalar!("UPDATE users SET sync_seq = sync_seq + 1 WHERE id = $1 RETURNING sync_seq", user.id)
        .fetch_one(&mut *tx)
        .await?;
    let spec_id = match n.copy_of_spec {
        Some(original) => copy_spec(&mut tx, user.id, original).await?,
        None => insert_spec(&mut tx, user.id, p.quiz_type, &p.tree, SpecKind::Cascade).await?,
    };
    let leave_set_id = if p.quiz_type.is_leave() { p.leaves.as_ref().map(|s| s.id) } else { None };
    sqlx::query!(
        "INSERT INTO cascades (id, user_id, name, quiz_type, lexicon_id, leave_set_id, spec_id, clear_threshold,
                               segment_size, progression, require_alphabetical, options_changed_at, options_seq,
                               options_device_id, question_count, depth, peak_depth, updated_seq)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 1, 1, $13)",
        n.id,
        user.id,
        n.name,
        p.quiz_type as QuizType,
        p.lexicon.id,
        leave_set_id,
        spec_id,
        n.clear_threshold,
        n.segment_size,
        n.progression as Progression,
        n.require_alphabetical,
        at,
        seq,
        n.device_id,
        count_q,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO cascade_questions (cascade_id, idx, question_key)
         SELECT $1, * FROM UNNEST($2::int4[], $3::text[])",
        n.id,
        &idx,
        &key_text,
    )
    .execute(&mut *tx)
    .await?;
    // The Source quiz takes a copy of the cascade's options.
    let inserted = sqlx::query!(
        "INSERT INTO quizzes (id, cascade_id, user_id, level, origin, segment_size, progression,
                              require_alphabetical, options_changed_at, options_seq, options_device_id,
                              shuffle_seed, questions_hash, question_count, created_seq, updated_seq)
         VALUES ($1, $2, $3, 1, 'source', $4, $5, $6, $7, $8, $9, $10, $11, $12, $8, $8)",
        n.source_quiz_id,
        n.id,
        user.id,
        n.segment_size,
        n.progression as Progression,
        n.require_alphabetical,
        at,
        seq,
        n.device_id,
        to_i64(seed),
        to_i64(hash),
        count_q,
    )
    .execute(&mut *tx)
    .await;
    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(d)) if d.constraint() == Some("quizzes_pkey") => {
            return Err(ApiError::conflict("invalid"));
        }
        Err(e) => return Err(e.into()),
    }
    sqlx::query!(
        "INSERT INTO quiz_questions (quiz_id, question_idx, position, updated_seq)
         SELECT $1, * , $4 FROM UNNEST($2::int4[], $3::int4[])",
        n.source_quiz_id,
        &idx,
        &positions,
        seq,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut conn = state.db.acquire().await?;
    let cascade = rows::cascade(&mut conn, user.id, n.id).await?.ok_or(ApiError::NotFound)?;
    let source = rows::source_quiz(&mut conn, n.id).await?.ok_or(ApiError::NotFound)?;
    Ok((StatusCode::CREATED, Json(json!({ "cascade": cascade, "source_quiz": source, "sync_seq": seq.to_string() })))
        .into_response())
}

async fn create(
    State(state): State<AppState>,
    Session(user): Session,
    ApiJson(b): ApiJson<CreateBody>,
) -> ApiResult<Response> {
    state.limits.search.check(&user.id)?;
    check_fields(&state, &b.name, b.clear_threshold, b.segment_size)?;
    // A repeated request returns what the first one made, before any search.
    if let Some(r) = existing(&state, &user, b.id).await? {
        return Ok(r);
    }
    let p = prepare(&state, &b.lexicon, b.quiz_type, &b.filters)?;
    let n = NewCascade {
        id: b.id,
        source_quiz_id: b.source_quiz_id,
        device_id: b.device_id,
        at: b.at,
        name: b.name,
        clear_threshold: b.clear_threshold as i16,
        segment_size: b.segment_size as i32,
        progression: b.progression,
        require_alphabetical: b.require_alphabetical,
        copy_of_spec: None,
    };
    store(&state, &user, n, p).await
}

/// A new cascade with the same name, a private copy of the same filters, and
/// the same threshold and quiz options. Subject to the cascade limit.
async fn start_over(
    State(state): State<AppState>,
    Session(user): Session,
    Path(original): Path<Uuid>,
    ApiJson(b): ApiJson<StartOverBody>,
) -> ApiResult<Response> {
    state.limits.search.check(&user.id)?;
    if let Some(r) = existing(&state, &user, b.id).await? {
        return Ok(r);
    }
    let mut conn = state.db.acquire().await?;
    let row = sqlx::query!(
        r#"SELECT c.name, c.spec_id, c.clear_threshold, c.segment_size, c.progression AS "progression: Progression",
                  c.require_alphabetical, l.name AS lexicon
           FROM cascades c JOIN lexicons l ON l.id = c.lexicon_id WHERE c.id = $1 AND c.user_id = $2"#,
        original,
        user.id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(ApiError::NotFound)?;
    let (quiz_type, tree) = load_spec(&mut conn, row.spec_id).await?.ok_or(ApiError::NotFound)?;
    drop(conn);
    let p = prepare(&state, &row.lexicon, quiz_type, &tree_to_json(&tree))?;
    let n = NewCascade {
        id: b.id,
        source_quiz_id: b.source_quiz_id,
        device_id: b.device_id,
        at: b.at,
        name: row.name,
        clear_threshold: row.clear_threshold,
        segment_size: row.segment_size,
        progression: row.progression,
        require_alphabetical: row.require_alphabetical,
        copy_of_spec: Some(row.spec_id),
    };
    store(&state, &user, n, p).await
}

// ---------------------------------------------------------------------------
// Card pages
// ---------------------------------------------------------------------------

fn int_param(q: &HashMap<String, String>, name: &str) -> Result<Option<i64>, ApiError> {
    match q.get(name) {
        None => Ok(None),
        Some(v) => v.parse::<i64>().map(Some).map_err(|_| ApiError::bad_request(&format!("invalid_{name}"))),
    }
}

fn flag(q: &HashMap<String, String>, name: &str) -> Result<bool, ApiError> {
    match q.get(name).map(String::as_str) {
        None | Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(_) => Err(ApiError::bad_request(&format!("invalid_{name}"))),
    }
}

/// Answer cards `[{ idx, key, answer }]`, or with `keys=1` the keys alone as
/// `{ from, keys }`. Served `private, no-store`: the device keeps every byte
/// in IndexedDB, and a second copy in the HTTP cache would sit outside every
/// budget.
async fn cards(
    State(state): State<AppState>,
    Session(user): Session,
    Path(cascade_id): Path<Uuid>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    state.limits.download.check(&user.id)?;
    let keys_only = flag(&q, "keys")?;
    let hooks = flag(&q, "hooks")?;
    let definitions = flag(&q, "definitions")?;
    let from = int_param(&q, "from")?.ok_or_else(|| ApiError::bad_request("missing_from"))?;
    let limit = int_param(&q, "limit")?.ok_or_else(|| ApiError::bad_request("missing_limit"))?;
    let max = if keys_only { MAX_KEYS_PER_PAGE } else { MAX_CARDS_PER_PAGE };
    if from < 0 || limit < 1 || limit > max {
        return Err(ApiError::bad_request("invalid_range"));
    }
    if keys_only && (q.contains_key("hooks") || q.contains_key("definitions")) {
        return Err(ApiError::bad_request("keys_only"));
    }
    let row = sqlx::query!(
        r#"SELECT c.quiz_type AS "quiz_type: QuizType", c.lexicon_id FROM cascades c WHERE c.id = $1 AND c.user_id = $2"#,
        cascade_id,
        user.id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let end = from.saturating_add(limit).min(i64::from(i32::MAX));
    let rows = sqlx::query!(
        "SELECT idx, question_key FROM cascade_questions WHERE cascade_id = $1 AND idx >= $2 AND idx < $3 ORDER BY idx",
        cascade_id,
        from.min(i64::from(i32::MAX)) as i32,
        end as i32,
    )
    .fetch_all(&state.db)
    .await?;
    let body = if keys_only {
        let keys: Vec<&str> = rows.iter().map(|r| r.question_key.as_str()).collect();
        json!({ "from": from, "keys": keys })
    } else {
        let snap = state.catalog.snapshot();
        let lex = snap.lexicons.get(&row.lexicon_id).ok_or(ApiError::Unavailable { retry_after_secs: 5 })?;
        let dist = &lex.distribution;
        let leaves = snap.leave_set_for(lex.id);
        let mut cards = Vec::with_capacity(rows.len());
        for r in &rows {
            let answer = match row.quiz_type {
                QuizType::Anagram => {
                    let alpha: Vec<Tile> = dist.parse_magpie(&r.question_key, false).unwrap_or_default();
                    let words: Vec<Value> = lex
                        .anagrams(&alpha)
                        .map(|w| {
                            let mut o = serde_json::Map::new();
                            o.insert("word".into(), json!(dist.to_magpie(&w.tiles)));
                            if hooks {
                                o.insert("front_hooks".into(), json!(dist.to_magpie(&w.front_hooks)));
                                o.insert("back_hooks".into(), json!(dist.to_magpie(&w.back_hooks)));
                            }
                            if definitions {
                                o.insert("definition".into(), json!(&*w.definition));
                            }
                            Value::Object(o)
                        })
                        .collect();
                    Value::Array(words)
                }
                QuizType::Definition => {
                    let tiles = dist.parse_magpie(&r.question_key, false).unwrap_or_default();
                    json!(lex.find(&tiles).map(|w| &*w.definition))
                }
                QuizType::LeaveValue => {
                    let tiles = dist.parse_magpie(&r.question_key, true).unwrap_or_default();
                    // The shortest text that round-trips the stored f64.
                    json!(leaves.and_then(|s| s.find(&tiles)).map(|l| l.value))
                }
            };
            cards.push(json!({ "idx": r.idx, "key": r.question_key, "answer": answer }));
        }
        Value::Array(cards)
    };
    let mut resp = Json(body).into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(resp)
}
