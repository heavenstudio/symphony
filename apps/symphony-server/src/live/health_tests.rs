//! The board's health as the hub records it: a board tick counts the open tasks nobody
//! orchestrates in the board it read, and one whose dispatch fails warns of it, on the
//! pages' server notice too, a minute of good passes clears it, with no flicker on a board failing
//! every other tick, a tick that cannot reach the board is a failed pass, a board that went back is
//! said unwell in its own words until a store opens it, the one server's record says the same,
//! `/api/health` answers it from the hub's memory, with no board read, and the one server appends
//! each hour of it to a day file from the same memory, a private server none.

use super::Hub;
use crate::front::Shared;
use crate::gate::Gate;
use crate::testing::{
    Tree,
    board::{Fixture, HOST, NOW, Seed},
    repository,
};
use axum::http::{Request, StatusCode};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_api::run_day;
use symphony_board::board_health::disk::{Low, THRESHOLD_GB};
use symphony_board::board_health::{CLEARS_AFTER, RECENT, Recorder, Report, hours};
use symphony_board::rpc::Client;
use symphony_board::sweep::trigger::now;

/// A loopback relay in front of a board's server that can be cut: cut, it ends every connection it
/// carries and each one made to it, until it is mended.
struct Relay {
    addr: String,
    /// Whether it carries, and the ends of each connection it carries.
    state: Arc<Mutex<(bool, Vec<TcpStream>)>>,
}

impl Relay {
    fn to(server: &str) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let state = Arc::new(Mutex::new((true, Vec::new())));
        let held = Arc::clone(&state);
        let server = server.to_string();
        std::thread::spawn(move || {
            for client in listener.incoming() {
                let Ok(client) = client else { return };
                // Decided and kept under the lock a cut takes, so none it misses is carried.
                let mut state = held.lock().unwrap();
                let upstream = TcpStream::connect(&server);
                let (true, Ok(upstream)) = (state.0, upstream) else {
                    let _ = client.shutdown(Shutdown::Both);
                    continue;
                };
                for (mut from, mut to) in [
                    (client.try_clone().unwrap(), upstream.try_clone().unwrap()),
                    (upstream.try_clone().unwrap(), client.try_clone().unwrap()),
                ] {
                    std::thread::spawn(move || {
                        let _ = std::io::copy(&mut from, &mut to);
                        let _ = to.shutdown(Shutdown::Write);
                    });
                }
                state.1.extend([client, upstream]);
            }
        });
        Relay { addr, state }
    }

    fn cut(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = false;
        for stream in state.1.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    fn mend(&self) {
        self.state.lock().unwrap().0 = true;
    }
}

/// A store on `fixture`'s board through `relay`. A cut ends its connections at once; an answer that
/// comes is waited for to a backstop only a stall reaches, never to a loaded machine's 10 s.
fn through(relay: &Relay, fixture: &Fixture) -> symphony_board::Store {
    let client = Client::new(&relay.addr, "sf", fixture.store.client().database())
        .waiting(test_time::backstop(Duration::from_secs(10)));
    symphony_board::Store::with(client, Box::new(|| NOW), Box::new(|_| false), HOST).unwrap()
}

/// The hub's clock, as the test moves it.
#[derive(Clone)]
struct Clock(Arc<Mutex<f64>>);

impl Clock {
    fn at(time: f64) -> Clock {
        Clock(Arc::new(Mutex::new(time)))
    }

    fn now(&self) -> f64 {
        *self.0.lock().unwrap()
    }

    fn pass(&self, seconds: f64) {
        *self.0.lock().unwrap() += seconds;
    }
}

fn hub(root: &Path, file: &Path, clock: &Clock) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    hub.health = Recorder::writing(file.to_path_buf());
    let clock = clock.clone();
    hub.clock = Box::new(move || clock.now());
    hub
}

