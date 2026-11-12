//! The timings: say, claim, inbox, looks of `tasks wait` and the `tasks api` answers land asks
//! for, each the `tasks` binary run as an agent runs it on the generated board, three times, the
//! median of the CPU each run cost within a bound: the `tasks` process's own and the board's
//! server's while it ran ([`crate::cpu`]), each run started once the server is quiet
//! ([`Board::quiet`]). That is the work the command did, which the machine's
//! load does not multiply as it multiplies a wall: at load 170 on 16 cores a command gets about a
//! tenth of a core, so its wall stretches some tenfold while its work stays what it was. The walls
//! are printed beside, for reading.
//! A say dispatches in the write (`TASKS_DISPATCH=inline`), so its cost is the write's and the
//! delivery to its recipients', 100 of them on this board. The looks of `tasks wait` are each
//! bounded by the work the look before it left them ([`looks`]): what a look does depends on
//! whether the one before it delivered its PULL, and that, while a look's delivery has a deadline
//! of real time ([`DEADLINE`]), on the wall it took.
//!
//! The bounds are stated once, at the end, with what justifies them. On 2026-11-07 the live board
//! answered a say in 6 to 9 minutes and a claim in 2: each recipient's inbox read cost 7 s of the
//! server's time, all of it work, which a bound on CPU holds at any load.

use crate::board::{Board, board};
use crate::generated;
use serde_json::{Value, json};
use std::time::Duration;

/// What is timed and its bound. A command line with its stdin, and the most CPU its median may take.
struct Timed {
    name: String,
    args: Vec<String>,
    input: String,
    bound: Duration,
}

/// One timed name's runs, each its wall and its CPU (the `tasks` process's and the server's), and
/// the bound of their median CPU.
struct Runs {
    walls: Vec<Duration>,
    cpus: Vec<(Duration, Duration)>,
    bound: Duration,
}

/// The middle of `values`, sorted.
fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

fn command(name: &str, args: &[&str], bound: Duration) -> Timed {
    Timed {
        name: name.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        input: String::new(),
        bound,
    }
}

fn api(method: &str, args: Value, bound: Duration) -> Timed {
    Timed {
        name: format!("api {method}"),
        args: vec!["api".into()],
        input: json!({"method": method, "args": args}).to_string(),
        bound,
    }
}

#[test]
fn say_claim_inbox_and_the_api_answer_within_their_bounds_on_a_board_at_scale() {
    let mut board = board();
    let thread = generated::thread();
    let writer = generated::writer();
    let recipient = generated::recipient();
    let orchestrator = generated::orchestrator();
    let mut timed = Vec::new();
    for n in 0..3 {
        timed.push(command(
            "say",
            &[
                "say",
                &thread,
                &format!("timed post {n}"),
                "--to",
                &recipient,
                "--as",
                &writer,
            ],
            SAY,
        ));
    }
    for _ in 0..3 {
        let slug = board.unclaimed();
        timed.push(command("claim", &["claim", &slug, "--as", &writer], CLAIM));
    }
    for _ in 0..3 {
        timed.push(command("inbox", &["inbox", "--as", &orchestrator], INBOX));
    }
    for _ in 0..3 {
        timed.extend([
            api("get", json!({"slug": thread}), API),
            api("tasks", json!({"status": "review"}), API),
            api("ever_executed", json!({"slug": thread}), API),
            api("sessions", json!({}), API),
            api("thread", json!({"slug": thread}), API),
            api("landings_posted", json!({}), API),
            api("last_messages", json!({}), API),
            api("to_owner", json!({"limit": 40}), API),
            api("inbox", json!({"handle": orchestrator}), API),
        ]);
    }
    let mut timings: std::collections::BTreeMap<String, Runs> = std::collections::BTreeMap::new();
    for step in &timed {
        let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
        let ran = board.run(&board.addr, &args, &step.input);
        assert_eq!(ran.code, 0, "{}: {}", step.name, ran.err);
        let entry = (timings.entry(step.name.clone())).or_insert(Runs {
            walls: Vec::new(),
            cpus: Vec::new(),
            bound: step.bound,
        });
        entry.walls.push(ran.wall);
        entry.cpus.push((ran.tasks_cpu, ran.server_cpu));
    }
    assert!(
        board
            .listeners
            .hears(&recipient, "timed post 2", Duration::from_secs(5)),
        "the last post reached its recipient: {:?}",
        board.listeners.heard(&recipient)
    );
    let mut over = Vec::new();
    for (name, Runs { walls, cpus, bound }) in timings {
        let wall = median(walls.clone());
        let cpu = median(
            cpus.iter()
                .map(|(tasks, server)| *tasks + *server)
                .collect(),
        );
        eprintln!(
            "perf: {name}: median CPU {cpu:.3?} of {cpus:.3?} (tasks, server), bound {bound:.1?}; median wall {wall:.3?} of {walls:.3?}"
        );
        // A write reads the board for each of its 100 recipients on the server: a meter reading
        // no work there reads another process.
        if ["say", "claim"].contains(&name.as_str()) {
            let idle: Vec<_> = cpus.iter().filter(|(_, server)| server.is_zero()).collect();
            assert!(idle.is_empty(), "{name}: the server took no CPU: {cpus:?}");
        }
        if cpu > bound {
            over.push(format!(
                "{name}: median CPU {cpu:.3?} over its bound {bound:.1?}"
            ));
        }
    }
    over.extend(looks(&board, &generated::waiter()));
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// What a look of `tasks wait` has to work out: the whole digest, at the first look and at one
/// after a look that built it and printed no PULL, which left it queued; or what changed since a
/// look that printed it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    Whole,
    Since,
}

