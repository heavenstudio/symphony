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
use crate::notices::hand::{self, Bounds, Running};
use crate::notices::runner::{Line, Look, Runner, Waiting};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use symphony::notices::{Key, Kind, Programs};
use symphony_board::held::Held;
use symphony_board::lock::Recovered as _;
use symphony_board::people::ownership::{self, Map, Owning, Thing};
use symphony_board::people::{People, Profiles};
use symphony_board::recording::State;

/// What the hub keeps of its notices' worker.
pub(crate) struct Worker {
    /// Wakes the worker: its only sender, so the worker ends once the hub is gone. Not a
    /// [`crate::backstop::Wakes`], whose loop keeps a sender of its own and so never sees its
    /// wakers go: a worker of a hub dropped without a stop would wait on and hold the record.
    wake: mpsc::SyncSender<()>,
    running: Arc<Running>,
    /// None where its thread did not start.
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// Where the link of a notice begins, `http://HOST:PORT`, asked by each look that makes one.
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
    /// The comments held on documents and not passed on, by number.
    held: Vec<i64>,
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

/// What the worker keeps from one turn to the next: its project, how its hands wait, how its links
/// begin and what its hands share with the server's stop, then its runner, what its last look read
/// and what went wrong that it said.
struct Turns {
    project: String,
    bounds: Bounds,
    origin: Origin,
    running: Arc<Running>,
    /// None while the gate is not open, or before it opened its record.
    runner: Option<Runner>,
    /// None before a look, and after the runner is let go.
    last: Option<Read>,
    said: Said,
}

impl Turns {
    fn new(project: &str, bounds: Bounds, origin: Origin, running: Arc<Running>) -> Self {
        Turns {
            project: project.into(),
            bounds,
            origin,
            running,
            runner: None,
            last: None,
            said: Said::default(),
        }
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
            Bounds::SERVER,
            Box::new(move || {
                if tailnet.is_none() {
                    tailnet = crate::gate::magicdns_name();
                }
                let host = tailnet.as_deref().unwrap_or("127.0.0.1");
                format!("http://{host}:{port}")
            }),
        );
    }

