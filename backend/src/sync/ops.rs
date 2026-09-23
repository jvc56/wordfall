//! Applying one operation to the database (PLAN.md § Operations,
//! § Conflicts), through the shared rule functions in `cascade::rules`.
//! Every row an operation changes is stamped with the request's sequence.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use super::prefs::{self, InputAction, InputKind};
use super::{Effects, IncomingOp};
use crate::cascade::order::{from_i64, questions_hash, shuffle, to_i64};
use crate::cascade::rows::{Origin, Progression, QuizStatus};
use crate::cascade::rules::{self, CascadeState, FinishResult, Grade, NewQuiz, Opts, QuizState, Reason, SegmentResult};

pub enum OpError {
    Rejected(Reason),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for OpError {
    fn from(e: sqlx::Error) -> Self {
        OpError::Db(e)
    }
}

impl From<Reason> for OpError {
    fn from(r: Reason) -> Self {
        OpError::Rejected(r)
    }
}

type OpResult = Result<Effects, OpError>;

pub struct Ctx {
    pub user: Uuid,
    pub device: Uuid,
    pub seq: i64,
    pub cap: u32,
}

// ---------------------------------------------------------------------------
// Parsing an operation's own fields; a malformed one is `invalid`
// ---------------------------------------------------------------------------

fn field<'a>(op: &'a IncomingOp, k: &str) -> Result<&'a Value, OpError> {
    op.body.get(k).ok_or(OpError::Rejected(Reason::Invalid))
}

fn uuid_of(op: &IncomingOp, k: &str) -> Result<Uuid, OpError> {
    field(op, k)?.as_str().and_then(|s| s.parse().ok()).ok_or(OpError::Rejected(Reason::Invalid))
}

fn u32_of(op: &IncomingOp, k: &str) -> Result<u32, OpError> {
    field(op, k)?.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or(OpError::Rejected(Reason::Invalid))
}

/// Seeds travel as decimal text of the unsigned value.
fn seed_of(op: &IncomingOp, k: &str) -> Result<u64, OpError> {
    field(op, k)?.as_str().and_then(|s| s.parse().ok()).ok_or(OpError::Rejected(Reason::Invalid))
}

fn at_of(op: &IncomingOp) -> Result<DateTime<Utc>, OpError> {
    op.at.ok_or(OpError::Rejected(Reason::Invalid))
}

// ---------------------------------------------------------------------------
// Loading state
// ---------------------------------------------------------------------------

pub struct Loaded {
    pub quiz_id: Uuid,
    pub cascade_id: Uuid,
    pub quiz: QuizState,
    pub cascade: CascadeState,
    pub correct: i32,
    pub missed: i32,
    pub updated_seq: i64,
    pub cursor_device_id: Option<Uuid>,
    pub cursor_moved_at: Option<DateTime<Utc>>,
    pub options_seq: i64,
    pub options_device_id: Uuid,
    pub options_changed_at: DateTime<Utc>,
}

pub async fn load_quiz(conn: &mut PgConnection, user: Uuid, quiz_id: Uuid) -> Result<Loaded, OpError> {
    let r = sqlx::query!(
        r#"SELECT q.cascade_id, q.level, q.origin AS "origin: Origin", q.segment_chain, q.segment_size,
                  q.progression AS "progression: Progression", q.require_alphabetical, q.attempt, q.shuffle_seed,
                  q.question_count, q.correct_count, q.missed_count, q.cursor, q.cursor_device_id,
                  q.cursor_moved_at, q.run_start, q.status AS "status: QuizStatus", q.updated_seq, q.options_seq,
                  q.options_device_id, q.options_changed_at,
                  c.clear_threshold, c.segment_size AS c_segment_size, c.progression AS "c_progression: Progression",
                  c.require_alphabetical AS c_require_alphabetical, c.depth, c.peak_depth,
                  c.attempts_since_completion, c.trashed_at IS NOT NULL AS "trashed!"
           FROM quizzes q JOIN cascades c ON c.id = q.cascade_id
           WHERE q.id = $1 AND q.user_id = $2"#,
        quiz_id,
        user
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(OpError::Rejected(Reason::NotFound))?;
    Ok(Loaded {
        quiz_id,
        cascade_id: r.cascade_id,
        quiz: QuizState {
            level: r.level as u32,
            origin: r.origin,
            segment_chain: r.segment_chain,
            opts: Opts { segment_size: r.segment_size as u32, progression: r.progression, require_alphabetical: r.require_alphabetical },
            attempt: r.attempt as u32,
            seed: from_i64(r.shuffle_seed),
            question_count: r.question_count as u32,
            cursor: r.cursor as u32,
            run_start: r.run_start as u32,
            active: r.status == QuizStatus::Active,
        },
        cascade: CascadeState {
            clear_threshold: r.clear_threshold as u32,
            opts: Opts {
                segment_size: r.c_segment_size as u32,
                progression: r.c_progression,
                require_alphabetical: r.c_require_alphabetical,
            },
            depth: r.depth as u32,
            peak_depth: r.peak_depth as u32,
            attempts_since_completion: r.attempts_since_completion as u32,
            trashed: r.trashed,
        },
        correct: r.correct_count,
        missed: r.missed_count,
        updated_seq: r.updated_seq,
        cursor_device_id: r.cursor_device_id,
        cursor_moved_at: r.cursor_moved_at,
        options_seq: r.options_seq,
        options_device_id: r.options_device_id,
        options_changed_at: r.options_changed_at,
    })
}

/// The clamped device time: `least(at, now() + 5 minutes)`.
async fn clamp(conn: &mut PgConnection, at: DateTime<Utc>) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar!(r#"SELECT LEAST($1::timestamptz, now() + interval '5 minutes') AS "t!""#, at)
        .fetch_one(conn)
        .await
}

async fn write_cascade(
    conn: &mut PgConnection,
    ctx: &Ctx,
    id: Uuid,
    c: &CascadeState,
    activity: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE cascades SET depth = $2, peak_depth = $3, attempts_since_completion = $4,
                             trashed_at = CASE WHEN $5 THEN trashed_at ELSE NULL END,
                             last_activity_at = CASE WHEN $6::timestamptz IS NULL THEN last_activity_at
                                                     ELSE greatest(last_activity_at, $6) END,
                             updated_seq = $7
         WHERE id = $1",
        id,
        c.depth as i32,
        c.peak_depth as i32,
        c.attempts_since_completion as i32,
        c.trashed,
        activity,
        ctx.seq,
    )
    .execute(conn)
    .await?;
    Ok(())
}

