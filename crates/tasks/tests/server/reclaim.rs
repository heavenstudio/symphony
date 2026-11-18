//! What the board's store takes on disk: a row rewritten in place many times leaves the store about
//! the size of what it holds, and a store that grew under SurrealKV's own value log is rebuilt from
//! an export of itself, with every database, task, thread and ask it held: two databases whose names
//! differ only in case each keep their own, a schema-17 board's asks come through it and the next,
//! and a rollback leaves no server on the store it moves aside.

use crate::children::Child as _;
use crate::common::{Scratch, exists, script};
use serde_json::{Value, json};
use std::path::Path;
use symphony_board::rpc::Client;
use symphony_board::server::{Home, Kept, Running, ServerError, Storage, bytes};
use symphony_board::{NAMESPACE, NewAsk, NewSession, NewTask, Store};

/// A memtable the value-log fixture can fill.
const MEMTABLE: u64 = 2 << 20;

/// Small enough that the bounded-store test flushes and compacts many times over.
const BOUNDED_MEMTABLE: u64 = 1 << 20;

/// The storage the live board's store grew under: SurrealKV's own value-log threshold, and a
/// memtable a test can fill.
const GREW_UNDER: Storage = Storage {
    value_log_threshold: 4096,
    memtable: MEMTABLE,
};

/// The most a store under `BOUNDED_MEMTABLE` may take beside what it holds, whatever compaction is behind:
/// SurrealKV compacts level 0 at 4 files and stalls writes at 12, each at most one memtable, beside
/// the write-ahead logs of the active memtable and of the 2 immutable ones it allows.
const BOUND: u64 = 16 * BOUNDED_MEMTABLE;

/// This fixture's reclaimed store measured 323,584 bytes. Write twice what its fourfold reduction
/// needs, rounded up to whole 64 KiB rewrites; the assertions below still judge the actual sizes.
const RECLAIM_REWRITES: u64 = (323_584_u64 * 4 * 2).div_ceil(65536);

fn board(running: &Running, db: &str) -> Store {
    Store::open(Client::new(&running.addr, "sf", db)).unwrap()
}

/// Rewrite one schemaless 64 KiB row, the size of an orchestrator's callback state, `times` times,
/// each with new random text: the bytes written.
fn rewrite(store: &Store, times: u64) -> u64 {
    for _ in 0..times {
        store
            .client()
            .query(
                "UPSERT callback_state:orchestrator SET seen = string::concat(rand::string(32768), rand::string(32768)) RETURN NONE;",
                json!({}),
            )
            .unwrap();
    }
    times * 65536
}

fn data(home: &Home) -> std::path::PathBuf {
    home.dir().join("data")
}

#[test]
fn a_row_rewritten_many_times_leaves_the_store_the_size_of_what_it_holds() {
    let s = Scratch::new("bounded");
    let home = s.home().storage(Storage {
        memtable: BOUNDED_MEMTABLE,
        ..Storage::BOARD
    });
    let store = board(&home.ensure().unwrap(), "tasks");
    let (mut written, mut most) = (0, 0);
    for _ in 0..16 {
        written += rewrite(&store, 4 * BOUND / 16 / 65536);
        most = most.max(bytes(&data(&home)));
    }
    assert!(written >= 4 * BOUND, "{written} bytes written");
    assert!(
        most <= BOUND,
        "the store took {most} bytes at most after {written} bytes of rewrites"
    );
    assert!(
        bytes(&data(&home).join("vlog")) < 65536,
        "no value went to the value log"
    );
    let seen = (store.raw().row("callback_state", "orchestrator"))
        .field("seen")
        .unwrap();
    let row = [json!(seen.as_str().unwrap().chars().count())];
    assert_eq!(row, [json!(65536)]);
}

/// Everything a board's tables hold, row by row: what a reclaim must carry whole.
fn holdings(store: &Store) -> Value {
    let tables = [
        "task",
        "message",
        "session",
        "role",
        "delivery",
        "counter",
        "callback_state",
        "meta",
        "ask",
    ];
    let rows: Vec<Value> = (tables.iter())
        .map(|table| store.raw().table(table).get().unwrap())
        .collect();
    json!(rows)
}

