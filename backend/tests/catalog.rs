//! PLAN.md § Admin, § Catalog Indexes, § API → Catalog and search / Admin, and
//! § Integration tests: upload validation, MAGPIE-DATA uploads, NOTIFY reload
//! across instances, the reconcile fallback, concurrent startup, deletion
//! references, admin authorization, `/api/lexicons` gating and maxima, the
//! per-IP catalog limit, and the admin upload limit.

mod common;

use std::collections::HashMap;
use std::time::Duration;

use axum::http::StatusCode;
use common::{Client, TestApp, fixture_manifest, fixture_path};
use serde_json::{Value, json};

async fn count(app: &TestApp, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(app.db())
        .await
        .unwrap()
}

async fn admin(app: &TestApp) -> Client<'_> {
    let c = app.signed_in("root").await;
    app.make_admin("root").await;
    c
}

fn error_lines(r: &common::TestResponse) -> Vec<Value> {
    r.json()["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["line"].clone())
        .collect()
}

const ENGLISH: &str =
    "?,?,2,0,0\nA,a,9,1,1\nB,b,2,3,0\nE,e,12,1,1\nS,s,4,1,0\nT,t,6,1,0\nX,x,1,8,0\n";

// ---------------------------------------------------------------------------
// Upload validation: one failing file per rule, with line numbers and nothing written
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn distribution_upload_rules(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("ADMIN_UPLOAD_RATE_PER_MINUTE", "1000")]).await;
    let mut a = admin(&app).await;
    let cases: Vec<(&str, &[u8], Vec<Value>)> = vec![
        ("fields", b"?,?,2,0,0\nA,a,9,1\n", vec![json!(2)]),
        ("first line blank", b"A,a,9,1,1\n", vec![json!(1)]),
        ("blank value", b"?,?,2,1,0\n", vec![json!(1)]),
        (
            "forbidden char",
            b"?,?,2,0,0\nA*,a*,9,1,1\n",
            vec![json!(2), json!(2)],
        ),
        (
            "whitespace inside",
            b"?,?,2,0,0\nA B,ab,9,1,1\n",
            vec![json!(2)],
        ),
        (
            "over 8 bytes",
            "?,?,2,0,0\nÇÇÇÇÇ,ç,1,1,0\n".as_bytes(),
            vec![json!(2)],
        ),
        (
            "duplicate letter",
            b"?,?,2,0,0\nA,a,9,1,1\nA,b,1,1,1\n",
            vec![json!(3)],
        ),
        (
            "duplicate blank letter",
            b"?,?,2,0,0\nA,a,9,1,1\nB,a,1,1,1\n",
            vec![json!(3)],
        ),
        (
            "blank letter is a letter",
            b"?,?,2,0,0\nA,a,9,1,1\nB,A,1,1,1\n",
            vec![json!(3)],
        ),
        ("negative count", b"?,?,2,0,0\nA,a,-9,1,1\n", vec![json!(2)]),
        (
            "non-integer value",
            b"?,?,2,0,0\nA,a,9,1.5,1\n",
            vec![json!(2)],
        ),
        ("vowel flag", b"?,?,2,0,0\nA,a,9,1,yes\n", vec![json!(2)]),
        (
            "one fullwidth form",
            b"?,?,2,0,0\nA,a,9,1,1,X,\n",
            vec![json!(2)],
        ),
        (
            "invalid utf-8",
            b"?,?,2,0,0\nA\xff,a,9,1,1\n",
            vec![json!(2)],
        ),
        ("empty file", b"\n  \n", vec![Value::Null]),
    ];
    for (i, (what, file, lines)) in cases.into_iter().enumerate() {
        let r = a
            .upload(
                "/api/admin/letter-distributions",
                &[("name", &format!("d{i}"))],
                "x.csv",
                file,
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}");
        assert_eq!(error_lines(&r), lines, "{what}: {:?}", r.json());
        assert_eq!(r.json()["total_errors"], lines.len(), "{what}");
    }
    assert_eq!(
        count(&app, "letter_distributions").await,
        0,
        "nothing was written"
    );

    // BOM, CRLF, blank lines and whitespace around fields are fine; the name
    // defaults to the file name without `.csv`.
    let ok = "\u{FEFF}?,?,2,0,0\r\n\r\n A , a , 9 , 1 , 1 \r\nL·L,l·l,1,10,0\r\n";
    let r = a
        .upload(
            "/api/admin/letter-distributions",
            &[],
            "my_dist.csv",
            ok.as_bytes(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.json());
    assert_eq!(r.json()["name"], "my_dist");
    // The name must be unused.
    let r = a
        .upload(
            "/api/admin/letter-distributions",
            &[("name", "my_dist")],
            "x.csv",
            ok.as_bytes(),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(error_lines(&r), vec![Value::Null]);
}

#[sqlx::test]
async fn lexicon_upload_rules(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("ADMIN_UPLOAD_RATE_PER_MINUTE", "1000")]).await;
    let mut a = admin(&app).await;
    let r = a
        .upload(
            "/api/admin/letter-distributions",
            &[("name", "en")],
            "en.csv",
            ENGLISH.as_bytes(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let long_def = "x".repeat(10_001);
    let cases: Vec<(&str, String, Vec<Value>)> = vec![
        ("fields", "BAT\t1\n".into(), vec![json!(1)]),
        ("extra field", "BAT\t1\tx\ty\n".into(), vec![json!(1)]),
        ("lower-case tile", "bat\t1\tx\n".into(), vec![json!(1)]),
        ("blank in a word", "BA?\t1\tx\n".into(), vec![json!(1)]),
        (
            "tile not in distribution",
            "CAT\t1\tx\n".into(),
            vec![json!(1)],
        ),
        ("malformed brackets", "B[A\t1\tx\n".into(), vec![json!(1)]),
        (
            "sixteen tiles",
            "ABABABABABABABABA\t1\tx\n".into(),
            vec![json!(1)],
        ),
        ("exponent", "BAT\t1e3\tx\n".into(), vec![json!(1)]),
        (
            "thousands separator",
            "BAT\t1,000\tx\n".into(),
            vec![json!(1)],
        ),
        ("NaN", "BAT\tNaN\tx\n".into(), vec![json!(1)]),
        ("Infinity", "BAT\tInfinity\tx\n".into(), vec![json!(1)]),
        ("empty definition", "BAT\t1\t \n".into(), vec![json!(1)]),
        (
            "long definition",
            format!("BAT\t1\t{long_def}\n"),
            vec![json!(1)],
        ),
        (
            "duplicate",
            "BAT\t1\tx\nTAB\t1\tx\nBAT\t2\ty\n".into(),
            vec![json!(3)],
        ),
        ("no words", "\n\n".into(), vec![Value::Null]),
    ];
    for (i, (what, file, lines)) in cases.into_iter().enumerate() {
        let r = a
            .upload(
                "/api/admin/lexicons",
                &[("name", &format!("L{i}")), ("letter_distribution", "en")],
                "x.tsv",
                file.as_bytes(),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}");
        assert_eq!(error_lines(&r), lines, "{what}: {:?}", r.json());
    }
    assert_eq!(count(&app, "lexicons").await, 0);
    assert_eq!(count(&app, "lexicon_words").await, 0);
    // An unknown distribution and a bad name are form errors.
    let r = a
        .upload(
            "/api/admin/lexicons",
            &[("name", "L"), ("letter_distribution", "nope")],
            "x.tsv",
            b"BAT\t1\tx\n",
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = a
        .upload(
            "/api/admin/lexicons",
            &[("name", "bad name"), ("letter_distribution", "en")],
            "x.tsv",
            b"BAT\t1\tx\n",
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = a
        .upload(
            "/api/admin/lexicons",
            &[("name", "OK"), ("letter_distribution", "en")],
            "x.tsv",
            b"BAT\t1\tx\nTAB\t2.5\ty\n",
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.json());
    assert_eq!(r.json()["word_count"], 2);
    let r = a
        .upload(
            "/api/admin/lexicons",
            &[("name", "OK"), ("letter_distribution", "en")],
            "x.tsv",
            b"BAT\t1\tx\n",
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "the name must be unused");
}

#[sqlx::test]
async fn leave_upload_rules(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("ADMIN_UPLOAD_RATE_PER_MINUTE", "1000")]).await;
    let mut a = admin(&app).await;
    a.upload(
        "/api/admin/letter-distributions",
        &[("name", "en")],
        "en.csv",
        ENGLISH.as_bytes(),
    )
    .await;
    a.upload(
        "/api/admin/lexicons",
        &[("name", "LX"), ("letter_distribution", "en")],
        "x.tsv",
        b"BAT\t1\tx\n",
    )
    .await;
    let cases: Vec<(&str, &str, Vec<Value>)> = vec![
        ("fields", "AB\n", vec![json!(1)]),
        ("seven tiles", "AAAABBB,1\n", vec![json!(1)]),
        ("over the bag", "XX,1\n", vec![json!(1)]),
        ("three blanks", "???,1\n", vec![json!(1)]),
        (
            "duplicate after canonical order",
            "SAT,1\nTAS,2\n",
            vec![json!(2)],
        ),
        ("too large", "A,1000000.001\n", vec![json!(1)]),
        ("not a number", "A,-\n", vec![json!(1)]),
        ("lower-case", "a,1\n", vec![json!(1)]),
        ("no leaves", " \n", vec![Value::Null]),
    ];
    for (what, file, lines) in cases {
        let r = a
            .upload(
                "/api/admin/leave-sets",
                &[("lexicon", "LX")],
                "x.csv",
                file.as_bytes(),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}");
        assert_eq!(error_lines(&r), lines, "{what}: {:?}", r.json());
    }
    assert_eq!(count(&app, "leave_sets").await, 0);
    let r = a
        .upload(
            "/api/admin/leave-sets",
            &[("lexicon", "LX")],
            "x.csv",
            b"SE?,1\n?,-1000000\n",
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.json());
    let leaves: Vec<String> = sqlx::query_scalar("SELECT leave FROM leave_values ORDER BY leave")
        .fetch_all(app.db())
        .await
        .unwrap();
    assert_eq!(
        leaves,
        ["?", "?ES"],
        "stored in canonical order, blank first"
    );
    // The lexicon must not already have leave values.
    let r = a
        .upload(
            "/api/admin/leave-sets",
            &[("lexicon", "LX")],
            "x.csv",
            b"A,1\n",
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn many_errors_report_the_first_thousand_and_the_total(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut a = admin(&app).await;
    let mut file = String::from("?,?,2,0,0\n");
    for _ in 0..2500 {
        file.push_str("A\n");
    }
    let r = a
        .upload(
            "/api/admin/letter-distributions",
            &[("name", "many")],
            "x.csv",
            file.as_bytes(),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let body = r.json();
    assert_eq!(body["errors"].as_array().unwrap().len(), 1000);
    assert_eq!(body["total_errors"], 2500);
    assert_eq!(body["errors"][0]["line"], 2);
    assert_eq!(body["errors"][999]["line"], 1001);
}

#[sqlx::test]
async fn uploads_over_the_size_limit_are_refused(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("ADMIN_UPLOAD_MAX_BYTES", "1000")]).await;
    let mut a = admin(&app).await;
    let big = format!("?,?,2,0,0\n{}", "A,a,1,1,1\n".repeat(200));
    let r = a
        .upload(
            "/api/admin/letter-distributions",
            &[("name", "big")],
            "x.csv",
            big.as_bytes(),
        )
        .await;
    assert!(
        r.status == StatusCode::BAD_REQUEST || r.status == StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        r.status
    );
    assert_eq!(count(&app, "letter_distributions").await, 0);
}

/// "Every letter distribution file in MAGPIE-DATA, fetched at a pinned commit,
/// uploads unchanged and produces the expected tiles."
#[sqlx::test]
async fn every_magpie_data_distribution_uploads_unchanged(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("ADMIN_UPLOAD_RATE_PER_MINUTE", "1000")]).await;
    let mut a = admin(&app).await;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/magpie-data");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "csv"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 12);
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        let r = a
            .upload("/api/admin/letter-distributions", &[], filename, &bytes)
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{filename}: {:?}", r.json());
        let name = filename.strip_suffix(".csv").unwrap();
        // The expected tiles, read naively from the file.
        let expected: Vec<Value> = String::from_utf8(bytes)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let f: Vec<&str> = l.split(',').collect();
                json!({ "letter": f[0], "blank_letter": f[1], "count": f[2].parse::<i64>().unwrap(),
                        "value": f[3].parse::<i64>().unwrap(), "is_vowel": f[4] == "1" })
            })
            .collect();
        let got = a.get(&format!("/api/letter-distributions/{name}")).await;
        assert_eq!(got.status, StatusCode::OK);
        assert_eq!(got.json()["tiles"], Value::Array(expected), "{filename}");
    }
}

