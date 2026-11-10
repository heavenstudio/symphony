//! The last look before a kill (the close's own `hc get`): cleanup, finish and the idle sweep close
//! a terminal only while that look finds the program they judged, still finished. Work resumed by
//! then, another program in the terminal, or a call that cannot say keeps it, and the sweep reopens
//! its record. A shell is closable only with the shell itself in its foreground.

use crate::screen::{kept_only, left};
use crate::{Hooked, World};
use serde_json::{Value, json};
use std::cell::Cell;
use std::rc::Rc;
use std::slice::from_ref;
use symphony_board::launch::procs::reap_busy_reason;
use symphony_board::launch::{Closure, LaunchError};

/// What happens to a terminal (its address) in the world.
type Change = fn(&World, &str);

/// An hc whose `nth` `hc get` finds the terminal changed: `change` edits it just before that call
/// answers. `gets` counts the calls.
fn changes_at_get<'world>(
    world: &'world World,
    nth: usize,
    change: &'world dyn Fn(&str),
    gets: &'world Cell<usize>,
) -> Hooked<'world> {
    let mut hooked = Hooked::new(world);
    hooked.instead = Box::new(move |args| {
        if args[0] == "get" {
            gets.set(gets.get() + 1);
            if gets.get() == nth {
                change(args[1]);
            }
        }
        None
    });
    hooked
}

/// The terminal's agent goes back to work.
const WORKING: Change = |w, t| {
    w.edit_terminal(t, |t| {
        t.insert("status".into(), json!("working"));
    })
};
/// The terminal's agent no longer says what it is doing.
const SILENT: Change = |w, t| {
    w.edit_terminal(t, |t| {
        t.insert("status".into(), Value::Null);
    })
};
/// A job starts under the terminal's program.
const JOB: Change = |w, t| w.run_under(w.pid_of(t), &["cargo test -p x"]);
/// The ended program runs again.
const RUNS_AGAIN: Change = |w, t| {
    w.edit_terminal(t, |t| {
        t.insert("state".into(), json!("live"));
    })
};
/// Another instance of the program runs in the terminal, under a pid of its own.
const ANOTHER_PID: Change = |w, t| {
    let pid = w.new_pid();
    w.edit_terminal(t, |t| {
        t.insert("pid".into(), json!(pid));
    })
};
/// `--keep` put a login shell in place of the lost program, and the shell became a job by `exec`.
const REPLACED_BY_A_JOB: Change = |w, t| {
    w.edit_terminal(t, |t| {
        t.insert("argv".into(), json!(["/bin/zsh", "-l"]));
    });
    became_a_job(w, t, w.new_pid());
};
/// The terminal's shell becomes a job by `exec`, under its own pid.
const EXEC: Change = |w, t| became_a_job(w, t, w.pid_of(t));

/// `terminal`'s program, `pid` now, is a job: no agent and no status, `node active-job.js` in its
/// foreground (as hc shows a program after `exec`), and nothing under it.
fn became_a_job(w: &World, terminal: &str, pid: i64) {
    w.edit_terminal(terminal, |t| {
        t.remove("agent");
        t.insert("pid".into(), json!(pid));
        t.insert("status".into(), Value::Null);
        let job = json!({"pid": pid, "name": "node", "argv": ["node", "active-job.js"]});
        t.insert("foreground".into(), job);
    });
    w.run_under(pid, &[]);
}

/// What a terminal cleanup would close runs.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// A finished task's idle agent.
    Agent,
    /// A shell with nothing in it.
    Shell,
    /// A program that ended.
    Ended,
}

impl Kind {
    /// A terminal of this kind cleanup would close, and the handle it would close it for.
    fn start(self, w: &World) -> (Option<String>, String) {
        match self {
            Kind::Agent => {
                let (handle, terminal) = w.worker("sim/jump", Some("done"));
                (Some(handle), terminal)
            }
            Kind::Shell => (None, w.add_terminal(false, None, json!({}))),
            Kind::Ended => (
                None,
                w.add_terminal(false, None, json!({"state": "finished"})),
            ),
        }
    }
}

