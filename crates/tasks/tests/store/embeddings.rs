use super::common::{Fixture, RawWrite, Seed};
use serde_json::json;
use symphony_board::embedding::{Chunk, DIMENSIONS, Embedder};
use symphony_board::{EmbeddingStats, Store};

pub(super) struct Fake {
    model: String,
    inputs: usize,
    broken: bool,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            model: "fake-v1".into(),
            inputs: 0,
            broken: false,
        }
    }
}
fn vector(text: &str) -> Vec<f32> {
    let mut v = vec![0.0; DIMENSIONS];
    if text.contains("secondary") {
        v[0] = 0.8;
        v[1] = 0.6;
    } else {
        v[usize::from(!text.contains("engine"))] = 1.0;
    }
    v
}

#[test]
fn repeated_neighbours_overfetch_until_the_requested_number_of_tasks_is_found() {
    let f = Fixture::new(
        Seed::default()
            .task("a/many", "ready")
            .task("b/one", "ready"),
    );
    f.other_client().write("UPDATE task SET body = $body WHERE slug = 'a/many'; UPDATE task SET body = 'engine secondary' WHERE slug = 'b/one';", json!({"body":"engine\n".repeat(80)})).unwrap();
    let mut fake = Fake::default();
    assert_eq!(catch_up(&f.store, &mut fake).chunks, 83);
    let hits = f
        .store
        .vector_search(&vector("engine"), "fake-v1", None, 2, false)
        .unwrap();
    assert_eq!(
        hits.iter().map(|h| h.slug.as_str()).collect::<Vec<_>>(),
        ["a/many", "b/one"]
    );
    assert!(hits[0].score > hits[1].score);

    // A task-only catch-up can leave a closer old-model task saturating the index.
    fake.model = "fake-v2".into();
    let changed = f
        .store
        .catch_up_embeddings(&mut fake, Some(&["b/one".into()]), 8, &mut |_, _| {})
        .unwrap();
    assert_eq!((changed.sources, changed.chunks), (1, 2));
    assert!(f.store.has_embeddings("fake-v1").unwrap());
    let hits = f
        .store
        .vector_search(&vector("engine"), "fake-v2", None, 1, false)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].slug, "b/one");
    assert!(hits[0].score > 0.79);
}

#[test]
fn briefs_larger_than_the_database_concat_limit_are_kept_in_full() {
    let f = Fixture::empty();
    let mut task = symphony_board::NewTask::new("large/brief", "Large brief", "owner");
    // Over 1 MiB of source text plus 1,001 vectors exceeds the 4 MiB RPC ceiling.
    let line = "a".repeat(1100);
    task.body = format!("{line}\n").repeat(1000);
    f.task_with(task.clone());
    let mut fake = Fake::default();
    let stats = catch_up(&f.store, &mut fake);
    assert_eq!((stats.sources, stats.chunks), (1, 1001));
    let found = f
        .other_client()
        .query(
            "RETURN count(SELECT id FROM task_chunk WHERE text = $line);",
            json!({"line":line}),
        )
        .unwrap();
    assert_eq!(found[0], json!(1000));
    assert_eq!(catch_up(&f.store, &mut fake).sources, 0);
}
impl Embedder for Fake {
    fn model_id(&self) -> &str {
        &self.model
    }
    fn chunks(&self, text: &str) -> Result<Vec<Chunk>, String> {
        Ok(text
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(|s| Chunk {
                text: s.into(),
                tokens: vec![1],
            })
            .collect())
    }
    fn embed(&mut self, chunks: &[Chunk]) -> Result<Vec<Vec<f32>>, String> {
        self.inputs += chunks.len();
        if self.broken {
            return Ok(Vec::new());
        }
        Ok(chunks.iter().map(|c| vector(&c.text)).collect())
    }
    fn query(&mut self, text: &str) -> Result<Vec<f32>, String> {
        Ok(vector(text))
    }
}
fn catch_up(store: &Store, fake: &mut Fake) -> EmbeddingStats {
    store
        .catch_up_embeddings(fake, None, 8, &mut |_, _| {})
        .unwrap()
}