// ---------------------------------------------------------------------------
// Loading changes into running servers
// ---------------------------------------------------------------------------

async fn eventually<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// "uploads followed by catalog reload across two in-process app instances
/// sharing one database (`NOTIFY`), and the reconcile fallback".
#[sqlx::test]
async fn uploads_reach_every_instance_by_notify(pool: sqlx::PgPool) {
    let one = TestApp::new(pool.clone()).await;
    let two = TestApp::new(pool).await;
    one.seed_fixture_catalog("root").await;
    eventually("instance two to load EN-FIX by NOTIFY", || async {
        let s = two.state.catalog.snapshot();
        s.lexicon_by_name("EN-FIX").is_some()
            && s.lexicon_by_name("CA-FIX").is_some()
            && s.leave_sets.len() == 2
    })
    .await;
    // Deletion reaches it too, and its status rows go at once.
    let mut a = one.client();
    a.cookies = one.signed_in("root2").await.cookies.clone();
    let root2 = a.get("/api/auth/me").await.json()["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    a.user_id = Some(root2);
    one.make_admin("root2").await;
    let id: i32 = sqlx::query_scalar("SELECT id FROM leave_sets ORDER BY id LIMIT 1")
        .fetch_one(one.db())
        .await
        .unwrap();
    let r = a.delete_empty(&format!("/api/admin/leave-sets/{id}")).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    eventually("both instances to drop the leave set", || async {
        !one.state.catalog.snapshot().leave_sets.contains_key(&id)
            && !two.state.catalog.snapshot().leave_sets.contains_key(&id)
    })
    .await;
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM catalog_instance_status WHERE item_kind = 'leave_set' AND item_id = $1",
    )
    .bind(id)
    .fetch_one(one.db())
    .await
    .unwrap();
    assert_eq!(rows, 0);
}

