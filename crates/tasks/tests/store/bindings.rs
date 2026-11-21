//! The checkout an in-process key's binding keeps: where a subagent's own tool calls run, and since
//! when, by the board's clock and the clock since boot, beside the session its orchestrator
//! registered elsewhere.
use crate::common::{Backend, Seed};

use crate::common::{Fixture, NOW};
use std::collections::BTreeMap;
use symphony_board::sys::Boottime;
use symphony_board::{Beat, Binding, NewSession};

mod provenance;

/// What the board keeps of the key the session `h` beats through.
fn binding_of(f: &Fixture, h: &str) -> Binding {
    f.store.agent_bindings().unwrap().remove(h).unwrap()
}

/// Subagent key `agent-1`'s binding, in `worktree` on `branch` since `came`, its calls there read
/// from `from` on, none of them reading the clock since boot.
fn kept(worktree: &str, branch: &str, came: f64, from: f64) -> Binding {
    Binding {
        agent: "agent-1".into(),
        in_process: true,
        worktree: worktree.into(),
        branch: branch.into(),
        placed_at: came,
        stay_from: from,
        boot_uncertain: !worktree.is_empty(),
        ..Binding::default()
    }
}

/// A reading of the clock since boot, `at` seconds into boot `boot`.
fn read(boot: &str, at: f64) -> Option<Boottime> {
    Some(Boottime {
        boot: boot.into(),
        at,
    })
}

/// Subagent key `agent-1`, bound to a session registered in the main checkout.
struct Subagent<'a> {
    f: &'a Fixture,
    handle: String,
}

impl<'a> Subagent<'a> {
    fn bound(f: &'a Fixture) -> Self {
        let handle = f.session_with(NewSession {
            worktree: "/main".into(),
            ..NewSession::new("worker")
        });
        assert!(f.store.bind_agent("agent-1", &handle, false).unwrap());
        Subagent { f, handle }
    }

    /// A tool call of its, `by` seconds after the board's clock, from the checkout given.
    fn call(&self, by: f64, place: Option<(&str, &str)>) {
        self.reading(by, place, None);
    }

    /// [`Subagent::call`], whose hook read the clock since boot as `boot`.
    fn reading(&self, by: f64, place: Option<(&str, &str)>, boot: Option<Boottime>) {
        self.f.advance(by);
        let beat = self
            .f
            .store
            .agent_beat_in("agent-1", &Beat::default(), place, boot.as_ref());
        assert_eq!(beat.unwrap().as_deref(), Some(self.handle.as_str()));
    }
}

on_both! {
fn a_subagents_binding_keeps_the_checkout_its_tool_calls_run_in_as_it_moves(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let s = Subagent::bound(&f);
    let h = s.handle.clone();
    assert_eq!(binding_of(&f, &h), kept("", "", 0.0, 0.0), "none seen yet");
    s.call(10.0, Some(("/wt/a", "tools/a")));
    let a = NOW + 10.0;
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "tools/a", a, a));
    s.call(10.0, Some(("/wt/a", "tools/a")));
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/a", "tools/a", a, a),
        "it came there once"
    );
    s.call(10.0, Some(("/wt/a", "")));
    let detached = NOW + 30.0;
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "", detached, detached));
    s.call(10.0, Some(("/wt/b", "tools/b")));
    let b = NOW + 40.0;
    assert_eq!(binding_of(&f, &h), kept("/wt/b", "tools/b", b, b));
    s.call(10.0, None);
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/b", "tools/b", b, b),
        "an unknown checkout moves nothing"
    );
    // Its session keeps where it registered, for readers that know no binding.
    assert_eq!(f.store.session(&h).unwrap().row.worktree, "/main");
}
}

on_both! {
/// The adversarial review's clock probe at the store: a call there that reads a time before any
/// other did lowers where its stay may date from, never when it came; a move starts both again.
fn a_clock_that_went_back_while_it_was_there_widens_its_stay_and_never_moves_when_it_came(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let s = Subagent::bound(&f);
    let h = s.handle.clone();
    s.call(0.0, Some(("/wt/a", "tools/a")));
    s.call(-5.0, Some(("/wt/a", "tools/a")));
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "tools/a", NOW, NOW - 5.0));
    s.call(2.0, Some(("/wt/a", "tools/a")));
    s.call(-4.0, None);
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/a", "tools/a", NOW, NOW - 5.0),
        "only the earliest counts, and only a call there"
    );
    s.call(-1.0, Some(("/wt/a", "tools/a")));
    assert_eq!(binding_of(&f, &h), kept("/wt/a", "tools/a", NOW, NOW - 8.0));
    s.call(20.0, Some(("/wt/b", "tools/b")));
    let b = NOW + 12.0;
    assert_eq!(binding_of(&f, &h), kept("/wt/b", "tools/b", b, b));
}
}