#[test]
fn a_store_that_grew_under_the_value_log_is_rebuilt_with_every_database_task_and_thread() {
    let s = Scratch::new("reclaim");
    let grown = s.home().storage(GREW_UNDER);
    let before_running = grown.ensure().unwrap();
    let live = board(&before_running, "tasks");
    let scratch = board(&before_running, "scratch_board");
    let worker = live
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    for n in 0..20 {
        let slug = format!("sim/task-{n:02}");
        live.create(&NewTask::new(&slug, &format!("Task {n}"), "owner"))
            .unwrap();
        live.say(&slug, &worker, &format!("note {n}: the brief, read"), None)
            .unwrap();
    }
    live.claim("sim/task-00", &worker).unwrap();
    scratch
        .create(&NewTask::new(
            "probe/scratch",
            "A scratch board's task",
            "owner",
        ))
        .unwrap();
    let written = rewrite(&live, RECLAIM_REWRITES);
    let (held, scratch_held) = (holdings(&live), holdings(&scratch));
    let thread = live.thread("sim/task-07").unwrap();
    let slugs = |store: &Store| -> Vec<String> {
        store
            .search("brief")
            .unwrap()
            .into_iter()
            .map(|hit| hit.slug)
            .collect()
    };
    let found = slugs(&live);
    assert!(!found.is_empty());

    let home = s.home();
    let reclaimed = home.reclaim().unwrap();
    // `before` is the old store at rest, its server ended: what the grown store kept of the writes.
    let before = reclaimed.before;
    eprintln!(
        "reclaim: {} bytes after, {before} before, {written} written",
        reclaimed.after
    );
    assert!(
        before >= written / 2,
        "{before} bytes for {written} written"
    );
    assert_eq!(reclaimed.after, bytes(&data(&home)));
    assert!(
        reclaimed.after * 4 < before,
        "{} bytes after, {before} before",
        reclaimed.after
    );
    assert!(
        reclaimed.databases.iter().any(|db| db == "sf/tasks")
            && reclaimed
                .databases
                .iter()
                .any(|db| db == "sf/scratch_board"),
        "{:?}",
        reclaimed.databases
    );
    assert_eq!(home.recorded(), Some(reclaimed.running.clone()));
    assert_eq!(home.ensure().unwrap(), reclaimed.running);
    assert!(!symphony_board::sys::pid_alive(before_running.pid));
    let (live, scratch) = (
        board(&reclaimed.running, "tasks"),
        board(&reclaimed.running, "scratch_board"),
    );
    assert_eq!(holdings(&live), held);
    assert_eq!(holdings(&scratch), scratch_held);
    assert_eq!(live.thread("sim/task-07").unwrap(), thread);
    assert_eq!(slugs(&live), found);
    let work = home.dir().join("reclaim");
    let exported: Vec<String> = std::fs::read_dir(work.join("exports"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(exported.len(), reclaimed.databases.len(), "{exported:?}");
    assert!(
        exported
            .iter()
            .any(|name| name.ends_with(&format!(".{NAMESPACE}.tasks.surql"))),
        "{exported:?}"
    );
    assert!(!exists(&work.join("old")) && !exists(&work.join("check")));
    // The old store it removed is kept, a snapshot taken before any server of the reclaim started
    // on it, that serves what the board held.
    let kept = reclaimed.kept.path();
    // A clone where the volume clones (APFS, btrfs, XFS), a plain copy where it cannot.
    assert_eq!(
        matches!(reclaimed.kept, Kept::Cloned(_)),
        symphony_board::server::snapshot::clones(&home.snapshots_folder()),
        "{:?}",
        reclaimed.kept
    );
    assert_eq!(kept.parent(), Some(home.snapshots_folder().as_path()));
    assert!(
        home.snapshots().unwrap().iter().any(|one| one.path == kept),
        "{:?}",
        home.snapshots()
    );
    let log = std::fs::read_to_string(home.dir().join("server.log")).unwrap();
    assert!(
        log.contains(&format!(
            "· a snapshot of the store before this reclaim: {}",
            kept.display()
        )),
        "{log}"
    );
    let recovered = served_from(&s, kept, GREW_UNDER);
    assert_eq!(
        holdings(&board(&recovered.ensure().unwrap(), "tasks")),
        held
    );
    recovered.stop().unwrap();
}

/// A board of its own in `s`'s folder, its store a clone of `kept` (never the snapshot itself), its
/// servers under `storage`.
fn served_from(s: &Scratch, kept: &Path, storage: Storage) -> Home {
    let dir = s.dir.join("recovered");
    std::fs::create_dir_all(&dir).unwrap();
    let cloned = symphony_board::sys::output(
        symphony_board::sys::Command::of("cp")
            .args(["-c", "-R"])
            .arg(kept)
            .arg(dir.join("data")),
    )
    .unwrap();
    assert!(cloned.status.success(), "{cloned:?}");
    Home::new(&dir, s.surreal()).storage(storage)
}

/// A store whose server ended uncleanly holds the writes its tables lack in its log, which the
/// reclaim's own server replays and its clean stop removes: the reclaim keeps the store before that
/// server starts, the log byte for byte, on a board whose starts take no snapshot, and the snapshot
/// is kept until the owner removes it. The reclaim carries every row.
#[test]
fn a_reclaim_with_no_server_running_keeps_the_store_and_its_log_before_its_own_server_starts() {
    use symphony_board::sys::pid_alive;
    let s = Scratch::new("reclaim-unclean");
    let running = s.home().ensure().unwrap();
    let store = board(&running, "tasks");
    for n in 0..20 {
        store
            .create(&NewTask::new(
                &format!("sim/kept-{n:02}"),
                "Kept by the log",
                "owner",
            ))
            .unwrap();
    }
    let held = holdings(&store);
    crate::common::signal(running.pid, "KILL");
    assert!(crate::common::gone(running.pid));
    let wal = data(&s.home()).join("wal");
    let logged: Vec<(std::ffi::OsString, Vec<u8>)> = std::fs::read_dir(&wal)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), std::fs::read(entry.path()).unwrap())
        })
        .filter(|(_, bytes)| !bytes.is_empty())
        .collect();
    assert!(!logged.is_empty(), "the kill left no log");

    let home = s.home().snapshotting(false);
    let reclaimed = home.reclaim().unwrap();
    assert!(!pid_alive(running.pid));
    assert_eq!(holdings(&board(&reclaimed.running, "tasks")), held);
    let kept = reclaimed.kept.path();
    for (name, bytes) in &logged {
        assert_eq!(
            &std::fs::read(kept.join("wal").join(name)).unwrap(),
            bytes,
            "{name:?}"
        );
    }
    let snapshots = home.snapshots().unwrap();
    assert_eq!(snapshots.len(), 1, "only the reclaim's: {snapshots:?}");
    assert!(snapshots[0].log, "it holds the log, so it is never pruned");
    assert_eq!(home.kept().0, vec![kept.to_path_buf()]);
    home.stop().unwrap();
}