/// Every open task has an orchestrator, on every board the server serves: its tick gives a gone
/// holder's open work to an orchestrator that stays, with no sweep run, and the count it records is
/// of what is left nobody's.
#[test]
fn a_tick_gives_a_gone_holders_open_work_to_an_orchestrator_that_stays() {
    let tree = Tree::new();
    let root = repository(&tree);
    let file = tree.path("symphony/tasks/health.json");
    let orchestrator = |handle: &str, seen: f64| symphony_board::rows::SessionRow {
        handle: handle.into(),
        kind: "orchestrator".into(),
        connected: NOW - 3_600.0,
        last_seen: seen,
        ..Default::default()
    };
    // amber held tools/quiet and went an hour ago; cobalt, registered as long ago, beats and holds
    // tools/mine.
    let fixture = Fixture::new(
        Seed::default()
            .session_row(orchestrator("amber-heron", NOW - 3_600.0))
            .session_row(orchestrator("cobalt-otter", NOW))
            .task("tools/mine", "ready")
            .task("tools/quiet", "ready")
            .role("tools/mine", "cobalt-otter", "orchestrator")
            .role("tools/quiet", "amber-heron", "orchestrator"),
    );
    let clock = Clock::at(now());
    let hub = hub(&root, &file, &clock);
    hub.board_read(&fixture.store).unwrap();
    let quiet = fixture.store.get("tools/quiet").unwrap();
    assert_eq!(quiet.holders("orchestrator"), ["cobalt-otter"]);
    let line = fixture.store.thread("tools/quiet").unwrap().pop().unwrap();
    assert_eq!(
        (
            line.sender.as_str(),
            line.text
                .starts_with("cobalt-otter was given orchestrator from amber-heron (")
        ),
        ("board", true),
        "{}",
        line.text
    );
    assert_eq!(hub.health.report(clock.now()).unowned, Some(0));
    // The next tick finds nothing to give, and writes nothing.
    let lines = fixture.store.thread("tools/quiet").unwrap().len();
    clock.pass(1.0);
    hub.board_read(&fixture.store).unwrap();
    assert_eq!(fixture.store.thread("tools/quiet").unwrap().len(), lines);
}

#[test]
fn a_failed_dispatch_pass_warns_until_a_minute_of_good_passes_and_the_record_says_the_same() {
    let tree = Tree::new();
    let root = repository(&tree);
    let file = tree.path("symphony/tasks/health.json");
    let fixture = Fixture::new(Seed::default().task("tools/clock", "ready"));
    let relay = Relay::to(fixture.store.client().addr());
    let store = through(&relay, &fixture);
    let clock = Clock::at(now());
    let hub = hub(&root, &file, &clock);

    hub.board_read(&store).unwrap();
    let ok = hub.health.report(clock.now());
    assert_eq!(
        (ok.dispatch.ok, ok.dispatch.failed, ok.warnings.len()),
        (1, 0, 0)
    );
    assert!(ok.queries.count > 0, "the tick's requests are counted");
    assert_eq!(
        ok.unowned,
        Some(1),
        "the tick's board read counts its one task nobody orchestrates"
    );
    assert_eq!(Report::read(&file).unwrap().unwrap().dispatch.ok, 1);
    assert_eq!(*hub.server_notice.borrow(), "", "a well board: no notice");
    assert_eq!(hub.health_notice(), "");

    clock.pass(1.0);
    relay.cut();
    assert!(
        hub.board_read(&store).is_err(),
        "a board that cannot be reached fails the tick"
    );
    let failed = hub.health.report(clock.now());
    assert_eq!((failed.dispatch.ok, failed.dispatch.failed), (1, 1));
    assert_eq!(failed.warnings.len(), 1, "{:?}", failed.warnings);
    let error = failed.dispatch.error.clone().unwrap();
    assert!(
        failed.warnings[0].starts_with("callback dispatch failing for ")
            && failed.warnings[0]
                .ends_with(&format!("(1 of 2 passes in the last hour failed): {error}")),
        "{:?}",
        failed.warnings
    );
    assert!(failed.queries.failed > 0, "{:?}", failed.queries);
    let slowest = failed.queries.slowest.unwrap();
    assert!(
        slowest.source.starts_with("crates/tasks/src/"),
        "the board's request is named by its line: {}",
        slowest.source
    );
    let recorded = Report::read(&file).unwrap().unwrap();
    assert_eq!(
        (recorded.dispatch.failed, recorded.warnings.len()),
        (1, 1),
        "the dispatch started failing: the record says so at once"
    );
    // Every page of the project hears it on the server's notice, and the picker says it too.
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
    assert_eq!(hub.health_notice(), DISPATCH_FAILING);

    clock.pass(1.0);
    relay.mend();
    hub.board_read(&store).unwrap();
    let recovering = hub.health.report(clock.now());
    assert_eq!((recovering.dispatch.ok, recovering.dispatch.failed), (2, 1));
    assert!(
        recovering.warnings.len() == 1
            && recovering.warnings[0].ends_with(&format!(
                "(1 of 3 passes in the last hour failed), ok again for 0 s: {error}"
            )),
        "one good pass does not clear it: {:?}",
        recovering.warnings
    );
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
    assert_eq!(hub.health_notice(), DISPATCH_FAILING);

    clock.pass(CLEARS_AFTER);
    hub.board_read(&store).unwrap();
    let cleared = hub.health.report(clock.now());
    assert_eq!(
        (
            cleared.dispatch.ok,
            cleared.dispatch.failed,
            cleared.warnings
        ),
        (3, 1, Vec::<String>::new()),
        "a minute of good passes clears the warning; the hour still counts the failure"
    );
    let recorded = Report::read(&file).unwrap().unwrap();
    assert_eq!(
        (recorded.dispatch.ok, recorded.warnings),
        (3, Vec::new()),
        "the dispatch stopped failing: the record says so at once"
    );
    assert_eq!(*hub.server_notice.borrow(), "", "and the pages hear it");
    assert_eq!(hub.health_notice(), "");
}