async fn quiz_questions(conn: &mut PgConnection, quiz: Uuid) -> Result<Vec<u32>, sqlx::Error> {
    let v: Vec<i32> = sqlx::query_scalar!("SELECT question_idx FROM quiz_questions WHERE quiz_id = $1 ORDER BY question_idx", quiz)
        .fetch_all(conn)
        .await?;
    Ok(v.into_iter().map(|i| i as u32).collect())
}

/// A new attempt in place: positions rewritten from the seed in one UPDATE …
/// FROM UNNEST, grades cleared. Only the quiz row is stamped; other devices
/// rebuild the order from the seed.
async fn reset_quiz(conn: &mut PgConnection, ctx: &Ctx, quiz: Uuid, attempt: u32, seed: u64, at: DateTime<Utc>) -> Result<Vec<u32>, sqlx::Error> {
    let questions = quiz_questions(conn, quiz).await?;
    let order = shuffle(&questions, seed);
    let idx: Vec<i32> = order.iter().map(|&i| i as i32).collect();
    let pos: Vec<i32> = (0..order.len() as i32).collect();
    sqlx::query!(
        "UPDATE quiz_questions q SET position = u.pos, grade = NULL, graded_at = NULL, graded_by_device_id = NULL
         FROM UNNEST($2::int4[], $3::int4[]) AS u(idx, pos)
         WHERE q.quiz_id = $1 AND q.question_idx = u.idx",
        quiz,
        &idx,
        &pos,
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "UPDATE quizzes SET attempt = $2, shuffle_seed = $3, correct_count = 0, missed_count = 0, cursor = 0,
                            cursor_moved_at = $4, cursor_device_id = $5, run_start = 0, updated_seq = $6
         WHERE id = $1",
        quiz,
        attempt as i32,
        to_i64(seed),
        at,
        ctx.device,
        ctx.seq,
    )
    .execute(conn)
    .await?;
    Ok(questions)
}

/// Inserts a quiz a rule created, with the device's id, its questions in
/// the seed's order, cursor 0 stamped from the operation, and options
/// stamped with the operation's device, sequence and time.
async fn insert_quiz(
    conn: &mut PgConnection,
    ctx: &Ctx,
    id: Uuid,
    cascade: Uuid,
    parent: Uuid,
    n: &NewQuiz,
    at: DateTime<Utc>,
) -> Result<(i32, u64), sqlx::Error> {
    let count = n.questions.len() as i32;
    let hash = questions_hash(&n.questions);
    sqlx::query!(
        "INSERT INTO quizzes (id, cascade_id, user_id, level, origin, origin_quiz_id, origin_attempt,
                              origin_segment_end, segment_chain, segment_size, progression, require_alphabetical,
                              options_changed_at, options_seq, options_device_id, shuffle_seed, questions_hash,
                              question_count, cursor_moved_at, cursor_device_id, created_seq, last_activity_at,
                              updated_seq)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $13, $15, $14,
                 greatest(now(), $13), $14)",
        id,
        cascade,
        ctx.user,
        n.level as i32,
        n.origin as Origin,
        parent,
        n.origin_attempt as i32,
        n.origin_segment_end.map(|e| e as i32),
        n.segment_chain,
        n.opts.segment_size as i32,
        n.opts.progression as Progression,
        n.opts.require_alphabetical,
        at,
        ctx.seq,
        ctx.device,
        to_i64(n.seed),
        to_i64(hash),
        count,
    )
    .execute(&mut *conn)
    .await?;
    let order = shuffle(&n.questions, n.seed);
    let idx: Vec<i32> = order.iter().map(|&i| i as i32).collect();
    let pos: Vec<i32> = (0..count).collect();
    sqlx::query!(
        "INSERT INTO quiz_questions (quiz_id, question_idx, position, updated_seq)
         SELECT $1, u.idx, u.pos, $4 FROM UNNEST($2::int4[], $3::int4[]) AS u(idx, pos)",
        id,
        &idx,
        &pos,
        ctx.seq,
    )
    .execute(conn)
    .await?;
    Ok((count, hash))
}

