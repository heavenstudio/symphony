//! The timings: say, claim, inbox, looks of `tasks wait` and the `tasks api` answers land asks
//! for, each the `tasks` binary run as an agent runs it on the generated board, three times, the
//! median of the CPU each run cost within a bound: the `tasks` process's own and what the board's
//! server took for it ([`crate::cpu`]), from quiet before the run to quiet again after it
//! ([`Board::bill`]), so what the server does for a command after its answer counts too. That is
//! the work the command did, which the machine's load does not multiply as it multiplies a wall:
//! at load 170 on 16 cores a command gets about a tenth of a core, so its wall stretches some
//! tenfold while its work stays what it was. The walls are printed beside, for reading.
//! A say dispatches in the write (`TASKS_DISPATCH=inline`), so its cost is the write's and the
//! delivery to its recipients', 100 of them on this board. The looks of `tasks wait` are each
//! bounded by the work the look before it left them ([`looks`]): the first builds the whole digest
//! and prints it as its PULL, the ones after it what changed since. A look delivers what it built
//! however long that took (`Store::deliver_pending` times a delivery from its own start, by the
//! board's clock, which these commands pin), so a look of the whole digest that printed no PULL is
//! wrong at any load.
//!
//! The bounds are stated once, at the end, with what justifies them. On 2026-11-07 the live board
//! answered a say in 6 to 9 minutes and a claim in 2: each recipient's inbox read cost 7 s of the
//! server's time, all of it work, which a bound on CPU holds at any load.

use crate::board::{Board, board};
use crate::cpu;
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
    if let Err(stall) = board
        .listeners
        .hears(&recipient, "timed post 2", Duration::from_secs(5))
    {
        panic!(
            "the last post never reached its recipient: {stall}; it heard {:?}",
            board.listeners.heard(&recipient)
        );
    }
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
        // A write reads the board for each of its 100 recipients on the server, seconds of its
        // work in each run.
        if ["say", "claim"].contains(&name.as_str()) {
            for (_, server) in &cpus {
                (cpu::worked(&format!("{name}, on the board's server"), *server))
                    .unwrap_or_else(|why| panic!("{why}: {cpus:?}"));
            }
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
/// after a look that printed no PULL, which left it queued; or what changed since a look that
/// printed it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    Whole,
    Since,
}

/// Whether a look delivered its PULL, the digest the looks after it say what changed since: a line
/// of its output that is one, whole or what changed (`cli/pull.rs`). A look runs its pane watch
/// first, under a lease of its own (`Store::collect_callbacks`), so a look that delivered no PULL
/// may still print the watch's line, a WATCH while hc cannot be asked, which is not the digest.
fn pulled(out: &str) -> bool {
    out.lines().any(|line| line.starts_with("PULL "))
}

/// What one look cost and did.
struct Seen {
    cpu: Duration,
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
    /// held to [`WHOLE`], and one that printed no PULL is wrong, as a look delivers the digest it
    /// built however long that took.
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
                } else {
                    self.wrong.push(format!(
                        "wait: look {n}, of the whole digest, printed no PULL: a look delivers the digest it built, however long that took"
                    ));
                }
                self.whole.push(seen.cpu);
            }
            Look::Since => self.since.push(seen.cpu),
        }
        look
    }

    /// What was wrong once the looking is done: what [`Looks::saw`] found, and the median of the
    /// looks of what changed since over [`WAIT`].
    fn wrong(mut self) -> Vec<String> {
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
            ran.wall,
            ran.out.lines().count(),
            seen.pulled
        );
    }
    looks.wrong()
}

/// The bounds of the median CPU, for the test's own (debug) build of `tasks` on a machine other
/// agents' runs share. Measured on SurrealDB on 2026-11-12 in five runs at load 23 to 140, one
/// with a workspace build beside it, the medians of CPU (wall): say 5.6 to 6.9 s (8.7 to 31.8 s),
/// of which the server's 1.6 to 2.1 s; inbox 0.12 to 0.16 s; the slowest api answer 0.08 to
/// 0.11 s (`inbox`). The bounds are the highest of those medians times 2.9 (say), 6.3 (inbox) and 4.5 (api);
/// claim's is said below. Load beyond the cores spreads a command over the efficiency cores and
/// caches others share, which moves its CPU far less than its wall, but moves it: in a review's
/// runs and later ones at load 62 to 225, say took 6.3 to 9.2 s, the highest with a workspace build
/// from scratch beside it and 2.2 times under its bound, at walls up to 65 s; inbox and the api up
/// to 0.19 s and 0.16 s. All far under what the regression guarded
/// costs, all of it the server's CPU: a say and a claim each read the inbox of every one of the 100
/// recipients (100 of a say's 823 requests), an inbox or an api `inbox` the orchestrator's once,
/// and the orchestrators' sessions read in one as `session WHERE handle IN (SELECT VALUE handle
/// FROM role WHERE role = 'orchestrator')`, the role scan again for each session, take 3.7 s of
/// the server's CPU on this board, where fetched by id they take 5 ms: 370 s for a say or a claim,
/// 3.7 s for an inbox.
///
/// A claim's write builds the PULL of the orchestrator it makes due, whose unanswered lookup reads
/// the open tasks' threads alone (`store/unanswered.rs`). Measured on SurrealDB on 2026-11-13,
/// the claim's median CPU was 6.8 s (the server's 1.6 to 2.1 s) at load 12 to 29, and 9.4 s (the server's 2.7
/// to 2.9 s) at load 75 to 124: its bound is 3.5 and 2.6 times those. It does not bound the
/// lookup alone, which its own test does (`unanswered`). The bounds are kept on PostgreSQL, whose
/// figures each run prints.
const SAY: Duration = Duration::from_secs(20);
const CLAIM: Duration = Duration::from_secs(24);
const INBOX: Duration = Duration::from_secs(1);
const API: Duration = Duration::from_millis(500);
/// The median of three looks of `tasks wait` by an orchestrator without a channel, each after a
/// look that printed its PULL, which say what changed since, their process's start included.
/// Measured in the same runs and later ones: medians of 0.49 to 0.70 s alone and up to 0.82 s with
/// a build beside them (walls 0.49 to 6.5 s); the bound is 3.7 times the highest.
const WAIT: Duration = Duration::from_secs(3);
/// Each look that builds the whole digest: the first, and one after a look that printed no PULL.
/// Measured in the same runs, the review's and later ones: 10.6 to 14.4 s alone and up to 19.4 s
/// with a build beside them; the bound is 2.3 times the highest.
const WHOLE: Duration = Duration::from_secs(45);
/// The most looks taken to time three of what changed since: past that, on a board where looks
/// print no PULL, each of them wrong, the looks timed so far are bounded.
const LOOKS: usize = 6;