const DISPATCH_FAILING: &str = "The task board is unwell: its callback dispatch is failing, so \
                                agents may not hear from it. Run “just tasks status” to see why.";

#[test]
fn the_notice_names_what_is_failing_and_nothing_while_the_board_is_well() {
    assert_eq!(super::health::notice([false, false]), "");
    assert_eq!(super::health::notice([true, false]), DISPATCH_FAILING);
    assert_eq!(
        super::health::notice([false, true]),
        "The task board is unwell: its idle sweep is failing. Run “just tasks status” to see why."
    );
    assert_eq!(
        super::health::notice([true, true]),
        "The task board is unwell: its callback dispatch and its idle sweep are failing, so \
         agents may not hear from it. Run “just tasks status” to see why."
    );
}

#[test]
fn the_notice_does_not_flicker_on_a_board_failing_every_other_tick() {
    let tree = Tree::new();
    let root = repository(&tree);
    let clock = Clock::at(now());
    let hub = hub(&root, &tree.path("health.json"), &clock);
    let mut notices = hub.server_notice.subscribe();
    let mut told = Vec::new();
    // A tick a second, as the hub's worker makes them: failing every other one for two minutes,
    // then well.
    for second in 0..240 {
        let outcome = if second < 120 && second % 2 == 0 {
            Err("the board did not answer")
        } else {
            Ok(())
        };
        hub.health.dispatched(hub.now(), outcome);
        hub.tell_health(hub.now());
        if notices.has_changed().unwrap() {
            told.push((second, notices.borrow_and_update().clone()));
        }
        clock.pass(1.0);
    }
    assert_eq!(
        told,
        [(0, DISPATCH_FAILING.to_string()), (119 + 60, String::new())],
        "told once as it starts failing, once a minute after its passes are good again"
    );
}