/// A store that can be neither cloned nor copied is never reclaimed: refused before anything is
/// exported, in the keep's words, and the board's server started again on its store, untouched.
#[test]
fn a_store_that_cannot_be_kept_is_not_reclaimed_and_is_served_again() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("reclaim-unkept");
    let home = s.home().snapshotting(false);
    let running = home.ensure().unwrap();
    board(&running, "tasks")
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    let held = holdings(&board(&running, "tasks"));
    let folder = home.snapshots_folder();
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let error = home.reclaim();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = error.unwrap_err();
    assert!(
        matches!(&error, ServerError::Reclaim(said) if said.starts_with(&format!(
            "the board's store at {} could not be kept before its reclaim: it could not be cloned (",
            data(&home).display()
        )) && said.contains("it was not reclaimed, and nothing was exported or removed")),
        "{error}"
    );
    assert!(!exists(&home.dir().join("reclaim")), "nothing exported");
    assert!(home.snapshots().unwrap().is_empty());
    let again = home.recorded().expect("the board's server runs again");
    assert_ne!(again.pid, running.pid);
    assert_eq!(holdings(&board(&again, "tasks")), held);
    home.stop().unwrap();
}

/// A reclaim `prepare` makes refuse, on a board of database `db`: refused saying each of `why`, the
/// board's server started again on its store, which holds what it held, and no new store beside it.
fn refused_and_served_again(name: &str, db: &str, prepare: impl FnOnce(&Store), why: &[&str]) {
    let s = Scratch::new(name);
    let home = s.home();
    let running = home.ensure().unwrap();
    let store = board(&running, db);
    store
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    prepare(&store);
    let held = holdings(&store);
    let error = home.reclaim().unwrap_err();
    assert!(
        matches!(&error, ServerError::Reclaim(said) if why.iter().all(|part| said.contains(part))),
        "{error}"
    );
    let again = home.recorded().expect("the board's server runs again");
    assert_ne!(again.pid, running.pid);
    assert_eq!(home.ensure().unwrap(), again);
    assert_eq!(holdings(&board(&again, db)), held);
    assert!(!exists(&home.dir().join("reclaim").join("data")));
}

