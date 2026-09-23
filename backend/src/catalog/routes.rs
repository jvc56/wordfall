//! Catalog endpoints (PLAN.md § API → Catalog and search, § API → Admin).

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use tower_http::timeout::TimeoutLayer;
use uuid::Uuid;

use super::upload::{self, UploadErrors};
use super::{ItemKind, store};
use crate::app::AppState;
use crate::auth::Admin;
use crate::auth::routes::ClientIp;
use crate::error::{ApiError, ApiResult};

/// Admin uploads are synchronous, with a 120-second timeout (PLAN.md § Upload limits).
pub const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

pub fn router(state: &AppState) -> Router<AppState> {
    let uploads = Router::new()
        .route("/api/admin/letter-distributions", post(upload_distribution))
        .route("/api/admin/lexicons", post(upload_lexicon))
        .route("/api/admin/leave-sets", post(upload_leaves))
        .layer(DefaultBodyLimit::max(
            state.config.admin_upload_max_bytes as usize,
        ))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            UPLOAD_TIMEOUT,
        ));
    Router::new()
        .route("/api/lexicons", get(lexicons))
        .route("/api/letter-distributions/{name}", get(letter_distribution))
        .route("/api/admin/catalog", get(admin_catalog))
        .route(
            "/api/admin/letter-distributions/{id}",
            delete(delete_distribution),
        )
        .route("/api/admin/lexicons/{id}", delete(delete_lexicon))
        .route("/api/admin/leave-sets/{id}", delete(delete_leave_set))
        .merge(uploads)
}

// ---------------------------------------------------------------------------
// Public catalog reads, limited per IP
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LexiconListing {
    name: String,
    letter_distribution: String,
    word_count: i32,
    leave_count: Option<i32>,
    max_num_anagrams: u32,
    max_order_rank: u32,
    max_leave_num_anagrams: Option<u32>,
    max_leave_order_rank: Option<u32>,
}

/// Only items loaded by every live instance are listed, so no listed item can
/// fail on another instance.
async fn lexicons(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
) -> ApiResult<Json<Vec<LexiconListing>>> {
    state.limits.catalog_ip.check(&ip)?;
    let rows = sqlx::query!(
        r#"WITH live AS (
             SELECT DISTINCT instance_id FROM catalog_instance_status
             WHERE heartbeat_at > now() - interval '3 minutes')
           SELECT l.id, l.name, d.name AS dist, l.word_count, ls.id AS "leave_set_id?",
                  ls.leave_count AS "leave_count?",
                  NOT EXISTS (SELECT 1 FROM live WHERE NOT EXISTS (
                      SELECT 1 FROM catalog_instance_status s
                      WHERE s.instance_id = live.instance_id AND s.item_kind = 'lexicon'
                        AND s.item_id = l.id
                        AND s.heartbeat_at > now() - interval '3 minutes')) AS "lexicon_loaded!",
                  ls.id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM live WHERE NOT EXISTS (
                      SELECT 1 FROM catalog_instance_status s
                      WHERE s.instance_id = live.instance_id AND s.item_kind = 'leave_set'
                        AND s.item_id = ls.id
                        AND s.heartbeat_at > now() - interval '3 minutes')) AS "leaves_loaded!"
           FROM lexicons l
           JOIN letter_distributions d ON d.id = l.letter_distribution_id
           LEFT JOIN leave_sets ls ON ls.lexicon_id = l.id
           ORDER BY l.name"#
    )
    .fetch_all(&state.db)
    .await?;
    let snap = state.catalog.snapshot();
    let mut out = Vec::new();
    for r in rows {
        if !r.lexicon_loaded {
            continue;
        }
        let Some(lex) = snap.lexicons.get(&r.id) else {
            continue;
        };
        let leaves = r
            .leave_set_id
            .filter(|_| r.leaves_loaded)
            .and_then(|id| snap.leave_sets.get(&id));
        out.push(LexiconListing {
            name: r.name,
            letter_distribution: r.dist,
            word_count: r.word_count,
            leave_count: leaves.map(|s| s.leave_count() as i32),
            max_num_anagrams: lex.max_num_anagrams,
            max_order_rank: lex.max_order_rank,
            max_leave_num_anagrams: leaves.map(|s| s.max_num_anagrams),
            max_leave_order_rank: leaves.map(|s| s.max_order_rank),
        });
    }
    Ok(Json(out))
}