on_both! {
/// What a call read of the clock since boot (on which Linux dates a process's start): the reading
/// of the call that came to a checkout, and its latest call's, whatever the board's clock reads.
fn a_subagents_binding_keeps_the_clock_since_boot_its_calls_read_when_it_came_and_last(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let s = Subagent::bound(&f);
    let h = s.handle.clone();
    let a = Some(("/wt/a", "tools/a"));
    let at_a = |last: Option<Boottime>, boot_uncertain| Binding {
        placed_boot: read("boot-1", 100.0),
        last_boot: last,
        boot_uncertain,
        ..kept("/wt/a", "tools/a", NOW, NOW - 5.0)
    };
    s.reading(0.0, a, read("boot-1", 100.0));
    // The wall clock set back 5 s: the clock since boot runs on, a call outside any checkout too.
    s.reading(-5.0, a, read("boot-1", 160.0));
    s.reading(0.0, None, read("boot-1", 220.0));
    assert_eq!(binding_of(&f, &h), at_a(read("boot-1", 220.0), false));
    // Calls side by side may land out of order: the later reading stays.
    s.reading(0.0, a, read("boot-1", 200.0));
    assert_eq!(binding_of(&f, &h), at_a(read("boot-1", 220.0), false));
    // A new boot's reading replaces the last boot's; when it came stays a reading of that boot.
    s.reading(0.0, a, read("boot-2", 5.0));
    assert_eq!(binding_of(&f, &h), at_a(read("boot-2", 5.0), false));
    // A call that read none leaves its last reading unknown, until one reads it again.
    s.reading(0.0, a, None);
    assert_eq!(binding_of(&f, &h), at_a(None, true));
    s.reading(0.0, a, read("boot-2", 9.0));
    assert_eq!(binding_of(&f, &h), at_a(read("boot-2", 9.0), true));
    // A move starts both again, from that call's reading, or from none.
    s.reading(0.0, Some(("/wt/b", "tools/b")), read("boot-2", 30.0));
    let b = Binding {
        placed_boot: read("boot-2", 30.0),
        last_boot: read("boot-2", 30.0),
        boot_uncertain: false,
        ..kept("/wt/b", "tools/b", NOW - 5.0, NOW - 5.0)
    };
    assert_eq!(binding_of(&f, &h), b);
    s.reading(0.0, a, None);
    assert_eq!(
        binding_of(&f, &h),
        kept("/wt/a", "tools/a", NOW - 5.0, NOW - 5.0)
    );
}
}

on_both! {
/// Missing samples must not erase the ordering guard, even across repeated failures or reboots.
/// Arrival order alone cannot retire the uncertainty; a new checkout stay can.
fn unreadable_boot_samples_keep_the_watermark_and_uncertainty_until_a_new_stay(backend: Backend) {
    struct Sample {
        reading: Option<Boottime>,
        kept: Option<Boottime>,
    }
    let f = Fixture::on(backend, Seed::default());
    let s = Subagent::bound(&f);
    let place = Some(("/wt/a", "tools/a"));
    s.reading(0.0, place, read("boot-1", 100.0));
    s.reading(1.0, place, read("boot-1", 200.0));
    for sample in [
        Sample {
            reading: None,
            kept: None,
        },
        Sample {
            reading: read("boot-1", 100.0),
            kept: None,
        },
        Sample {
            reading: read("boot-1", 200.0),
            kept: None,
        },
        Sample {
            reading: None,
            kept: None,
        },
        Sample {
            reading: read("boot-1", 199.0),
            kept: None,
        },
        Sample {
            reading: read("boot-1", 200.5),
            kept: read("boot-1", 200.5),
        },
        Sample {
            reading: None,
            kept: None,
        },
        Sample {
            reading: read("boot-1", 200.5),
            kept: None,
        },
        Sample {
            reading: read("boot-2", 5.0),
            kept: read("boot-2", 5.0),
        },
        Sample {
            reading: read("boot-1", 999.0),
            kept: read("boot-2", 5.0),
        },
        Sample {
            reading: None,
            kept: None,
        },
        Sample {
            reading: read("boot-1", 999.0),
            kept: None,
        },
        Sample {
            reading: read("boot-2", 6.0),
            kept: read("boot-2", 6.0),
        },
        Sample {
            reading: read("boot-3", 1.0),
            kept: read("boot-3", 1.0),
        },
        Sample {
            reading: read("boot-2", 99.0),
            kept: read("boot-3", 1.0),
        },
    ] {
        // A wall-clock correction must not change boot ordering either.
        let reading = format!("{:?}", sample.reading);
        s.reading(-0.25, place, sample.reading);
        let expected = Binding {
            placed_boot: read("boot-1", 100.0),
            last_boot: sample.kept,
            boot_uncertain: true,
            ..kept("/wt/a", "tools/a", NOW, f.now().min(NOW))
        };
        assert_eq!(binding_of(&f, &s.handle), expected, "{reading}");
    }
    s.reading(10.0, Some(("/wt/b", "tools/b")), read("boot-3", 30.0));
    let expected = Binding {
        placed_boot: read("boot-3", 30.0),
        last_boot: read("boot-3", 30.0),
        boot_uncertain: false,
        ..kept("/wt/b", "tools/b", f.now(), f.now())
    };
    assert_eq!(binding_of(&f, &s.handle), expected);
}
}