#[test]
fn a_store_holding_a_name_the_board_cannot_say_is_left_as_it_was_and_served_again() {
    let not_plain = |store: &Store| {
        store
            .client()
            .query("DEFINE NAMESPACE `not-plain`;", json!({}))
            .unwrap();
    };
    refused_and_served_again("reclaim-refused", "tasks", not_plain, &["\"not-plain\""]);
}

/// Nothing a reclaim would carry is the board, so it carries nothing and removes nothing.
#[test]
fn a_store_without_the_board_s_own_database_is_left_as_it_was_and_served_again() {
    refused_and_served_again(
        "reclaim-no-board",
        "scratch_board",
        |_| {},
        &[
            "holds no sf/tasks, the board's own database",
            "sf/scratch_board",
        ],
    );
}

/// The board's server on the new store starts and its address cannot be recorded (here a folder
/// takes the address file's place): that server is ended before the stores move back, so the old
/// store's server is the only one, and what the board writes after the rollback stays the board's
/// through a stop and the next start.
#[test]
fn a_replacement_whose_record_fails_is_ended_and_no_task_written_after_the_rollback_is_lost() {
    let s = Scratch::new("reclaim-record");
    let running = s.home().ensure().unwrap();
    board(&running, "tasks")
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    let quoted = |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
    let (old, address) = (s.dir.join("reclaim").join("old"), s.dir.join("server.json"));
    let (failed, replacement) = (s.dir.join("record-failed"), s.dir.join("replacement"));
    // Past the swap (the old store moved aside), the first start makes the address a folder and
    // says its pid, which `exec` keeps; every later start takes the folder away again.
    let surreal = script(
        &s.dir.join("record-fails-surreal"),
        &format!(
            r#"if [ "$1" = start ]; then
    if [ -d {old} ] && [ ! -e {failed} ]; then
        : > {failed}
        mkdir {address}
        printf '%s\n' "$$" > {replacement}
    elif [ -e {failed} ]; then
        rmdir {address} 2>/dev/null || true
    fi
fi
exec {real} "$@""#,
            old = quoted(&old),
            failed = quoted(&failed),
            address = quoted(&address),
            replacement = quoted(&replacement),
            real = quoted(s.surreal()),
        ),
    );
    let home = Home::new(&s.dir, &surreal);
    let error = home.reclaim().unwrap_err();
    assert!(
        matches!(&error, ServerError::Reclaim(said) if said.contains("server.json")),
        "{error}"
    );
    let replacement: i64 = std::fs::read_to_string(&replacement)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        !symphony_board::sys::pid_alive(replacement),
        "the server on the new store, pid {replacement}, still runs"
    );
    let again = home.recorded().expect("the board's server runs again");
    board(&again, "tasks")
        .create(&NewTask::new(
            "sim/after",
            "Written after the rollback",
            "owner",
        ))
        .unwrap();
    home.stop().unwrap();
    let reopened = board(&home.ensure().unwrap(), "tasks");
    for (slug, title) in [
        ("sim/jump", "Jump departure"),
        ("sim/after", "Written after the rollback"),
    ] {
        assert_eq!(reopened.get(slug).unwrap().title, title, "{slug}");
    }
}

