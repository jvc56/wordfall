//! PLAN.md § Integration tests → Schema tests.

mod common;

use common::TestApp;
use serde_json::{json, Value};
use uuid::Uuid;
use wordfall::search::store::{insert_spec, load_spec, SpecKind};
use wordfall::search::wire::{parse_tree, tree_to_json, QuizType};

fn contract_conditions() -> Vec<Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../contract-fixtures/filters/conditions.json");
    let f: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    f["valid"].as_array().unwrap().iter().map(|v| v["condition"].clone()).collect()
}

async fn user(app: &TestApp) -> Uuid {
    app.signed_in("owner").await.user_id.unwrap()
}

async fn spec_id(app: &TestApp, owner: Uuid) -> Uuid {
    let id: Uuid = sqlx::query_scalar("INSERT INTO search_specs (user_id, quiz_type) VALUES ($1, 'anagram') RETURNING id")
        .bind(owner)
        .fetch_one(app.db())
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_groups (spec_id, id, parent_id, op, order_in_group) VALUES ($1, 0, NULL, 'and', 0)")
        .bind(id)
        .execute(app.db())
        .await
        .unwrap();
    id
}

/// Inserts one condition row with the given columns set; returns whether the
/// schema accepted it.
async fn try_condition(app: &TestApp, spec: Uuid, pos: i16, ctype: &str, negated: bool, cols: &[(&str, &str)]) -> bool {
    let mut names = vec!["spec_id", "position", "group_id", "order_in_group", "condition_type", "negated"];
    let mut values = vec![
        format!("'{spec}'"),
        pos.to_string(),
        "0".into(),
        pos.to_string(),
        format!("'{ctype}'"),
        negated.to_string(),
    ];
    for (k, v) in cols {
        names.push(k);
        values.push(v.to_string());
    }
    let sql = format!("INSERT INTO search_conditions ({}) VALUES ({})", names.join(", "), values.join(", "));
    sqlx::query(sqlx::AssertSqlSafe(sql)).execute(app.db()).await.is_ok()
}

/// Every one of the 23 condition types round-trips through `search_conditions`
/// and back, and a nested AND / OR tree round-trips unchanged with its spec's
/// `quiz_type`.
#[sqlx::test]
async fn every_condition_type_and_a_nested_tree_round_trip(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    app.seed_fixture_catalog("root").await;
    let owner = user(&app).await;
    let conds = contract_conditions();
    assert_eq!(conds.len(), 23);
    let tree = json!({ "op": "and", "children": [
        conds[0], conds[1],
        { "op": "or", "children": [ conds[2], conds[3], { "op": "and", "children": conds[4..12].to_vec() } ] },
        { "op": "and", "children": conds[12..23].to_vec() } ] });
    for (kind, quiz_type) in [(SpecKind::Cascade, QuizType::Anagram), (SpecKind::SavedSearch, QuizType::LeaveValue)] {
        let parsed = parse_tree(&tree).unwrap();
        let mut conn = app.db().acquire().await.unwrap();
        let id = insert_spec(&mut conn, owner, quiz_type, &parsed, kind).await.unwrap();
        let (q, back) = load_spec(&mut conn, id).await.unwrap().unwrap();
        assert_eq!(q, quiz_type, "the spec's quiz_type is carried back out");
        assert_eq!(tree_to_json(&back), tree, "child order included");
        // In Lexicon in both stored forms: the id in a cascade's spec, the
        // name in a saved search's.
        let (id_col, name_col): (Option<i16>, Option<String>) = sqlx::query_as(
            "SELECT other_lexicon_id, text_value FROM search_conditions WHERE spec_id = $1 AND condition_type = 'in_lexicon'",
        )
        .bind(id)
        .fetch_one(app.db())
        .await
        .unwrap();
        match kind {
            SpecKind::Cascade => assert!(id_col.is_some() && name_col.is_none()),
            SpecKind::SavedSearch => assert!(id_col.is_none() && name_col.as_deref() == Some("EN-FIX")),
        }
    }
    // An In Lexicon row holding both, or neither, is rejected.
    let spec = spec_id(&app, owner).await;
    assert!(!try_condition(&app, spec, 0, "in_lexicon", false, &[("text_value", "'EN-FIX'"), ("other_lexicon_id", "1")]).await);
    assert!(!try_condition(&app, spec, 0, "in_lexicon", false, &[]).await);
}

