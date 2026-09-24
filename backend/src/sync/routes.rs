//! `POST /api/sync`, and the questions and grades endpoints (PLAN.md § API →
//! Cascades and sync).

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use super::ops::{self, Ctx, OpError};
use super::pull::{self, PageToken};
use super::{is_transient, IncomingOp, OpResult, OP_TYPES, RESULT_BEARING};
use crate::app::AppState;
use crate::auth::session::renewal;
use crate::auth::Session;
use crate::cascade::order::{from_i64, to_i64};
use crate::cascade::rules::{Grade, Reason};
use crate::error::{ApiError, ApiResult};
use crate::extract::ApiJson;

pub const MAX_INDEX_PAGE: i64 = 50_000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/sync", post(sync))
        .route("/api/cascades/{id}/quizzes/{quiz_id}/questions", get(questions))
        .route("/api/cascades/{id}/quizzes/{quiz_id}/grades", get(grades))
}

fn bad(what: &str) -> ApiError {
    ApiError::BadRequest(json!({ "error": "invalid_request", "message": what }))
}

/// A number, or decimal text of one.
// PQ-012: sequences are read as a JSON number or as decimal text.
fn integer(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s.parse().ok(),
        _ => None,
    }
}

struct Request {
    device_id: Uuid,
    app_version: i64,
    cursor: Option<i64>,
    page_token: Option<PageToken>,
    question_rows_for: Vec<Uuid>,
    ops: Vec<IncomingOp>,
}

/// The request-level checks, made before anything is locked or applied.
fn parse_request(state: &AppState, user: Uuid, body: &Value) -> ApiResult<Request> {
    let obj = body.as_object().ok_or_else(|| bad("the body must be an object"))?;
    for k in obj.keys() {
        if !["device_id", "app_version", "cursor", "page_token", "question_rows_for", "ops"].contains(&k.as_str()) {
            return Err(bad(&format!("unexpected field {k}")));
        }
    }
    let device_id = obj
        .get("device_id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| bad("device_id"))?;
    // The frontend build's monotonic integer build number.
    let app_version = obj.get("app_version").and_then(integer).ok_or_else(|| bad("app_version must be numeric"))?;
    let cursor = match obj.get("cursor") {
        None | Some(Value::Null) => None,
        Some(v) => Some(integer(v).ok_or_else(|| bad("cursor"))?),
    };
    let ops_json: &[Value] = match obj.get("ops") {
        None => &[],
        Some(Value::Array(a)) => a,
        Some(_) => return Err(bad("ops must be an array")),
    };
    if ops_json.len() > state.config.sync_max_ops as usize {
        return Err(bad("too many operations"));
    }
    let page_token = match obj.get("page_token") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            if !ops_json.is_empty() {
                return Err(bad("a page request carries no operations"));
            }
            let t = PageToken::decode(s).filter(|t| t.u == user).ok_or_else(|| bad("page_token"))?;
            Some(t)
        }
        Some(_) => return Err(bad("page_token")),
    };
    let question_rows_for: Vec<Uuid> = match obj.get("question_rows_for") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => {
            if a.len() > state.config.max_cascades_per_user as usize {
                return Err(bad("question_rows_for is longer than the cascade limit"));
            }
            a.iter()
                .map(|v| v.as_str().and_then(|s| s.parse().ok()).ok_or_else(|| bad("question_rows_for")))
                .collect::<ApiResult<_>>()?
        }
        Some(_) => return Err(bad("question_rows_for")),
    };
    let mut ops = Vec::with_capacity(ops_json.len());
    for o in ops_json {
        let m = o.as_object().ok_or_else(|| bad("an operation must be an object"))?;
        if m.contains_key("device_id") {
            return Err(bad("device_id is sent once per request, never per operation"));
        }
        let id = m.get("id").and_then(Value::as_str).and_then(|s| s.parse().ok()).ok_or_else(|| bad("operation id"))?;
        let device_seq = m.get("device_seq").and_then(Value::as_i64).ok_or_else(|| bad("device_seq"))?;
        let op_type = m.get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
        if op_type == "set_cascade_options" || op_type == "set_quiz_options" {
            let target = if op_type == "set_cascade_options" { "cascade_id" } else { "quiz_id" };
            for k in m.keys() {
                let allowed = ["id", "device_seq", "seen_seq", "at", "type", target, "segment_size", "progression", "require_alphabetical"];
                if !allowed.contains(&k.as_str()) {
                    return Err(bad(&format!("{op_type} carries only the fields that changed; {k} is not one")));
                }
            }
        }
        ops.push(IncomingOp {
            id,
            device_seq,
            seen_seq: m.get("seen_seq").and_then(Value::as_i64).unwrap_or(-1),
            at: m.get("at").and_then(Value::as_str).and_then(|s| s.parse::<DateTime<Utc>>().ok()),
            op_type,
            body: o.clone(),
        });
    }
    ops.sort_by_key(|o| o.device_seq);
    Ok(Request { device_id, app_version, cursor, page_token, question_rows_for, ops })
}