async fn id_free(conn: &mut PgConnection, user: Uuid, id: Uuid) -> Result<(), OpError> {
    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM quizzes WHERE id = $1)
               OR EXISTS (SELECT 1 FROM sync_tombstones WHERE user_id = $2 AND entity = 'quiz' AND entity_id = $1)
           AS "taken!""#,
        id,
        user
    )
    .fetch_one(conn)
    .await?;
    if taken {
        return Err(OpError::Rejected(Reason::Invalid));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The operations
// ---------------------------------------------------------------------------

pub async fn apply(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    match op.op_type.as_str() {
        "grade" => grade(conn, ctx, op).await,
        "move_cursor" => move_cursor(conn, ctx, op).await,
        "finish" => finish(conn, ctx, op).await,
        "finish_segment" => finish_segment(conn, ctx, op).await,
        "restore_quiz" => restore_quiz(conn, ctx, op).await,
        "trash_cascade" => trash_cascade(conn, ctx, op).await,
        "restore_cascade" => restore_cascade(conn, ctx, op).await,
        "purge_quiz" => purge_quiz(conn, ctx, op).await,
        "purge_cascade" => purge_cascade(conn, ctx, op).await,
        "set_cascade_options" => set_cascade_options(conn, ctx, op).await,
        "set_quiz_options" => set_quiz_options(conn, ctx, op).await,
        "set_preferences" => set_preferences(conn, ctx, op).await,
        "set_bindings" => set_bindings(conn, ctx, op).await,
        _ => Err(OpError::Rejected(Reason::Invalid)),
    }
}

async fn grade(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let quiz = uuid_of(op, "quiz_id")?;
    let attempt = u32_of(op, "attempt")?;
    let attempt_seed = seed_of(op, "attempt_seed")?;
    let idx = u32_of(op, "question_idx")?;
    let g: Grade = serde_json::from_value(field(op, "grade")?.clone()).map_err(|_| OpError::Rejected(Reason::Invalid))?;
    let at = at_of(op)?;
    let l = load_quiz(conn, ctx.user, quiz).await?;
    rules::check_live(&l.cascade, &l.quiz)?;
    rules::check_attempt(&l.quiz, attempt, attempt_seed)?;
    let row = sqlx::query!(
        r#"SELECT grade AS "grade: Grade", graded_at, graded_by_device_id, updated_seq
           FROM quiz_questions WHERE quiz_id = $1 AND question_idx = $2"#,
        quiz,
        idx as i32
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(OpError::Rejected(Reason::NotFound))?;
    let at = clamp(conn, at).await?;
    // The question is ungraded, or its grade is this device's, or the device
    // had seen it, or this change is later.
    let wins = row.grade.is_none()
        || row.graded_by_device_id == Some(ctx.device)
        || row.updated_seq <= op.seen_seq
        || row.graded_at.is_none_or(|t| at > t);
    if !wins {
        return Err(OpError::Rejected(Reason::Stale));
    }
    let delta = |g: Option<Grade>, want: Grade| i32::from(g == Some(want));
    let dc = delta(Some(g), Grade::Correct) - delta(row.grade, Grade::Correct);
    let dm = delta(Some(g), Grade::Missed) - delta(row.grade, Grade::Missed);
    sqlx::query!(
        "UPDATE quiz_questions SET grade = $3, graded_at = $4, graded_by_device_id = $5, updated_seq = $6
         WHERE quiz_id = $1 AND question_idx = $2",
        quiz,
        idx as i32,
        g as Grade,
        at,
        ctx.device,
        ctx.seq,
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "UPDATE quizzes SET correct_count = correct_count + $2, missed_count = missed_count + $3,
                            last_activity_at = greatest(last_activity_at, $4), updated_seq = $5
         WHERE id = $1",
        quiz,
        dc,
        dm,
        at,
        ctx.seq,
    )
    .execute(&mut *conn)
    .await?;
    write_cascade(conn, ctx, l.cascade_id, &l.cascade, Some(at)).await?;
    Ok(Effects::default())
}

async fn move_cursor(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let quiz = uuid_of(op, "quiz_id")?;
    let attempt = u32_of(op, "attempt")?;
    let attempt_seed = seed_of(op, "attempt_seed")?;
    let position = u32_of(op, "position")?;
    let at = at_of(op)?;
    let l = load_quiz(conn, ctx.user, quiz).await?;
    rules::check_live(&l.cascade, &l.quiz)?;
    rules::check_attempt(&l.quiz, attempt, attempt_seed)?;
    rules::check_move_cursor(&l.quiz, position)?;
    let at = clamp(conn, at).await?;
    let wins = l.cursor_moved_at.is_none()
        || l.cursor_device_id == Some(ctx.device)
        || l.updated_seq <= op.seen_seq
        || l.cursor_moved_at.is_none_or(|t| at > t);
    if !wins {
        return Err(OpError::Rejected(Reason::Stale));
    }
    sqlx::query!(
        "UPDATE quizzes SET cursor = $2, cursor_moved_at = $3, cursor_device_id = $4, updated_seq = $5 WHERE id = $1",
        quiz,
        position as i32,
        at,
        ctx.device,
        ctx.seq,
    )
    .execute(conn)
    .await?;
    Ok(Effects::default())
}

async fn record_attempt(
    conn: &mut PgConnection,
    ctx: &Ctx,
    l: &Loaded,
    outcome: rules::Outcome,
    at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO quiz_attempts (quiz_id, user_id, attempt, question_count, correct_count, missed_count, outcome,
                                    shuffle_seed, finished_at, updated_seq)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        l.quiz_id,
        ctx.user,
        l.quiz.attempt as i32,
        l.quiz.question_count as i32,
        l.correct,
        l.missed,
        outcome as rules::Outcome,
        to_i64(l.quiz.seed),
        at,
        ctx.seq,
    )
    .execute(conn)
    .await?;
    Ok(())
}

async fn finish(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let quiz = uuid_of(op, "quiz_id")?;
    let attempt = u32_of(op, "attempt")?;
    let attempt_seed = seed_of(op, "attempt_seed")?;
    let seed = seed_of(op, "shuffle_seed")?;
    let new_id = uuid_of(op, "new_quiz_id")?;
    let at = at_of(op)?;
    let mut l = load_quiz(conn, ctx.user, quiz).await?;
    rules::check_live(&l.cascade, &l.quiz)?;
    // PQ-013: the attempt before the depth.
    rules::check_attempt(&l.quiz, attempt, attempt_seed)?;
    rules::check_deepest(&l.cascade, &l.quiz)?;
    if (l.correct + l.missed) as u32 != l.quiz.question_count {
        return Err(OpError::Rejected(Reason::Ungraded));
    }
    id_free(conn, ctx.user, new_id).await?;
    let at = clamp(conn, at).await?;
    let misses: Vec<i32> = sqlx::query_scalar!(
        "SELECT question_idx FROM quiz_questions WHERE quiz_id = $1 AND grade = 'missed' ORDER BY question_idx",
        quiz
    )
    .fetch_all(&mut *conn)
    .await?;
    let misses: Vec<u32> = misses.into_iter().map(|i| i as u32).collect();
    // The server's grades decide, and the quiz's own progression.
    let r = rules::finish(&mut l.cascade, &l.quiz, l.correct as u32, misses, seed);
    let outcome = r.outcome();
    record_attempt(conn, ctx, &l, outcome, at).await?;
    let mut fx = Effects { outcome: Some(outcome.as_str().to_owned()), ..Default::default() };
    match &r {
        FinishResult::Finished { replacement, .. } => {
            // Cleared before a replacement is inserted at the same level: the
            // one-active-per-level index is checked per statement.
            sqlx::query!(
                "UPDATE quizzes SET status = 'cleared', cleared_at = now(), last_activity_at = greatest(last_activity_at, $2),
                                    updated_seq = $3 WHERE id = $1",
                quiz,
                at,
                ctx.seq
            )
            .execute(&mut *conn)
            .await?;
            if let Some(n) = replacement {
                let (c, h) = insert_quiz(conn, ctx, new_id, l.cascade_id, quiz, n, at).await?;
                fx.new_quiz_question_count = Some(c);
                fx.new_quiz_questions_hash = Some(h);
            }
        }
        FinishResult::Descended { reset_seed, new_level } => {
            let qs = reset_quiz(conn, ctx, quiz, l.quiz.attempt + 1, *reset_seed, at).await?;
            let _ = qs;
            let (c, h) = insert_quiz(conn, ctx, new_id, l.cascade_id, quiz, new_level, at).await?;
            fx.new_quiz_question_count = Some(c);
            fx.new_quiz_questions_hash = Some(h);
        }
        FinishResult::Completed { reset_seed, .. } | FinishResult::Reshuffled { reset_seed } => {
            let qs = reset_quiz(conn, ctx, quiz, l.quiz.attempt + 1, *reset_seed, at).await?;
            fx.new_quiz_question_count = Some(qs.len() as i32);
            fx.new_quiz_questions_hash = Some(questions_hash(&qs));
        }
    }
    if !matches!(r, FinishResult::Finished { .. }) {
        sqlx::query!("UPDATE quizzes SET last_activity_at = greatest(last_activity_at, $2) WHERE id = $1", quiz, at)
            .execute(&mut *conn)
            .await?;
    }
    write_cascade(conn, ctx, l.cascade_id, &l.cascade, Some(at)).await?;
    if matches!(r, FinishResult::Completed { .. }) {
        sqlx::query!("UPDATE cascades SET completed_at = now() WHERE id = $1", l.cascade_id).execute(&mut *conn).await?;
    }
    Ok(fx)
}

async fn finish_segment(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let quiz = uuid_of(op, "quiz_id")?;
    let attempt = u32_of(op, "attempt")?;
    let attempt_seed = seed_of(op, "attempt_seed")?;
    let end = u32_of(op, "segment_end")?;
    let seed = seed_of(op, "shuffle_seed")?;
    let new_id = uuid_of(op, "new_quiz_id")?;
    let at = at_of(op)?;
    let mut l = load_quiz(conn, ctx.user, quiz).await?;
    rules::check_live(&l.cascade, &l.quiz)?;
    // PQ-013: the attempt and the duplicate before the depth.
    rules::check_attempt(&l.quiz, attempt, attempt_seed)?;
    rules::check_segment_end(&l.quiz, end)?;
    let duplicate = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM quizzes WHERE origin = 'segment' AND origin_quiz_id = $1
                          AND origin_attempt = $2 AND origin_segment_end = $3) AS "d!""#,
        quiz,
        l.quiz.attempt as i32,
        end as i32
    )
    .fetch_one(&mut *conn)
    .await?;
    if duplicate {
        return Err(OpError::Rejected(Reason::DuplicateSegment));
    }
    rules::check_deepest(&l.cascade, &l.quiz)?;
    rules::check_segment_past_cursor(&l.quiz, end)?;
    let ungraded = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!" FROM quiz_questions WHERE quiz_id = $1 AND position < $2 AND grade IS NULL"#,
        quiz,
        end as i32
    )
    .fetch_one(&mut *conn)
    .await?;
    if ungraded > 0 {
        return Err(OpError::Rejected(Reason::Ungraded));
    }
    id_free(conn, ctx.user, new_id).await?;
    let at = clamp(conn, at).await?;
    let run_misses: Vec<i32> = sqlx::query_scalar!(
        "SELECT question_idx FROM quiz_questions WHERE quiz_id = $1 AND position >= $2 AND position < $3
           AND grade = 'missed' ORDER BY question_idx",
        quiz,
        l.quiz.run_start as i32,
        end as i32
    )
    .fetch_all(&mut *conn)
    .await?;
    let r = rules::finish_segment(&mut l.cascade, &l.quiz, run_misses.into_iter().map(|i| i as u32).collect(), end, seed);
    let mut fx = Effects::default();
    match &r {
        SegmentResult::Drilled(n) => {
            let (c, h) = insert_quiz(conn, ctx, new_id, l.cascade_id, quiz, n, at).await?;
            fx.outcome = Some("drilled".into());
            fx.new_quiz_question_count = Some(c);
            fx.new_quiz_questions_hash = Some(h);
        }
        SegmentResult::Continued => fx.outcome = Some("continued".into()),
    }
    sqlx::query!(
        "UPDATE quizzes SET cursor = $2, run_start = $2, cursor_moved_at = $3, cursor_device_id = $4,
                            last_activity_at = greatest(last_activity_at, $3), updated_seq = $5 WHERE id = $1",
        quiz,
        end as i32,
        at,
        ctx.device,
        ctx.seq
    )
    .execute(&mut *conn)
    .await?;
    write_cascade(conn, ctx, l.cascade_id, &l.cascade, Some(at)).await?;
    Ok(fx)
}