#[sqlx::test]
async fn the_reconcile_fallback_catches_a_missed_notification(pool: sqlx::PgPool) {
    let one = TestApp::new(pool.clone()).await;
    // A second instance whose LISTEN never runs: only reconcile can load it.
    let two = TestApp::new_unready(pool).await;
    wordfall::catalog::reconcile(&two.state).await.unwrap();
    two.state
        .catalog_ready
        .store(true, std::sync::atomic::Ordering::Release);
    one.seed_fixture_catalog("root").await;
    assert!(
        two.state
            .catalog
            .snapshot()
            .lexicon_by_name("EN-FIX")
            .is_none()
    );
    wordfall::catalog::reconcile(&two.state).await.unwrap();
    assert!(
        two.state
            .catalog
            .snapshot()
            .lexicon_by_name("EN-FIX")
            .is_some()
    );
}

/// "two app instances booted at the same time against an empty database both
/// reaching ready, with the migrations applied once".
#[sqlx::test(migrations = false)]
async fn two_instances_start_together_on_an_empty_database(pool: sqlx::PgPool) {
    let (a, b) = tokio::join!(
        wordfall::app::MIGRATOR.run(&pool),
        wordfall::app::MIGRATOR.run(&pool)
    );
    a.unwrap();
    b.unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, wordfall::app::MIGRATOR.iter().count() as i64);
    let (one, two) = tokio::join!(TestApp::new(pool.clone()), TestApp::new(pool.clone()));
    assert_eq!(one.get("/health").await.status, StatusCode::OK);
    assert_eq!(two.get("/health").await.status, StatusCode::OK);
}