/// For each type, a row with a missing or extra parameter, or with Not where
/// it isn't allowed, is rejected by the `CHECK`.
#[sqlx::test]
async fn the_check_refuses_missing_extra_and_not(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let owner = user(&app).await;
    let spec = spec_id(&app, owner).await;
    let text = ("text_value", "'A'");
    let range = [("min_value", "1"), ("max_value", "2")];
    let order = [("min_value", "1"), ("max_value", "2"), ("lax", "true")];
    let cases: Vec<(&str, Vec<(&str, &str)>)> = vec![
        ("anagram_match", vec![text]),
        ("pattern_match", vec![text]),
        ("subanagram_match", vec![text]),
        ("length", range.to_vec()),
        ("in_lexicon", vec![text]),
        ("in_word_list", vec![]),
        ("num_vowels", range.to_vec()),
        ("includes_letters", vec![text]),
        ("probability_order", order.to_vec()),
        ("limit_by_probability_order", order.to_vec()),
        ("playability_order", order.to_vec()),
        ("limit_by_playability_order", order.to_vec()),
        ("num_unique_letters", range.to_vec()),
        ("point_value", range.to_vec()),
        ("takes_prefix", vec![text]),
        ("takes_suffix", vec![text]),
        ("part_of_speech", vec![("part_of_speech_value", "'noun'")]),
        ("definition", vec![text]),
        ("consists_of", vec![text, ("min_value", "70"), ("max_value", "100")]),
        ("num_anagrams", range.to_vec()),
        ("front_inner_hook", vec![]),
        ("back_inner_hook", vec![]),
        ("leave_value", vec![("min_leave_value", "1.5")]),
    ];
    let negatable = [
        "anagram_match", "pattern_match", "subanagram_match", "in_lexicon", "in_word_list", "includes_letters",
        "takes_prefix", "takes_suffix", "part_of_speech", "definition", "front_inner_hook", "back_inner_hook",
    ];
    let mut pos = 0i16;
    for (t, cols) in &cases {
        pos += 1;
        assert!(try_condition(&app, spec, pos, t, false, cols).await, "{t} valid");
        // Missing a parameter.
        if !cols.is_empty() && *t != "leave_value" {
            pos += 1;
            assert!(!try_condition(&app, spec, pos, t, false, &cols[..cols.len() - 1]).await, "{t} missing");
        }
        // An extra parameter.
        pos += 1;
        let mut extra = cols.clone();
        extra.push(if cols.iter().any(|(k, _)| *k == "part_of_speech_value") {
            ("text_value", "'x'")
        } else {
            ("part_of_speech_value", "'noun'")
        });
        assert!(!try_condition(&app, spec, pos, t, false, &extra).await, "{t} extra");
        // Not where it isn't allowed.
        pos += 1;
        assert_eq!(try_condition(&app, spec, pos, t, true, cols).await, negatable.contains(t), "{t} negated");
    }
    // Leave Value with both bounds missing.
    pos += 1;
    assert!(!try_condition(&app, spec, pos, "leave_value", false, &[]).await);
}

/// "A spec of 100 groups round-trips, and a 101st is rejected by the `id` CHECK."
#[sqlx::test]
async fn a_spec_of_one_hundred_groups(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    app.seed_fixture_catalog("root").await;
    let owner = user(&app).await;
    let children: Vec<Value> = (0..99)
        .map(|i| json!({ "op": "and", "children": [
            { "type": "length", "negated": false, "min": 2 + (i % 5), "max": 8 } ] }))
        .collect();
    let tree = json!({ "op": "and", "children": children });
    let mut conn = app.db().acquire().await.unwrap();
    let id = insert_spec(&mut conn, owner, QuizType::Anagram, &parse_tree(&tree).unwrap(), SpecKind::SavedSearch)
        .await
        .unwrap();
    let (_, back) = load_spec(&mut conn, id).await.unwrap().unwrap();
    assert_eq!(tree_to_json(&back), tree);
    let r = sqlx::query("INSERT INTO search_groups (spec_id, id, parent_id, op, order_in_group) VALUES ($1, 100, 0, 'and', 99)")
        .bind(id)
        .execute(app.db())
        .await;
    assert!(r.is_err());
}

