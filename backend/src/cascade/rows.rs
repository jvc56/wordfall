//! Cascade and quiz rows on the wire (PLAN.md § API → Cascades and sync).
//!
//! Every 64-bit value is the decimal text of the unsigned value, never a JSON
//! number: seeds and hashes are stored as two's-complement `i64` and converted
//! back on the way out. Sequences travel as decimal text too.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use super::order::from_i64;
use crate::search::wire::QuizType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "quiz_progression", rename_all = "snake_case")]
pub enum Progression {
    Ladder,
    Drill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "quiz_origin", rename_all = "snake_case")]
pub enum Origin {
    Source,
    ClearReplacement,
    DrillReplacement,
    Descent,
    Segment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "quiz_status", rename_all = "snake_case")]
pub enum QuizStatus {
    Active,
    Cleared,
}

/// Serialises a u64 stored as i64 as its unsigned decimal text.
pub mod u64_text {
    use serde::Serializer;

    pub fn serialize<S: Serializer>(v: &i64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::from_i64(*v).to_string())
    }
}

/// Serialises a sequence as decimal text.
pub mod seq_text {
    use serde::Serializer;

    pub fn serialize<S: Serializer>(v: &i64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CascadeRow {
    pub id: Uuid,
    pub name: String,
    pub quiz_type: QuizType,
    pub lexicon: String,
    pub letter_distribution: String,
    pub clear_threshold: i16,
    pub segment_size: i32,
    pub progression: Progression,
    pub require_alphabetical: bool,
    pub options_changed_at: DateTime<Utc>,
    #[serde(with = "seq_text")]
    pub options_seq: i64,
    pub options_device_id: Uuid,
    pub question_count: i32,
    pub depth: i32,
    pub peak_depth: i32,
    pub attempts_since_completion: i32,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub trashed_at: Option<DateTime<Utc>>,
    #[serde(with = "seq_text")]
    pub updated_seq: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct QuizRow {
    pub id: Uuid,
    pub cascade_id: Uuid,
    pub level: i32,
    pub origin: Origin,
    pub origin_quiz_id: Option<Uuid>,
    pub origin_attempt: Option<i32>,
    pub origin_segment_end: Option<i32>,
    pub status: QuizStatus,
    pub segment_chain: bool,
    pub segment_size: i32,
    pub progression: Progression,
    pub require_alphabetical: bool,
    pub options_changed_at: DateTime<Utc>,
    #[serde(with = "seq_text")]
    pub options_seq: i64,
    pub options_device_id: Uuid,
    pub attempt: i32,
    #[serde(with = "u64_text")]
    pub shuffle_seed: i64,
    #[serde(with = "u64_text")]
    pub questions_hash: i64,
    pub question_count: i32,
    pub correct_count: i32,
    pub missed_count: i32,
    pub cursor: i32,
    pub cursor_moved_at: Option<DateTime<Utc>>,
    pub cursor_device_id: Option<Uuid>,
    pub run_start: i32,
    pub created_at: DateTime<Utc>,
    #[serde(with = "seq_text")]
    pub created_seq: i64,
    pub last_activity_at: DateTime<Utc>,
    pub cleared_at: Option<DateTime<Utc>>,
    #[serde(with = "seq_text")]
    pub updated_seq: i64,
}

pub const CASCADE_SELECT: &str = r#"
    SELECT c.id, c.name, c.quiz_type, l.name AS lexicon, d.name AS letter_distribution,
           c.clear_threshold, c.segment_size, c.progression, c.require_alphabetical,
           c.options_changed_at, c.options_seq, c.options_device_id, c.question_count, c.depth,
           c.peak_depth, c.attempts_since_completion, c.created_at, c.last_activity_at,
           c.completed_at, c.trashed_at, c.updated_seq
    FROM cascades c
    JOIN lexicons l ON l.id = c.lexicon_id
    JOIN letter_distributions d ON d.id = l.letter_distribution_id"#;

pub const QUIZ_SELECT: &str = r#"
    SELECT q.id, q.cascade_id, q.level, q.origin, q.origin_quiz_id, q.origin_attempt,
           q.origin_segment_end, q.status, q.segment_chain, q.segment_size, q.progression,
           q.require_alphabetical, q.options_changed_at, q.options_seq, q.options_device_id,
           q.attempt, q.shuffle_seed, q.questions_hash, q.question_count, q.correct_count,
           q.missed_count, q.cursor, q.cursor_moved_at, q.cursor_device_id, q.run_start,
           q.created_at, q.created_seq, q.last_activity_at, q.cleared_at, q.updated_seq
    FROM quizzes q"#;

pub async fn cascade(conn: &mut PgConnection, user_id: Uuid, id: Uuid) -> Result<Option<CascadeRow>, sqlx::Error> {
    sqlx::query_as::<_, CascadeRow>(sqlx::AssertSqlSafe(format!("{CASCADE_SELECT} WHERE c.id = $1 AND c.user_id = $2")))
        .bind(id)
        .bind(user_id)
        .fetch_optional(conn)
        .await
}

pub async fn source_quiz(conn: &mut PgConnection, cascade_id: Uuid) -> Result<Option<QuizRow>, sqlx::Error> {
    sqlx::query_as::<_, QuizRow>(sqlx::AssertSqlSafe(format!(
        "{QUIZ_SELECT} WHERE q.cascade_id = $1 AND q.origin = 'source'"
    )))
    .bind(cascade_id)
    .fetch_optional(conn)
    .await
}

pub fn seed_text(v: i64) -> String {
    from_i64(v).to_string()
}