fn maintain(store: &Store, fake: &mut Fake) -> EmbeddingStats {
    store.maintain_embeddings(fake, 8, &mut |_, _| {}).unwrap()
}

#[test]
fn idle_maintenance_uses_one_checkpoint_read_across_children_and_checks_no_sources() {
    let f = Fixture::new(Seed::default().task("cached/task", "ready"));
    f.other_client()
        .write(
            "BEGIN TRANSACTION; REMOVE TABLE task_embedding_checkpoint; UPSERT meta:schema SET version = 7; COMMIT TRANSACTION;",
            json!({}),
        )
        .unwrap();
    let upgraded = f.other();
    let mut fake = Fake::default();
    assert_eq!(maintain(&upgraded, &mut fake).sources, 1);
    let other = f.other();
    let before = other.client().requests();
    let inputs = fake.inputs;
    let idle = maintain(&other, &mut fake);
    assert_eq!((idle.tasks, idle.sources, idle.chunks), (0, 0, 0));
    assert_eq!(fake.inputs, inputs);
    assert_eq!(other.client().requests() - before, 1);
    for batch in [0, 65] {
        assert!(
            other
                .maintain_embeddings(&mut fake, batch, &mut |_, _| {})
                .is_err()
        );
    }
}

#[test]
fn maintenance_detects_message_counters_task_stamps_deletions_and_model_changes() {
    let f = Fixture::new(
        Seed::default()
            .task("cached/task", "ready")
            .session("worker"),
    );
    let mut fake = Fake::default();
    maintain(&f.store, &mut fake);
    f.store
        .say("cached/task", "worker", "engine knowledge", None)
        .unwrap();
    assert_eq!(maintain(&f.store, &mut fake).sources, 1);
    // Independent update stamps also cover writes without a message and multiple writes at one
    // clock instant. Only the changed brief is inferred; the existing message remains cached.
    for stamp in ["updated += 1", "version += 1"] {
        f.other_client()
            .write(
                &format!("UPDATE task SET title = $title, {stamp};"),
                json!({"title": stamp}),
            )
            .unwrap();
        assert_eq!(maintain(&f.store, &mut fake).sources, 1);
    }
    f.task("new/task");
    assert_eq!(maintain(&f.store, &mut fake).sources, 1);
    f.other_client()
        .write("DELETE task WHERE slug = 'new/task';", json!({}))
        .unwrap();
    assert_eq!(maintain(&f.store, &mut fake).sources, 0);
    assert_eq!(
        f.other_client()
            .query(
                "RETURN count(SELECT id FROM task_chunk WHERE slug = 'new/task');",
                json!({})
            )
            .unwrap()[0],
        json!(0)
    );
    fake.model = "fake-v2".into();
    assert_eq!(maintain(&f.store, &mut fake).sources, 2);
    assert_eq!(maintain(&f.store, &mut fake).sources, 0);
}

/// The checkpoint a pass writes is one digest whatever the board holds, and replaces the whole
/// snapshot an older pass kept, which never counts as current.
#[test]
fn the_maintenance_checkpoint_is_one_digest_on_any_board_and_replaces_an_older_snapshot() {
    let seed = (0..300).fold(Seed::default(), |seed, n| {
        seed.task(&format!("area/a-long-task-slug-{n:03}"), "ready")
    });
    let f = Fixture::new(seed);
    // What an older pass kept for this very board, which that pass took as current.
    f.other_client()
        .write(
            "UPSERT task_embedding_checkpoint:board SET snapshot = {model: 'fake-v1',
                message: {last: (SELECT VALUE last FROM ONLY counter:message) ?? 0,
                    ts: (SELECT VALUE ts FROM ONLY counter:message) ?? 0},
                tasks: (SELECT slug, updated, version FROM task ORDER BY slug)};",
            json!({}),
        )
        .unwrap();
    let mut fake = Fake::default();
    assert_eq!(maintain(&f.store, &mut fake).sources, 300);
    let row = f
        .other_client()
        .query(
            "RETURN SELECT * OMIT id FROM ONLY task_embedding_checkpoint:board;",
            json!({}),
        )
        .unwrap()
        .remove(0);
    let digest = row["digest"].as_str().unwrap().to_owned();
    assert_eq!(row, json!({"digest": digest}));
    assert_eq!(digest.len(), 64);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()), "{digest}");
    assert_eq!(maintain(&f.store, &mut fake).sources, 0);
}