async fn restore_quiz(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let quiz = uuid_of(op, "quiz_id")?;
    let seed = seed_of(op, "shuffle_seed")?;
    let at = at_of(op)?;
    let mut l = load_quiz(conn, ctx.user, quiz).await?;
    if l.quiz.active {
        return Err(OpError::Rejected(Reason::NotCleared));
    }
    let at = clamp(conn, at).await?;
    let r = rules::restore_quiz(&mut l.cascade, &l.quiz, seed);
    let qs = reset_quiz(conn, ctx, quiz, r.attempt, r.seed, at).await?;
    sqlx::query!(
        "UPDATE quizzes SET status = 'active', cleared_at = NULL, level = $2, segment_size = $3, progression = $4,
                            require_alphabetical = $5, options_changed_at = $6, options_seq = $7,
                            options_device_id = $8, last_activity_at = greatest(last_activity_at, $6)
         WHERE id = $1",
        quiz,
        r.level as i32,
        r.opts.segment_size as i32,
        r.opts.progression as Progression,
        r.opts.require_alphabetical,
        at,
        ctx.seq,
        ctx.device,
    )
    .execute(&mut *conn)
    .await?;
    write_cascade(conn, ctx, l.cascade_id, &l.cascade, Some(at)).await?;
    if r.cascade_restored {
        // Brought back with its quiz, and only then its cleared quizzes' clocks restart.
        restart_clocks(conn, ctx, l.cascade_id).await?;
    }
    Ok(Effects {
        outcome: None,
        new_quiz_question_count: Some(qs.len() as i32),
        new_quiz_questions_hash: Some(questions_hash(&qs)),
    })
}

