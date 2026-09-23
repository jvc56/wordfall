//! Catalog rows in Postgres: loading them into indexes, and writing uploads.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::index::RawWord;
use super::tiles::{Distribution, Tile, TileDef};
use super::upload::{ParsedDistribution, ParsedLeave, ParsedWord};

pub struct Items {
    pub distributions: Vec<i16>,
    /// (lexicon id, distribution id)
    pub lexicons: Vec<(i16, i16)>,
    /// (leave set id, lexicon id)
    pub leave_sets: Vec<(i32, i16)>,
}

pub async fn list_items(db: &PgPool) -> Result<Items, sqlx::Error> {
    let distributions = sqlx::query_scalar!("SELECT id FROM letter_distributions ORDER BY id")
        .fetch_all(db)
        .await?;
    let lexicons = sqlx::query!("SELECT id, letter_distribution_id FROM lexicons ORDER BY id")
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|r| (r.id, r.letter_distribution_id))
        .collect();
    let leave_sets = sqlx::query!("SELECT id, lexicon_id FROM leave_sets ORDER BY id")
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|r| (r.id, r.lexicon_id))
        .collect();
    Ok(Items {
        distributions,
        lexicons,
        leave_sets,
    })
}

pub async fn load_distribution(db: &PgPool, id: i16) -> Result<Option<Distribution>, sqlx::Error> {
    let Some(name) = sqlx::query_scalar!("SELECT name FROM letter_distributions WHERE id = $1", id)
        .fetch_optional(db)
        .await?
    else {
        return Ok(None);
    };
    let tiles = sqlx::query!(
        "SELECT letter, blank_letter, count, value, is_vowel FROM letter_distribution_tiles
         WHERE letter_distribution_id = $1 ORDER BY position",
        id
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|r| TileDef {
        letter: r.letter,
        blank_letter: r.blank_letter,
        count: r.count as u16,
        value: r.value as u16,
        is_vowel: r.is_vowel,
    })
    .collect();
    Ok(Some(Distribution::new(id, name, tiles)))
}

/// Stored words are canonical MAGPIE notation, validated on upload.
pub async fn load_lexicon(
    db: &PgPool,
    id: i16,
    dist: &Distribution,
) -> anyhow::Result<Option<(String, Vec<RawWord>)>> {
    let Some(name) = sqlx::query_scalar!("SELECT name FROM lexicons WHERE id = $1", id)
        .fetch_optional(db)
        .await?
    else {
        return Ok(None);
    };
    let rows = sqlx::query!(
        "SELECT word, playability, definition FROM lexicon_words WHERE lexicon_id = $1",
        id
    )
    .fetch_all(db)
    .await?;
    let mut words = Vec::with_capacity(rows.len());
    for r in rows {
        let tiles = dist
            .parse_magpie(&r.word, false)
            .map_err(|e| anyhow::anyhow!("stored word {:?} does not parse: {e}", r.word))?;
        words.push(RawWord {
            tiles,
            playability: r.playability,
            definition: r.definition,
        });
    }
    Ok(Some((name, words)))
}

pub async fn load_leaves(
    db: &PgPool,
    id: i32,
    dist: &Distribution,
) -> anyhow::Result<Option<Vec<(Vec<Tile>, f64)>>> {
    let exists = sqlx::query_scalar!("SELECT 1 AS \"one!\" FROM leave_sets WHERE id = $1", id)
        .fetch_optional(db)
        .await?;
    if exists.is_none() {
        return Ok(None);
    }
    let rows = sqlx::query!(
        "SELECT leave, value FROM leave_values WHERE leave_set_id = $1",
        id
    )
    .fetch_all(db)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let tiles = dist
            .parse_magpie(&r.leave, true)
            .map_err(|e| anyhow::anyhow!("stored leave {:?} does not parse: {e}", r.leave))?;
        out.push((tiles, r.value));
    }
    Ok(Some(out))
}

const BATCH: usize = 5000;

pub async fn insert_distribution(
    tx: &mut PgConnection,
    name: &str,
    uploaded_by: Uuid,
    parsed: &ParsedDistribution,
) -> Result<i16, sqlx::Error> {
    let id = sqlx::query_scalar!(
        "INSERT INTO letter_distributions (name, uploaded_by) VALUES ($1, $2) RETURNING id",
        name,
        uploaded_by
    )
    .fetch_one(&mut *tx)
    .await?;
    for (pos, (t, fw)) in parsed.tiles.iter().zip(&parsed.fullwidth).enumerate() {
        sqlx::query!(
            "INSERT INTO letter_distribution_tiles
               (letter_distribution_id, position, letter, blank_letter, count, value, is_vowel,
                fullwidth_letter, fullwidth_blank_letter)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            id,
            pos as i16,
            t.letter,
            t.blank_letter,
            t.count as i16,
            t.value as i16,
            t.is_vowel,
            fw.as_ref().map(|f| f.0.as_str()),
            fw.as_ref().map(|f| f.1.as_str()),
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(id)
}

pub async fn insert_lexicon(
    tx: &mut PgConnection,
    name: &str,
    dist_id: i16,
    uploaded_by: Uuid,
    words: &[ParsedWord],
) -> Result<i16, sqlx::Error> {
    let id = sqlx::query_scalar!(
        "INSERT INTO lexicons (name, letter_distribution_id, word_count, uploaded_by)
         VALUES ($1, $2, $3, $4) RETURNING id",
        name,
        dist_id,
        words.len() as i32,
        uploaded_by
    )
    .fetch_one(&mut *tx)
    .await?;
    for chunk in words.chunks(BATCH) {
        let w: Vec<&str> = chunk.iter().map(|x| x.word.as_str()).collect();
        let p: Vec<f64> = chunk.iter().map(|x| x.playability).collect();
        let d: Vec<&str> = chunk.iter().map(|x| x.definition.as_str()).collect();
        sqlx::query!(
            "INSERT INTO lexicon_words (lexicon_id, word, playability, definition)
             SELECT $1, * FROM UNNEST($2::text[], $3::float8[], $4::text[])",
            id,
            &w as &[&str],
            &p,
            &d as &[&str],
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(id)
}

pub async fn insert_leave_set(
    tx: &mut PgConnection,
    lexicon_id: i16,
    uploaded_by: Uuid,
    leaves: &[ParsedLeave],
) -> Result<i32, sqlx::Error> {
    let id = sqlx::query_scalar!(
        "INSERT INTO leave_sets (lexicon_id, leave_count, uploaded_by) VALUES ($1, $2, $3) RETURNING id",
        lexicon_id,
        leaves.len() as i32,
        uploaded_by
    )
    .fetch_one(&mut *tx)
    .await?;
    for chunk in leaves.chunks(BATCH) {
        let l: Vec<&str> = chunk.iter().map(|x| x.leave.as_str()).collect();
        let v: Vec<f64> = chunk.iter().map(|x| x.value).collect();
        sqlx::query!(
            "INSERT INTO leave_values (leave_set_id, leave, value)
             SELECT $1, * FROM UNNEST($2::text[], $3::float8[])",
            id,
            &l as &[&str],
            &v,
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(id)
}

pub async fn notify(tx: &mut PgConnection) -> Result<(), sqlx::Error> {
    // Delivered when the transaction commits.
    sqlx::query!("SELECT pg_notify('catalog_changed', '')")
        .fetch_one(tx)
        .await?;
    Ok(())
}