#[test]
fn a_failed_update_and_the_boards_health_share_the_notice_and_each_clears_alone() {
    let tree = Tree::new();
    let root = repository(&tree);
    let at = now();
    let hub = hub(&root, &tree.path("health.json"), &Clock::at(at));
    let mut notices = hub.server_notice.subscribe();

    // A tick that tells no change sends nothing.
    hub.tell_health(at);
    assert!(
        !notices.has_changed().unwrap(),
        "a well board tells nothing"
    );

    hub.health.dispatched(at, Err("the board did not answer"));
    hub.tell_health(at);
    assert_eq!(*notices.borrow_and_update(), DISPATCH_FAILING);
    hub.health
        .dispatched(at + 1.0, Err("the board did not answer"));
    hub.tell_health(at + 1.0);
    assert!(
        !notices.has_changed().unwrap(),
        "still failing: the pages are not told again"
    );

    hub.tell_update("Symphony server update failed.".into());
    assert_eq!(
        *notices.borrow_and_update(),
        format!("Symphony server update failed. · {DISPATCH_FAILING}")
    );

    hub.health.dispatched(at + 2.0, Ok(()));
    hub.tell_health(at + 2.0);
    assert!(
        !notices.has_changed().unwrap(),
        "one good pass: still failing"
    );
    let later = at + 2.0 + CLEARS_AFTER;
    hub.health.dispatched(later, Ok(()));
    hub.tell_health(later);
    assert_eq!(
        *notices.borrow_and_update(),
        "Symphony server update failed.",
        "the board's health clears, the update's failure stays"
    );

    hub.health.swept(later + 1.0, Err("exit status: 3"));
    hub.tell_health(later + 1.0);
    hub.tell_update(String::new());
    assert_eq!(
        *notices.borrow_and_update(),
        super::health::notice([false, true]),
        "the update's failure clears, the board's health stays"
    );

    // A failed sweep with no sweep since is said for ten minutes, then the tick that sees it
    // older clears it.
    hub.tell_health(later + 1.0 + RECENT + 1.0);
    assert_eq!(*notices.borrow_and_update(), "");
}

#[test]
fn a_low_disk_is_said_on_every_page_beside_the_boards_health_until_it_is_freed() {
    let tree = Tree::new();
    let root = repository(&tree);
    let at = now();
    let mut hub = hub(&root, &tree.path("health.json"), &Clock::at(at));
    // The disk the hub reads, as the test sets it, against the default threshold.
    let free = Arc::new(Mutex::new(30_000_000_000_u64));
    let read = Arc::clone(&free);
    hub.low_disk =
        Box::new(move || Low::of(Path::new("/disk"), *read.lock().unwrap(), THRESHOLD_GB));
    let mut notices = hub.server_notice.subscribe();
    const LOW: &str = "The disk has under 20 GB free: a full disk fails every build and can \
                       corrupt the task board. `just tasks status` says how much.";

    hub.tell_health(at);
    assert!(!notices.has_changed().unwrap(), "30 GB free: nothing said");

    *free.lock().unwrap() = 12_345_678_901;
    hub.tell_health(at);
    assert_eq!(*notices.borrow_and_update(), LOW);
    *free.lock().unwrap() = 11_000_000_000;
    hub.tell_health(at);
    assert!(
        !notices.has_changed().unwrap(),
        "still low, a build having written more: the pages are not told again"
    );

    hub.health.dispatched(at, Err("the board did not answer"));
    hub.tell_health(at);
    assert_eq!(
        *notices.borrow_and_update(),
        format!("{DISPATCH_FAILING} · {LOW}"),
        "the board's health and the disk share the notice"
    );

    *free.lock().unwrap() = 25_000_000_000;
    hub.tell_health(at);
    assert_eq!(
        *notices.borrow_and_update(),
        DISPATCH_FAILING,
        "the disk freed: its part clears alone"
    );
}