/// A terminal cleanup judged closable, changed right before the close's own look.
struct Case {
    kind: Kind,
    change: Change,
    /// Why cleanup keeps it.
    why: &'static str,
}

const ANOTHER: &str = "another program runs in it now";

#[test]
fn cleanup_keeps_a_terminal_its_last_get_finds_at_work_again_or_running_another_program() {
    let job = "work started under it: runs cargo test -p x";
    let cases = [
        Case {
            kind: Kind::Agent,
            change: WORKING,
            why: "its agent is working now",
        },
        Case {
            kind: Kind::Agent,
            change: SILENT,
            why: "its agent is silent now",
        },
        Case {
            kind: Kind::Agent,
            change: JOB,
            why: job,
        },
        Case {
            kind: Kind::Shell,
            change: JOB,
            why: job,
        },
        Case {
            kind: Kind::Ended,
            change: RUNS_AGAIN,
            why: "its program runs again",
        },
        Case {
            kind: Kind::Agent,
            change: ANOTHER_PID,
            why: ANOTHER,
        },
        Case {
            kind: Kind::Agent,
            change: REPLACED_BY_A_JOB,
            why: ANOTHER,
        },
        // The same pid: only the foreground says the shell is gone.
        Case {
            kind: Kind::Shell,
            change: EXEC,
            why: ANOTHER,
        },
    ];
    for Case { kind, change, why } in cases {
        let w = World::new();
        let (handle, terminal) = kind.start(&w);
        // The first get is cleanup's own look; the second, the close's, finds the change.
        let gets = Cell::new(0);
        let change = |t: &str| change(&w, t);
        let closures = w.clean_through(&changes_at_get(&w, 2, &change, &gets));
        let kept = left(&terminal, handle.as_deref(), why);
        assert_eq!(closures, [kept], "{kind:?}: {why}");
        assert_eq!(gets.get(), 2, "{kind:?}: {why}");
        assert!(w.has_terminal(&terminal), "{terminal} was closed");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        // Kept by that look, the window is not asked again: only cleanup's own look read it.
        assert_eq!(w.calls_of(&["app", "status"]).len(), 1, "{kind:?}: {why}");
        let sessions = w.store().task_sessions("sim/jump").unwrap();
        assert!(sessions.iter().all(|s| s.closed.is_none()), "{sessions:?}");
    }
}

#[test]
fn the_idle_sweep_keeps_a_terminal_its_last_get_finds_at_work_again_and_reopens_its_record() {
    let job = reap_busy_reason(&["cargo test -p x".to_string()]).unwrap();
    for (resume, why) in [
        (WORKING, "its agent is working now".to_string()),
        (JOB, format!("work started under it: {job}")),
    ] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        // The third get is the close's own, after the closure is recorded.
        let gets = Cell::new(0);
        let resume = |t: &str| resume(&w, t);
        let hooked = changes_at_get(&w, 3, &resume, &gets);
        let kept = kept_only(&terminal, &handle, &why, true);
        assert_eq!(w.sweep_through(&hooked), kept);
        assert_eq!(gets.get(), 3);
        assert!(w.has_terminal(&terminal), "{terminal} was closed");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        let [open] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!((open.closed, open.pane.as_str()), (None, terminal.as_str()));
        // Idle again, it waits a whole idle wait from the start before it closes.
        w.edit_terminal(&terminal, |t| {
            t.insert("status".into(), json!("idle"));
        });
        w.run_under(w.pid_of(&terminal), &[]);
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        assert_eq!(w.sweep_through(&w.hc()).closed.len(), 1);
        assert!(!w.has_terminal(&terminal), "{terminal} stayed");
    }
}