#[test]
fn failed_and_racing_passes_leave_work_visible_to_the_next_maintenance_child() {
    let f = Fixture::new(Seed::default().task("cached/task", "ready"));
    let mut fake = Fake {
        broken: true,
        ..Default::default()
    };
    assert!(
        f.store
            .maintain_embeddings(&mut fake, 8, &mut |_, _| {})
            .is_err()
    );
    fake.broken = false;
    let stats = f
        .store
        .maintain_embeddings(&mut fake, 8, &mut |_, _| {
            f.other_client()
                .write(
                    "UPDATE task SET title = 'racing edit', version += 1;",
                    json!({}),
                )
                .unwrap();
        })
        .unwrap();
    assert_eq!(stats.sources, 1);
    assert_eq!(maintain(&f.store, &mut fake).sources, 1);
    assert_eq!(maintain(&f.store, &mut fake).sources, 0);
}

#[test]
fn a_partial_catch_up_cannot_mark_the_whole_board_current() {
    let f = Fixture::new(
        Seed::default()
            .task("one/task", "ready")
            .task("two/task", "ready"),
    );
    let mut fake = Fake::default();
    assert_eq!(
        f.store
            .catch_up_embeddings(&mut fake, Some(&["one/task".into()]), 8, &mut |_, _| {})
            .unwrap()
            .sources,
        1
    );
    assert_eq!(maintain(&f.store, &mut fake).sources, 1);
    // A board copy replaces derived caches even if its task stamps and message counter agree.
    let dump = f.store.dump().unwrap();
    f.store.copy(&dump).unwrap();
    assert_eq!(maintain(&f.store, &mut fake).sources, 2);
    // Explicit work under another model invalidates a successful maintenance checkpoint too.
    fake.model = "fake-v2".into();
    assert_eq!(
        f.store
            .catch_up_embeddings(&mut fake, Some(&["one/task".into()]), 8, &mut |_, _| {})
            .unwrap()
            .sources,
        1
    );
    fake.model = "fake-v1".into();
    assert_eq!(maintain(&f.store, &mut fake).sources, 1);
}

#[test]
fn maintenance_finds_tasks_and_messages_created_after_the_previous_pass() {
    let f = Fixture::new(Seed::default().task("old/task", "ready").session("worker"));
    let mut fake = Fake::default();
    catch_up(&f.store, &mut fake);
    let before = fake.inputs;
    f.store
        .create(&symphony_board::NewTask::new(
            "new/task",
            "engine new task",
            "worker",
        ))
        .unwrap();
    let task = catch_up(&f.store, &mut fake);
    assert_eq!((task.tasks, task.sources, task.chunks), (1, 1, 1));
    assert_eq!(fake.inputs, before + 1);
    let hits = f
        .store
        .vector_search(&vector("engine"), "fake-v1", None, 1, false)
        .unwrap();
    assert_eq!(hits[0].slug, "new/task");
    f.store
        .say("new/task", "worker", "secondary knowledge", None)
        .unwrap();
    let message = catch_up(&f.store, &mut fake);
    assert_eq!((message.tasks, message.sources, message.chunks), (1, 1, 1));
    let hits = f
        .store
        .vector_search(&vector("secondary"), "fake-v1", None, 1, false)
        .unwrap();
    assert_eq!(hits[0].slug, "new/task");
    assert_eq!(hits[0].source, "message");
    assert_eq!(catch_up(&f.store, &mut fake).chunks, 0);
}