/// Whether a look delivered its PULL, the digest the looks after it say what changed since: a line
/// of its output that is one, whole or what changed (`cli/pull.rs`). A look runs its pane watch
/// first, under a lease of its own (`Store::collect_callbacks`), so one whose PULL went undelivered
/// past the deadline may still print the watch's line, a WATCH while hc cannot be asked, which is
/// not the digest.
fn pulled(out: &str) -> bool {
    out.lines().any(|line| line.starts_with("PULL "))
}

/// What one look cost and did.
struct Seen {
    cpu: Duration,
    wall: Duration,
    pulled: bool,
}

/// The looks taken so far, each counted as what it had to work out, and what was wrong with them.
#[derive(Default)]
struct Looks {
    whole: Vec<Duration>,
    since: Vec<Duration>,
    /// Whether a look printed its PULL: every look after it is of what changed since.
    told: bool,
    wrong: Vec<String>,
}

impl Looks {
    /// What the next look has to work out.
    fn next(&self) -> Look {
        if self.told { Look::Since } else { Look::Whole }
    }

    /// Whether to look again: until three looks of what changed since are timed, at most
    /// [`LOOKS`] looks in all.
    fn wanted(&self) -> bool {
        self.since.len() < 3 && self.whole.len() + self.since.len() < LOOKS
    }

    /// A look's cost and output, counted as what it had to work out: a look of the whole digest is
    /// held to [`WHOLE`], and one that printed no PULL within [`DEADLINE`] is wrong, since only a
    /// deadline past keeps it from delivering one.
    fn saw(&mut self, seen: &Seen) -> Look {
        let look = self.next();
        let n = self.whole.len() + self.since.len() + 1;
        match look {
            Look::Whole => {
                if seen.cpu > WHOLE {
                    self.wrong.push(format!(
                        "wait: look {n}, of the whole digest, took CPU {:.3?}, over its bound {WHOLE:.1?}",
                        seen.cpu
                    ));
                }
                if seen.pulled {
                    self.told = true;
                } else if seen.wall < DEADLINE {
                    self.wrong.push(format!(
                        "wait: look {n}, of the whole digest, printed no PULL in {:.3?}, within the lease's deadline of {DEADLINE:.0?}",
                        seen.wall
                    ));
                }
                self.whole.push(seen.cpu);
            }
            Look::Since => self.since.push(seen.cpu),
        }
        look
    }

