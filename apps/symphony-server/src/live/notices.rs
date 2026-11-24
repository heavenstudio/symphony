//! The machine's one server's notices for its project ([`crate::notices`]): a worker of its own
//! beside the board's ticks, so a program's run never holds a tick. Each board read and each
//! refresh of the documents wakes it, and so does the next notice coming due. A pass reads the
//! programs and the profiles from the machine folder, and the board's people, map and lines after
//! the record's, and the cards waiting from the main page's queue, again only when the board's
//! newest line, the page, the people or the map moved; then it hands what is due.
//!
//! It tells only while its server generation's recording gate stands open
//! ([`symphony_board::recording`]), as the board's recorders record: a reload's replacement,
//! whose gate is held until its hand-off commits, reads no record and tells nothing until then,
//! and the generation it replaces, whose gate closes as it commits, lets its record go. The record
//! is read again each time the gate opens, and its lock ([`symphony::notices::RECORD_LOCK`]) keeps
//! a hand still under way in the other generation from overlapping the read.
//!
//! The hub's stop ends a program under way with its group and joins the worker; a hub dropped
//! without a stop takes the worker's wake with it, which ends the worker too.

use super::Hub;
use crate::notices::hand::{self, Running};
use crate::notices::runner::{Line, Look, Runner, Waiting};
use std::collections::BTreeMap;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use symphony::notices::{Key, Kind, Programs};
use symphony_board::lock::Recovered as _;
use symphony_board::people::ownership::{self, Map, Thing};
use symphony_board::people::{People, Profiles};
use symphony_board::recording::State;

/// What the hub keeps of its notices' worker.
pub(crate) struct Worker {
    /// Wakes the worker: its only sender, so the worker ends once the hub is gone.
    wake: mpsc::SyncSender<()>,
    running: Arc<Running>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// Where the link of a notice begins, `http://HOST:PORT`, asked as each notice is made.
pub(crate) type Origin = Box<dyn FnMut() -> String + Send>;

/// What a look read, so the next reads again only when one of them moved.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Read {
    /// The board's newest line.
    latest: i64,
    /// The main page's version, none before it is read.
    page: Option<u64>,
    /// The board's people revision and its map's.
    people: i64,
    map: i64,
}