#[derive(Serialize)]
struct TileOut {
    letter: String,
    blank_letter: String,
    count: i16,
    value: i16,
    is_vowel: bool,
}

/// Read straight from `letter_distribution_tiles`, never from an index, so it
/// answers from an instance still indexing the lexicons built on it.
async fn letter_distribution(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    state.limits.catalog_ip.check(&ip)?;
    let Some(id) = sqlx::query_scalar!("SELECT id FROM letter_distributions WHERE name = $1", name)
        .fetch_optional(&state.db)
        .await?
    else {
        return Err(ApiError::NotFound);
    };
    let tiles: Vec<TileOut> = sqlx::query_as!(
        TileOut,
        "SELECT letter, blank_letter, count, value, is_vowel FROM letter_distribution_tiles
         WHERE letter_distribution_id = $1 ORDER BY position",
        id
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "name": name, "tiles": tiles })).into_response())
}

// ---------------------------------------------------------------------------
// Admin overview
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct InstanceOut {
    instance_id: Uuid,
    heartbeat_at: DateTime<Utc>,
}

async fn admin_catalog(
    State(state): State<AppState>,
    Admin(_): Admin,
) -> ApiResult<Json<serde_json::Value>> {
    let instances: Vec<InstanceOut> = sqlx::query_as!(
        InstanceOut,
        r#"SELECT instance_id, max(heartbeat_at) AS "heartbeat_at!" FROM catalog_instance_status
           WHERE heartbeat_at > now() - interval '3 minutes'
           GROUP BY instance_id ORDER BY instance_id"#
    )
    .fetch_all(&state.db)
    .await?;
    let status = sqlx::query!(
        r#"SELECT instance_id, item_kind AS "item_kind: ItemKind", item_id FROM catalog_instance_status
           WHERE heartbeat_at > now() - interval '3 minutes'"#
    )
    .fetch_all(&state.db)
    .await?;
    let mut loaded_by: HashMap<(ItemKind, i32), Vec<Uuid>> = HashMap::new();
    for s in status {
        loaded_by
            .entry((s.item_kind, s.item_id))
            .or_default()
            .push(s.instance_id);
    }
    let live = instances.len();
    let load = |kind: ItemKind, id: i32| {
        let by = loaded_by.get(&(kind, id)).cloned().unwrap_or_default();
        let loading = live == 0 || by.len() < live;
        (by, loading)
    };
    let snap = state.catalog.snapshot();

    let dists = sqlx::query!(
        r#"SELECT d.id, d.name, d.uploaded_at, u.username::text AS "uploaded_by?",
                  (SELECT count(*) FROM letter_distribution_tiles t WHERE t.letter_distribution_id = d.id) AS "tile_count!",
                  (SELECT count(*) FROM lexicons l WHERE l.letter_distribution_id = d.id) AS "lexicon_count!"
           FROM letter_distributions d LEFT JOIN users u ON u.id = d.uploaded_by ORDER BY d.name"#
    )
    .fetch_all(&state.db)
    .await?;
    let lexicons = sqlx::query!(
        r#"SELECT l.id, l.name, d.name AS dist, l.word_count, l.uploaded_at, u.username::text AS "uploaded_by?",
                  EXISTS (SELECT 1 FROM leave_sets s WHERE s.lexicon_id = l.id) AS "has_leave_set!",
                  (SELECT count(*) FROM cascades c WHERE c.lexicon_id = l.id
                     OR EXISTS (SELECT 1 FROM search_conditions sc
                                WHERE sc.spec_id = c.spec_id AND sc.other_lexicon_id = l.id)) AS "cascade_count!"
           FROM lexicons l JOIN letter_distributions d ON d.id = l.letter_distribution_id
           LEFT JOIN users u ON u.id = l.uploaded_by ORDER BY l.name"#
    )
    .fetch_all(&state.db)
    .await?;
    let leave_sets = sqlx::query!(
        r#"SELECT s.id, l.name AS lexicon, s.leave_count, s.uploaded_at, u.username::text AS "uploaded_by?",
                  (SELECT count(*) FROM cascades c WHERE c.leave_set_id = s.id) AS "cascade_count!"
           FROM leave_sets s JOIN lexicons l ON l.id = s.lexicon_id
           LEFT JOIN users u ON u.id = s.uploaded_by ORDER BY l.name"#
    )
    .fetch_all(&state.db)
    .await?;

    let dists: Vec<_> = dists
        .into_iter()
        .map(|d| {
            let (by, loading) = load(ItemKind::LetterDistribution, i32::from(d.id));
            json!({ "id": d.id, "name": d.name, "tile_count": d.tile_count, "uploaded_by": d.uploaded_by,
                    "uploaded_at": d.uploaded_at, "lexicon_count": d.lexicon_count,
                    "loaded_by": by, "loading": loading })
        })
        .collect();
    let lexicons: Vec<_> = lexicons
        .into_iter()
        .map(|l| {
            let (by, loading) = load(ItemKind::Lexicon, i32::from(l.id));
            let idx = snap.lexicons.get(&l.id);
            json!({ "id": l.id, "name": l.name, "letter_distribution": l.dist, "word_count": l.word_count,
                    "uploaded_by": l.uploaded_by, "uploaded_at": l.uploaded_at,
                    "has_leave_set": l.has_leave_set, "cascade_count": l.cascade_count,
                    "loaded_by": by, "loading": loading,
                    "index_bytes": idx.map(|i| i.approx_bytes), "build_ms": idx.map(|i| i.build_ms) })
        })
        .collect();
    let leave_sets: Vec<_> = leave_sets
        .into_iter()
        .map(|s| {
            let (by, loading) = load(ItemKind::LeaveSet, s.id);
            let idx = snap.leave_sets.get(&s.id);
            json!({ "id": s.id, "lexicon": s.lexicon, "leave_count": s.leave_count,
                    "uploaded_by": s.uploaded_by, "uploaded_at": s.uploaded_at,
                    "cascade_count": s.cascade_count, "loaded_by": by, "loading": loading,
                    "index_bytes": idx.map(|i| i.approx_bytes), "build_ms": idx.map(|i| i.build_ms) })
        })
        .collect();
    Ok(Json(json!({
        "instance_id": state.catalog.instance_id,
        "instances": instances,
        "letter_distributions": dists,
        "lexicons": lexicons,
        "leave_sets": leave_sets,
    })))
}