fn recorded(
    op_id: Uuid,
    status: &str,
    reason: Option<String>,
    outcome: Option<String>,
    count: Option<i32>,
    hash: Option<i64>,
) -> OpResult {
    OpResult {
        op_id,
        status: if status == "applied" { "applied" } else { "rejected" },
        reason,
        outcome,
        new_quiz_question_count: count,
        new_quiz_questions_hash: hash.map(|h| from_i64(h).to_string()),
    }
}

async fn record(
    conn: &mut sqlx::PgConnection,
    user: Uuid,
    device: Uuid,
    op: &IncomingOp,
    r: &OpResult,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sync_operations (id, user_id, device_id, device_seq, op_type, status, reason, outcome,
                                      new_quiz_question_count, new_quiz_questions_hash)
         VALUES ($1, $2, $3, $4, $5::sync_op_type, $6::sync_op_status, $7, $8, $9, $10)",
    )
    .bind(op.id)
    .bind(user)
    .bind(device)
    .bind(op.device_seq)
    .bind(&op.op_type)
    .bind(r.status)
    .bind(&r.reason)
    .bind(&r.outcome)
    .bind(r.new_quiz_question_count)
    .bind(r.new_quiz_questions_hash.as_ref().map(|h| to_i64(h.parse::<u64>().unwrap_or(0))))
    .execute(conn)
    .await?;
    Ok(())
}

enum Pushed {
    Done { results: Vec<OpResult>, seq: i64, floor: i64 },
    /// A cursor above the user's sync_seq: nothing applied.
    AheadOfServer { seq: i64 },
}