#[cfg(test)]
mod counted {
    use super::*;

    fn seen(cpu: f64, pulled: bool) -> Seen {
        Seen {
            cpu: Duration::from_secs_f64(cpu),
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

    /// A look of the whole digest that printed its watch's line and no PULL is wrong, however
    /// long it took, and leaves the next look of the whole digest, which prints it; three cheap
    /// looks follow.
    #[test]
    fn a_whole_digest_with_its_watch_alone_is_wrong_and_the_next_look_builds_it_again() {
        let outs = [
            WATCH,
            "PULL executing=3 ready_unstaffed=27\n  ready: area01/task-2001",
            "WAIT nothing changed in 0 s",
            "WAIT nothing changed in 0 s",
            "WAIT nothing changed in 0 s",
        ];
        let costs = [13.4, 13.5, 0.6, 0.6, 0.6];
        let looked: Vec<Seen> = (outs.iter().zip(costs))
            .map(|(out, cpu)| seen(cpu, pulled(out)))
            .collect();
        let (what, wrong) = counted(&looked);
        assert_eq!(what[..2], [Look::Whole; 2]);
        assert_eq!(what[2..], [Look::Since; 3]);
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        assert!(wrong[0].contains("look 1, of the whole digest, printed no PULL"));
    }

    /// The first look builds the whole digest; once one printed its PULL, the looks after it are of
    /// what changed since, whether they print one or not, and three of those end the looking.
    #[test]
    fn a_delivered_whole_digest_makes_the_looks_after_it_of_what_changed_since() {
        let (what, wrong) = counted(&[
            seen(12.0, true),
            seen(0.5, false),
            seen(0.6, false),
            seen(0.4, true),
            seen(0.5, false),
        ]);
        assert_eq!(what, [Look::Whole, Look::Since, Look::Since, Look::Since]);
        assert_eq!(wrong, Vec::<String>::new());
    }

    /// Looking ends after [`LOOKS`] looks when none prints its PULL, each of them wrong, and bounds
    /// a look of what changed since that came last.
    #[test]
    fn looks_end_at_the_stated_number_when_they_print_no_pull_each_wrong() {
        let (what, wrong) = counted(
            &(0..LOOKS + 2)
                .map(|_| seen(12.0, false))
                .collect::<Vec<_>>(),
        );
        assert_eq!(what, vec![Look::Whole; LOOKS]);
        assert_eq!(wrong.len(), LOOKS, "{wrong:?}");
        let outcomes: Vec<Seen> = (0..LOOKS + 2).map(|n| seen(12.0, n == LOOKS - 2)).collect();
        let (what, wrong) = counted(&outcomes);
        assert_eq!(what[..LOOKS - 1], vec![Look::Whole; LOOKS - 1][..]);
        assert_eq!(what[LOOKS - 1..], [Look::Since]);
        // Four looks printed no PULL, and the one look of what changed since cost a whole
        // digest's 12 s: over its bound.
        assert_eq!(wrong.len(), LOOKS - 1, "{wrong:?}");
        assert!(wrong[LOOKS - 2].contains("since a delivered one, over its bound"));
    }

    /// The bounds: a look of the whole digest over [`WHOLE`], and a median of the looks of what
    /// changed since over [`WAIT`], each wrong; the median, not one look, is held.
    #[test]
    fn a_whole_digest_over_its_bound_and_a_median_since_over_its_are_wrong() {
        let (_, wrong) = counted(&[
            seen(46.0, true),
            seen(3.5, false),
            seen(0.5, false),
            seen(3.1, false),
        ]);
        assert_eq!(wrong.len(), 2, "{wrong:?}");
        assert!(wrong[0].contains("look 1, of the whole digest, took CPU 46"));
        assert!(wrong[1].contains("median CPU 3.1"));
        let (_, held) = counted(&[
            seen(12.0, true),
            seen(9.0, false),
            seen(0.5, false),
            seen(0.6, false),
        ]);
        assert_eq!(
            held,
            Vec::<String>::new(),
            "one slow look of three is not the median"
        );
    }
}