// ---------------------------------------------------------------------------
// Uploads
// ---------------------------------------------------------------------------

struct Form {
    fields: HashMap<String, String>,
    file: Option<(String, Vec<u8>)>,
}

async fn read_form(mp: &mut Multipart) -> ApiResult<Form> {
    let mut form = Form {
        fields: HashMap::new(),
        file: None,
    };
    loop {
        let field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return Err(multipart_error(e)),
        };
        let name = field.name().unwrap_or_default().to_owned();
        if name == "file" {
            let filename = field.file_name().unwrap_or_default().to_owned();
            let bytes = field.bytes().await.map_err(multipart_error)?;
            form.file = Some((filename, bytes.to_vec()));
        } else {
            let text = field.text().await.map_err(multipart_error)?;
            form.fields.insert(name, text.trim().to_owned());
        }
    }
    Ok(form)
}

fn multipart_error(e: axum::extract::multipart::MultipartError) -> ApiError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::BadRequest(
            json!({ "errors": [{ "line": null, "message": "the file is larger than 100 MB" }], "total_errors": 1 }),
        )
    } else {
        ApiError::BadRequest(
            json!({ "errors": [{ "line": null, "message": e.body_text() }], "total_errors": 1 }),
        )
    }
}

fn upload_errors(e: UploadErrors) -> ApiError {
    ApiError::BadRequest(serde_json::to_value(e).expect("serialise"))
}