/// A project whose settings do not load is served with the defaults, and every page says so and
/// why until they load; each change asks the loop to read the documents again.
#[test]
fn a_project_whose_settings_do_not_load_says_so_on_every_page_until_they_do() {
    use std::sync::atomic::Ordering::SeqCst;
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = hub(&root, &tree.path("health.json"), &Clock::at(now()));
    let mut notices = hub.server_notice.subscribe();
    hub.tell_project();
    assert!(!notices.has_changed().unwrap(), "it loads: nothing said");
    assert!(!hub.rescan.load(SeqCst));

    let file = root.join(symphony::project::FILE);
    let entrypoint = |types: &str| {
        format!("[[entrypoints]]\nurl = \"/docs/\"\nfolder = \"docs\"\ntypes = [{types}]\n")
    };
    std::fs::write(&file, entrypoint("\"none\"")).unwrap();
    hub.tell_project();
    let said = notices.borrow_and_update().clone();
    let why = symphony::project::Project::load(&root).unwrap_err();
    assert!(
        why.contains("type \"none\": no renderer handles it"),
        "{why}"
    );
    assert_eq!(said, super::health::project(&why));
    assert!(
        said.starts_with(
            "This project's settings do not load, so it is served with the defaults: symphony.toml: "
        ),
        "{said}"
    );
    assert!(hub.rescan.swap(false, SeqCst), "a change asks for a read");
    hub.tell_project();
    assert!(
        !notices.has_changed().unwrap(),
        "still not loading: the pages are not told again"
    );
    assert!(!hub.rescan.load(SeqCst));

    std::fs::write(&file, entrypoint("\"markdown\", \"media\"")).unwrap();
    hub.tell_project();
    assert_eq!(
        *notices.borrow_and_update(),
        "",
        "it loads: its part clears"
    );
    assert!(hub.rescan.swap(false, SeqCst));
}

#[test]
fn the_machines_disk_is_read_where_the_project_is_against_the_setting() {
    let tree = Tree::new();
    let root = repository(&tree);
    let setting = |gb: &str| -> std::collections::BTreeMap<String, String> {
        [(
            symphony_board::board_health::disk::VAR.to_string(),
            gb.to_string(),
        )]
        .into_iter()
        .collect()
    };
    // Under a threshold over any disk there is: low, read at the project's folder.
    let low = super::health::reading(&root, &setting("1000000000000"))().unwrap();
    assert_eq!(
        (low.path.as_path(), low.threshold_gb),
        (root.as_path(), 1e12)
    );
    assert!(low.free > 0, "{low:?}");
    assert_eq!(super::health::reading(&root, &setting("0"))(), None);
    // The library's own tests read a disk that is never low, whatever the machine's.
    let hub = Hub::new(&root, &root.join("build/client"));
    assert_eq!((hub.low_disk)(), None);
}

#[test]
fn a_tick_that_cannot_reach_the_board_is_a_failed_pass() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = hub(&root, &tree.path("health.json"), &Clock::at(now()));
    assert!(
        hub.reconnect(&mut |_: &Path| Err("connection refused".to_string()))
            .is_none()
    );
    let report = hub.health.report(now());
    assert_eq!(
        (report.dispatch.failed, report.dispatch.error.as_deref()),
        (
            1,
            Some("no pass, the board could not be reached: connection refused")
        )
    );
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
}

#[test]
fn a_board_that_went_back_is_said_unwell_with_its_line_on_every_page_until_a_store_opens_it() {
    let tree = Tree::new();
    let root = repository(&tree);
    let file = tree.path("symphony/tasks/health.json");
    let clock = Clock::at(now());
    let hub = hub(&root, &file, &clock);
    let refusal = symphony_board::live::seen::Rollback {
        project: "alpha".into(),
        from: 16,
        since: NOW,
        to: Some(13),
        record: tree.path(".git/symphony/tasks/schema-seen.log"),
    }
    .refusal();
    let went_back = "The task board is unwell: it went back from schema 16 to 13, so what was \
                     written since is lost: nothing was written; do not run `just tasks migrate`; \
                     the Symphony's docs/TASKS.md#a-board-that-went-back says how to recover it.";

    assert!(
        hub.reconnect(&mut |_: &Path| Err(refusal.clone()))
            .is_none()
    );
    assert_eq!(
        *hub.server_notice.borrow(),
        went_back,
        "not the dispatch's words"
    );
    assert_eq!(
        hub.health_notice(),
        went_back,
        "and the picker says the same"
    );
    let report = hub.health.report(clock.now());
    assert_eq!(
        report.warnings[0],
        format!(
            "the task board is unwell: {}",
            &went_back["The task board is unwell: ".len()..went_back.len() - 1]
        )
    );
    assert_eq!(
        Report::read(&file).unwrap().unwrap().rollback,
        report.rollback,
        "the one server's record, which `tasks status` reads, says it too"
    );

    // Accepted or recovered, the next store that opens it clears it; the failed pass stays a
    // minute, as any does.
    let fixture = Fixture::new(Seed::default());
    clock.pass(1.0);
    let opened = hub.reconnect(&mut |_: &Path| {
        symphony_board::Store::with(
            Client::new(
                fixture.store.client().addr(),
                "sf",
                fixture.store.client().database(),
            ),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
        )
        .map_err(|e| e.to_string())
    });
    assert!(opened.is_some());
    assert_eq!(hub.health.rollback(), None);
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
    assert_eq!(Report::read(&file).unwrap().unwrap().rollback, None);
}