/// PLAN.md § Admin → Loading changes: "/health reports ready only once every
/// catalog item in the database has been indexed at startup".
#[sqlx::test]
async fn health_waits_for_the_startup_load(pool: sqlx::PgPool) {
    let one = TestApp::new(pool.clone()).await;
    one.seed_fixture_catalog("root").await;
    let two = TestApp::new_unready(pool).await;
    assert_eq!(
        two.get("/health").await.status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    wordfall::catalog::startup(&two.state).await.unwrap();
    assert_eq!(two.get("/health").await.status, StatusCode::OK);
    assert_eq!(two.state.catalog.snapshot().lexicons.len(), 3);
}

// ---------------------------------------------------------------------------
// /api/lexicons and /api/letter-distributions/:name
// ---------------------------------------------------------------------------

async fn listed(app: &TestApp) -> Vec<String> {
    let mut c = app.client_from("198.51.100.200");
    let r = c.get("/api/lexicons").await;
    assert_eq!(r.status, StatusCode::OK);
    r.json()
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_owned())
        .collect()
}

/// "`/api/lexicons` omitting an item one of two in-process instances has not
/// loaded, and listing it once both have written `catalog_instance_status`
/// rows, with a stale heartbeat ignored" (PQ-005 for the startup case).
#[sqlx::test]
async fn lexicons_lists_only_items_every_live_instance_loaded(pool: sqlx::PgPool) {
    let one = TestApp::new(pool.clone()).await;
    // A live second instance that has loaded the distributions but not the lexicons.
    let other = uuid::Uuid::new_v4();
    one.seed_fixture_catalog("root").await;
    sqlx::query(
        "INSERT INTO catalog_instance_status (instance_id, item_kind, item_id)
         SELECT $1, 'letter_distribution', id FROM letter_distributions",
    )
    .bind(other)
    .execute(one.db())
    .await
    .unwrap();
    assert!(
        listed(&one).await.is_empty(),
        "the second instance has not loaded them"
    );
    // Once it has, they are listed.
    sqlx::query(
        "INSERT INTO catalog_instance_status (instance_id, item_kind, item_id)
         SELECT $1, 'lexicon', id FROM lexicons",
    )
    .bind(other)
    .execute(one.db())
    .await
    .unwrap();
    assert_eq!(listed(&one).await, ["CA-FIX", "EN-FIX", "EN-FIX-OLD"]);
    // A third instance with a stale heartbeat is ignored.
    let stale = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO catalog_instance_status (instance_id, item_kind, item_id, heartbeat_at)
         VALUES ($1, 'letter_distribution', 1, now() - interval '4 minutes')",
    )
    .bind(stale)
    .execute(one.db())
    .await
    .unwrap();
    assert_eq!(listed(&one).await.len(), 3);
    // The purge task prunes the stale rows and leaves the live ones.
    assert!(wordfall::purge::run_once(&one.state).await.unwrap());
    let stale_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM catalog_instance_status WHERE instance_id = $1")
            .bind(stale)
            .fetch_one(one.db())
            .await
            .unwrap();
    assert_eq!(stale_rows, 0);
    assert_eq!(listed(&one).await.len(), 3);
    // A booting instance writes nothing, so it never hides an item.
    let booting = TestApp::new_unready(one.db().clone()).await;
    let _ = &booting;
    assert_eq!(listed(&one).await.len(), 3);
}