fn form_error(message: impl Into<String>) -> ApiError {
    upload_errors(UploadErrors::single(None, message))
}

fn trigger_reconcile(state: &AppState) {
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = super::reconcile(&st).await {
            tracing::error!(error = %e, "catalog reconcile failed");
        }
    });
}

async fn upload_distribution(
    State(state): State<AppState>,
    Admin(user): Admin,
    mut mp: Multipart,
) -> ApiResult<Response> {
    state.limits.admin_upload.check(&user.id)?;
    let form = read_form(&mut mp).await?;
    let (filename, bytes) = form.file.ok_or_else(|| form_error("choose a file"))?;
    // The name defaults to the file name without `.csv`.
    let name = match form.fields.get("name").filter(|n| !n.is_empty()) {
        Some(n) => n.clone(),
        None => filename
            .strip_suffix(".csv")
            .unwrap_or(&filename)
            .to_owned(),
    };
    if !upload::is_valid_distribution_name(&name) {
        return Err(form_error(
            "the name must be 1–32 letters, digits, spaces, underscores or hyphens",
        ));
    }
    let taken = sqlx::query_scalar!(
        "SELECT 1 AS \"one!\" FROM letter_distributions WHERE name = $1",
        name
    )
    .fetch_optional(&state.db)
    .await?;
    if taken.is_some() {
        return Err(form_error(format!(
            "a letter distribution named {name:?} already exists"
        )));
    }
    let parsed = tokio::task::spawn_blocking(move || upload::parse_distribution(&bytes))
        .await
        .map_err(|e| ApiError::Internal(e.into()))?
        .map_err(upload_errors)?;
    let mut tx = state.db.begin().await?;
    let id = match store::insert_distribution(&mut tx, &name, user.id, &parsed).await {
        Ok(id) => id,
        Err(sqlx::Error::Database(d))
            if d.constraint() == Some("letter_distributions_name_key") =>
        {
            return Err(form_error(format!(
                "a letter distribution named {name:?} already exists"
            )));
        }
        Err(e) => return Err(e.into()),
    };
    store::notify(&mut tx).await?;
    tx.commit().await?;
    trigger_reconcile(&state);
    let tiles: Vec<_> = parsed
        .tiles
        .iter()
        .map(|t| {
            json!({ "letter": t.letter, "blank_letter": t.blank_letter, "count": t.count,
                         "value": t.value, "is_vowel": t.is_vowel })
        })
        .collect();
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "name": name, "tiles": tiles })),
    )
        .into_response())
}