/// Two databases whose names differ only in case, on a filesystem that folds case as macOS's
/// does: each comes out of a reclaim interrupted between its moves holding what it held, never
/// the other's. A normal start refuses the interrupted store and never makes an empty one.
#[test]
fn databases_whose_names_differ_only_in_case_each_keep_their_own_tasks_through_a_reclaim() {
    let s = Scratch::new("reclaim-case");
    let home = s.home();
    let running = home.ensure().unwrap();
    board(&running, "tasks")
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    let cases = [
        ("CaseProbe", "The upper-case database's own"),
        ("caseprobe", "The lower-case database's own"),
    ];
    for (db, title) in cases {
        board(&running, db)
            .create(&NewTask::new("probe/own", title, "owner"))
            .unwrap();
    }
    let held = cases.map(|(db, _)| holdings(&board(&running, db)));
    assert_ne!(held[0], held[1]);
    let tasks_held = holdings(&board(&running, "tasks"));
    home.stop().unwrap();
    let old = home.dir().join("reclaim/old");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::rename(data(&home), &old).unwrap();
    let error = home.ensure().unwrap_err();
    assert!(
        error.to_string().contains(&old.display().to_string()),
        "{error}"
    );
    assert!(!exists(&data(&home)), "no empty store was made");
    let reclaimed = home.reclaim().unwrap();
    assert_eq!(holdings(&board(&reclaimed.running, "tasks")), tasks_held);
    for ((db, title), held) in cases.into_iter().zip(held) {
        let after = board(&reclaimed.running, db);
        assert_eq!(after.get("probe/own").unwrap().title, title, "{db}");
        assert_eq!(holdings(&after), held, "{db}");
    }
    assert!(!exists(&old));
}

/// The line SurrealDB 3.3.0 prints for the elements of schema 17's `before ON ask`, an array field
/// named by a SurrealQL word, which a store an import built holds twice (`same_export` in
/// `src/server/reclaim.rs`).
const BEFORE_ELEMENTS: &str =
    "DEFINE FIELD OVERWRITE `before`.* ON ask TYPE object PERMISSIONS FULL;";

/// The export of `sf/tasks` a reclaim of `home` keeps.
fn tasks_export(home: &Home) -> String {
    let exports = home.dir().join("reclaim").join("exports");
    let named: Vec<_> = std::fs::read_dir(&exports)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.to_string_lossy()
                .ends_with(&format!(".{NAMESPACE}.tasks.surql"))
        })
        .collect();
    assert_eq!(named.len(), 1, "{named:?}");
    std::fs::read_to_string(&named[0]).unwrap()
}