/// Sets `cleared_at` to now on a cascade's cleared quizzes, so each gets a full
/// retention period after the cascade comes back from the Trash.
async fn restart_clocks(conn: &mut PgConnection, ctx: &Ctx, cascade: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE quizzes SET cleared_at = now(), updated_seq = $2 WHERE cascade_id = $1 AND status = 'cleared'",
        cascade,
        ctx.seq
    )
    .execute(conn)
    .await?;
    Ok(())
}

struct CascadeRef {
    trashed: bool,
    options_seq: i64,
    options_device_id: Uuid,
    options_changed_at: DateTime<Utc>,
}

async fn load_cascade(conn: &mut PgConnection, user: Uuid, id: Uuid) -> Result<CascadeRef, OpError> {
    let r = sqlx::query!(
        r#"SELECT trashed_at IS NOT NULL AS "trashed!", options_seq, options_device_id, options_changed_at
           FROM cascades WHERE id = $1 AND user_id = $2"#,
        id,
        user
    )
    .fetch_optional(conn)
    .await?
    .ok_or(OpError::Rejected(Reason::NotFound))?;
    Ok(CascadeRef {
        trashed: r.trashed,
        options_seq: r.options_seq,
        options_device_id: r.options_device_id,
        options_changed_at: r.options_changed_at,
    })
}

