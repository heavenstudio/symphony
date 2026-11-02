//! Search uses the real analyzer on an isolated board, with independent expected rows and fields.

use crate::{Case, Ran};
use serde_json::Value;
use tasks::NewTask;

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

#[test]
fn embedding_maintenance_validates_session_batch_size_and_task_before_loading_a_model() {
    let c = Case::new();
    let missing = c.run_as("", &["embed"], "");
    assert_eq!(missing.code, 2);
    assert!(missing.err.contains("no session"));
    let actor = c.register("worker");
    for size in ["0", "65", "-1"] {
        let ran = c.run_as(&actor, &["embed", "--batch-size", size], "");
        assert_eq!(ran.code, 1);
        assert!(ran.err.contains("--batch-size must be between 1 and 64"));
        let threads = c.run_as(&actor, &["embed", "--threads", size], "");
        assert_eq!(threads.code, 1);
        assert!(threads.err.contains("--threads must be between 1 and 64"));
    }
    let ran = c.run_as(&actor, &["embed", "--task", "missing/task"], "");
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains("no task 'missing/task'"), "{}", ran.err);
}

#[test]
fn embedding_maintenance_reuses_current_chunks_without_loading_a_model_or_requiring_an_agent() {
    use tasks::embedding::{DIMENSIONS, MODEL_ID, REVISION, text_hash};
    let mut c = Case::new();
    let root = crate::scratch("embedding-maintenance");
    c.env.cwd = crate::repository(&root);
    // A regression that loads the runtime fails its checksum before it can use the network.
    let cache = root.join(".git/sf-task-models").join(REVISION);
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("model.onnx"),
        "model loading is forbidden in this test",
    )
    .unwrap();
    create(&c, "cached/task", "Engine task", "", 2);
    let mut vector = vec![0.0_f32; DIMENSIONS];
    vector[0] = 1.0;
    c.store().client().query(
        "CREATE type::record('task_chunk', ['cached/task', 'brief', 0]) SET slug = 'cached/task', source = 'brief', source_hash = $hash, text_hash = $chunk_hash, text = 'Engine task', model = $model, embedding = $vector; CREATE type::record('task_embedding_source', ['cached/task', 'brief']) SET slug = 'cached/task', source = 'brief', source_hash = $hash, model = $model, parts = 1;",
        serde_json::json!({"hash":text_hash("Engine task\n"),"chunk_hash":text_hash("Engine task"),"model":MODEL_ID,"vector":vector}),
    ).unwrap();
    for _ in 0..2 {
        let ran = c.run_as("", &["embed", "--maintenance"], "");
        assert_eq!(ran.code, 0, "{}", said(&ran));
        assert!(
            ran.out
                .contains("embedded 0 tasks, 0 changed sources, 0 chunks"),
            "{}",
            said(&ran)
        );
        assert!(!cache.join("tokenizer.json").exists());
    }
    let store = c.store();
    let key = text_hash(&format!(
        "{}\n{}\n{}",
        store.client().addr(),
        tasks::NAMESPACE,
        store.client().database()
    ));
    let writer = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(
            root.join(".git/sf-task-models")
                .join(format!("maintenance-{key}.lock")),
        )
        .unwrap();
    writer.lock().unwrap();
    let busy = c.run_as("", &["embed", "--maintenance"], "");
    assert_eq!(busy.code, 0, "{}", said(&busy));
    assert!(
        busy.out.contains("an embedding pass is already running"),
        "{}",
        said(&busy)
    );
    let actor = c.register("worker");
    let refused = c.run_as(&actor, &["embed"], "");
    assert_eq!(refused.code, 1, "{}", said(&refused));
    assert!(
        refused
            .err
            .contains("another embedding pass is already running"),
        "{}",
        said(&refused)
    );
    // Unlocked, not only closed: a child another test forked shares the lock until it execs.
    writer.unlock().unwrap();
    drop(writer);
    let owner = c.run_as("owner", &["embed", "--maintenance"], "");
    assert_eq!(owner.code, 1, "{}", said(&owner));
    assert!(
        owner.err.contains("the owner acts from the Symphony"),
        "{}",
        said(&owner)
    );
    create(&c, "new/task", "A new task", "new knowledge", 2);
    let selected = c.run_as("", &["embed", "--maintenance", "--task", "cached/task"], "");
    assert_eq!(selected.code, 0, "{}", said(&selected));
    assert!(
        selected
            .out
            .contains("embedded 0 tasks, 0 changed sources, 0 chunks"),
        "{}",
        said(&selected)
    );
    let changed = c.run_as("", &["embed", "--maintenance"], "");
    assert_eq!(changed.code, 2, "{}", said(&changed));
    assert!(
        changed.err.contains("SHA-256 mismatch"),
        "{}",
        said(&changed)
    );
    assert!(!cache.join("tokenizer.json").exists());
    std::fs::remove_dir_all(root).unwrap();
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
    // No repository/model cache is available in this fixture: fallback must stay fully local
    // to the keyword store, preserve every JSON field and never try to load/download a model.
    assert_eq!(json(&c, &["search", "needle", "--mode", "hybrid"]), rows);
    assert_eq!(json(&c, &["search", "needle", "--mode", "keyword"]), rows);
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
    c.ok(&actor, &["move", "needle", "blocked"]);
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
        for mode in ["keyword", "vector", "hybrid", "exact"] {
            for json in [false, true] {
                let mut args = vec!["search", &query, "--mode", mode];
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
}

#[test]
fn search_help_names_query_limits_and_every_status_without_a_board() {
    let ran = crate::run(&tasks::cli::Env::default(), &["search", "--help"], "");
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