async fn push(state: &AppState, user: Uuid, req: &Request) -> Result<Pushed, ApiError> {
    let transient = |e: sqlx::Error| {
        if is_transient(&e) {
            ApiError::Unavailable { retry_after_secs: 1 }
        } else {
            ApiError::Internal(e.into())
        }
    };
    let mut tx = state.db.begin().await.map_err(transient)?;
    // The user row lock: two syncs for one user never run concurrently, and
    // the purge task takes the same lock first.
    let u = sqlx::query!("SELECT sync_seq, sync_floor_seq FROM users WHERE id = $1 FOR UPDATE", user)
        .fetch_one(&mut *tx)
        .await
        .map_err(transient)?;
    if req.cursor.is_some_and(|c| c > u.sync_seq) {
        tx.rollback().await.ok();
        return Ok(Pushed::AheadOfServer { seq: u.sync_seq });
    }
    let seq = sqlx::query_scalar!("UPDATE users SET sync_seq = sync_seq + 1 WHERE id = $1 RETURNING sync_seq", user)
        .fetch_one(&mut *tx)
        .await
        .map_err(transient)?;
    let mark = sqlx::query_scalar!(
        "SELECT acked_below FROM sync_devices WHERE user_id = $1 AND device_id = $2",
        user,
        req.device_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(transient)?
    .unwrap_or(1);
    // `now()` is the transaction's start and fixed for its length.
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()").fetch_one(&mut *tx).await.map_err(transient)?;
    let ctx = Ctx { user, device: req.device_id, seq, cap: state.config.max_quiz_questions, now };
    // The batch's recorded operations and used device_seqs, read once rather
    // than twice per operation; each operation recorded below joins them, so
    // a repeat later in the same batch is answered exactly as before.
    let ids: Vec<Uuid> = req.ops.iter().map(|o| o.id).collect();
    let rows: Vec<(Uuid, Uuid, String, Option<String>, Option<String>, Option<i32>, Option<i64>)> = sqlx::query_as(
        "SELECT id, user_id, status::text, reason, outcome, new_quiz_question_count, new_quiz_questions_hash
         FROM sync_operations WHERE id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(transient)?;
    let mut prior: HashMap<Uuid, OpResult> = rows
        .into_iter()
        .map(|(id, owner, status, reason, outcome, count, hash)| {
            let r = if owner != user { OpResult::rejected(id, Reason::NotFound) } else { recorded(id, &status, reason, outcome, count, hash) };
            (id, r)
        })
        .collect();
    let seqs: Vec<i64> = req.ops.iter().map(|o| o.device_seq).collect();
    let mut used: HashSet<i64> = sqlx::query_scalar::<_, i64>(
        "SELECT device_seq FROM sync_operations WHERE user_id = $1 AND device_id = $2 AND device_seq = ANY($3)",
    )
    .bind(user)
    .bind(req.device_id)
    .bind(&seqs)
    .fetch_all(&mut *tx)
    .await
    .map_err(transient)?
    .into_iter()
    .collect();
    let mut results = Vec::with_capacity(req.ops.len());
    for op in &req.ops {
        // A repeated operation returns its recorded result, unchanged.
        if let Some(p) = prior.get(&op.id) {
            results.push(p.clone());
            continue;
        }
        if op.device_seq < 1 {
            results.push(OpResult::rejected(op.id, Reason::Invalid));
            continue;
        }
        // Below the device's mark with no record: acknowledged before, never applied again.
        if op.device_seq < mark {
            results.push(OpResult::applied(op.id));
            continue;
        }
        let reused = used.contains(&op.device_seq);
        if reused || !OP_TYPES.contains(&op.op_type.as_str()) {
            results.push(OpResult::rejected(op.id, Reason::Invalid));
            continue;
        }
        let result = if op.seen_seq < 0 || op.seen_seq > u.sync_seq {
            OpResult::rejected(op.id, Reason::Invalid)
        } else {
            sqlx::query("SAVEPOINT op").execute(&mut *tx).await.map_err(transient)?;
            match ops::apply(&mut tx, &ctx, op).await {
                Ok(fx) => {
                    sqlx::query("RELEASE SAVEPOINT op").execute(&mut *tx).await.map_err(transient)?;
                    OpResult {
                        op_id: op.id,
                        status: "applied",
                        reason: None,
                        outcome: fx.outcome,
                        new_quiz_question_count: fx.new_quiz_question_count,
                        new_quiz_questions_hash: fx.new_quiz_questions_hash.map(|h| h.to_string()),
                    }
                }
                Err(OpError::Rejected(reason)) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT op").execute(&mut *tx).await.map_err(transient)?;
                    OpResult::rejected(op.id, reason)
                }
                Err(OpError::Db(e)) if is_transient(&e) => return Err(ApiError::Unavailable { retry_after_secs: 1 }),
                Err(OpError::Db(e)) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT op").execute(&mut *tx).await.map_err(transient)?;
                    tracing::error!(error = %e, op = %op.body, "sync operation failed with a database error");
                    OpResult::rejected(op.id, Reason::Error)
                }
            }
        };
        record(&mut tx, user, req.device_id, op, &result).await.map_err(transient)?;
        prior.insert(op.id, result.clone());
        used.insert(op.device_seq);
        results.push(result);
    }
    if let Some(min) = req.ops.iter().map(|o| o.device_seq).filter(|s| *s >= 1).min() {
        let new_mark = sqlx::query_scalar!(
            "INSERT INTO sync_devices (user_id, device_id, acked_below) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, device_id) DO UPDATE SET acked_below = greatest(sync_devices.acked_below, EXCLUDED.acked_below)
             RETURNING acked_below",
            user,
            req.device_id,
            min
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(transient)?;
        // Records below the mark whose operations return no result fields.
        sqlx::query(
            "DELETE FROM sync_operations WHERE user_id = $1 AND device_id = $2 AND device_seq < $3
               AND op_type::text <> ALL($4)",
        )
        .bind(user)
        .bind(req.device_id)
        .bind(new_mark)
        .bind(RESULT_BEARING.map(String::from).to_vec())
        .execute(&mut *tx)
        .await
        .map_err(transient)?;
    }
    // Counted by type and reason from the logs (PLAN.md § Deployment and
    // Operations → Monitoring; infra/alarms.tf).
    for (op, r) in req.ops.iter().zip(&results) {
        if r.status == "rejected" {
            tracing::info!(op_type = %op.op_type, reason = r.reason.as_deref().unwrap_or(""), "sync operation rejected");
        }
    }
    tx.commit().await.map_err(transient)?;
    Ok(Pushed::Done { results, seq, floor: u.sync_floor_seq })
}

async fn sync(
    State(state): State<AppState>,
    jar: CookieJar,
    Session(user): Session,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Response> {
    state.limits.sync.check(&user.id)?;
    let req = parse_request(&state, user.id, &body)?;
    // The sliding TTL: a sync in the last seven days of a cookie's life gets a fresh one.
    let cookies = renewal(&state, &user, &jar);
    let respond = |status: StatusCode, v: Value| match &cookies {
        Some(c) => (status, c.clone(), Json(v)).into_response(),
        None => (status, Json(v)).into_response(),
    };

    if let Some(token) = &req.page_token {
        // A page request takes no new sequence; its token fixes the ceiling.
        let mut conn = state.db.acquire().await?;
        let (changes, next) = pull::page(&mut conn, token).await?;
        let mut v = json!({ "results": [], "changes": changes, "sync_seq": token.ceil.to_string() });
        if let Some(n) = next {
            v["next_page_token"] = json!(n.encode());
        }
        return Ok(respond(StatusCode::OK, v));
    }

    let (results, seq, floor) = match push(&state, user.id, &req).await? {
        Pushed::AheadOfServer { seq } => {
            return Ok(respond(
                StatusCode::OK,
                json!({ "results": [], "sync_seq": seq.to_string(), "resync_required": true }),
            ));
        }
        Pushed::Done { results, seq, floor } => (results, seq, floor),
    };
    // A request below MIN_APP_VERSION has its operations applied, then 426.
    let status = if (req.app_version as u64) < state.config.min_app_version {
        StatusCode::UPGRADE_REQUIRED
    } else {
        StatusCode::OK
    };
    if req.cursor.is_some_and(|c| c < floor) {
        return Ok(respond(
            status,
            json!({ "results": results, "sync_seq": seq.to_string(), "resync_required": true }),
        ));
    }
    let token = PageToken { u: user.id, cur: req.cursor, ceil: seq, qrf: req.question_rows_for.clone(), table: 0, after: vec![] };
    let mut conn = state.db.acquire().await?;
    let (changes, next) = pull::page(&mut conn, &token).await?;
    let mut v = json!({ "results": results, "changes": changes, "sync_seq": seq.to_string() });
    if let Some(n) = next {
        v["next_page_token"] = json!(n.encode());
    }
    Ok(respond(status, v))
}

// ---------------------------------------------------------------------------
// The questions and grades endpoints
// ---------------------------------------------------------------------------

fn page_params(q: &HashMap<String, String>) -> ApiResult<(i64, i64)> {
    let from = q.get("from").and_then(|s| s.parse::<i64>().ok()).ok_or_else(|| ApiError::bad_request("from"))?;
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).ok_or_else(|| ApiError::bad_request("limit"))?;
    if from < 0 || !(1..=MAX_INDEX_PAGE).contains(&limit) {
        return Err(ApiError::bad_request("invalid_range"));
    }
    Ok((from, limit))
}

/// A quiz's question indexes in idx order; `from` is an offset into that
/// dense list. The set never changes, so it is served with immutable caching.
async fn questions(
    State(state): State<AppState>,
    Session(user): Session,
    Path((cascade, quiz)): Path<(Uuid, Uuid)>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    state.limits.download.check(&user.id)?;
    let (from, limit) = page_params(&q)?;
    let origin = sqlx::query_scalar!(
        "SELECT origin::text FROM quizzes WHERE id = $1 AND cascade_id = $2 AND user_id = $3",
        quiz,
        cascade,
        user.id
    )
    .fetch_optional(&state.db)
    .await?
    .flatten()
    .ok_or(ApiError::NotFound)?;
    // A Source quiz's indexes are implied.
    if origin == "source" {
        return Err(ApiError::NotFound);
    }
    let idx: Vec<i32> = sqlx::query_scalar!(
        "SELECT question_idx FROM quiz_questions WHERE quiz_id = $1 ORDER BY question_idx OFFSET $2 LIMIT $3",
        quiz,
        from,
        limit
    )
    .fetch_all(&state.db)
    .await?;
    let mut resp = Json(idx).into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable"));
    Ok(resp)
}

/// The quiz's graded rows for the attempt its row names, as parallel arrays,
/// with `from` an index: the rows whose idx is in `from … from+limit−1`.
async fn grades(
    State(state): State<AppState>,
    Session(user): Session,
    Path((cascade, quiz)): Path<(Uuid, Uuid)>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    state.limits.download.check(&user.id)?;
    let (from, limit) = page_params(&q)?;
    let row = sqlx::query!(
        "SELECT attempt, shuffle_seed FROM quizzes WHERE id = $1 AND cascade_id = $2 AND user_id = $3",
        quiz,
        cascade,
        user.id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let end = (from + limit).min(i64::from(i32::MAX));
    let rows = sqlx::query!(
        r#"SELECT question_idx, grade AS "grade!: Grade", graded_at AS "graded_at!" FROM quiz_questions
           WHERE quiz_id = $1 AND grade IS NOT NULL AND question_idx >= $2 AND question_idx < $3
           ORDER BY question_idx"#,
        quiz,
        from.min(i64::from(i32::MAX)) as i32,
        end as i32
    )
    .fetch_all(&state.db)
    .await?;
    let body = json!({
        "attempt": row.attempt,
        "shuffle_seed": from_i64(row.shuffle_seed).to_string(),
        "question_idx": rows.iter().map(|r| r.question_idx).collect::<Vec<_>>(),
        "grade": rows.iter().map(|r| r.grade).collect::<Vec<_>>(),
        "graded_at": rows.iter().map(|r| r.graded_at).collect::<Vec<_>>(),
    });
    let mut resp = Json(body).into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(resp)
}