async fn trash_cascade(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let id = uuid_of(op, "cascade_id")?;
    let c = load_cascade(conn, ctx.user, id).await?;
    if c.trashed {
        return Err(OpError::Rejected(Reason::Trashed));
    }
    sqlx::query!("UPDATE cascades SET trashed_at = now(), updated_seq = $2 WHERE id = $1", id, ctx.seq)
        .execute(conn)
        .await?;
    Ok(Effects::default())
}

async fn restore_cascade(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let id = uuid_of(op, "cascade_id")?;
    let at = at_of(op)?;
    let c = load_cascade(conn, ctx.user, id).await?;
    if !c.trashed {
        return Err(OpError::Rejected(Reason::NotTrashed));
    }
    let at = clamp(conn, at).await?;
    sqlx::query!(
        "UPDATE cascades SET trashed_at = NULL, last_activity_at = greatest(last_activity_at, $2), updated_seq = $3
         WHERE id = $1",
        id,
        at,
        ctx.seq
    )
    .execute(&mut *conn)
    .await?;
    restart_clocks(conn, ctx, id).await?;
    Ok(Effects::default())
}

/// Writes a tombstone for a purged cascade or quiz, so other devices remove it.
pub async fn tombstone(conn: &mut PgConnection, user: Uuid, entity: &str, id: Uuid, seq: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO sync_tombstones (user_id, entity, entity_id, seq) VALUES ($1, $2::text::sync_entity, $3, $4)
         ON CONFLICT (user_id, entity, entity_id) DO UPDATE SET seq = EXCLUDED.seq, deleted_at = now()",
        user,
        entity,
        id,
        seq
    )
    .execute(conn)
    .await?;
    Ok(())
}

async fn purge_quiz(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let quiz = uuid_of(op, "quiz_id")?;
    let l = load_quiz(conn, ctx.user, quiz).await?;
    // A trashed cascade is purged as one unit.
    if l.cascade.trashed {
        return Err(OpError::Rejected(Reason::Trashed));
    }
    if l.quiz.active {
        return Err(OpError::Rejected(Reason::NotCleared));
    }
    sqlx::query!("DELETE FROM quizzes WHERE id = $1", quiz).execute(&mut *conn).await?;
    tombstone(conn, ctx.user, "quiz", quiz, ctx.seq).await?;
    Ok(Effects::default())
}