/// A schema-17 board's asks, open, joined, held and handed on, answered, closed by hand and gone,
/// come out of a reclaim with every row and their counter; the store it built takes new asks and
/// holders, and is reclaimed again the same, though it holds the definition of `before`'s elements
/// twice, as SurrealDB's import made it.
#[test]
fn a_board_s_asks_held_answered_and_closed_come_through_a_reclaim_and_the_next_whole() {
    let s = Scratch::new("reclaim-asks");
    let home = s.home();
    let store = board(&home.ensure().unwrap(), "tasks");
    let [first, second] =
        ["worker", "worker"].map(|kind| store.register(&NewSession::new(kind)).unwrap().row.handle);
    store
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    let line = |kind: &str, words: &str| NewAsk {
        kind: kind.into(),
        path: Some("docs/DECISIONS.md".into()),
        card: Some("owner-asks".into()),
        block: Some("b-7".into()),
        task: Some("sim/jump".into()),
        thread: Some("chat-7".into()),
        words: words.into(),
        ..NewAsk::default()
    };
    let verdict = store.open_ask(&line("verdict", "why not B?")).unwrap().id;
    assert_eq!(
        store
            .open_ask(&line("message", "and the picture?"))
            .unwrap()
            .id,
        verdict,
        "a message on the card joins its verdict's ask"
    );
    let task = store.open_ask(&line("task", "which port?")).unwrap().id;
    store.hold_ask(task, Some(&first), "routed").unwrap();
    store.hold_ask(task, Some(&second), "handed on").unwrap();
    store.answer_ask(task, Some(&second), "message 12").unwrap();
    let chat = store.open_ask(&line("chat", "still there?")).unwrap().id;
    store.hold_ask(chat, Some(&first), "the chat's").unwrap();
    store.close_ask(chat, &second, "said twice").unwrap();
    let comment = store.open_ask(&line("comment", "this block?")).unwrap().id;
    store.hold_ask(comment, Some(&second), "routed").unwrap();
    store.hold_ask(comment, None, "left").unwrap();
    store.mark_gone(comment, Some("block b-7 is gone")).unwrap();
    let asks = store.asks(Some(0.0)).unwrap();
    assert_eq!(asks.len(), 4, "{asks:?}");
    let handed = store.ask(task).unwrap();
    assert!(
        !handed.open && handed.answered.is_some() && handed.before.len() == 1,
        "{handed:?}"
    );
    assert!(store.ask(chat).unwrap().closed);
    assert!(store.ask(comment).unwrap().gone.is_some());
    let held = holdings(&store);

    let reclaimed = home.reclaim().unwrap();
    let store = board(&reclaimed.running, "tasks");
    assert_eq!(holdings(&store), held);
    assert_eq!(store.asks(Some(0.0)).unwrap(), asks);
    assert_eq!(store.ask(task).unwrap(), handed);
    assert_eq!(tasks_export(&home).matches(BEFORE_ELEMENTS).count(), 1);

    let another_card = NewAsk {
        card: Some("ask-ages".into()),
        ..line("verdict", "and now?")
    };
    let after = store.open_ask(&another_card).unwrap();
    assert_eq!(after.id, 5, "the asks' counter came through: {after:?}");
    store.hold_ask(after.id, Some(&first), "routed").unwrap();
    let after = store
        .hold_ask(after.id, Some(&second), "handed on")
        .unwrap();
    assert_eq!(after.before.len(), 1, "{after:?}");
    let held = holdings(&store);

    let again = home.reclaim().unwrap();
    let store = board(&again.running, "tasks");
    assert_eq!(holdings(&store), held);
    assert_eq!(store.ask(after.id).unwrap(), after);
    assert_eq!(
        tasks_export(&home).matches(BEFORE_ELEMENTS).count(),
        2,
        "the store the first reclaim built held SurrealDB's repeat of `before`'s elements: if this \
         SurrealDB no longer makes it, `same_export` has nothing to read past"
    );
}

