//! Search finds and ranks alike on either database: the search tests' boards
//! (`tests/cli/search.rs`), made on each through the store, answer each of their queries, and a
//! few more, with the same tasks in the same order, each score the same to the bit, and the same
//! text for each one's excerpt. A score is SurrealDB's BM25, and a write's words are searched
//! from that write on, a changed text's old ones no longer.

use crate::common::{Fixture, Seed};
use serde_json::json;
use symphony_board::{NewTask, SearchHit, Store};

fn create(store: &Store, slug: &str, title: &str, body: &str) {
    let mut task = NewTask::new(slug, title, "owner");
    task.body = body.into();
    store.create(&task).unwrap();
}

/// The search tests' boards in one, and a task holding what else the analyzer splits and folds.
fn board(store: &Store) {
    for slug in ["other/a", "other/b", "other/c", "other/d", "other/e"] {
        create(store, slug, "Unrelated work", "");
    }
    create(store, "needle", "Exact identifier", "");
    create(store, "z-title", "A needle task", "");
    create(store, "engine/jump", "Jump drive", "The engine hums");
    create(store, "engine/warp", "Warp engine", "");
    store
        .say("other/b", "owner", "the engine room is cold", None)
        .unwrap();
    for n in 0..25 {
        create(store, &format!("topic/{n:02}"), "Matching topic", "");
    }
    create(store, "topic/title", "Crème nebula glow", "nebula body");
    create(store, "topic/body", "Comets", "The orange comet is bright");
    create(store, "topic/thread", "Engines", "");
    store
        .say("topic/thread", "owner", "Quantum drives hum", None)
        .unwrap();
    create(store, "topic/a", "Needle...", "");
    let large = format!("giantneedle {}", "a".repeat(1_048_576));
    create(store, "large/brief", "Large brief", &large);
    create(
        store,
        "tools/folds",
        "Naïve façade in İstanbul — x² and ½ of a1_b",
        "Œuvre, Ærø, Straße, 12 µs, Łódź, сила, 中文 … → it’s \"quoted\" (a/b)",
    );
}

/// The search tests' queries, the cap tests' at their limits, and what the folds task holds.
fn queries() -> Vec<String> {
    let mut queries: Vec<String> = [
        "needle",
        "engine",
        "matching",
        "creme nebula",
        "nebula",
        "orange comet",
        "quantum",
        "",
        "...",
        "absentword",
        "giantneedle",
        "Needle needle",
        "needle absentword",
        "engine/jump",
        "jump drive engine",
        "the",
        "topic/07",
        "naive facade",
        "Naïve FAÇADE",
        "istanbul",
        "İstanbul",
        "x2",
        "x²",
        "1/2",
        "½",
        "a1_b",
        "oeuvre aero strasse",
        "µs",
        "us 12",
        "lodz",
        "сила",
        "中文",
        "it s",
        "quoted a/b",
        "→",
    ]
    .map(String::from)
    .to_vec();
    queries.extend([
        "x".repeat(1024),
        "é".repeat(512),
        ["needle"; 32].join(" "),
        "a1".repeat(16),
        "a!".repeat(16),
        "İ".repeat(16),
    ]);
    queries
}

/// What a search answers: its hits, each score as its bits, and each one's excerpt (its slug,
/// field, and the field's text begun).
pub type Answer = (Vec<(String, u64)>, Vec<(String, String, String)>);

/// What `store` answers `query`.
pub fn answered(store: &Store, query: &str) -> Answer {
    let hits = store.search(query).unwrap();
    let slugs: Vec<&str> = hits.iter().map(|hit| hit.slug.as_str()).collect();
    let sources = store.search_sources(query, &slugs).unwrap();
    let sources = (sources.into_values())
        .map(|found| {
            (
                found.slug,
                found.source,
                found.text.chars().take(80).collect(),
            )
        })
        .collect();
    let hits = (hits.iter())
        .map(|hit| (hit.slug.clone(), hit.score.to_bits()))
        .collect();
    (hits, sources)
}