/// Deletes a cascade by id, table by table, with its spec, and writes
/// tombstones for it and each of its quizzes. Shared with the purge task.
pub async fn purge_cascade_rows(conn: &mut PgConnection, user: Uuid, id: Uuid, seq: i64) -> Result<u64, sqlx::Error> {
    let quizzes: Vec<Uuid> = sqlx::query_scalar!("SELECT id FROM quizzes WHERE cascade_id = $1", id)
        .fetch_all(&mut *conn)
        .await?;
    sqlx::query!("DELETE FROM quiz_attempts WHERE quiz_id = ANY($1)", &quizzes).execute(&mut *conn).await?;
    sqlx::query!("DELETE FROM quiz_questions WHERE quiz_id = ANY($1)", &quizzes).execute(&mut *conn).await?;
    sqlx::query!("UPDATE quizzes SET origin_quiz_id = NULL WHERE cascade_id = $1", id).execute(&mut *conn).await?;
    sqlx::query!("DELETE FROM quizzes WHERE cascade_id = $1", id).execute(&mut *conn).await?;
    sqlx::query!("DELETE FROM cascade_questions WHERE cascade_id = $1", id).execute(&mut *conn).await?;
    let spec = sqlx::query_scalar!("DELETE FROM cascades WHERE id = $1 RETURNING spec_id", id)
        .fetch_one(&mut *conn)
        .await?;
    sqlx::query!("DELETE FROM search_specs WHERE id = $1", spec).execute(&mut *conn).await?;
    sqlx::query!(
        "INSERT INTO sync_tombstones (user_id, entity, entity_id, seq)
         SELECT $1, 'quiz', q, $3 FROM UNNEST($2::uuid[]) AS q
         ON CONFLICT (user_id, entity, entity_id) DO UPDATE SET seq = EXCLUDED.seq, deleted_at = now()",
        user,
        &quizzes,
        seq
    )
    .execute(&mut *conn)
    .await?;
    tombstone(conn, user, "cascade", id, seq).await?;
    Ok(quizzes.len() as u64)
}

async fn purge_cascade(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let id = uuid_of(op, "cascade_id")?;
    let c = load_cascade(conn, ctx.user, id).await?;
    if !c.trashed {
        return Err(OpError::Rejected(Reason::NotTrashed));
    }
    purge_cascade_rows(conn, ctx.user, id, ctx.seq).await?;
    Ok(Effects::default())
}

struct OptionFields {
    segment_size: Option<u32>,
    progression: Option<Progression>,
    require_alphabetical: Option<bool>,
}

/// The carried option fields, every value in range or `invalid`.
fn option_fields(op: &IncomingOp, cap: u32) -> Result<OptionFields, OpError> {
    let segment_size = match op.body.get("segment_size") {
        None => None,
        Some(v) => Some(rules::check_segment_size(v.as_i64().ok_or(Reason::Invalid)?, cap)?),
    };
    let progression = match op.body.get("progression") {
        None => None,
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(|_| OpError::Rejected(Reason::Invalid))?),
    };
    let require_alphabetical = match op.body.get("require_alphabetical") {
        None => None,
        Some(v) => Some(v.as_bool().ok_or(Reason::Invalid)?),
    };
    Ok(OptionFields { segment_size, progression, require_alphabetical })
}

/// Latest wins whole: this device's own change, or one made after seeing
/// the current options, or a later `at`.
fn options_win(ctx: &Ctx, op: &IncomingOp, device: Uuid, seq: i64, changed: DateTime<Utc>, at: DateTime<Utc>) -> bool {
    device == ctx.device || seq <= op.seen_seq || at > changed
}

async fn set_cascade_options(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let id = uuid_of(op, "cascade_id")?;
    let at = at_of(op)?;
    let c = load_cascade(conn, ctx.user, id).await?;
    if c.trashed {
        return Err(OpError::Rejected(Reason::Trashed));
    }
    let f = option_fields(op, ctx.cap)?;
    let at = clamp(conn, at).await?;
    if !options_win(ctx, op, c.options_device_id, c.options_seq, c.options_changed_at, at) {
        return Err(OpError::Rejected(Reason::Stale));
    }
    sqlx::query!(
        "UPDATE cascades SET segment_size = coalesce($2, segment_size), progression = coalesce($3, progression),
                             require_alphabetical = coalesce($4, require_alphabetical), options_changed_at = $5,
                             options_seq = $6, options_device_id = $7, updated_seq = $6
         WHERE id = $1",
        id,
        f.segment_size.map(|s| s as i32),
        f.progression as Option<Progression>,
        f.require_alphabetical,
        at,
        ctx.seq,
        ctx.device,
    )
    .execute(conn)
    .await?;
    Ok(Effects::default())
}

