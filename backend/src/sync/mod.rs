//! `POST /api/sync` (PLAN.md § Offline and Sync → The sync cycle, § API →
//! Cascades and sync): a push, then a pull, in one request.

pub mod ops;
pub mod prefs;
pub mod pull;
pub mod routes;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::cascade::rules::Reason;

/// Operation kinds whose records carry result fields and are kept for
/// `SYNC_RETENTION_DAYS`; every other record goes once acknowledged.
pub const RESULT_BEARING: [&str; 3] = ["finish", "finish_segment", "restore_quiz"];

/// Rows in one pull page, of any kind.
pub const PAGE_ROWS: i64 = 50_000;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct OpResult {
    pub op_id: Uuid,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_quiz_question_count: Option<i32>,
    /// The unsigned decimal text of the hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_quiz_questions_hash: Option<String>,
}

impl OpResult {
    pub fn applied(op_id: Uuid) -> Self {
        OpResult { op_id, status: "applied", ..Default::default() }
    }

    pub fn rejected(op_id: Uuid, reason: Reason) -> Self {
        OpResult { op_id, status: "rejected", reason: Some(reason.as_str().to_owned()), ..Default::default() }
    }
}

/// What applying an operation produced, for its result and its record.
#[derive(Debug, Clone, Default)]
pub struct Effects {
    pub outcome: Option<String>,
    pub new_quiz_question_count: Option<i32>,
    pub new_quiz_questions_hash: Option<u64>,
}

/// An operation as sent, with the fields every operation carries.
#[derive(Debug, Clone)]
pub struct IncomingOp {
    pub id: Uuid,
    pub device_seq: i64,
    pub seen_seq: i64,
    pub at: Option<DateTime<Utc>>,
    pub op_type: String,
    pub body: Value,
}

pub const OP_TYPES: [&str; 13] = [
    "grade",
    "move_cursor",
    "finish",
    "finish_segment",
    "restore_quiz",
    "trash_cascade",
    "restore_cascade",
    "purge_quiz",
    "purge_cascade",
    "set_preferences",
    "set_bindings",
    "set_cascade_options",
    "set_quiz_options",
];

/// Whether a database error is transient: the whole request is answered
/// `503` and nothing recorded, so the device resends the same batch.
pub fn is_transient(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::Database(d) => matches!(
            d.code().as_deref(),
            // serialization_failure, deadlock_detected, lock_not_available,
            // query_canceled (statement timeout), admin shutdown / connection failures
            Some("40001") | Some("40P01") | Some("55P03") | Some("57014") | Some("57P01") | Some("08006") | Some("08003")
        ),
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::WorkerCrashed => true,
        _ => false,
    }
}