#[sqlx::test]
async fn leave_value_bounds_must_be_finite(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let owner = user(&app).await;
    let spec = spec_id(&app, owner).await;
    let mut pos = 0;
    for (min, max) in [
        ("'NaN'", "1"),
        ("1", "'NaN'"),
        ("'NaN'", "'NaN'"),
        ("'Infinity'", "NULL"),
        ("NULL", "'-Infinity'"),
    ] {
        pos += 1;
        assert!(
            !try_condition(&app, spec, pos, "leave_value", false, &[("min_leave_value", min), ("max_leave_value", max)]).await,
            "{min} {max}"
        );
    }
    pos += 1;
    assert!(try_condition(&app, spec, pos, "leave_value", false, &[("min_leave_value", "-1000000"), ("max_leave_value", "1000000")]).await);
}

#[sqlx::test]
async fn text_value_holds_five_hundred_characters(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let owner = user(&app).await;
    let spec = spec_id(&app, owner).await;
    let ok = format!("'{}'", "A".repeat(500));
    let long = format!("'{}'", "A".repeat(501));
    assert!(try_condition(&app, spec, 1, "definition", false, &[("text_value", &ok)]).await);
    assert!(!try_condition(&app, spec, 2, "definition", false, &[("text_value", &long)]).await);
}

async fn minimal_cascade(app: &TestApp, owner: Uuid, lexicon: i16, leave_set: Option<i32>) -> Result<Uuid, sqlx::Error> {
    let spec = spec_id(app, owner).await;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO cascades (id, user_id, name, quiz_type, lexicon_id, leave_set_id, spec_id, clear_threshold,
                               options_changed_at, options_seq, options_device_id, question_count, depth, updated_seq)
         VALUES ($1, $2, 'c', $3::quiz_type, $4, $5, $6, 80, now(), 1, gen_random_uuid(), 10, 1, 1)",
    )
    .bind(id)
    .bind(owner)
    .bind(if leave_set.is_some() { "leave_value" } else { "anagram" })
    .bind(lexicon)
    .bind(leave_set)
    .bind(spec)
    .execute(app.db())
    .await
    .map(|_| id)
}

async fn quiz(app: &TestApp, owner: Uuid, cascade: Uuid, level: i32, origin: &str, status: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO quizzes (id, cascade_id, user_id, level, origin, status, cleared_at, options_changed_at,
                              options_seq, options_device_id, shuffle_seed, questions_hash, question_count,
                              created_seq, updated_seq)
         VALUES (gen_random_uuid(), $1, $2, $3, $4::quiz_origin, $5::quiz_status,
                 CASE WHEN $5 = 'cleared' THEN now() END, now(), 1, gen_random_uuid(), 1, 1, 10, 1, 1)",
    )
    .bind(cascade)
    .bind(owner)
    .bind(level)
    .bind(origin)
    .bind(status)
    .execute(app.db())
    .await
    .map(|_| ())
}

/// The cascade stack is enforced by the database where it can be.
#[sqlx::test]
async fn the_cascade_stack_constraints(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    app.seed_fixture_catalog("root").await;
    let owner = user(&app).await;
    let en: i16 = sqlx::query_scalar("SELECT id FROM lexicons WHERE name = 'EN-FIX'").fetch_one(app.db()).await.unwrap();
    let ca_leaves: i32 = sqlx::query_scalar(
        "SELECT s.id FROM leave_sets s JOIN lexicons l ON l.id = s.lexicon_id WHERE l.name = 'CA-FIX'",
    )
    .fetch_one(app.db())
    .await
    .unwrap();
    // A Leave Value cascade whose leave set belongs to another lexicon.
    assert!(minimal_cascade(&app, owner, en, Some(ca_leaves)).await.is_err());

    let c = minimal_cascade(&app, owner, en, None).await.unwrap();
    quiz(&app, owner, c, 1, "source", "active").await.unwrap();
    quiz(&app, owner, c, 2, "descent", "active").await.unwrap();
    // A second active quiz at the same level.
    assert!(quiz(&app, owner, c, 2, "descent", "active").await.is_err());
    // A second Source quiz.
    assert!(quiz(&app, owner, c, 1, "source", "active").await.is_err());
    let c2 = minimal_cascade(&app, owner, en, None).await.unwrap();
    // A Source quiz at any level but 1, or cleared.
    assert!(quiz(&app, owner, c2, 2, "source", "active").await.is_err());
    assert!(quiz(&app, owner, c2, 1, "source", "cleared").await.is_err());
    // A non-Source quiz at Level 1.
    assert!(quiz(&app, owner, c2, 1, "descent", "active").await.is_err());
    // A cascade with depth 0.
    let r = sqlx::query("UPDATE cascades SET depth = 0 WHERE id = $1").bind(c2).execute(app.db()).await;
    assert!(r.is_err());
}