async fn set_quiz_options(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let quiz = uuid_of(op, "quiz_id")?;
    let at = at_of(op)?;
    let l = load_quiz(conn, ctx.user, quiz).await?;
    rules::check_live(&l.cascade, &l.quiz)?;
    let f = option_fields(op, ctx.cap)?;
    rules::check_quiz_option_fields(&l.quiz, f.progression.is_some(), f.segment_size.is_some())?;
    let at = clamp(conn, at).await?;
    if !options_win(ctx, op, l.options_device_id, l.options_seq, l.options_changed_at, at) {
        return Err(OpError::Rejected(Reason::Stale));
    }
    sqlx::query!(
        "UPDATE quizzes SET segment_size = coalesce($2, segment_size), progression = coalesce($3, progression),
                            require_alphabetical = coalesce($4, require_alphabetical), options_changed_at = $5,
                            options_seq = $6, options_device_id = $7, updated_seq = $6
         WHERE id = $1",
        quiz,
        f.segment_size.map(|s| s as i32),
        f.progression as Option<Progression>,
        f.require_alphabetical,
        at,
        ctx.seq,
        ctx.device,
    )
    .execute(conn)
    .await?;
    Ok(Effects::default())
}

async fn set_preferences(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let at = at_of(op)?;
    let mut fields = op.body.as_object().cloned().unwrap_or_default();
    for k in ["id", "device_seq", "seen_seq", "at", "type"] {
        fields.remove(k);
    }
    let p = prefs::parse_preferences(&fields, ctx.cap)?;
    let row = sqlx::query!(
        "SELECT changed_at, changed_by_device_id, updated_seq FROM user_preferences WHERE user_id = $1 FOR UPDATE",
        ctx.user
    )
    .fetch_one(&mut *conn)
    .await?;
    let at = clamp(conn, at).await?;
    let wins = row.changed_by_device_id == Some(ctx.device) || row.updated_seq <= op.seen_seq || at > row.changed_at;
    if !wins {
        return Err(OpError::Rejected(Reason::Stale));
    }
    sqlx::query!(
        "UPDATE user_preferences SET
            default_clear_threshold = coalesce($2, default_clear_threshold),
            leave_value_decimals = coalesce($3, leave_value_decimals),
            anagram_show_definitions = coalesce($4, anagram_show_definitions),
            anagram_show_hooks = coalesce($5, anagram_show_hooks),
            anagram_answer_mode = coalesce($6, anagram_answer_mode),
            default_segment_size = coalesce($7, default_segment_size),
            default_progression = coalesce($8, default_progression),
            default_require_alphabetical = coalesce($9, default_require_alphabetical),
            changed_at = $10, changed_by_device_id = $11, updated_seq = $12
         WHERE user_id = $1",
        ctx.user,
        p.default_clear_threshold,
        p.leave_value_decimals,
        p.anagram_show_definitions,
        p.anagram_show_hooks,
        p.anagram_answer_mode as Option<prefs::AnswerMode>,
        p.default_segment_size,
        p.default_progression as Option<Progression>,
        p.default_require_alphabetical,
        at,
        ctx.device,
        ctx.seq,
    )
    .execute(conn)
    .await?;
    Ok(Effects::default())
}

async fn set_bindings(conn: &mut PgConnection, ctx: &Ctx, op: &IncomingOp) -> OpResult {
    let at = at_of(op)?;
    let list = prefs::validate_bindings(field(op, "bindings")?)?;
    let row = sqlx::query!(
        "SELECT bindings_changed_at, bindings_device_id, updated_seq FROM user_preferences WHERE user_id = $1 FOR UPDATE",
        ctx.user
    )
    .fetch_one(&mut *conn)
    .await?;
    let at = clamp(conn, at).await?;
    let wins = row.bindings_device_id == Some(ctx.device) || row.updated_seq <= op.seen_seq || at > row.bindings_changed_at;
    if !wins {
        return Err(OpError::Rejected(Reason::Stale));
    }
    sqlx::query!("DELETE FROM user_input_bindings WHERE user_id = $1", ctx.user).execute(&mut *conn).await?;
    let mut slots = std::collections::HashMap::<InputAction, i16>::new();
    for b in &list {
        let slot = slots.entry(b.action).or_insert(0);
        sqlx::query!(
            "INSERT INTO user_input_bindings (user_id, action, slot, kind, code, ctrl, shift, alt, meta)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            ctx.user,
            b.action as InputAction,
            *slot,
            b.kind as InputKind,
            b.code,
            b.ctrl,
            b.shift,
            b.alt,
            b.meta,
        )
        .execute(&mut *conn)
        .await?;
        *slot += 1;
    }
    sqlx::query!(
        "UPDATE user_preferences SET bindings_changed_at = $2, bindings_device_id = $3, updated_seq = $4 WHERE user_id = $1",
        ctx.user,
        at,
        ctx.device,
        ctx.seq
    )
    .execute(conn)
    .await?;
    Ok(Effects::default())
}