async fn upload_lexicon(
    State(state): State<AppState>,
    Admin(user): Admin,
    mut mp: Multipart,
) -> ApiResult<Response> {
    state.limits.admin_upload.check(&user.id)?;
    let form = read_form(&mut mp).await?;
    let (_, bytes) = form.file.ok_or_else(|| form_error("choose a file"))?;
    let name = form.fields.get("name").cloned().unwrap_or_default();
    if !upload::is_valid_lexicon_name(&name) {
        return Err(form_error(
            "the name must be 1–32 letters, digits, underscores or hyphens",
        ));
    }
    let dist_name = form
        .fields
        .get("letter_distribution")
        .cloned()
        .unwrap_or_default();
    let taken = sqlx::query_scalar!("SELECT 1 AS \"one!\" FROM lexicons WHERE name = $1", name)
        .fetch_optional(&state.db)
        .await?;
    if taken.is_some() {
        return Err(form_error(format!(
            "a lexicon named {name:?} already exists"
        )));
    }
    let Some(dist_id) = sqlx::query_scalar!(
        "SELECT id FROM letter_distributions WHERE name = $1",
        dist_name
    )
    .fetch_optional(&state.db)
    .await?
    else {
        return Err(form_error(format!(
            "there is no letter distribution named {dist_name:?}"
        )));
    };
    let dist = store::load_distribution(&state.db, dist_id)
        .await?
        .ok_or_else(|| {
            form_error(format!(
                "there is no letter distribution named {dist_name:?}"
            ))
        })?;
    let words = tokio::task::spawn_blocking(move || upload::parse_lexicon(&bytes, &dist))
        .await
        .map_err(|e| ApiError::Internal(e.into()))?
        .map_err(upload_errors)?;
    let mut tx = state.db.begin().await?;
    let id = match store::insert_lexicon(&mut tx, &name, dist_id, user.id, &words).await {
        Ok(id) => id,
        Err(sqlx::Error::Database(d)) if d.constraint() == Some("lexicons_name_key") => {
            return Err(form_error(format!(
                "a lexicon named {name:?} already exists"
            )));
        }
        Err(e) => return Err(e.into()),
    };
    store::notify(&mut tx).await?;
    tx.commit().await?;
    trigger_reconcile(&state);
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "name": name, "letter_distribution": dist_name, "word_count": words.len() })),
    )
        .into_response())
}

async fn upload_leaves(
    State(state): State<AppState>,
    Admin(user): Admin,
    mut mp: Multipart,
) -> ApiResult<Response> {
    state.limits.admin_upload.check(&user.id)?;
    let form = read_form(&mut mp).await?;
    let (_, bytes) = form.file.ok_or_else(|| form_error("choose a file"))?;
    let lexicon = form.fields.get("lexicon").cloned().unwrap_or_default();
    let Some(row) = sqlx::query!(
        "SELECT l.id, l.letter_distribution_id,
                EXISTS (SELECT 1 FROM leave_sets s WHERE s.lexicon_id = l.id) AS \"has_leaves!\"
         FROM lexicons l WHERE l.name = $1",
        lexicon
    )
    .fetch_optional(&state.db)
    .await?
    else {
        return Err(form_error(format!("there is no lexicon named {lexicon:?}")));
    };
    if row.has_leaves {
        return Err(form_error(format!(
            "{lexicon} already has leave values; delete them first to replace them"
        )));
    }
    let dist = store::load_distribution(&state.db, row.letter_distribution_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("lexicon without distribution")))?;
    let leaves = tokio::task::spawn_blocking(move || upload::parse_leaves(&bytes, &dist))
        .await
        .map_err(|e| ApiError::Internal(e.into()))?
        .map_err(upload_errors)?;
    let mut tx = state.db.begin().await?;
    let id = match store::insert_leave_set(&mut tx, row.id, user.id, &leaves).await {
        Ok(id) => id,
        Err(sqlx::Error::Database(d)) if d.constraint() == Some("leave_sets_lexicon_id_key") => {
            return Err(form_error(format!("{lexicon} already has leave values")));
        }
        Err(e) => return Err(e.into()),
    };
    store::notify(&mut tx).await?;
    tx.commit().await?;
    trigger_reconcile(&state);
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "lexicon": lexicon, "leave_count": leaves.len() })),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Deletion: refused while in use, with what still uses the item
// ---------------------------------------------------------------------------

fn in_use(details: serde_json::Value) -> ApiError {
    let mut v = json!({ "error": "in_use" });
    if let (Some(obj), Some(d)) = (v.as_object_mut(), details.as_object()) {
        obj.extend(d.clone());
    }
    ApiError::Conflict(v)
}

fn is_fk_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.code().as_deref() == Some("23503"))
}