#[test]
fn deleted_or_malformed_message_sources_cannot_reappear_from_cached_neighbours() {
    let f = Fixture::new(Seed::default().task("sim/a", "ready").session("worker"));
    let message = f.store.say("sim/a", "worker", "engine", None).unwrap();
    catch_up(&f.store, &mut Fake::default());
    f.other_client()
        .write(
            "DELETE message WHERE sid = $sid;",
            json!({"sid":message.id}),
        )
        .unwrap();
    let check = || {
        let hits = f
            .store
            .vector_search(&vector("engine"), "fake-v1", None, 1, false)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "body");
        assert_eq!(hits[0].score, 0.0);
    };
    check();
    f.other_client()
        .write(
            "UPDATE task_chunk SET source = 'message:not-a-number' WHERE source != 'brief';",
            json!({}),
        )
        .unwrap();
    check();
}

#[test]
fn a_deleted_task_with_surviving_messages_cannot_consume_a_vector_result() {
    let f = Fixture::new(
        Seed::default()
            .task("a/deleted", "ready")
            .task("b/remaining", "ready")
            .session("worker"),
    );
    f.store.say("a/deleted", "worker", "engine", None).unwrap();
    catch_up(&f.store, &mut Fake::default());
    f.other_client()
        .write("DELETE type::record('task', 'a/deleted');", json!({}))
        .unwrap();
    let hits = f
        .store
        .vector_search(&vector("engine"), "fake-v1", None, 1, false)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].slug, "b/remaining");
    assert_eq!(hits[0].score, 0.0);
}

#[test]
fn changed_source_hash_is_stale_unchanged_is_not_and_new_model_reembeds_every_source() {
    let f = Fixture::new(Seed::default().task("sim/a", "ready").session("worker"));
    f.store
        .say(
            "sim/a",
            "worker",
            "engine knowledge only in a message",
            None,
        )
        .unwrap();
    let mut fake = Fake::default();
    let initial = catch_up(&f.store, &mut fake);
    assert_eq!((initial.tasks, initial.sources, initial.chunks), (1, 2, 2));
    let inputs = fake.inputs;
    assert_eq!(catch_up(&f.store, &mut fake).sources, 0);
    assert_eq!(fake.inputs, inputs);
    f.other_client()
        .write("UPDATE task SET body = 'engine changed';", json!({}))
        .unwrap();
    let hits = f
        .store
        .vector_search(&vector("other"), "fake-v1", None, 5, false)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].source, "message"); // obsolete brief is excluded even before catch-up
    assert_eq!(hits[0].score, 0.0); // the stale brief would be an exact 1.0 match for this query
    let changed = catch_up(&f.store, &mut fake);
    assert_eq!((changed.sources, changed.chunks), (1, 2));
    assert_eq!(fake.inputs, inputs + 2);
    fake.model = "fake-v2".into();
    assert_eq!(catch_up(&f.store, &mut fake).sources, 2);
    assert!(!f.store.has_embeddings("fake-v1").unwrap());
    assert!(f.store.has_embeddings("fake-v2").unwrap());
    assert_eq!(catch_up(&f.store, &mut fake).sources, 0);
    f.other_client()
        .write("UPDATE task SET title = 'new title';", json!({}))
        .unwrap();
    fake.broken = true;
    assert!(
        f.store
            .catch_up_embeddings(&mut fake, None, 8, &mut |_, _| {})
            .unwrap_err()
            .to_string()
            .contains("number of vectors")
    );
    assert_eq!(
        f.other_client()
            .query(
                "RETURN count(SELECT id FROM task_chunk WHERE source = 'brief');",
                json!({})
            )
            .unwrap()[0],
        json!(2)
    );
    fake.broken = false;
    assert_eq!(catch_up(&f.store, &mut fake).sources, 1);
}