    /// What was wrong once the looking is done: what [`Looks::saw`] found, and the median of the
    /// looks of what changed since over [`WAIT`]. Fewer than three of those, when [`LOOKS`] ran
    /// out first, are bounded as they are: only looks of the whole digest printing no PULL past the
    /// deadline, which is load's doing, take the place of the others.
    fn wrong(mut self) -> Vec<String> {
        if self.since.len() < 3 {
            eprintln!(
                "perf: wait: {} of {} looks were of what changed since; the others built the whole digest, and each that printed no PULL ran past the lease's {DEADLINE:.0?} deadline (tools/pull-delivery-after-plan)",
                self.since.len(),
                self.whole.len() + self.since.len()
            );
        }
        if !self.since.is_empty() {
            let cpu = median(self.since.clone());
            eprintln!(
                "perf: wait: median CPU {cpu:.3?} of {:.3?} since a delivered look, bound {WAIT:.1?}; of the whole digest {:.3?}, bound {WHOLE:.1?}",
                self.since, self.whole
            );
            if cpu > WAIT {
                self.wrong.push(format!(
                    "wait: median CPU {cpu:.3?} of the looks since a delivered one, over its bound {WAIT:.1?}"
                ));
            }
        }
        self.wrong
    }
}

/// Looks of `tasks wait` by `waiter`, an orchestrator without a channel, each bounded by the work
/// it had to do, as the look before it says ([`Looks`]). What is over its bound, or wrong, is
/// returned.
fn looks(board: &Board, waiter: &str) -> Vec<String> {
    let mut looks = Looks::default();
    while looks.wanted() {
        let ran = board.run(&board.addr, &["wait", "--timeout", "0", "--as", waiter], "");
        assert_eq!(ran.code, 0, "wait: {}", ran.err);
        let seen = Seen {
            cpu: ran.tasks_cpu + ran.server_cpu,
            wall: ran.wall,
            pulled: pulled(&ran.out),
        };
        let look = looks.saw(&seen);
        // Its PULL's head, else its first line.
        let head: String = (ran.out.lines().find(|line| line.starts_with("PULL ")))
            .or_else(|| ran.out.lines().next())
            .unwrap_or("")
            .chars()
            .take(80)
            .collect();
        eprintln!(
            "perf: wait: a look of {look:?}: CPU {:.3?} ({:.3?}, {:.3?}), wall {:.3?}, {} lines, PULL {}: {head}",
            seen.cpu,
            ran.tasks_cpu,
            ran.server_cpu,
            seen.wall,
            ran.out.lines().count(),
            seen.pulled
        );
    }
    looks.wrong()
}

/// The bounds of the median CPU, for the test's own (debug) build of `tasks` on a machine other
/// agents' runs share. Measured on 2026-11-12 in five runs at load 23 to 140, one with a workspace
/// build beside it, the medians of CPU (wall): say 5.6 to 6.9 s (8.7 to 31.8 s), of which the
/// server's 1.6 to 2.1 s; claim 14.9 to 18.7 s (17.6 to 29.5 s), the server's 9.9 to 13.0 s;
/// inbox 0.12 to 0.16 s; the slowest api answer 0.08 to 0.11 s (`inbox`). Load beyond the cores
/// spreads a command over the efficiency cores and caches others share: it moved a say's CPU by
/// under a quarter where it moved its wall nearly fourfold, and a review's runs and later ones at
/// load 62 to 225 came within a tenth of the highest (say 7.3 s, claim 20.2 s, 2.7 and 3.0 times
/// under their bounds). The bounds are the highest of those medians times 2.9
/// (say), 3.2 (claim), 6.3 (inbox) and 4.5 (api), and far under what the regression guarded costs,
/// all of it the server's CPU: a say and a claim each read the inbox of every one of the 100
/// recipients (100 of a say's 823 requests), an inbox or an api `inbox` the orchestrator's once,
/// and the orchestrators' sessions read in one as `session WHERE handle IN (SELECT VALUE handle
/// FROM role WHERE role = 'orchestrator')`, the role scan again for each session, take 3.7 s of
/// the server's CPU on this board, where fetched by id they take 5 ms: 370 s for a say or a claim,
/// 3.7 s for an inbox.
const SAY: Duration = Duration::from_secs(20);
const CLAIM: Duration = Duration::from_secs(60);
const INBOX: Duration = Duration::from_secs(1);
const API: Duration = Duration::from_millis(500);
/// The median of three looks of `tasks wait` by an orchestrator without a channel, each after a
/// look that printed its PULL, which say what changed since, their process's start included.
/// Measured in the same runs and later ones: medians of 0.49 to 0.70 s (walls 0.49 to 2.9 s); the
/// bound is 4.3 times the highest.
const WAIT: Duration = Duration::from_secs(3);
/// Each look that builds the whole digest: the first, and one after a look that built it and
/// printed no PULL. Measured in the same runs, the review's and later ones: 10.6 to 14.4 s; the
/// bound is 3.1 times the highest.
const WHOLE: Duration = Duration::from_secs(45);
/// The lease's delivery deadline (`CALLBACK_LEASE_SECONDS / 2`, `store/callbacks.rs`), a real
/// clock's: a look holds the recipient's lease while it builds its PULL, and delivers nothing once
/// the deadline passed, leaving the PULL queued, so the next look builds the whole digest again.
/// Known and tracked as tools/pull-delivery-after-plan. So a look of the whole digest at a wall
/// past it, a loaded machine's, may print no PULL (its pane watch's WATCH it still prints), and
/// the look after it is of the whole digest too; one that printed none within it fails.
const DEADLINE: Duration = Duration::from_secs(30);
/// The most looks taken to time three of what changed since: past that, the looks timed so far are
/// bounded, and the test says how many of what changed since there were.
const LOOKS: usize = 6;