/// Without arguments, the board of the repository it runs in: the one in its git common dir. The
/// command's one whole run, so it reads the line it says; which board its arguments name, a folder
/// named over the repository's among them, and the line for each engine and each way the old store
/// is kept are the unit tests' beside `reclaim_main`.
#[test]
fn tasks_reclaim_in_a_repository_rebuilds_the_board_in_its_common_dir() {
    let s = Scratch::new("reclaim-repository");
    let repo = s.dir.join("repo");
    let made = symphony_board::sys::output(
        symphony_board::sys::Command::of("git")
            .args(["init", "-q"])
            .arg(&repo),
    )
    .unwrap();
    assert!(made.status.success());
    let common = repo.canonicalize().unwrap().join(".git");
    let home = Home::in_common(&common, Some(s.surreal()));
    board(&home.ensure().unwrap(), "tasks")
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    let env = symphony_board::cli::Env {
        vars: [(
            "SYMPHONY_SURREAL".to_string(),
            s.surreal().display().to_string(),
        )]
        .into(),
        cwd: repo.clone(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::reclaim_main(&[], &env, &mut out, &mut err);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let said = String::from_utf8(out).unwrap();
    let exports = common
        .join("symphony/tasks")
        .join("reclaim")
        .join("exports");
    let again = home.recorded().unwrap();
    // The old store, stopped cleanly by the reclaim, holds no log: kept as a snapshot, the first
    // the reclaim took, among the newest whose log is empty.
    let snapshots = home.snapshots().unwrap();
    let old = snapshots.last().expect("the old store is kept");
    assert!(!old.log, "{snapshots:?}");
    let how = if symphony_board::server::snapshot::clones(&home.snapshots_folder()) {
        "a copy-on-write clone"
    } else {
        "a plain copy, as this volume cannot clone"
    };
    for part in [
        "tasks-reclaim: the board's store took ".to_string(),
        format!("their exports are in {}", exports.display()),
        format!(
            "; the old store is kept at {} ({how}): its log is empty, so it is kept among the \
             newest 3 such snapshots, which later starts' snapshots prune, unless the owner \
             removes it first (`rm -rf {}`), and the room it takes comes back only then; ",
            old.path.display(),
            old.path.display()
        ),
        format!(
            "the board's server is pid {} at {}\n",
            again.pid, again.addr
        ),
    ] {
        assert!(said.contains(&part), "{said}");
    }
    assert_eq!(
        board(&again, "tasks").get("sim/jump").unwrap().title,
        "Jump departure"
    );
    home.stop().unwrap();
}

#[test]
fn bytes_counts_what_a_folder_holds_and_nothing_for_what_is_not_there() {
    let s = Scratch::new("bytes");
    let folder = s.dir.join("counted");
    std::fs::create_dir_all(folder.join("deep")).unwrap();
    std::fs::write(folder.join("deep").join("one"), vec![7; 100_000]).unwrap();
    std::fs::write(folder.join("two"), vec![7; 50_000]).unwrap();
    let counted = bytes(&folder);
    assert!(
        (150_000..150_000 + 64 * 1024).contains(&counted),
        "{counted}"
    );
    assert_eq!(bytes(Path::new("/nowhere/at/all")), 0);
}

/// A board that went back below the record of the schema it was seen at, kept beside its store, is
/// never reclaimed: refused while its server runs, which is left running, before the halt's clean
/// stop could remove a log segment its store never applied; and with none running, by the reclaim's
/// own start on the store, once the reclaim has kept the store, then serves it again.
#[test]
fn a_reclaim_of_a_board_below_its_record_is_refused_and_its_server_left_serving_it() {
    use symphony_board::live::{self, seen};
    let s = Scratch::new("reclaim-went-back");
    let home = s.home().snapshotting(true);
    let running = home.ensure().unwrap();
    board(&running, "tasks")
        .create(&NewTask::new("sim/kept", "Kept by the refusal", "owner"))
        .unwrap();
    let this = symphony_board::Schema::this_build().version();
    let record = s.dir.join(seen::FILE);
    std::fs::write(&record, format!("{} 1800000000 seen x\n", this + 1)).unwrap();
    let why = format!(
        "the reclaim stopped and the board runs on its old store, untouched: {}",
        seen::Rollback {
            project: live::project_of(s.dir.parent().unwrap()),
            from: this + 1,
            since: 1_800_000_000.0,
            to: Some(this),
            record: record.clone(),
        }
        .refusal()
    );

    let error = home.reclaim().unwrap_err();
    assert_eq!(error.to_string(), why);
    assert_eq!(home.recorded(), Some(running.clone()), "never halted");
    assert!(symphony_board::sys::pid_alive(running.pid));
    assert!(!exists(&s.dir.join("reclaim")), "nothing exported");
    assert!(home.snapshots().unwrap().is_empty());

    home.stop().unwrap();
    let error = home.reclaim().unwrap_err();
    assert_eq!(error.to_string(), why);
    let kept = home.snapshots().unwrap();
    assert_eq!(
        kept.len(),
        2,
        "the reclaim's and the one before the start serving it again: {kept:?}"
    );
    let log = std::fs::read_to_string(s.dir.join("server.log")).unwrap();
    assert!(
        log.contains(&format!(
            "· a snapshot of the store before this reclaim: {}",
            kept[1].path.display()
        )),
        "{log}"
    );
    assert!(!exists(&s.dir.join("reclaim/data")), "no new store");
    let again = home.recorded().expect("the board's server runs again");
    assert_eq!(
        board(&again, "tasks").get("sim/kept").unwrap().slug,
        "sim/kept"
    );
    home.stop().unwrap();
}
