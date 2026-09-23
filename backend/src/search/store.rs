//! Filter specifications in Postgres (PLAN.md § Schema → Filter
//! specifications): the AND / OR tree as `search_groups` rows with group 0 at
//! the top, typed parameter columns per condition, and In Word List entries in
//! `search_condition_words`.
//!
//! An In Lexicon row holds the lexicon's id in a cascade's spec (pinning it)
//! and its name in a saved search's (pinning nothing).

use std::collections::HashMap;

use sqlx::PgConnection;
use uuid::Uuid;

use super::wire::{ConditionType, GroupOp, Params, QuizType, WireCondition, WireGroup, WireNode};
use crate::catalog::pos::PartOfSpeech;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SpecKind {
    /// In Lexicon stores `other_lexicon_id`.
    Cascade,
    /// In Lexicon stores the name in `text_value`.
    SavedSearch,
}

struct GroupRow {
    id: i16,
    parent: Option<i16>,
    op: GroupOp,
    order: i16,
}

struct CondRow<'a> {
    position: i16,
    group: i16,
    order: i16,
    cond: &'a WireCondition,
}

fn flatten<'a>(g: &'a WireGroup, parent: Option<i16>, order: i16, groups: &mut Vec<GroupRow>, conds: &mut Vec<CondRow<'a>>) {
    let id = groups.len() as i16;
    groups.push(GroupRow { id, parent, op: g.op, order });
    for (i, c) in g.children.iter().enumerate() {
        match c {
            WireNode::Group(sub) => flatten(sub, Some(id), i as i16, groups, conds),
            WireNode::Condition(cond) => {
                let position = conds.len() as i16;
                conds.push(CondRow { position, group: id, order: i as i16, cond });
            }
        }
    }
}

/// Resolves In Lexicon names to ids for a cascade's spec.
pub async fn lexicon_ids(conn: &mut PgConnection, tree: &WireGroup) -> Result<HashMap<String, i16>, sqlx::Error> {
    let mut names = Vec::new();
    collect_lexicons(tree, &mut names);
    if names.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query!("SELECT id, name FROM lexicons WHERE name = ANY($1)", &names)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().map(|r| (r.name, r.id)).collect())
}

fn collect_lexicons(g: &WireGroup, out: &mut Vec<String>) {
    for c in &g.children {
        match c {
            WireNode::Group(sub) => collect_lexicons(sub, out),
            WireNode::Condition(WireCondition { params: Params::Lexicon(n), .. }) => out.push(n.clone()),
            _ => {}
        }
    }
}