#[test]
fn each_search_tests_query_finds_and_ranks_alike_on_both_databases() {
    let (surreal, postgres) = (
        Fixture::empty(),
        Fixture::new(Backend::PostgreSql, Seed::default()),
    );
    board(&surreal.store);
    board(&postgres.store);
    for query in queries() {
        let there = answered(&surreal.store, &query);
        assert_eq!(answered(&postgres.store, &query), there, "{query:?}");
    }
    // What SurrealDB answers, as the search tests read it.
    let slugs = |query: &str| -> Vec<String> {
        let hits = surreal.store.search(query).unwrap();
        hits.into_iter().map(|hit| hit.slug).collect()
    };
    // "A needle task" is three terms, "Needle..." four.
    assert_eq!(
        slugs("needle"),
        ["needle", "z-title", "topic/a"].map(String::from)
    );
    assert_eq!(
        slugs("engine"),
        ["engine/jump", "engine/warp", "other/b"].map(String::from)
    );
    assert_eq!(slugs("creme nebula"), ["topic/title"]);
    assert_eq!(slugs("naive facade"), ["tools/folds"]);
    assert_eq!(slugs("oeuvre aero strasse"), ["tools/folds"]);
    assert_eq!(slugs("µs"), ["tools/folds"]);
    assert!(slugs("absentword").is_empty() && slugs("...").is_empty() && slugs("→").is_empty());
}

/// Six briefs scored as SurrealDB 3.3 scores them, by hand: `needle` is in two of six briefs
/// (an empty one among the six), eighteen terms in all.
#[test]
fn a_score_is_surrealdbs_bm25_to_the_bit() {
    let f = Fixture::new(Seed::default());
    let briefs = [
        "needle in the hay needle",
        "the hay",
        "",
        "a needle",
        "...",
        "hay hay hay stack",
    ];
    for (n, brief) in briefs.iter().enumerate() {
        create(&f.store, &format!("b/{n}"), "T", brief);
    }
    let scored = |query: &str| {
        let hits = f.store.search(query).unwrap();
        hits.into_iter()
            .map(|hit| (hit.slug, hit.score))
            .collect::<Vec<_>>()
    };
    let needle = [
        ("b/3".to_string(), 0.654_749_691_486_358_6),
        ("b/0".into(), 0.594_857_513_904_571_5),
    ];
    let before = f.store.database().requests();
    assert_eq!(scored("needle"), needle);
    assert_eq!(
        f.store.database().requests() - before,
        1,
        "a search is one request"
    );
    let before = f.store.database().requests();
    let sources = f.store.search_sources("needle", &["b/0", "b/3"]).unwrap();
    assert_eq!(
        f.store.database().requests() - before,
        1,
        "and its excerpts one more"
    );
    assert_eq!(sources["b/0"].text, "needle in the hay needle");
    // A term the query holds twice counts once; one in half the briefs or more scores nothing.
    assert_eq!(scored("needle needle hay"), [needle[1].clone()]);
    assert_eq!(
        scored("hay").iter().map(|hit| hit.1).collect::<Vec<_>>(),
        [0.0; 3]
    );
    // A title's score is three times its own, a slug's four, an exact slug a million.
    let title = f.store.search("t").unwrap();
    assert_eq!(title.len(), 6);
    assert!(title.iter().all(|hit| hit.score == title[0].score));
    assert_eq!(
        f.store.search("b/3").unwrap()[0],
        SearchHit {
            slug: "b/3".into(),
            score: 1e6
        }
    );
}

/// A text changed is searched by its new words, a task gone by none, a thread line by its own.
#[test]
fn a_writes_words_are_searched_from_that_write_on() {
    let f = Fixture::new(Seed::default());
    create(&f.store, "a/one", "Comet tail", "");
    create(&f.store, "a/two", "Other", "");
    let found = |query: &str| -> Vec<String> {
        let hits = f.store.search(query).unwrap();
        hits.into_iter().map(|hit| hit.slug).collect()
    };
    assert_eq!(found("comet"), ["a/one"]);
    let raw = f.store.raw();
    raw.row("task", "a/one")
        .set(json!({"title": "Nebula tail"}))
        .unwrap();
    assert!(found("comet").is_empty());
    assert_eq!(found("nebula tail"), ["a/one"]);
    raw.row("task", "a/one")
        .set(json!({"status": "ready", "priority": 1}))
        .unwrap();
    assert_eq!(found("nebula"), ["a/one"]);
    f.store
        .say("a/two", "owner", "a comet again", None)
        .unwrap();
    assert_eq!(found("comet"), ["a/two"]);
    raw.row("task", "a/one").delete().unwrap();
    assert!(found("nebula").is_empty());
    assert_eq!(found("tail"), Vec::<String>::new());
}