#[cfg(test)]
mod counted {
    use super::*;

    fn seen(cpu: f64, wall: f64, pulled: bool) -> Seen {
        Seen {
            cpu: Duration::from_secs_f64(cpu),
            wall: Duration::from_secs_f64(wall),
            pulled,
        }
    }

    /// A look's pane watch line while hc cannot be asked, as the generated board's first look
    /// prints it.
    const WATCH: &str = "WATCH hc unanswered at observation: hc ls: usage: the socket path";

    /// Each of `outcomes` counted in turn while more looks are wanted: what each was, how many
    /// were taken, and what was wrong.
    fn counted(outcomes: &[Seen]) -> (Vec<Look>, Vec<String>) {
        let mut looks = Looks::default();
        let mut what = Vec::new();
        for seen in outcomes {
            if !looks.wanted() {
                break;
            }
            what.push(looks.saw(seen));
        }
        (what, looks.wrong())
    }

    /// A look delivered its PULL when it printed one, whole or what changed, after its watch's
    /// line or alone; a look that printed only its watch's line, or nothing changed, did not.
    #[test]
    fn a_look_delivered_its_pull_only_when_it_printed_one() {
        assert!(pulled(&format!(
            "{WATCH}\nPULL executing=3 ready_unstaffed=27\n  ready: area01/task-2001"
        )));
        assert!(pulled("PULL counts unchanged\n  resolved low_volume keen"));
        assert!(!pulled(WATCH));
        assert!(!pulled(&format!("{WATCH}\n")));
        assert!(!pulled("WAIT nothing changed in 0 s\n"));
        assert!(!pulled(""));
        assert!(!pulled("\n"));
    }

    /// A look past the deadline that printed its watch's line and no PULL leaves the next look of
    /// the whole digest, as a run of the perf target at load 220 saw: 72 s of wall printing the
    /// WATCH alone, then the whole digest's PULL at 13.5 s of CPU, then three cheap looks.
    #[test]
    fn a_look_past_the_deadline_with_its_watch_alone_leaves_the_next_of_the_whole_digest() {
        let outs = [
            WATCH,
            "PULL executing=3 ready_unstaffed=27\n  ready: area01/task-2001",
            "WAIT nothing changed in 0 s",
            "WAIT nothing changed in 0 s",
            "WAIT nothing changed in 0 s",
        ];
        let costs = [
            (13.4, 72.3),
            (13.5, 25.7),
            (0.6, 1.6),
            (0.6, 1.1),
            (0.6, 1.0),
        ];
        let looked: Vec<Seen> = (outs.iter().zip(costs))
            .map(|(out, (cpu, wall))| seen(cpu, wall, pulled(out)))
            .collect();
        let (what, wrong) = counted(&looked);
        assert_eq!(what[..2], [Look::Whole; 2]);
        assert_eq!(what[2..], [Look::Since; 3]);
        assert_eq!(wrong, Vec::<String>::new());
    }