#[sqlx::test]
async fn a_new_instance_lists_items_once_it_turns_ready(pool: sqlx::PgPool) {
    let one = TestApp::new(pool.clone()).await;
    one.seed_fixture_catalog("root").await;
    // A second instance that is live (it has a row) but has not yet loaded
    // CA-FIX, the state an item is in while it is still being built there.
    let two = TestApp::new_unready(pool).await;
    wordfall::catalog::startup(&two.state).await.unwrap();
    let ca: i32 = sqlx::query_scalar("SELECT id::int FROM lexicons WHERE name = 'CA-FIX'")
        .fetch_one(one.db())
        .await
        .unwrap();
    sqlx::query("DELETE FROM catalog_instance_status WHERE instance_id = $1 AND item_kind = 'lexicon' AND item_id = $2")
        .bind(two.state.catalog.instance_id)
        .bind(ca)
        .execute(one.db())
        .await
        .unwrap();
    assert!(!listed(&one).await.contains(&"CA-FIX".to_owned()));
    sqlx::query("INSERT INTO catalog_instance_status (instance_id, item_kind, item_id) VALUES ($1, 'lexicon', $2)")
        .bind(two.state.catalog.instance_id)
        .bind(ca)
        .execute(one.db())
        .await
        .unwrap();
    assert!(listed(&one).await.contains(&"CA-FIX".to_owned()));
}