/// Uncommon Latin letters still match their ASCII spelling. Decomposition must not leave ø
/// behind in ǿ, or change the spelling of ŉ and the long s. Both query directions are checked.
#[test]
fn latin_letters_beyond_extended_a_find_and_rank_alike() {
    let (surreal, postgres) = (
        Fixture::empty(),
        Fixture::new(Backend::PostgreSql, Seed::default()),
    );
    let cases = [
        ("ƒoo", "foo"),
        ("ƀar", "bar"),
        ("ǿresund", "oresund"),
        ("Crème", "creme"),
        ("ǣther", "aether"),
        ("ſalt", "salt"),
        ("ꜳrdvark", "aardvark"),
        ("ﬃeld", "ffield"),
        ("ｆｕｌｌ", "full"),
        ("ɖelta", "delta"),
        ("ȣzel", "ouzel"),
        ("Øresund", "oresund"),
        ("ƑOO", "foo"),
    ];
    for (n, (title, _)) in cases.iter().enumerate() {
        let slug = format!("latin/{n}");
        create(&surreal.store, &slug, title, "");
        create(&postgres.store, &slug, title, "");
    }
    create(&surreal.store, "latin/apostrophe", "ŉame", "");
    create(&postgres.store, "latin/apostrophe", "ŉame", "");
    for (n, (title, ascii)) in cases.iter().enumerate() {
        for query in [title, ascii] {
            let expected = answered(&surreal.store, query);
            assert!(
                expected
                    .0
                    .iter()
                    .any(|(slug, _)| *slug == format!("latin/{n}")),
                "{query:?}"
            );
            assert_eq!(answered(&postgres.store, query), expected, "{query:?}");
        }
    }
    // ascii folding follows tokenization: ŉame becomes one term "'name", while the ASCII
    // query 'name is two terms. These do not match even on SurrealDB.
    for query in ["ŉame", "'name", "name"] {
        let expected = answered(&surreal.store, query);
        assert_eq!(expected.0.len(), usize::from(query == "ŉame"), "{query}");
        assert_eq!(answered(&postgres.store, query), expected, "{query}");
    }
}

/// Compare the analyzer itself across the Latin blocks, including every letter in Extended-B,
/// IPA, phonetic extensions, Extended Additional/C/D/E, the ligatures and fullwidth forms.
/// These ranges are independent of the implementation's exception data.
#[test]
fn the_analyzer_agrees_over_latin_blocks() {
    use symphony_board::{Database, pg::Step};
    let (surreal, postgres) = (
        Fixture::empty(),
        Fixture::new(Backend::PostgreSql, Seed::default()),
    );
    let Database::PostgreSql(pg) = postgres.other_database() else {
        unreachable!()
    };
    // Named Latin alphabetic characters in Unicode 14's blocks, not the implementation's
    // folding table; phonetic Greek/Cyrillic letters in the same blocks are outside this contract.
    for letters in include_str!("latin-letters.txt").lines() {
        let text = letters.chars().map(|c| format!("{c} ")).collect::<String>();
        let there = surreal
            .other_client()
            .query(
                "RETURN search::analyze('task_words', $text);",
                json!({"text": text}),
            )
            .unwrap();
        let param = json!({"text": text}).to_string();
        let here = pg.request(&[Step::with("SELECT coalesce(jsonb_agg(term ORDER BY at), '[]') FROM board_terms($1::jsonb ->> 'text')", &param)]).unwrap();
        let here: serde_json::Value = serde_json::from_str(here[0].value().unwrap()).unwrap();
        assert_eq!(here, there[0], "Latin block {letters}");
    }
}
