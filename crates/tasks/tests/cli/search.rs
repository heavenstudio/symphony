//! Search uses the real analyzer on an isolated board, with independent expected rows and fields.

use crate::{Case, Ran};
use serde_json::Value;
use symphony_board::NewTask;

/// What a run printed, for an assertion's message.
fn said(ran: &Ran) -> String {
    format!(
        "exit {}\nstdout:\n{}\nstderr:\n{}",
        ran.code, ran.out, ran.err
    )
}

fn create(c: &Case, slug: &str, title: &str, body: &str, priority: i64) {
    let mut task = NewTask::new(slug, title, "owner");
    task.body = body.into();
    task.priority = priority;
    c.store().create(&task).unwrap();
}

fn filler(c: &Case) {
    for slug in ["other/a", "other/b", "other/c", "other/d", "other/e"] {
        create(c, slug, "Unrelated work", "", 2);
    }
}

fn json(c: &Case, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    serde_json::from_str(&c.ok("owner", &args)).unwrap()
}

#[test]
fn search_keeps_relevance_order_and_list_fields_and_filters_before_limiting() {
    let c = Case::new();
    filler(&c);
    create(&c, "needle", "Exact identifier", "", 4);
    create(&c, "z-title", "A needle task", "", 0);
    let reserved = c.register("worker");
    c.store().reserve("z-title", &reserved, 60.0).unwrap();
    let rows = json(&c, &["search", "needle"]);
    let rows = rows["tasks"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["slug"], "needle");
    assert_eq!(rows[1]["slug"], "z-title");
    assert!(rows[0]["score"].as_f64().unwrap() > rows[1]["score"].as_f64().unwrap());
    assert_eq!(rows[0]["snippet_source"], "title");
    assert_eq!(rows[0]["snippet"], "Exact identifier");
    let listed = json(&c, &["list"]);
    for row in rows {
        let mut fields = row.as_object().unwrap().clone();
        for extra in ["score", "snippet", "snippet_source"] {
            assert!(fields.remove(extra).is_some());
        }
        let list_row = listed["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["slug"] == row["slug"])
            .unwrap();
        assert_eq!(&Value::Object(fields), list_row);
    }
    let actor = c.register("worker");
    c.ok(&actor, &["move", "needle", "blocked", "--on", "parked"]);
    let filtered = json(
        &c,
        &["search", "needle", "--status", "ready", "--limit", "1"],
    );
    assert_eq!(filtered["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["tasks"][0]["slug"], "z-title");
    assert_eq!(
        c.ok("owner", &["search", "needle", "--limit", "1"]),
        "p4 blocked     needle                       Exact identifier\n  title: Exact identifier\n"
    );
}

/// The board searches by words alone, the search by meaning it had deleted: what a write adds is
/// found by its words, and the modes and the catch-up of the search by meaning are refused as any
/// unknown word is.
#[test]
fn the_board_searches_by_words_alone_and_refuses_the_deleted_modes() {
    let c = Case::new();
    filler(&c);
    create(&c, "engine/jump", "Jump drive", "The engine hums", 2);
    let first = json(&c, &["search", "engine"]);
    assert_eq!(first["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        c.ok("owner", &["search", "engine"]),
        "p2 ready       engine/jump                  Jump drive\n  body: The engine hums\n"
    );
    // The board searches what later writes wrote.
    create(&c, "engine/warp", "Warp engine", "", 2);
    c.store()
        .say("other/b", "owner", "the engine room is cold", None)
        .unwrap();
    let found = json(&c, &["search", "engine"]);
    let mut slugs: Vec<_> = found["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["slug"].as_str().unwrap())
        .collect();
    symphony_board::order::sort_by(&mut slugs, &Ord::cmp);
    assert_eq!(slugs, ["engine/jump", "engine/warp", "other/b"]);

    // The modes and the catch-up are gone from the command line, refused as any unknown word is.
    for mode in ["keyword", "vector", "hybrid", "exact"] {
        let ran = c.run_as("owner", &["search", "engine", "--mode", mode], "");
        assert_eq!(ran.code, 2, "{}", said(&ran));
        assert!(
            ran.err.contains("unrecognized arguments: --mode"),
            "{}",
            said(&ran)
        );
        assert!(ran.out.is_empty(), "{}", said(&ran));
    }
    let ran = c.run_as("owner", &["embed", "--maintenance"], "");
    assert_eq!(ran.code, 2, "{}", said(&ran));
    assert!(ran.err.contains("no command 'embed'"), "{}", said(&ran));
}

#[test]
fn search_defaults_to_twenty_rows_and_accepts_a_larger_limit() {
    let c = Case::new();
    for n in 0..25 {
        create(&c, &format!("topic/{n:02}"), "Matching topic", "", 2);
    }
    let default = json(&c, &["search", "matching"]);
    assert_eq!(default["tasks"].as_array().unwrap().len(), 20);
    assert_eq!(default["tasks"][0]["slug"], "topic/00");
    assert_eq!(default["tasks"][19]["slug"], "topic/19");
    assert_eq!(
        json(&c, &["search", "matching", "--limit", "30"])["tasks"]
            .as_array()
            .unwrap()
            .len(),
        25
    );
}

#[test]
fn search_excerpts_come_from_matching_titles_bodies_and_thread_messages() {
    let c = Case::new();
    filler(&c);
    create(&c, "topic/title", "Crème nebula glow", "nebula body", 2);
    create(&c, "topic/body", "Comets", "The orange comet is bright", 2);
    create(&c, "topic/thread", "Engines", "", 2);
    c.store()
        .say("topic/thread", "owner", "Quantum drives hum", None)
        .unwrap();
    for (query, slug, source, snippet) in [
        ("creme nebula", "topic/title", "title", "Crème nebula glow"),
        ("nebula", "topic/title", "title", "Crème nebula glow"),
        (
            "orange comet",
            "topic/body",
            "body",
            "The orange comet is bright",
        ),
        ("quantum", "topic/thread", "message", "Quantum drives hum"),
    ] {
        let result = json(&c, &["search", query]);
        let rows = result["tasks"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{query}: {result}");
        assert_eq!(rows[0]["slug"], slug);
        assert_eq!(rows[0]["snippet_source"], source);
        assert_eq!(rows[0]["snippet"], snippet);
    }
    assert_eq!(
        c.ok("owner", &["search", "quantum"]),
        "p2 ready       topic/thread                 Engines\n  message: Quantum drives hum\n"
    );
}

#[test]
fn search_empty_matches_and_bad_limits_have_explicit_results() {
    let c = Case::new();
    // The database can match punctuation in this title unless the empty-term guard runs.
    create(&c, "topic/a", "Needle...", "", 2);
    for query in ["", "...", "absentword"] {
        assert_eq!(
            json(&c, &["search", query]),
            serde_json::json!({"tasks": []})
        );
        assert_eq!(c.ok("owner", &["search", query]), "");
    }
    for limit in ["0", "-1"] {
        let ran = c.run_as("owner", &["search", "needle", "--limit", limit], "");
        assert_eq!(ran.code, 1);
        assert!(ran.out.is_empty());
        assert_eq!(ran.err, "✖ --limit must be a positive integer\n");
    }
    for args in [
        vec!["search"],
        vec!["search", "needle", "--limit", "many"],
        vec!["search", "needle", "--status", "missing"],
    ] {
        let ran = c.run_as("owner", &args, "");
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.err);
        assert!(ran.out.is_empty());
        assert!(ran.err.contains("error:"));
    }
}

#[test]
fn search_excerpts_a_matching_brief_larger_than_one_mebibyte() {
    let c = Case::new();
    create(
        &c,
        "large/brief",
        "Large brief",
        &format!("giantneedle {}", "a".repeat(1_048_576)),
        2,
    );
    let result = json(&c, &["search", "giantneedle"]);
    assert_eq!(result["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(result["tasks"][0]["slug"], "large/brief");
    assert_eq!(result["tasks"][0]["snippet_source"], "body");
    let snippet = result["tasks"][0]["snippet"].as_str().unwrap();
    assert!(snippet.starts_with("giantneedle "));
    assert!(snippet.ends_with('…'));
    assert_eq!(snippet.chars().count(), 201);
}

#[test]
fn search_caps_query_bytes_and_terms_including_analyzer_boundaries() {
    let c = Case::new();
    create(&c, "topic/a", "Needle", "", 2);
    for query in [
        "x".repeat(1024),
        "é".repeat(512),
        ["needle"; 32].join(" "),
        "a1".repeat(16),
        "a!".repeat(16),
        "İ".repeat(16),
    ] {
        let ran = c.run_as("owner", &["search", &query, "--json"], "");
        assert_eq!(ran.code, 0, "{} bytes: {}", query.len(), ran.err);
        assert!(ran.err.is_empty());
        let rows: Value = serde_json::from_str(&ran.out).unwrap();
        assert!(rows["tasks"].is_array());
    }
    for (query, message) in [
        (
            "x".repeat(1025),
            "search query must be at most 1024 UTF-8 bytes",
        ),
        (
            "é".repeat(513),
            "search query must be at most 1024 UTF-8 bytes",
        ),
        (
            ["needle"; 33].join(" "),
            "search query must contain at most 32 terms",
        ),
        (
            "a1".repeat(16) + "a",
            "search query must contain at most 32 terms",
        ),
        (
            "a!".repeat(16) + "a",
            "search query must contain at most 32 terms",
        ),
        (
            "İ".repeat(16) + "a",
            "search query must contain at most 32 terms",
        ),
    ] {
        for json in [false, true] {
            let mut args = vec!["search", &query];
            if json {
                args.push("--json");
            }
            let ran = c.run_as("owner", &args, "");
            assert_eq!(ran.code, 1, "{} bytes: {}", query.len(), ran.err);
            assert!(ran.out.is_empty());
            assert_eq!(ran.err, format!("✖ {message}\n"));
        }
    }
}

#[test]
fn search_help_names_query_limits_and_every_status_without_a_board() {
    let ran = crate::run(
        &symphony_board::cli::Env::default(),
        &["search", "--help"],
        "",
    );
    assert_eq!(ran.code, 0);
    assert!(ran.err.is_empty());
    for text in [
        "positional arguments:\n  query",
        "1024 UTF-8 bytes and 32 terms",
        "--status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}",
    ] {
        assert!(ran.out.contains(text), "missing {text:?}: {}", ran.out);
    }
}

#[test]
fn equally_scored_message_excerpts_choose_text_order_before_insertion_order() {
    let c = Case::new();
    filler(&c);
    create(&c, "topic/ties", "Engines", "", 2);
    for text in ["Nebula omega", "Nebula alpha", "Nebula alpha"] {
        c.store().say("topic/ties", "owner", text, None).unwrap();
    }
    let result = json(&c, &["search", "nebula"]);
    let rows = result["tasks"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["snippet_source"], "message");
    assert_eq!(rows[0]["snippet"], "Nebula alpha");
}