/// While a reach of the board is refused because a start replays its store's log, this server's
/// own, run apart from its reads, or another's it waited on, every page and the picker say the
/// board is starting, with the log's size, in place of the dispatch's words; the failed pass is
/// counted all the same. A refusal holding no replay, or a store that opens, ends it.
#[test]
fn a_board_replaying_its_log_is_said_starting_on_every_page_until_a_reach_finds_no_replay() {
    let tree = Tree::new();
    let root = repository(&tree);
    let clock = Clock::at(now());
    let hub = hub(&root, &tree.path("health.json"), &clock);
    let log = tree.path(".git/symphony/tasks/server.log");
    let apart = format!(
        "the board's server is started apart from this read, by pid 4242, since 10:00: a private \
         replay of the store's log of 412.4 MiB before the board's server starts: a log of \
         gigabytes takes minutes, and its server's lines go to {}; this read does not wait for it, \
         and the next one asks again",
        log.display()
    );
    let starting = "The task board is starting: its store's log of 412.4 MiB is replayed in a \
                    private server before the board's server starts (a log of gigabytes takes \
                    minutes), and the board answers once that is through.";
    assert!(hub.reconnect(&mut |_: &Path| Err(apart.clone())).is_none());
    assert_eq!(
        *hub.server_notice.borrow(),
        starting,
        "not the dispatch's words"
    );
    assert_eq!(
        hub.health_notice(),
        starting,
        "and the picker says the same"
    );
    let report = hub.health.report(clock.now());
    assert_eq!(report.dispatch.failed, 1, "a failed pass all the same");
    assert!(
        report
            .dispatch
            .error
            .as_deref()
            .is_some_and(|error| error.ends_with(&apart)),
        "`tasks status` says why: {report:?}"
    );

    // Another's replay, the read refused once it waited its bound.
    let waited = format!(
        "the board's starters' lock ({}) is held by pid 77, since 10:01: a private replay of the \
         store's log of 1.0 GiB before the board's server starts: a log of gigabytes takes \
         minutes, and its server's lines go to {}; this read waits at most 5 s for it, and the \
         next one tries again",
        tree.path(".git/symphony/tasks/lock").display(),
        log.display()
    );
    clock.pass(1.0);
    assert!(hub.reconnect(&mut |_: &Path| Err(waited.clone())).is_none());
    assert_eq!(
        *hub.server_notice.borrow(),
        starting.replace("412.4 MiB", "1.0 GiB")
    );

    // A refusal that holds no replay: the dispatch's words again.
    clock.pass(1.0);
    assert!(
        hub.reconnect(&mut |_: &Path| Err("connection refused".to_string()))
            .is_none()
    );
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);

    // A store that opens ends it; the failed passes stay a minute, as any do.
    clock.pass(1.0);
    assert!(hub.reconnect(&mut |_: &Path| Err(apart.clone())).is_none());
    assert_eq!(*hub.server_notice.borrow(), starting);
    let fixture = Fixture::new(Seed::default());
    clock.pass(1.0);
    let opened = hub.reconnect(&mut |_: &Path| {
        symphony_board::Store::with(
            Client::new(
                fixture.store.client().addr(),
                "sf",
                fixture.store.client().database(),
            ),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
        )
        .map_err(|e| e.to_string())
    });
    assert!(opened.is_some());
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
    assert_eq!(hub.health_notice(), DISPATCH_FAILING);
}