/// Stores a spec and returns its id.
pub async fn insert_spec(
    conn: &mut PgConnection,
    user_id: Uuid,
    quiz_type: QuizType,
    tree: &WireGroup,
    kind: SpecKind,
) -> Result<Uuid, sqlx::Error> {
    let lex = if kind == SpecKind::Cascade { lexicon_ids(conn, tree).await? } else { HashMap::new() };
    let spec_id = sqlx::query_scalar!(
        "INSERT INTO search_specs (user_id, quiz_type) VALUES ($1, $2) RETURNING id",
        user_id,
        quiz_type as QuizType
    )
    .fetch_one(&mut *conn)
    .await?;
    let mut groups = Vec::new();
    let mut conds = Vec::new();
    flatten(tree, None, 0, &mut groups, &mut conds);

    let gid: Vec<i16> = groups.iter().map(|g| g.id).collect();
    let gparent: Vec<Option<i16>> = groups.iter().map(|g| g.parent).collect();
    let gop: Vec<GroupOp> = groups.iter().map(|g| g.op).collect();
    let gorder: Vec<i16> = groups.iter().map(|g| g.order).collect();
    sqlx::query!(
        "INSERT INTO search_groups (spec_id, id, parent_id, op, order_in_group)
         SELECT $1, * FROM UNNEST($2::int2[], $3::int2[], $4::group_op[], $5::int2[])",
        spec_id,
        &gid,
        &gparent as &[Option<i16>],
        &gop as &[GroupOp],
        &gorder,
    )
    .execute(&mut *conn)
    .await?;

    let mut words_pos: Vec<i16> = Vec::new();
    let mut words_entry: Vec<String> = Vec::new();
    for r in &conds {
        let c = r.cond;
        let (mut text, mut pos, mut other, mut min, mut max, mut lmin, mut lmax, mut lax): (
            Option<String>,
            Option<PartOfSpeech>,
            Option<i16>,
            Option<i32>,
            Option<i32>,
            Option<f64>,
            Option<f64>,
            Option<bool>,
        ) = (None, None, None, None, None, None, None, None);
        match &c.params {
            Params::Pattern(s) | Params::Tiles(s) | Params::Text(s) => text = Some(s.clone()),
            Params::Range { min: a, max: b } => {
                min = Some(*a as i32);
                max = Some(*b as i32);
            }
            Params::Order { min: a, max: b, lax: l } => {
                min = Some(*a as i32);
                max = Some(*b as i32);
                lax = Some(*l);
            }
            Params::ConsistsOf { tiles, min: a, max: b } => {
                text = Some(tiles.clone());
                min = Some(*a as i32);
                max = Some(*b as i32);
            }
            Params::Lexicon(name) => match kind {
                SpecKind::Cascade => other = lex.get(name).copied(),
                SpecKind::SavedSearch => text = Some(name.clone()),
            },
            Params::Entries(entries) => {
                let mut uniq: Vec<&String> = entries.iter().collect();
                uniq.sort();
                uniq.dedup();
                for e in uniq {
                    words_pos.push(r.position);
                    words_entry.push(e.clone());
                }
            }
            Params::PartOfSpeech(p) => pos = Some(*p),
            Params::None => {}
            Params::LeaveValue { min: a, max: b } => {
                lmin = *a;
                lmax = *b;
            }
        }
        sqlx::query!(
            "INSERT INTO search_conditions
               (spec_id, position, group_id, order_in_group, condition_type, negated, text_value,
                part_of_speech_value, other_lexicon_id, min_value, max_value, min_leave_value,
                max_leave_value, lax)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
            spec_id,
            r.position,
            r.group,
            r.order,
            c.ctype as ConditionType,
            c.negated,
            text,
            pos as Option<PartOfSpeech>,
            other,
            min,
            max,
            lmin,
            lmax,
            lax,
        )
        .execute(&mut *conn)
        .await?;
    }
    if !words_pos.is_empty() {
        sqlx::query!(
            "INSERT INTO search_condition_words (spec_id, position, entry)
             SELECT $1, * FROM UNNEST($2::int2[], $3::text[])",
            spec_id,
            &words_pos,
            &words_entry,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(spec_id)
}

/// Reads a spec back into the wire form. An In Lexicon row stored by id comes
/// back with the lexicon's current name.
pub async fn load_spec(conn: &mut PgConnection, spec_id: Uuid) -> Result<Option<(QuizType, WireGroup)>, sqlx::Error> {
    let Some(quiz_type) = sqlx::query_scalar!(
        r#"SELECT quiz_type AS "quiz_type: QuizType" FROM search_specs WHERE id = $1"#,
        spec_id
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    let groups = sqlx::query!(
        r#"SELECT id, parent_id, op AS "op: GroupOp", order_in_group FROM search_groups WHERE spec_id = $1"#,
        spec_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let conds = sqlx::query!(
        r#"SELECT c.position, c.group_id, c.order_in_group, c.condition_type AS "ctype: ConditionType",
                  c.negated, c.text_value, c.part_of_speech_value AS "pos: PartOfSpeech",
                  c.other_lexicon_id, l.name AS "other_lexicon_name?", c.min_value, c.max_value,
                  c.min_leave_value, c.max_leave_value, c.lax
           FROM search_conditions c LEFT JOIN lexicons l ON l.id = c.other_lexicon_id
           WHERE c.spec_id = $1"#,
        spec_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let words = sqlx::query!(
        r#"SELECT position, entry FROM search_condition_words WHERE spec_id = $1 ORDER BY position, entry COLLATE "C""#,
        spec_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut entries: HashMap<i16, Vec<String>> = HashMap::new();
    for w in words {
        entries.entry(w.position).or_default().push(w.entry);
    }

    // Children of each group, ordered by order_in_group.
    enum Child {
        Group(i16),
        Cond(WireCondition),
    }
    let mut children: HashMap<i16, Vec<(i16, Child)>> = HashMap::new();
    let mut ops: HashMap<i16, GroupOp> = HashMap::new();
    for g in &groups {
        ops.insert(g.id, g.op);
        if let Some(p) = g.parent_id {
            children.entry(p).or_default().push((g.order_in_group, Child::Group(g.id)));
        }
    }
    for c in conds {
        let min = i64::from(c.min_value.unwrap_or(0));
        let max = i64::from(c.max_value.unwrap_or(0));
        use ConditionType as T;
        let params = match c.ctype {
            T::AnagramMatch | T::PatternMatch | T::SubanagramMatch => Params::Pattern(c.text_value.unwrap_or_default()),
            T::Length | T::NumVowels | T::NumUniqueLetters | T::PointValue | T::NumAnagrams => Params::Range { min, max },
            T::ProbabilityOrder | T::LimitByProbabilityOrder | T::PlayabilityOrder | T::LimitByPlayabilityOrder => {
                Params::Order { min, max, lax: c.lax.unwrap_or(true) }
            }
            T::ConsistsOf => Params::ConsistsOf { tiles: c.text_value.unwrap_or_default(), min, max },
            T::IncludesLetters | T::TakesPrefix | T::TakesSuffix => Params::Tiles(c.text_value.unwrap_or_default()),
            T::InLexicon => Params::Lexicon(c.other_lexicon_name.or(c.text_value).unwrap_or_default()),
            T::InWordList => Params::Entries(entries.remove(&c.position).unwrap_or_default()),
            T::PartOfSpeech => Params::PartOfSpeech(c.pos.unwrap_or(PartOfSpeech::Noun)),
            T::Definition => Params::Text(c.text_value.unwrap_or_default()),
            T::FrontInnerHook | T::BackInnerHook => Params::None,
            T::LeaveValue => Params::LeaveValue { min: c.min_leave_value, max: c.max_leave_value },
        };
        children.entry(c.group_id).or_default().push((
            c.order_in_group,
            Child::Cond(WireCondition { ctype: c.ctype, negated: c.negated, params }),
        ));
    }
    fn build(id: i16, ops: &HashMap<i16, GroupOp>, children: &mut HashMap<i16, Vec<(i16, Child)>>) -> WireGroup {
        let mut kids = children.remove(&id).unwrap_or_default();
        kids.sort_by_key(|(o, _)| *o);
        let nodes = kids
            .into_iter()
            .map(|(_, c)| match c {
                Child::Group(g) => WireNode::Group(build(g, ops, children)),
                Child::Cond(c) => WireNode::Condition(c),
            })
            .collect();
        WireGroup { op: ops.get(&id).copied().unwrap_or(GroupOp::And), children: nodes }
    }
    Ok(Some((quiz_type, build(0, &ops, &mut children))))
}

/// The total In Word List entries of a spec.
pub async fn word_list_entries(conn: &mut PgConnection, spec_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM search_condition_words WHERE spec_id = $1"#, spec_id)
        .fetch_one(conn)
        .await
}

/// Copies a cascade's spec for Start over, `quiz_type` included.
pub async fn copy_spec(conn: &mut PgConnection, user_id: Uuid, spec_id: Uuid) -> Result<Uuid, sqlx::Error> {
    let new_id = sqlx::query_scalar!(
        "INSERT INTO search_specs (user_id, quiz_type) SELECT $1, quiz_type FROM search_specs WHERE id = $2
         RETURNING id",
        user_id,
        spec_id
    )
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query!(
        "INSERT INTO search_groups (spec_id, id, parent_id, op, order_in_group)
         SELECT $1, id, parent_id, op, order_in_group FROM search_groups WHERE spec_id = $2",
        new_id,
        spec_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "INSERT INTO search_conditions (spec_id, position, group_id, order_in_group, condition_type, negated,
                                        text_value, part_of_speech_value, other_lexicon_id, min_value,
                                        max_value, min_leave_value, max_leave_value, lax)
         SELECT $1, position, group_id, order_in_group, condition_type, negated, text_value,
                part_of_speech_value, other_lexicon_id, min_value, max_value, min_leave_value,
                max_leave_value, lax
         FROM search_conditions WHERE spec_id = $2",
        new_id,
        spec_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "INSERT INTO search_condition_words (spec_id, position, entry)
         SELECT $1, position, entry FROM search_condition_words WHERE spec_id = $2",
        new_id,
        spec_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(new_id)
}