async fn delete_distribution(
    State(state): State<AppState>,
    Admin(_): Admin,
    Path(id): Path<i16>,
) -> ApiResult<StatusCode> {
    let users: Vec<String> = sqlx::query_scalar!(
        "SELECT name FROM lexicons WHERE letter_distribution_id = $1 ORDER BY name",
        id
    )
    .fetch_all(&state.db)
    .await?;
    if !users.is_empty() {
        return Err(in_use(json!({ "lexicons": users })));
    }
    let mut tx = state.db.begin().await?;
    let r = sqlx::query!("DELETE FROM letter_distributions WHERE id = $1", id)
        .execute(&mut *tx)
        .await;
    match r {
        Ok(r) if r.rows_affected() == 0 => return Err(ApiError::NotFound),
        Ok(_) => {}
        Err(e) if is_fk_violation(&e) => return Err(in_use(json!({}))),
        Err(e) => return Err(e.into()),
    }
    store::notify(&mut tx).await?;
    tx.commit().await?;
    trigger_reconcile(&state);
    Ok(StatusCode::NO_CONTENT)
}

/// A lexicon is in use while it has leave values or any cascade refers to it,
/// as its own lexicon or through an In Lexicon row in the cascade's spec. A
/// saved search's In Lexicon row names the lexicon and pins nothing.
async fn delete_lexicon(
    State(state): State<AppState>,
    Admin(_): Admin,
    Path(id): Path<i16>,
) -> ApiResult<StatusCode> {
    let r = sqlx::query!(
        r#"SELECT EXISTS (SELECT 1 FROM leave_sets WHERE lexicon_id = $1) AS "has_leave_set!",
                  (SELECT count(*) FROM cascades WHERE lexicon_id = $1) AS "cascades!",
                  (SELECT count(DISTINCT c.id) FROM cascades c JOIN search_conditions sc ON sc.spec_id = c.spec_id
                   WHERE sc.other_lexicon_id = $1) AS "in_lexicon_cascades!""#,
        id
    )
    .fetch_one(&state.db)
    .await?;
    if r.has_leave_set || r.cascades > 0 || r.in_lexicon_cascades > 0 {
        return Err(in_use(json!({
            "leave_set": r.has_leave_set,
            "cascades": r.cascades,
            "in_lexicon_cascades": r.in_lexicon_cascades,
        })));
    }
    let mut tx = state.db.begin().await?;
    match sqlx::query!("DELETE FROM lexicons WHERE id = $1", id)
        .execute(&mut *tx)
        .await
    {
        Ok(r) if r.rows_affected() == 0 => return Err(ApiError::NotFound),
        Ok(_) => {}
        Err(e) if is_fk_violation(&e) => return Err(in_use(json!({}))),
        Err(e) => return Err(e.into()),
    }
    store::notify(&mut tx).await?;
    tx.commit().await?;
    trigger_reconcile(&state);
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_leave_set(
    State(state): State<AppState>,
    Admin(_): Admin,
    Path(id): Path<i32>,
) -> ApiResult<StatusCode> {
    let cascades = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!" FROM cascades WHERE leave_set_id = $1"#,
        id
    )
    .fetch_one(&state.db)
    .await?;
    if cascades > 0 {
        return Err(in_use(json!({ "cascades": cascades })));
    }
    let mut tx = state.db.begin().await?;
    match sqlx::query!("DELETE FROM leave_sets WHERE id = $1", id)
        .execute(&mut *tx)
        .await
    {
        Ok(r) if r.rows_affected() == 0 => return Err(ApiError::NotFound),
        Ok(_) => {}
        Err(e) if is_fk_violation(&e) => return Err(in_use(json!({}))),
        Err(e) => return Err(e.into()),
    }
    store::notify(&mut tx).await?;
    tx.commit().await?;
    trigger_reconcile(&state);
    Ok(StatusCode::NO_CONTENT)
}