/// Brute-force maxima over the fixture files, computed without the index.
fn brute_force_maxima() -> HashMap<String, (u64, u64, Option<u64>, Option<u64>)> {
    fn tiles(word: &str, order: &[String]) -> Vec<usize> {
        let mut out = Vec::new();
        let mut chars = word.chars().peekable();
        while let Some(c) = chars.next() {
            let t = if c == '[' {
                let mut s = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    s.push(d);
                }
                s
            } else {
                c.to_string()
            };
            out.push(order.iter().position(|l| *l == t).unwrap());
        }
        out
    }
    let manifest = fixture_manifest();
    let dist_of: HashMap<String, Vec<String>> = manifest
        .iter()
        .filter(|m| m.kind == "distribution")
        .map(|m| {
            let text = std::fs::read_to_string(fixture_path(&m.file)).unwrap();
            (
                m.name.clone(),
                text.lines()
                    .map(|l| l.split(',').next().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect();
    let mut out = HashMap::new();
    for lex in manifest.iter().filter(|m| m.kind == "lexicon") {
        let order = &dist_of[lex.parent.as_ref().unwrap()];
        let text = std::fs::read_to_string(fixture_path(&lex.file)).unwrap();
        let words: Vec<Vec<usize>> = text
            .lines()
            .map(|l| tiles(l.split('\t').next().unwrap(), order))
            .collect();
        let mut by_alpha: HashMap<Vec<usize>, u64> = HashMap::new();
        let mut by_len: HashMap<usize, u64> = HashMap::new();
        for w in &words {
            let mut a = w.clone();
            a.sort();
            *by_alpha.entry(a).or_default() += 1;
            *by_len.entry(w.len()).or_default() += 1;
        }
        let leaves = manifest
            .iter()
            .find(|m| m.kind == "leaves" && m.name == lex.name)
            .map(|m| {
                let text = std::fs::read_to_string(fixture_path(&m.file)).unwrap();
                let mut max_anagrams = 0;
                let mut by_size: HashMap<usize, u64> = HashMap::new();
                for l in text.lines() {
                    let mut t = tiles(l.split(',').next().unwrap(), order);
                    t.sort();
                    *by_size.entry(t.len()).or_default() += 1;
                    if !t.contains(&0) {
                        max_anagrams = max_anagrams.max(by_alpha.get(&t).copied().unwrap_or(0));
                    }
                }
                (max_anagrams, *by_size.values().max().unwrap())
            });
        out.insert(
            lex.name.clone(),
            (
                *by_alpha.values().max().unwrap(),
                *by_len.values().max().unwrap(),
                leaves.map(|l| l.0),
                leaves.map(|l| l.1),
            ),
        );
    }
    out
}

#[sqlx::test]
async fn lexicons_reports_the_four_maxima(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    app.seed_fixture_catalog("root").await;
    let mut c = app.client();
    let r = c.get("/api/lexicons").await;
    let expected = brute_force_maxima();
    let listed = r.json();
    assert_eq!(listed.as_array().unwrap().len(), 3);
    for l in listed.as_array().unwrap() {
        let name = l["name"].as_str().unwrap();
        let (anagrams, order, leave_anagrams, leave_order) = expected[name];
        assert_eq!(l["max_num_anagrams"], anagrams, "{name}");
        assert_eq!(l["max_order_rank"], order, "{name}");
        assert_eq!(l["max_leave_num_anagrams"], json!(leave_anagrams), "{name}");
        assert_eq!(l["max_leave_order_rank"], json!(leave_order), "{name}");
    }
    let en = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "EN-FIX")
        .unwrap();
    assert_eq!(en["letter_distribution"], "english");
    assert_eq!(en["word_count"], 154);
    assert_eq!(en["max_num_anagrams"], 9, "AEINRST");
    let old = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "EN-FIX-OLD")
        .unwrap();
    assert!(old["leave_count"].is_null() && old["max_leave_order_rank"].is_null());
}

#[sqlx::test]
async fn a_distribution_answers_before_its_lexicons_are_indexed(pool: sqlx::PgPool) {
    let one = TestApp::new(pool.clone()).await;
    one.seed_fixture_catalog("root").await;
    let two = TestApp::new_unready(pool).await;
    assert!(two.state.catalog.snapshot().lexicons.is_empty());
    let mut c = two.client();
    let r = c.get("/api/letter-distributions/catalan").await;
    assert_eq!(r.status, StatusCode::OK);
    let body = r.json();
    let letters: Vec<String> = body["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["letter"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(letters[0], "?");
    assert!(letters.contains(&"L·L".to_owned()) && letters.contains(&"NY".to_owned()));
    assert_eq!(
        c.get("/api/letter-distributions/nope").await.status,
        StatusCode::NOT_FOUND
    );
}

/// "The two endpoints that need no session are limited per IP".
#[sqlx::test]
async fn catalog_reads_are_limited_per_ip(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("CATALOG_RATE_PER_MINUTE", "2")]).await;
    app.seed_fixture_catalog("root").await;
    let mut c = app.client_from("198.51.100.1");
    assert_eq!(c.get("/api/lexicons").await.status, StatusCode::OK);
    assert_eq!(
        c.get("/api/letter-distributions/english").await.status,
        StatusCode::OK
    );
    for path in ["/api/lexicons", "/api/letter-distributions/english"] {
        let r = c.get(path).await;
        assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{path}");
        assert!(r.retry_after().is_some());
    }
    let mut other = app.client_from("198.51.100.2");
    assert_eq!(other.get("/api/lexicons").await.status, StatusCode::OK);
    // A signed-in user's per-user limits are a different bucket.
    let mut signed = app.signed_in("user1").await;
    signed.ip = "198.51.100.1".into();
    assert_eq!(signed.get("/api/auth/me").await.status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Admin authorization and the admin upload limit
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn non_admins_get_not_found_on_every_admin_route(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut root = admin(&app).await;
    let mut user = app.signed_in("plain").await;
    let routes = [
        ("GET", "/api/admin/catalog"),
        ("POST", "/api/admin/letter-distributions"),
        ("POST", "/api/admin/lexicons"),
        ("POST", "/api/admin/leave-sets"),
        ("DELETE", "/api/admin/letter-distributions/1"),
        ("DELETE", "/api/admin/lexicons/1"),
        ("DELETE", "/api/admin/leave-sets/1"),
    ];
    for (method, path) in routes {
        let r = match method {
            "GET" => user.get(path).await,
            "POST" => {
                user.upload(path, &[("name", "x")], "x.csv", ENGLISH.as_bytes())
                    .await
            }
            _ => user.delete_empty(path).await,
        };
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    assert_eq!(count(&app, "letter_distributions").await, 0);
    // Revoking is_admin takes effect on the next request.
    assert_eq!(root.get("/api/admin/catalog").await.status, StatusCode::OK);
    sqlx::query("UPDATE users SET is_admin = false WHERE username = 'root'")
        .execute(app.db())
        .await
        .unwrap();
    assert_eq!(
        root.get("/api/admin/catalog").await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn admin_uploads_have_their_own_limit(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("ADMIN_UPLOAD_RATE_PER_MINUTE", "1")]).await;
    let mut a = admin(&app).await;
    let r = a
        .upload(
            "/api/admin/letter-distributions",
            &[("name", "one")],
            "x.csv",
            ENGLISH.as_bytes(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let r = a
        .upload(
            "/api/admin/letter-distributions",
            &[("name", "two")],
            "x.csv",
            ENGLISH.as_bytes(),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.retry_after().is_some());
    assert_eq!(
        a.get("/api/admin/catalog").await.status,
        StatusCode::OK,
        "other buckets untouched"
    );
}

// ---------------------------------------------------------------------------
// Deletion refused while in use
// ---------------------------------------------------------------------------

/// Minimal rows referencing the catalog: a cascade on a lexicon (optionally a
/// leave set), and a spec holding an In Lexicon row.
async fn insert_cascade(
    app: &TestApp,
    lexicon: &str,
    leave_set: Option<i32>,
    in_lexicon: Option<&str>,
) -> uuid::Uuid {
    let user: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(app.db())
        .await
        .unwrap();
    let lex: i16 = sqlx::query_scalar("SELECT id FROM lexicons WHERE name = $1")
        .bind(lexicon)
        .fetch_one(app.db())
        .await
        .unwrap();
    let spec: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO search_specs (user_id, quiz_type) VALUES ($1, $2::quiz_type) RETURNING id",
    )
    .bind(user)
    .bind(if leave_set.is_some() {
        "leave_value"
    } else {
        "anagram"
    })
    .fetch_one(app.db())
    .await
    .unwrap();
    sqlx::query("INSERT INTO search_groups (spec_id, id, parent_id, op, order_in_group) VALUES ($1, 0, NULL, 'and', 0)")
        .bind(spec)
        .execute(app.db())
        .await
        .unwrap();
    if let Some(other) = in_lexicon {
        let other: i16 = sqlx::query_scalar("SELECT id FROM lexicons WHERE name = $1")
            .bind(other)
            .fetch_one(app.db())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO search_conditions (spec_id, position, group_id, order_in_group, condition_type, other_lexicon_id)
             VALUES ($1, 0, 0, 0, 'in_lexicon', $2)",
        )
        .bind(spec)
        .bind(other)
        .execute(app.db())
        .await
        .unwrap();
    }
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO cascades (id, user_id, name, quiz_type, lexicon_id, leave_set_id, spec_id, clear_threshold,
                               options_changed_at, options_seq, options_device_id, question_count, depth, updated_seq)
         VALUES ($1, $2, 'c', $3::quiz_type, $4, $5, $6, 80, now(), 1, gen_random_uuid(), 1, 1, 1)",
    )
    .bind(id)
    .bind(user)
    .bind(if leave_set.is_some() { "leave_value" } else { "anagram" })
    .bind(lex)
    .bind(leave_set)
    .bind(spec)
    .execute(app.db())
    .await
    .unwrap();
    id
}

#[sqlx::test]
async fn deletion_is_refused_for_each_kind_of_reference(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut a = app.seed_fixture_catalog("root").await;
    let id = |sql: &'static str| {
        let db = app.db().clone();
        async move {
            sqlx::query_scalar::<_, i32>(sql)
                .fetch_one(&db)
                .await
                .unwrap()
        }
    };
    let english = id("SELECT id::int FROM letter_distributions WHERE name = 'english'").await;
    let en = id("SELECT id::int FROM lexicons WHERE name = 'EN-FIX'").await;
    let old = id("SELECT id::int FROM lexicons WHERE name = 'EN-FIX-OLD'").await;
    let en_leaves = id("SELECT s.id FROM leave_sets s JOIN lexicons l ON l.id = s.lexicon_id WHERE l.name = 'EN-FIX'").await;

    // A distribution is in use while any lexicon refers to it.
    let r = a
        .delete_empty(&format!("/api/admin/letter-distributions/{english}"))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["lexicons"], json!(["EN-FIX", "EN-FIX-OLD"]));
    // A lexicon is in use while it has leave values.
    let r = a.delete_empty(&format!("/api/admin/lexicons/{en}")).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["leave_set"], true);
    // A leave set is in use while a Leave Value cascade refers to it.
    let cascade = insert_cascade(&app, "EN-FIX", Some(en_leaves), None).await;
    let r = a
        .delete_empty(&format!("/api/admin/leave-sets/{en_leaves}"))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["cascades"], 1);
    // An In Lexicon row in a (live or trashed) cascade's spec pins a lexicon.
    let pinning = insert_cascade(&app, "EN-FIX", None, Some("EN-FIX-OLD")).await;
    sqlx::query("UPDATE cascades SET trashed_at = now() WHERE id = $1")
        .bind(pinning)
        .execute(app.db())
        .await
        .unwrap();
    let r = a.delete_empty(&format!("/api/admin/lexicons/{old}")).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["in_lexicon_cascades"], 1);
    let catalog = a.get("/api/admin/catalog").await.json();
    let old_row = catalog["lexicons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "EN-FIX-OLD")
        .unwrap();
    assert_eq!(old_row["cascade_count"], 1, "/admin counts only cascades");

    // Once unreferenced, deletion is allowed and cascades to the rows.
    // (A purge deletes the cascade's private spec with it.)
    sqlx::query("DELETE FROM cascades")
        .execute(app.db())
        .await
        .unwrap();
    sqlx::query("DELETE FROM search_specs")
        .execute(app.db())
        .await
        .unwrap();
    let _ = cascade;
    assert_eq!(
        a.delete_empty(&format!("/api/admin/leave-sets/{en_leaves}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        a.delete_empty(&format!("/api/admin/lexicons/{old}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let words: i64 = sqlx::query_scalar("SELECT count(*) FROM lexicon_words WHERE lexicon_id = $1")
        .bind(old as i16)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(words, 0);
    assert_eq!(
        a.delete_empty(&format!("/api/admin/lexicons/{old}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

/// "a lexicon named only by a saved search's In Lexicon row deleted without refusal".
#[sqlx::test]
async fn a_saved_search_does_not_pin_a_lexicon(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut a = app.seed_fixture_catalog("root").await;
    let user: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(app.db())
        .await
        .unwrap();
    let spec: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO search_specs (user_id, quiz_type) VALUES ($1, 'anagram') RETURNING id",
    )
    .bind(user)
    .fetch_one(app.db())
    .await
    .unwrap();
    sqlx::query("INSERT INTO search_groups (spec_id, id, parent_id, op, order_in_group) VALUES ($1, 0, NULL, 'and', 0)")
        .bind(spec)
        .execute(app.db())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO search_conditions (spec_id, position, group_id, order_in_group, condition_type, text_value)
         VALUES ($1, 0, 0, 0, 'in_lexicon', 'EN-FIX-OLD')",
    )
    .bind(spec)
    .execute(app.db())
    .await
    .unwrap();
    sqlx::query("INSERT INTO saved_searches (id, user_id, name, spec_id) VALUES (gen_random_uuid(), $1, 's', $2)")
        .bind(user)
        .bind(spec)
        .execute(app.db())
        .await
        .unwrap();
    let old: i16 = sqlx::query_scalar("SELECT id FROM lexicons WHERE name = 'EN-FIX-OLD'")
        .fetch_one(app.db())
        .await
        .unwrap();
    let catalog = a.get("/api/admin/catalog").await.json();
    let row = catalog["lexicons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "EN-FIX-OLD")
        .unwrap();
    assert_eq!(row["cascade_count"], 0);
    assert_eq!(
        a.delete_empty(&format!("/api/admin/lexicons/{old}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn admin_catalog_shows_sizes_uploaders_and_load_status(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut a = app.seed_fixture_catalog("root").await;
    let c = a.get("/api/admin/catalog").await.json();
    assert_eq!(c["letter_distributions"].as_array().unwrap().len(), 2);
    let en = c["lexicons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "EN-FIX")
        .unwrap();
    assert_eq!(en["uploaded_by"], "root");
    assert_eq!(en["word_count"], 154);
    assert_eq!(en["loading"], false);
    assert_eq!(en["loaded_by"], json!([app.state.catalog.instance_id]));
    assert!(en["index_bytes"].as_u64().unwrap() > 0);
    assert_eq!(c["instances"].as_array().unwrap().len(), 1);
}