/// What a pass said that went wrong, by where: said again only once it changes.
#[derive(Default)]
struct Said(BTreeMap<&'static str, String>);

impl Said {
    fn once(&mut self, project: &str, place: &'static str, why: String) {
        if self.0.get(place) != Some(&why) {
            eprintln!("NOTICE {project}: {why}");
            self.0.insert(place, why);
        }
    }

    fn clear(&mut self, place: &'static str) {
        self.0.remove(place);
    }
}

impl Hub {
    /// Start telling the people of the project `project`, served on `port`, of its notices: the
    /// machine's one server's alone. A link begins with the machine's tailnet name, else
    /// `127.0.0.1`, over http.
    pub(crate) fn start_notices(self: &Arc<Self>, project: &str, port: u16) {
        let mut tailnet = None;
        self.start_notices_with(
            project,
            Box::new(move || {
                if tailnet.is_none() {
                    tailnet = crate::gate::magicdns_name();
                }
                let host = tailnet.as_deref().unwrap_or("127.0.0.1");
                format!("http://{host}:{port}")
            }),
        );
    }

    /// [`Hub::start_notices`], each link begun by `origin`.
    pub(super) fn start_notices_with(self: &Arc<Self>, project: &str, origin: Origin) {
        let (wake, woken) = mpsc::sync_channel(1);
        let running = Arc::new(Running::default());
        let (weak, project, worker_running) =
            (Arc::downgrade(self), project.to_string(), running.clone());
        let started = std::thread::Builder::new()
            .name("review notices".into())
            .spawn(move || work(&weak, &woken, &worker_running, &project, origin));
        match started {
            Ok(thread) => {
                let worker = Worker {
                    wake,
                    running,
                    thread: Mutex::new(Some(thread)),
                };
                if let Err(worker) = self.notice_worker.set(worker) {
                    // Started twice: the second ends at once.
                    worker.running.stop();
                }
            }
            Err(error) => eprintln!("NOTICE: no worker started: {error}"),
        }
    }

    /// Wake the notices' worker, where the hub has one: something it reads may have moved.
    pub(crate) fn wake_notices(&self) {
        if let Some(worker) = self.notice_worker.get() {
            let _ = worker.wake.try_send(());
        }
    }

    /// End a notice's program under way, with its group, and the worker after it.
    pub(super) fn stop_notices(&self) {
        let Some(worker) = self.notice_worker.get() else {
            return;
        };
        worker.running.stop();
        let _ = worker.wake.try_send(());
        let thread = worker.thread.lock().recover().take();
        if let Some(thread) = thread
            && thread.join().is_err()
        {
            eprintln!("NOTICE: the worker panicked");
        }
    }

    /// The runner of the project `project`, its digests under the machine's key: none where the
    /// hub has no machine folder, or the key cannot be read or made there, said once.
    fn open_notices(&self, project: &str, said: &mut Said) -> Option<Runner> {
        let machine = self.machine.as_deref()?;
        let key = match Key::of(machine) {
            Ok(key) => key,
            Err(why) => {
                said.once(project, "key", format!("nothing is told: {why}"));
                return None;
            }
        };
        said.clear("key");
        let (runner, unread) = match Runner::open(&self.root, project, key) {
            Ok(opened) => opened,
            Err(why) => {
                said.once(project, "lock", format!("nothing is told yet: {why}"));
                return None;
            }
        };
        said.clear("lock");
        if let Some(why) = unread {
            eprintln!("NOTICE {project}: its record is made anew, as it does not read: {why}");
        }
        Some(runner)
    }

    /// One turn of the worker: a pass of the project's runner while its server generation's gate
    /// stands open, the runner opened (its record read) as the gate opens; none while it is held
    /// or closed, or before it is given, and the runner let go, so the record is read again when
    /// it opens.
    fn notice_turn(
        &self,
        runner: &mut Option<Runner>,
        last: &mut Option<Read>,
        running: &Running,
        origin: &mut dyn FnMut() -> String,
        said: &mut Said,
        project: &str,
    ) {
        let open = (self.recording_gate.get())
            .is_some_and(|gate| matches!(gate.stands(), State::Open { .. }));
        if !open {
            *runner = None;
            *last = None;
            return;
        }
        if runner.is_none() {
            *runner = self.open_notices(project, said);
            *last = None;
        }
        if let Some(runner) = runner {
            self.notice_pass(runner, last, running, origin, said, project);
        }
    }

    /// One pass of `runner`: a look, where what it reads moved since `last`, then the hand of what
    /// is due. A machine folder, a board or profiles that cannot be read leaves the record as it
    /// was, said once; programs that are refused send nothing, said once, and the look goes on.
    fn notice_pass(
        &self,
        runner: &mut Runner,
        last: &mut Option<Read>,
        running: &Running,
        origin: &mut dyn FnMut() -> String,
        said: &mut Said,
        project: &str,
    ) {
        let Some(machine) = self.machine.as_deref() else {
            return;
        };
        let programs = match Programs::read(machine) {
            Ok(programs) => {
                said.clear("programs");
                programs
            }
            Err(why) => {
                said.once(project, "programs", format!("nothing is sent: {why}"));
                Programs::default()
            }
        };
        let profiles = match Profiles::read(machine) {
            Ok(profiles) => {
                said.clear("profiles");
                profiles
            }
            Err(why) => return said.once(project, "profiles", why),
        };
        let Some(latest) = *self.latest_message.lock().recover() else {
            return;
        };
        let Ok(store) = self.reader() else {
            return;
        };
        let (people, map) = match store.people().and_then(|p| Ok((p, store.ownership_map()?))) {
            Ok(read) => {
                said.clear("board");
                read
            }
            Err(error) => {
                return said.once(project, "board", format!("the board's people: {error}"));
            }
        };
        let page = {
            let pages = self.pages.read().recover();
            (pages.get("")).map(|page| {
                let cards: Vec<(String, String, String)> = (page.queue.waiting.iter())
                    .map(|card| (card.doc.clone(), card.slug.clone(), card.href()))
                    .collect();
                (page.board.version, cards)
            })
        };
        let read = Read {
            latest,
            page: page.as_ref().map(|(version, _)| *version),
            people: people.revision,
            map: map.revision,
        };
        let now = (self.clock)();
        if last.as_ref() != Some(&read) {
            let lines = match runner.cursor() {
                Some(cursor) if cursor < latest => {
                    match store.messages_after_rows(cursor, latest) {
                        Ok(rows) => told_lines(&store, &people, &map, rows),
                        Err(error) => {
                            return said.once(
                                project,
                                "lines",
                                format!("the board's lines: {error}"),
                            );
                        }
                    }
                }
                _ => Vec::new(),
            };
            said.clear("lines");
            let cards = page.map(|(_, cards)| {
                let named: Vec<(String, String)> = (cards.iter())
                    .map(|(doc, slug, _)| (doc.clone(), slug.clone()))
                    .collect();
                let owners = symphony::owners::waiting(&people, &map, &self.root, &named);
                (cards.into_iter().zip(owners))
                    .map(|((doc, slug, href), owning)| Waiting {
                        key: format!("{doc}#{slug}"),
                        page: href,
                        owning,
                    })
                    .collect()
            });
            let look = Look {
                people: &people,
                cards,
                lines,
                latest,
            };
            match runner.look(&look, &profiles, &programs, now, origin) {
                Ok(()) => *last = Some(read),
                Err(why) => return said.once(project, "record", why),
            }
        }
        let clock = || (self.clock)();
        let mut handing = |program: &std::path::Path, line: &str, hidden: &[&str]| {
            hand::hand(program, line, hidden, running, &|pid| {
                symphony_board::sys::gone(i64::from(pid), hand::MOST)
            })
        };
        let mut log = |line: String| eprintln!("{line}");
        match runner.hand_due(&profiles, &programs, &clock, &mut handing, &mut log) {
            Ok(()) => said.clear("record"),
            Err(why) => said.once(project, "record", why),
        }
    }
}

/// The board lines among `rows` that tell, each with whose its task is on `people` and `map`: a
/// reviewer's verdict, and an agent's question to the person at the machine (a `msg` line to
/// `owner` that no person wrote). A line of a task the board no longer holds tells no one.
fn told_lines(
    store: &symphony_board::Store,
    people: &People,
    map: &Map,
    rows: Vec<symphony_board::rows::MessageRow>,
) -> Vec<Line> {
    let mut groups: BTreeMap<String, Option<Option<String>>> = BTreeMap::new();
    let mut lines = Vec::new();
    for row in rows {
        let question =
            row.kind == "msg" && row.recipient.as_deref() == Some("owner") && !row.by_person();
        let (id, slug) = (row.id, row.slug.clone());
        let verdict = symphony_board::reviewer_verdict(&symphony_board::Message::from(row));
        let kind = match (verdict, question) {
            (Some(_), _) => Kind::Verdict,
            (None, true) => Kind::Question,
            (None, false) => continue,
        };
        let group = groups.entry(slug.clone()).or_insert_with(|| {
            (store.get(&slug).ok()).map(|task| task.group().map(str::to_string))
        });
        let Some(group) = group else {
            continue;
        };
        let places = Thing::Task(slug.clone()).places();
        lines.push(Line {
            id,
            kind,
            page: format!("/task/{slug}"),
            owning: ownership::owning(people, map, group.as_deref(), &places),
        });
    }
    lines
}

/// The worker: a pass at each wake, and when the next notice is due, until the server stops or the
/// hub is gone.
fn work(
    hub: &Weak<Hub>,
    woken: &mpsc::Receiver<()>,
    running: &Running,
    project: &str,
    mut origin: Origin,
) {
    let (mut runner, mut last, mut said) = (None, None, Said::default());
    loop {
        if running.stopping() {
            return;
        }
        let now = {
            let Some(hub) = hub.upgrade() else {
                return;
            };
            hub.notice_turn(
                &mut runner,
                &mut last,
                running,
                &mut *origin,
                &mut said,
                project,
            );
            (hub.clock)()
        };
        // Until a wake, or until the next notice is due: a retry's backoff, its deadline the
        // record's own (RULES.md `code/no-timed-waits`).
        let woke = match runner.as_ref().and_then(Runner::next_due) {
            None => woken.recv().is_ok(),
            Some(due) => !matches!(
                woken.recv_timeout(Duration::from_secs_f64((due - now).max(0.0))),
                Err(RecvTimeoutError::Disconnected)
            ),
        };
        if !woke {
            return;
        }
    }
}

#[cfg(test)]
#[path = "notices_tests.rs"]
mod tests;