#[test]
fn the_idle_sweep_keeps_a_terminal_running_another_program_by_its_last_look_and_closes_an_ended_one()
 {
    // The sweep's side of cleanup's cases above (it never takes a shell, so never one's `exec`).
    let ended: Change = |w, t| {
        w.edit_terminal(t, |t| {
            t.insert("state".into(), json!("finished"));
        })
    };
    for (change, closes) in [
        (ANOTHER_PID, false),
        (REPLACED_BY_A_JOB, false),
        (ended, true),
    ] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        let gets = Cell::new(0);
        let change = |t: &str| change(&w, t);
        let swept = w.sweep_through(&changes_at_get(&w, 3, &change, &gets));
        assert_eq!(swept.closed.len(), usize::from(closes), "{swept:?}");
        assert_eq!(gets.get(), 3, "{swept:?}");
        assert_eq!(w.has_terminal(&terminal), !closes, "{terminal}");
        // An ended program has nothing to kill: it is only removed.
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!(record.closed.is_some(), closes, "{record:?}");
    }
}

#[test]
fn cleanup_keeps_a_terminal_whose_last_look_cannot_read_its_processes_or_finds_no_pid() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    // The ps table answers cleanup's own look, then fails the close's.
    let tables = Rc::new(Cell::new(0));
    let read = Rc::clone(&tables);
    let processes = move |_: i64| {
        read.set(read.get() + 1);
        if read.get() == 1 {
            Ok(Vec::new())
        } else {
            Err(LaunchError("ps: no answer".into()))
        }
    };
    let closures = symphony_board::launch::cleanup(w.store(), &w.hc(), &processes, false, &[]);
    let why = "never closed on no information: ps: no answer";
    assert_eq!(closures.unwrap(), [left(&terminal, Some(&handle), why)]);
    assert_eq!(tables.get(), 2);
    // hc knows no pid for it by the close's look.
    let gets = Cell::new(0);
    let no_pid = |t: &str| {
        w.edit_terminal(t, |t| {
            t.insert("pid".into(), Value::Null);
        })
    };
    let closures = w.clean_through(&changes_at_get(&w, 2, &no_pid, &gets));
    let why = "hc knows no pid for it now";
    assert_eq!(closures, [left(&terminal, Some(&handle), why)]);
    assert!(w.has_terminal(&terminal), "{terminal} was closed");
    assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
}

#[test]
fn cleanup_closes_a_shell_only_with_the_shell_itself_in_its_foreground() {
    let w = World::new();
    // A job the shell runs in front, before the process table lists it under the shell.
    let fronted = w.add_terminal(false, None, json!({}));
    let job = w.new_pid();
    w.edit_terminal(&fronted, |t| {
        let sleep = json!({"pid": job, "name": "sleep", "argv": ["sleep", "20"]});
        t.insert("foreground".into(), sleep);
    });
    // A job the shell became by `exec`: nothing under it, since what is under a program never
    // includes the program.
    let became = w.add_terminal(false, None, json!({}));
    EXEC(&w, &became);
    // Nothing hc shows in front says nothing is idle.
    let blank = w.add_terminal(false, None, json!({"foreground": null}));
    // An idle agent with another program holding its terminal's foreground.
    let (_, agent) = w.worker("sim/jump", Some("done"));
    let vim = w.new_pid();
    w.edit_terminal(&agent, |t| {
        let vim = json!({"pid": vim, "name": "vim", "argv": ["vim", "notes"]});
        t.insert("foreground".into(), vim);
    });
    // The login shell `--keep` starts, as hc shows it, idle at its prompt.
    let login = w.add_terminal(false, None, json!({"argv": ["/bin/zsh", "-l"]}));
    let pid = w.pid_of(&login);
    w.edit_terminal(&login, |t| {
        let zsh = json!({"pid": pid, "name": "zsh", "argv": ["/bin/zsh", "-l"]});
        t.insert("foreground".into(), zsh);
    });
    let closed = Closure {
        pane: login.clone(),
        handle: None,
        why: "an idle shell".into(),
        restore: String::new(),
        closed: true,
        kept: false,
    };
    assert_eq!(w.clean_through(&w.hc()), [closed]);
    for kept in [&fronted, &became, &blank, &agent] {
        assert!(w.has_terminal(kept), "{kept} was closed");
    }
    let killed: Vec<String> = w
        .calls_of(&["kill"])
        .into_iter()
        .map(|c| c[1].clone())
        .collect();
    assert_eq!(killed, [login]);
}