#[test]
fn neighbours_fold_chunks_into_tasks_and_filter_status_before_the_task_limit() {
    let f = Fixture::new(
        Seed::default()
            .task("a/done", "done")
            .task("b/ready", "ready")
            .task("c/ready", "ready")
            .session("worker"),
    );
    f.other_client()
        .write(
            "UPDATE task SET body = 'engine one\nengine two\nengine three';",
            json!({}),
        )
        .unwrap();
    f.store
        .say("b/ready", "worker", "engine thread", None)
        .unwrap();
    let mut fake = Fake::default();
    assert_eq!(catch_up(&f.store, &mut fake).sources, 4);
    for exact in [false, true] {
        let hits = f
            .store
            .vector_search(&vector("engine"), "fake-v1", None, 3, exact)
            .unwrap();
        assert_eq!(
            hits.iter().map(|h| h.slug.as_str()).collect::<Vec<_>>(),
            ["a/done", "b/ready", "c/ready"]
        );
        let filtered = f
            .store
            .vector_search(&vector("engine"), "fake-v1", Some("ready"), 1, exact)
            .unwrap();
        assert_eq!(
            filtered.iter().map(|h| h.slug.as_str()).collect::<Vec<_>>(),
            ["b/ready"]
        );
    }
    // Moving a task needs no re-embedding and immediately changes vector eligibility.
    f.other_client()
        .write(
            "UPDATE task SET status = 'blocked' WHERE slug = 'b/ready';",
            json!({}),
        )
        .unwrap();
    assert_eq!(
        f.store
            .vector_search(&vector("engine"), "fake-v1", Some("ready"), 1, false)
            .unwrap()[0]
            .slug,
        "c/ready"
    );
    assert!(
        f.store
            .vector_search(&vector("engine"), "unknown", None, 3, false)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn only_selected_tasks_are_embedded_deleted_sources_are_removed_and_bad_vectors_are_refused() {
    let f = Fixture::new(
        Seed::default()
            .task("sim/a", "ready")
            .task("sim/b", "ready"),
    );
    let mut fake = Fake::default();
    let stats = f
        .store
        .catch_up_embeddings(&mut fake, Some(&["sim/a".into()]), 1, &mut |_, _| {})
        .unwrap();
    assert_eq!((stats.tasks, stats.sources), (1, 1));
    f.other_client()
        .write("DELETE task_chunk;", json!({}))
        .unwrap();
    assert_eq!(
        f.other_client()
            .query("RETURN count(SELECT id FROM task_chunk);", json!({}))
            .unwrap()[0],
        json!(0)
    );
    let repaired = f
        .store
        .catch_up_embeddings(&mut fake, Some(&["sim/a".into()]), 1, &mut |_, _| {})
        .unwrap();
    assert_eq!((repaired.sources, repaired.chunks), (1, 1));
    assert_eq!(
        f.store
            .vector_search(&vector("other"), "fake-v1", None, 5, false)
            .unwrap()[0]
            .slug,
        "sim/a"
    );
    f.other_client()
        .write("DELETE task WHERE slug = 'sim/a';", json!({}))
        .unwrap();
    catch_up(&f.store, &mut fake);
    let rows = f
        .other_client()
        .query("SELECT slug FROM task_chunk;", json!({}))
        .unwrap();
    assert_eq!(rows[0], json!([{"slug":"sim/b"}]));
    let wrong_length = f
        .other_client()
        .write("UPDATE task_chunk SET embedding = [1.0, 0.0];", json!({}))
        .unwrap_err()
        .to_string();
    assert!(wrong_length.contains("384"), "{wrong_length}");
    f.other_client()
        .write("UPDATE task SET title = '', body = '';", json!({}))
        .unwrap();
    let empty = catch_up(&f.store, &mut fake);
    assert_eq!((empty.sources, empty.chunks), (1, 0));
    assert_eq!(catch_up(&f.store, &mut fake).sources, 0);
    assert!(!f.store.has_embeddings("fake-v1").unwrap());
    for bad in [
        vec![1.0; 3],
        vec![f32::NAN; DIMENSIONS],
        vec![0.0; DIMENSIONS],
    ] {
        assert!(
            f.store
                .vector_search(&bad, "fake-v1", None, 1, false)
                .is_err()
        );
    }
    assert!(
        f.store
            .catch_up_embeddings(&mut fake, None, 0, &mut |_, _| {})
            .is_err()
    );
}