#[test]
fn a_project_with_no_board_counts_no_failed_pass_and_is_not_called_unwell() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = hub(&root, &tree.path("health.json"), &Clock::at(now()));
    let missing = symphony_board::server::ServerError::NoSurreal(tree.path("surreal")).to_string();
    assert!(
        hub.reconnect(&mut |_: &Path| Err(missing.clone()))
            .is_none()
    );
    let report = hub.health.report(now());
    assert_eq!(
        (report.dispatch.failed, report.warnings.len()),
        (0, 0),
        "{report:?}"
    );
    assert_eq!(*hub.server_notice.borrow(), "");
    assert!(hub.boardless(), "the hub knows it has no board");
    // A board that is there and cannot be reached is another matter: a failed pass, and unwell.
    assert!(
        hub.reconnect(&mut |_: &Path| Err("connection refused".to_string()))
            .is_none()
    );
    assert!(!hub.boardless());
    assert_eq!(hub.health.report(now()).dispatch.failed, 1);
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
}

#[test]
fn a_board_whose_store_cannot_be_served_is_a_failed_pass_and_unwell_not_boardless() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = hub(&root, &tree.path("health.json"), &Clock::at(now()));
    let unserved = symphony_board::server::ServerError::Unserved {
        surreal: tree.path("surreal"),
        data: tree.path("symphony/tasks/data"),
    }
    .to_string();
    assert!(
        hub.reconnect(&mut |_: &Path| Err(unserved.clone()))
            .is_none()
    );
    let report = hub.health.report(now());
    assert_eq!(
        (report.dispatch.failed, report.warnings.len()),
        (1, 1),
        "{report:?}"
    );
    assert_eq!(
        report.dispatch.error.as_deref(),
        Some(format!("no pass, the board could not be reached: {unserved}").as_str())
    );
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
    assert!(!hub.boardless());
}

#[tokio::test]
async fn the_health_route_answers_from_the_hubs_memory() {
    let tree = Tree::new();
    let root = repository(&tree);
    let shared = Shared::new(
        1,
        1234,
        &root,
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let ask = |shared: Arc<Shared>| async move {
        let request = Request::builder()
            .uri("/api/health")
            .header("Host", "localhost:1234")
            .body(())
            .unwrap();
        let response = crate::api::answer(&shared, request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, body)
    };
    let (status, _) = ask(shared.clone()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "no hub yet");

    // A hub whose board never answered: the route reads no board and waits on no first read.
    let hub = Arc::new(Hub::new(&root, &root.join("build/client")));
    hub.health
        .dispatched(now(), Err("the board did not answer"));
    assert!(shared.api.set(hub.clone()).is_ok());
    let (status, body) = ask(shared).await;
    assert_eq!(status, StatusCode::OK);
    let report: Report = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        (report.dispatch.failed, report.dispatch.error.as_deref()),
        (1, Some("the board did not answer"))
    );
    assert_eq!(report.warnings, hub.health.report(report.at).warnings);
}

/// The start of an hour, in seconds since the epoch.
const HOUR_START: f64 = 1_800_000_000.0;