    /// [`Hub::start_notices`], each hand waiting by `bounds` and each link begun by `origin`. The
    /// worker is made once: a second start starts nothing, and lets its `origin` go.
    pub(super) fn start_notices_with(
        self: &Arc<Self>,
        project: &str,
        bounds: Bounds,
        origin: Origin,
    ) {
        self.notice_worker.get_or_init(|| {
            let (wake, woken) = mpsc::sync_channel(1);
            let running = Arc::new(Running::default());
            let turns = Turns::new(project, bounds, origin, running.clone());
            let weak = Arc::downgrade(self);
            let thread = std::thread::Builder::new()
                .name("review notices".into())
                .spawn(move || work(&weak, &woken, turns))
                .map_err(|error| eprintln!("NOTICE: no worker started: {error}"))
                .ok();
            Worker {
                wake,
                running,
                thread: Mutex::new(thread),
            }
        });
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
    fn notice_turn(&self, turns: &mut Turns) {
        let open = (self.recording_gate.get())
            .is_some_and(|gate| matches!(gate.stands(), State::Open { .. }));
        if !open {
            turns.runner = None;
            turns.last = None;
            return;
        }
        if turns.runner.is_none() {
            turns.runner = self.open_notices(&turns.project, &mut turns.said);
            turns.last = None;
        }
        self.notice_pass(turns);
    }

    /// One pass of the turns' runner: a look, where what it reads moved since the last, then the
    /// hand of what is due. A machine folder, a board or profiles that cannot be read leaves the
    /// record as it was, said once; programs that are refused send nothing, said once, and the
    /// look goes on.
    fn notice_pass(&self, turns: &mut Turns) {
        let Turns {
            project,
            bounds,
            origin,
            running,
            runner,
            last,
            said,
        } = turns;
        let (Some(runner), Some(machine)) = (runner.as_mut(), self.machine.as_deref()) else {
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
        let held = match store.held_on_documents() {
            Ok(held) => {
                said.clear("held");
                held
            }
            Err(error) => {
                let why = format!("the comments held on documents: {error}");
                return said.once(project, "held", why);
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
            held: held.iter().map(|held| held.id).collect(),
        };
        let now = (self.clock)();
        if last.as_ref() != Some(&read) {
            let lines = match runner.cursor() {
                Some(cursor) if cursor < latest => {
                    let rows = store.messages_after_rows(cursor, latest);
                    match rows.and_then(|rows| told_lines(&store, &people, &map, rows)) {
                        Ok(lines) => lines,
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
                let cards = (cards.into_iter().zip(owners)).map(|((doc, slug, href), owning)| {
                    Waiting::card(format!("{doc}#{slug}"), href, owning)
                });
                cards
                    .chain(held_waiting(&people, &map, &self.root, &held))
                    .collect()
            });
            let look = Look {
                people: &people,
                cards,
                lines,
                latest,
            };
            match runner.look(&look, &profiles, &programs, now, &mut **origin) {
                Ok(()) => *last = Some(read),
                Err(why) => return said.once(project, "record", why),
            }
        }
        let clock = || (self.clock)();
        let ends = |pid: u32, within: Duration| symphony_board::sys::gone(i64::from(pid), within);
        let mut handing = |program: &std::path::Path, line: &str, hidden: &[&str]| {
            hand::hand(program, line, hidden, running, &ends, *bounds)
        };
        let mut log = |line: String| eprintln!("{line}");
        match runner.hand_due(&profiles, &programs, &clock, &mut handing, &mut log) {
            Ok(()) => said.clear("record"),
            Err(why) => said.once(project, "record", why),
        }
    }
}

/// The board lines among `rows` that tell, each with whom it tells on `people` and `map`: a
/// reviewer's verdict and an agent's question to the person at the machine (a `msg` line to
/// `owner` that no person wrote) tell whose its task is; the board's line of a held message, a line
/// to `owner` whose number a message held on its task has (`symphony_board::held`), tells those
/// who may pass it on ([`passers`]), never its author, a link to its place on the task's page, and
/// no one once it was passed on. A line of a task the board no longer holds tells no one.
///
/// # Errors
///
/// The messages held on a task cannot be read: no line is taken, and the look reads them again.
fn told_lines(
    store: &symphony_board::Store,
    people: &People,
    map: &Map,
    rows: Vec<symphony_board::rows::MessageRow>,
) -> Result<Vec<Line>, symphony_board::Error> {
    let mut groups: BTreeMap<String, Option<Option<String>>> = BTreeMap::new();
    let mut held: BTreeMap<String, Vec<Held>> = BTreeMap::new();
    let mut lines = Vec::new();
    for row in rows {
        let question =
            row.kind == "msg" && row.recipient.as_deref() == Some("owner") && !row.by_person();
        // Its text is only where to look: a held row of its number says it is one.
        let marker = question && row.text.starts_with(symphony_board::held::MARKER);
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
        let owning = ownership::owning(people, map, group.as_deref(), &places);
        let message = if marker && kind == Kind::Question {
            let on_task = match held.entry(slug.clone()) {
                Entry::Occupied(read) => read.into_mut(),
                Entry::Vacant(unread) => unread.insert(store.held_on(&slug)?),
            };
            on_task.iter().find(|message| message.id == id)
        } else {
            None
        };
        lines.push(match message {
            Some(message) if message.passed.is_some() => continue,
            Some(message) => Line {
                id,
                kind: Kind::Held,
                page: format!("/task/{slug}#message-{id}"),
                told: passers(people, &owning, &message.author),
            },
            None => Line {
                id,
                kind,
                page: format!("/task/{slug}"),
                told: ownership::told(people, &owning),
            },
        });
    }
    Ok(lines)
}

/// Who is told of a message `author` wrote, held on a task `owning` says whose it is on a board of
/// `people`: those who may pass it on ([`crate::powers::may_pass`]) of whose the task is, its
/// group's members or the person it is written to; else, where none of them may, every person of
/// the board who may; never its author. None where nobody but its author may, and on a board that
/// names nobody: the person at the machine, whom no profile names, is told by the page.
fn passers(people: &People, owning: &Owning, author: &str) -> Vec<String> {
    let may = |name: &String| name != author && crate::powers::may_pass(people, name);
    let own: Vec<String> = (ownership::told(people, owning).into_iter())
        .filter(may)
        .collect();
    if !own.is_empty() {
        return own;
    }
    (people.members.iter())
        .map(|member| member.name.clone())
        .filter(may)
        .collect()
}

/// The comments `held` on documents, not passed on, each waiting for someone who may pass it on
/// ([`passers`]) of whose its card or its document is on `people` and `map` (`root` the project's
/// checkout, its documents read once each): a notice links its place on its document's page,
/// `/DOC#held-N`, a branch's under its scope.
fn held_waiting(
    people: &People,
    map: &Map,
    root: &std::path::Path,
    held: &[symphony_board::held::OnDocument],
) -> Vec<Waiting> {
    let docs: Vec<(String, String)> = (held.iter())
        .map(|held| {
            let place = &held.document;
            let doc = match &place.scope {
                Some(scope) => format!("@{scope}/{}", place.path),
                None => place.path.clone(),
            };
            (doc, place.card.clone().unwrap_or_default())
        })
        .collect();
    let owners = symphony::owners::waiting(people, map, root, &docs);
    (held.iter().zip(docs).zip(owners))
        .map(|((held, (doc, _)), owning)| Waiting {
            key: format!("held:{}", held.id),
            page: format!("/{doc}#held-{}", held.id),
            told: Some(passers(people, &owning, &held.author)),
            owning,
            kind: Kind::Held,
        })
        .collect()
}

/// The worker: a pass at each wake, and when the next notice is due, until the server stops or the
/// hub is gone.
fn work(hub: &Weak<Hub>, woken: &mpsc::Receiver<()>, mut turns: Turns) {
    loop {
        if turns.running.stopping() {
            return;
        }
        let now = {
            let Some(hub) = hub.upgrade() else {
                return;
            };
            hub.notice_turn(&mut turns);
            (hub.clock)()
        };
        // Until a wake, or until the next notice is due: a retry's backoff, its deadline the
        // record's own (RULES.md `code/no-timed-waits`).
        let woke = match turns.runner.as_ref().and_then(Runner::next_due) {
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