    /// The first look builds the whole digest; once one printed its PULL, the looks after it are of
    /// what changed since, whether they print one or not, and three of those end the looking.
    #[test]
    fn a_delivered_whole_digest_makes_the_looks_after_it_of_what_changed_since() {
        let (what, wrong) = counted(&[
            seen(12.0, 18.0, true),
            seen(0.5, 0.8, false),
            seen(0.6, 0.7, false),
            seen(0.4, 0.6, true),
            seen(0.5, 0.6, false),
        ]);
        assert_eq!(what, [Look::Whole, Look::Since, Look::Since, Look::Since]);
        assert_eq!(wrong, Vec::<String>::new());
    }

    /// A look of the whole digest past the deadline that printed no PULL leaves the next look
    /// of the whole digest too; that is load's, not wrong.
    #[test]
    fn a_whole_digest_undelivered_past_the_deadline_is_built_again_and_not_wrong() {
        let (what, wrong) = counted(&[
            seen(14.4, 116.6, false),
            seen(13.4, 63.6, false),
            seen(12.1, 18.4, true),
            seen(0.6, 0.9, false),
            seen(0.6, 0.9, false),
            seen(0.6, 0.9, false),
        ]);
        let whole = [Look::Whole; 3];
        assert_eq!(what[..3], whole);
        assert_eq!(what[3..], [Look::Since; 3]);
        assert_eq!(wrong, Vec::<String>::new());
    }

    /// A look of the whole digest that printed no PULL within the deadline is not the deadline's
    /// doing: wrong, and the next look is of the whole digest still.
    #[test]
    fn a_whole_digest_undelivered_within_the_deadline_is_wrong() {
        let (what, wrong) = counted(&[seen(12.0, 29.9, false), seen(12.0, 20.0, true)]);
        assert_eq!(what, [Look::Whole, Look::Whole]);
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        assert!(wrong[0].contains("look 1, of the whole digest, printed no PULL in 29.9"));
    }

    /// Looking ends after [`LOOKS`] looks, though fewer than three were of what changed since; it
    /// bounds those it timed, and says nothing is wrong for the deadline alone.
    #[test]
    fn looks_end_at_the_stated_number_and_bound_what_they_timed() {
        let outcomes: Vec<Seen> = (0..LOOKS + 2)
            .map(|n| seen(12.0, 40.0, n == LOOKS - 2))
            .collect();
        let (what, wrong) = counted(&outcomes);
        assert_eq!(what.len(), LOOKS);
        assert_eq!(what[..LOOKS - 1], vec![Look::Whole; LOOKS - 1][..]);
        assert_eq!(what[LOOKS - 1], Look::Since);
        // The one look of what changed since cost a whole digest's 12 s: over its bound.
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        assert!(wrong[0].contains("since a delivered one, over its bound"));
        let (_, none) = counted(
            &(0..LOOKS)
                .map(|_| seen(12.0, 40.0, false))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            none,
            Vec::<String>::new(),
            "only the deadline kept them all from delivering"
        );
    }

    /// The bounds: a look of the whole digest over [`WHOLE`], and a median of the looks of what
    /// changed since over [`WAIT`], each wrong; the median, not one look, is held.
    #[test]
    fn a_whole_digest_over_its_bound_and_a_median_since_over_its_are_wrong() {
        let (_, wrong) = counted(&[
            seen(46.0, 50.0, true),
            seen(3.5, 4.0, false),
            seen(0.5, 0.6, false),
            seen(3.1, 3.2, false),
        ]);
        assert_eq!(wrong.len(), 2, "{wrong:?}");
        assert!(wrong[0].contains("look 1, of the whole digest, took CPU 46"));
        assert!(wrong[1].contains("median CPU 3.1"));
        let (_, held) = counted(&[
            seen(12.0, 20.0, true),
            seen(9.0, 9.5, false),
            seen(0.5, 0.6, false),
            seen(0.6, 0.7, false),
        ]);
        assert_eq!(
            held,
            Vec::<String>::new(),
            "one slow look of three is not the median"
        );
    }
}