/// Unknown and duplicate samples leave the metadata untouched once their evidence is kept.
// SurrealDB alone: installs a SurrealDB event to count writes to binding metadata.
#[test]
fn repeated_unknown_or_older_samples_do_not_rewrite_binding_metadata() {
    use serde_json::json;

    let f = Fixture::empty();
    let s = Subagent::bound(&f);
    let place = Some(("/wt/a", "tools/a"));
    s.reading(0.0, place, read("boot-1", 100.0));
    f.store.client().query("DEFINE TABLE callback_audit SCHEMALESS; DEFINE EVENT binding_writes ON callback_state WHEN true THEN { UPSERT callback_audit:writes SET count = (count ?? 0) + 1; };", json!({})).unwrap();
    let writes = || {
        (f.store.raw().row("callback_audit", "writes"))
            .field("count")
            .unwrap()
    };
    s.reading(1.0, place, None);
    assert_eq!(writes(), json!(1));
    s.reading(1.0, place, None);
    s.reading(1.0, place, read("boot-1", 99.0));
    s.reading(1.0, place, read("boot-1", 100.0));
    assert_eq!(
        writes(),
        json!(1),
        "uncertainty and watermark are already kept"
    );
    s.reading(1.0, place, read("boot-1", 101.0));
    assert_eq!(writes(), json!(2), "new evidence is written");
    s.reading(1.0, place, read("boot-1", 101.0));
    assert_eq!(writes(), json!(2), "a duplicate reading is already kept");
}

on_both! {
fn a_beat_from_a_checkout_is_one_request_as_a_beat_without_one(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session("worker");
    assert!(f.store.bind_agent("agent-1", &h, false).unwrap());
    let boot = |at: f64| Boottime {
        boot: "boot-1".into(),
        at,
    };
    let requests = |place, boot: Option<&Boottime>| {
        let before = f.store.database().requests();
        f.store
            .agent_beat_in("agent-1", &Beat::default(), place, boot)
            .unwrap();
        f.store.database().requests() - before
    };
    let plain = requests(None, None);
    assert_eq!(requests(Some(("/wt/a", "a")), Some(&boot(1.0))), plain);
    assert_eq!(requests(Some(("/wt/b", "b")), Some(&boot(2.0))), plain);
    f.advance(-5.0);
    assert_eq!(requests(Some(("/wt/b", "b")), Some(&boot(3.0))), plain);
    assert_eq!(requests(None, Some(&boot(2.0))), plain);
}
}

on_both! {
fn a_key_bound_to_no_session_keeps_no_checkout(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session("worker");
    f.store.leave(&h).unwrap();
    assert!(!f.store.bind_agent("agent-1", &h, false).unwrap());
    let boot = Boottime {
        boot: "boot-1".into(),
        at: 1.0,
    };
    let beat = f.store.agent_beat_in(
        "agent-1",
        &Beat::default(),
        Some(("/wt/a", "a")),
        Some(&boot),
    );
    assert_eq!(beat.unwrap(), None);
    let other = f.session("worker");
    assert!(f.store.bind_agent("agent-1", &other, false).unwrap());
    assert_eq!(binding_of(&f, &other), kept("", "", 0.0, 0.0));
}
}

on_both! {
fn a_full_sessions_binding_keeps_no_checkout_and_reads_as_no_subagents(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session("worker");
    assert!(f.store.bind_agent("session-1", &h, true).unwrap());
    let boot = Boottime {
        boot: "boot-1".into(),
        at: 1.0,
    };
    f.store
        .agent_beat_in(
            "session-1",
            &Beat::default(),
            Some(("/wt/a", "a")),
            Some(&boot),
        )
        .unwrap();
    let full = Binding {
        agent: "session-1".into(),
        ..Binding::default()
    };
    assert_eq!(binding_of(&f, &h), full);
    assert_eq!(
        f.store.bound_agents().unwrap(),
        BTreeMap::from([(h, "session-1".to_string())])
    );
}
}