#[test]
fn the_hubs_ticks_append_a_line_an_hour_its_stop_the_hour_so_far_and_none_asks_the_board() {
    let tree = Tree::new();
    let root = repository(&tree);
    let folder = tree.path("hours");
    let fixture = Fixture::new(Seed::default().task("tools/clock", "ready"));
    let clock = Clock::at(HOUR_START - 0.5);
    let hub = hub(&root, &tree.path("health.json"), &clock);
    // On a machine that never sleeps: its clock that stops while it sleeps runs with the test's.
    hub.health.keep_hours(folder.clone(), |now| now, |_| {});
    hub.gate_recorders(Arc::default());
    // The first tick, just before the hour: its line counts from the hour's start.
    hub.board_read(&fixture.store).unwrap();
    // A tick every ten minutes.
    for _ in 0..5 {
        clock.pass(600.0);
        hub.board_read(&fixture.store).unwrap();
    }
    let log = fixture.store.client().queries();
    let every = i64::MIN..=i64::MAX;
    let sent = log.within(&every).0.count;
    assert!(sent > 0, "the ticks asked the board");

    // The idle sweep's thread, past the hour's end: the hour's line, from the hub's memory.
    clock.pass(601.0);
    hub.health.swept(clock.now(), Ok(()));
    assert_eq!(
        log.within(&every).0.count,
        sent,
        "appending the line sent the board nothing"
    );
    let day = run_day(HOUR_START);
    let lines = hours::read(&folder, &day);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert_eq!(
        (line.start, line.from, line.to, line.partial, line.pid),
        (
            HOUR_START,
            HOUR_START,
            HOUR_START + 3600.0,
            false,
            std::process::id()
        )
    );
    assert_eq!(
        (line.dispatch.ok, line.dispatch.failed, line.sweep.ok),
        (5, 0, 0),
        "its five ticks in the hour; the sweep after it is the next hour's"
    );

    // A tick in the next hour, then the hub stops: the hour so far, marked partial.
    clock.pass(60.0);
    hub.board_read(&fixture.store).unwrap();
    hub.stop();
    let lines = hours::read(&folder, &day);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
        (lines[1].from, lines[1].to, lines[1].partial),
        (HOUR_START + 3600.0, HOUR_START + 3660.5, true)
    );
    assert_eq!((lines[1].dispatch.ok, lines[1].sweep.ok), (1, 1));
}

#[test]
fn only_the_shared_servers_hub_keeps_its_hours_and_in_the_common_dir() {
    for shared in [false, true] {
        let tree = Tree::new();
        let root = repository(&tree);
        let hub = Hub::start_watching::<notify::NullWatcher>(
            &root,
            &root.join("build/client"),
            false,
            shared,
            crate::backstop::PERIOD,
            |_: &Path| Err("no board in this test".to_string()),
        )
        .unwrap();
        let folder = symphony_board::git::common_dir(&root)
            .unwrap()
            .join(hours::FOLDER);
        assert_eq!(
            hub.health.hours_folder(),
            shared.then(|| folder.clone()),
            "shared: {shared}"
        );
        // Given its gate as `api::start` gives it; the passes half a minute apart, as the machine's
        // own clock that stops while it sleeps reads them (`hours::awake`), so it never slept.
        hub.gate_recorders(Arc::default());
        hub.health.dispatched(HOUR_START, Ok(()));
        hub.health.dispatched(HOUR_START + 30.0, Ok(()));
        hub.health.stop_hours(HOUR_START + 30.5);
        assert_eq!(
            hours::read(&folder, &run_day(HOUR_START)).len(),
            usize::from(shared),
            "a private server writes no line: shared {shared}"
        );
        assert_eq!(folder.exists(), shared);
        hub.stop();
    }
}

/// The shared server's hub stops by appending the hour so far, unless its project was removed under
/// it: that line would make the removed checkout's folders anew, so it writes none.
#[test]
fn a_hub_whose_project_was_removed_appends_no_hour_as_it_stops() {
    for removed in [false, true] {
        let tree = Tree::new();
        let root = repository(&tree);
        // Its records in the project's common dir, as the shared server keeps them.
        let common = symphony_board::git::common_dir(&root).unwrap();
        let folder = common.join(hours::FOLDER);
        let clock = Clock::at(HOUR_START);
        let hub = hub(&root, &common.join("symphony/tasks/health.json"), &clock);
        hub.health.keep_hours(folder.clone(), |now| now, |_| {});
        hub.gate_recorders(Arc::default());
        // Two ticks half a minute apart, and the stop half a second after the second.
        hub.health.dispatched(HOUR_START, Ok(()));
        clock.pass(30.0);
        hub.health.dispatched(clock.now(), Ok(()));
        clock.pass(0.5);
        if removed {
            std::fs::remove_dir_all(&root).unwrap();
        }
        hub.stop();
        if removed {
            assert!(!root.exists(), "its stop made its folder anew");
        } else {
            let lines = hours::read(&folder, &run_day(HOUR_START));
            assert_eq!(lines.len(), 1, "the hour so far: {lines:?}");
            assert_eq!(
                (lines[0].to, lines[0].partial, lines[0].dispatch.ok),
                (HOUR_START + 30.5, true, 2)
            );
        }
    }
}
