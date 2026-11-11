//! Exact CLI output consumed by agents and scripts.

use crate::common::NOW;
use crate::{Case, git, scratch};
use std::fs;
use symphony_board::sys::local_minute;
use symphony_board::{Message, NewLaunch, Role, Session, Task};

#[test]
fn each_command_says_what_it_did_in_one_line() {
    let c = Case::new();
    let (orch, exe, rev) = (
        c.register("orchestrator"),
        c.register("worker"),
        c.register("reviewer"),
    );
    let says = |who: &str, args: &[&str], said: &str| assert_eq!(c.ok(who, args), said, "{args:?}");
    says(&orch, &["create", "sim/a", "A"], "created sim/a (ready)\n");
    says(
        &orch,
        &["create", "sim/b", "B", "--stage", "design"],
        "created sim/b (design)\n",
    );
    says(
        &orch,
        &["role", "sim/a", "orchestrator"],
        "orchestrator on sim/a\n",
    );
    says(&exe, &["claim", "sim/a"], "claimed sim/a\n");
    says(&exe, &["reserve", "sim/b"], "reserved sim/b\n");
    says(&exe, &["release", "sim/b"], "released sim/b (design)\n");
    says(&exe, &["move", "sim/a", "review"], "sim/a is review\n");
    says(&rev, &["role", "sim/a", "reviewer"], "reviewer on sim/a\n");
    let sent = c.ok(&exe, &["say", "sim/a", "look", "--to", "@reviewer"]);
    let id = sent
        .strip_prefix("sent #")
        .and_then(|rest| rest.strip_suffix(" on sim/a\n"))
        .unwrap_or_default();
    assert!(
        !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()),
        "{sent:?}"
    );
    says(&rev, &["verdict", "sim/a", "approve"], "sim/a is done\n");
    says(
        &orch,
        &["parent", "sim/b", "sim/a"],
        "sim/b is under sim/a\n",
    );
    says(
        &orch,
        &["parent", "sim/b", "--none"],
        "sim/b is under nothing\n",
    );
    c.ok(
        &orch,
        &[
            "create",
            "sim/b/m",
            "M",
            "--parent",
            "sim/b",
            "--instruction",
        ],
    );
    c.ok(&exe, &["claim", "sim/b/m"]);
    says(&exe, &["done", "sim/b/m", "4 ms"], "sim/b/m is done\n");
    says(&exe, &["next"], "sim/b  p2  B\n");
    says(&exe, &["leave"], "left\n");
}

/// `show --json`'s `stages` for a task made and claimed at `NOW`, shown at `NOW`.
const STAGES: &str = r#"{
  "statuses": {
   "ready": {
    "first_entered": 1800000000.0,
    "seconds": 0.0,
    "visits": 1
   },
   "in_progress": {
    "first_entered": 1800000000.0,
    "seconds": 0.0,
    "visits": 1
   }
  },
  "unclaimed_seconds": 0.0,
  "first_claimed": 1800000000.0,
  "bounces": 0,
  "current": {
   "status": "in_progress",
   "since": 1800000000.0
  },
  "closed": null
 }"#;

#[test]
fn list_is_a_tree_of_aligned_rows() {
    let c = Case::new();
    let (orch, exe) = (c.register("orchestrator"), c.register("worker"));
    c.ok(&orch, &["create", "sim/a", "Alpha", "--priority", "1"]);
    c.ok(&orch, &["create", "sim/a/b", "Beta", "--parent", "sim/a"]);
    c.ok(
        &orch,
        &[
            "create",
            "a-very-long-slug/that-overflows-the-column",
            "Long",
        ],
    );
    c.ok(&exe, &["claim", "sim/a"]);
    c.ok(&orch, &["role", "sim/a", "orchestrator"]);
    // Roles taken at one moment come by handle.
    let mut held = [(&exe, "executor"), (&orch, "orchestrator")];
    held.sort();
    let held: Vec<String> = held.iter().map(|(h, role)| format!("{role} {h}")).collect();
    assert_eq!(
        c.ok("", &["list"]),
        [
            format!(
                "p1 in_progress sim/a                        Alpha  [{}]\n",
                held.join(", ")
            ),
            "  p2 ready       sim/a/b                      Beta\n".to_string(),
            "p2 ready       a-very-long-slug/that-overflows-the-column Long\n".to_string(),
        ]
        .concat()
    );
}

/// JSON as Python's `json.dumps(..., indent=1)` lays it out: one item a line, a space of indent a
/// level, every character past ASCII escaped, a float as its `repr`.
mod py {
    use super::*;

    pub fn text(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                ' '..='~' => out.push(c),
                _ => {
                    let mut units = [0u16; 2];
                    for unit in c.encode_utf16(&mut units) {
                        out.push_str(&format!("\\u{unit:04x}"));
                    }
                }
            }
        }
        out.push('"');
        out
    }

    pub fn float(x: f64) -> String {
        format!("{x:?}")
    }

    pub fn or_null(value: Option<String>) -> String {
        value.unwrap_or_else(|| "null".into())
    }

    /// The items, each already laid out at `depth + 1`.
    pub fn list(items: Vec<String>, depth: usize) -> String {
        if items.is_empty() {
            return "[]".into();
        }
        let pad = " ".repeat(depth + 1);
        let body: Vec<String> = items.iter().map(|item| format!("{pad}{item}")).collect();
        format!("[\n{}\n{}]", body.join(",\n"), " ".repeat(depth))
    }

    /// The fields in their order, each value already laid out at `depth + 1`.
    pub fn object(fields: Vec<(&str, String)>, depth: usize) -> String {
        let pad = " ".repeat(depth + 1);
        let body: Vec<String> = fields
            .iter()
            .map(|(key, value)| format!("{pad}{}: {value}", text(key)))
            .collect();
        format!("{{\n{}\n{}}}", body.join(",\n"), " ".repeat(depth))
    }

    pub fn texts(items: &[String], depth: usize) -> String {
        list(items.iter().map(|s| text(s)).collect(), depth)
    }

    fn role(r: &Role, depth: usize) -> String {
        object(
            vec![
                ("handle", text(&r.handle)),
                ("role", text(&r.role)),
                ("since", float(r.since)),
                ("expires", or_null(r.expires.map(float))),
                ("health", text(r.health.as_str())),
            ],
            depth,
        )
    }

    /// A task's JSON record, its fields in order.
    pub fn task(t: &Task, depth: usize) -> String {
        let roles = t.roles.iter().map(|r| role(r, depth + 2)).collect();
        object(
            vec![
                ("slug", text(&t.slug)),
                ("title", text(&t.title)),
                ("body", text(&t.body)),
                ("status", text(&t.status)),
                ("priority", t.priority.to_string()),
                ("parent", or_null(t.parent.as_deref().map(text))),
                ("created_by", text(&t.created_by)),
                ("created", float(t.created)),
                ("updated", float(t.updated)),
                ("links", texts(&t.links, depth + 1)),
                ("depends", texts(&t.depends, depth + 1)),
                ("roles", list(roles, depth + 1)),
                ("instruction", t.instruction.to_string()),
            ],
            depth,
        )
    }

    pub fn message(m: &Message, depth: usize) -> String {
        object(
            vec![
                ("id", m.id.to_string()),
                ("slug", text(&m.slug)),
                ("sender", text(&m.sender)),
                ("recipient", or_null(m.recipient.as_deref().map(text))),
                ("kind", text(&m.kind)),
                ("text", text(&m.text)),
                ("ts", float(m.ts)),
            ],
            depth,
        )
    }

    pub fn session(s: &Session, depth: usize) -> String {
        let r = &s.row;
        let affinity: Vec<String> = r
            .affinity
            .split(',')
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .collect();
        object(
            vec![
                ("handle", text(&r.handle)),
                ("kind", text(&r.kind)),
                ("parent", or_null(r.parent.as_deref().map(text))),
                ("worktree", text(&r.worktree)),
                ("branch", text(&r.branch)),
                ("model", text(&r.model)),
                ("pid", or_null(r.pid.map(|p| p.to_string()))),
                ("host", text(&r.host)),
                ("affinity", texts(&affinity, depth + 1)),
                ("connected", float(r.connected)),
                ("last_seen", float(r.last_seen)),
                ("last_tool", or_null(r.last_tool.map(float))),
                ("left_at", or_null(r.left_at.map(float))),
                ("health", text(s.health.as_str())),
            ],
            depth,
        )
    }
}

#[test]
fn json_is_the_records_as_python_lays_them_out() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    c.ok(
        &orch,
        &[
            "create",
            "sim/a",
            "Ünïcode — title",
            "--link",
            "docs/X.md#H",
        ],
    );
    c.ok(&orch, &["create", "sim/a/b", "B", "--parent", "sim/a"]);
    c.ok(&orch, &["claim", "sim/a"]);
    c.ok(&orch, &["say", "sim/a", "line\nbreak"]);
    let board = c.store();
    let tasks = |status: Option<&str>| -> String {
        let all = board.tasks(status).unwrap();
        let laid: Vec<String> = all.iter().map(|t| py::task(t, 2)).collect();
        py::object(vec![("tasks", py::list(laid, 1))], 0) + "\n"
    };
    assert_eq!(c.ok("", &["list", "--json"]), tasks(None));
    assert_eq!(
        c.ok("", &["list", "--json", "--status", "ready"]),
        tasks(Some("ready"))
    );
    let thread: Vec<String> = board
        .thread("sim/a")
        .unwrap()
        .iter()
        .map(|m| py::message(m, 2))
        .collect();
    let children: Vec<String> = board
        .children("sim/a")
        .unwrap()
        .iter()
        .map(|t| py::task(t, 2))
        .collect();
    let shown = py::object(
        vec![
            ("task", py::task(&board.get("sim/a").unwrap(), 1)),
            ("thread", py::list(thread, 1)),
            ("children", py::list(children, 1)),
            ("stages", STAGES.into()),
        ],
        0,
    );
    assert_eq!(c.ok("", &["show", "sim/a", "--json"]), shown + "\n");
    let sessions: Vec<String> = board
        .sessions()
        .unwrap()
        .iter()
        .map(|s| py::session(s, 2))
        .collect();
    assert_eq!(
        c.ok("", &["who", "--json"]),
        py::object(vec![("sessions", py::list(sessions, 1))], 0) + "\n"
    );
}

#[test]
fn the_python_layout_escapes_past_ascii_and_nests_by_one_space() {
    // The layout the case above holds the binary to, held to Python's own words for one record.
    // A backslash, `u` and four hex digits: Python's escape of one UTF-16 unit.
    let unit = |hex: &str| format!("{}u{hex}", '\\');
    let escaped = format!(
        "\"{}n{} {} \\\"q\\\"\\n{}{}\"",
        unit("00dc"),
        unit("00ef"),
        unit("2014"),
        unit("d83d"),
        unit("de00")
    );
    assert_eq!(py::text("Ünï — \"q\"\n😀"), escaped);
    let nested = py::object(
        vec![
            ("a", py::list(vec!["1".into(), py::list(vec![], 2)], 1)),
            ("b", py::float(1.5)),
        ],
        0,
    );
    assert_eq!(nested, "{\n \"a\": [\n  1,\n  []\n ],\n \"b\": 1.5\n}");
}

/// A thread line as `show` prints it.
fn line(m: &Message) -> String {
    let to = m
        .recipient
        .as_ref()
        .map(|r| format!(" to {r}"))
        .unwrap_or_default();
    format!(
        "[{}] {}{to} ({}): {}",
        local_minute(m.ts),
        m.sender,
        m.kind,
        m.text
    )
}

#[test]
fn show_prints_the_task_its_people_its_subtasks_its_body_and_its_thread() {
    let c = Case::new();
    let (orch, exe) = (c.register("orchestrator"), c.register("worker"));
    c.ok(&orch, &["create", "sim/base", "Base"]);
    c.ok(
        &orch,
        &[
            "create",
            "sim/a",
            "Alpha",
            "--priority",
            "1",
            "--link",
            "docs/A.md",
            "--link",
            "docs/B.md#H",
            "--depends",
            "sim/base",
            "--parent",
            "sim/base",
            "--body",
            "First line.\n\nSecond.\n\n",
        ],
    );
    c.ok(
        &orch,
        &[
            "create",
            "sim/a/do",
            "Do",
            "--parent",
            "sim/a",
            "--instruction",
        ],
    );
    c.ok(&orch, &["create", "sim/a/two", "Two", "--parent", "sim/a"]);
    c.ok(&orch, &["role", "sim/a", "orchestrator"]); // it waits on sim/base: nobody executes it yet
    c.ok(&exe, &["claim", "sim/a/do"]);
    c.ok(&exe, &["say", "sim/a", "on it"]);
    c.ok(&exe, &["say", "sim/a", "for you", "--to", "owner"]);
    let thread: Vec<String> = c
        .store()
        .thread("sim/a")
        .unwrap()
        .iter()
        .map(line)
        .collect();
    let mut expected = vec![
        "sim/a — Alpha  [ready, p1]".to_string(),
        format!("  orchestrator {orch} (live)"),
        "  links: docs/A.md, docs/B.md#H".into(),
        "  depends on: sim/base".into(),
        "  parent: sim/base".into(),
        "  Stages: unclaimed 0.0 h · ready 0.0 h".into(),
        "  subtasks:".into(),
        format!("    sim/a/do                     in_progress executor {exe}"),
        "    sim/a/two                    ready       -".into(),
        String::new(),
        "First line.\n\nSecond.".into(),
        String::new(),
    ];
    expected.extend(thread);
    assert_eq!(c.ok("", &["show", "sim/a"]), expected.join("\n") + "\n");
    let thread: Vec<String> = c
        .store()
        .thread("sim/a/do")
        .unwrap()
        .iter()
        .map(line)
        .collect();
    let mut expected = vec![
        "sim/a/do — Do  [in_progress, p2]".to_string(),
        format!("  executor {exe} (live)"),
        "  parent: sim/a".into(),
        "  Stages: unclaimed 0.0 h · ready 0.0 h · in_progress 0.0 h".into(),
        "  an instruction subtask: close it with `just tasks done sim/a/do \"what came of it\"`"
            .into(),
        String::new(),
    ];
    expected.extend(thread);
    assert_eq!(c.ok("", &["show", "sim/a/do"]), expected.join("\n") + "\n");
}

#[test]
fn show_lists_the_sessions_launched_for_it() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", "sim/a", "A"]);
    let rev = c.register("reviewer");
    let board = c.store();
    board
        .record_launch(&NewLaunch {
            claude_session: "sess-1".into(),
            worktree: "/wt/a".into(),
            branch: "sim/a".into(),
            pane: "w1:p1".into(),
            ..NewLaunch::new("sim/a", &orch, "executor")
        })
        .unwrap();
    board
        .record_launch(&NewLaunch {
            claude_session: "sess-2".into(),
            worktree: "/wt/b".into(),
            ..NewLaunch::new("sim/a", &rev, "reviewer")
        })
        .unwrap();
    board
        .close_task_session("sim/a", &rev, "idle", "r")
        .unwrap();
    let shown = c.ok("", &["show", "sim/a"]);
    let block = shown
        .split("  Sessions:\n")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .unwrap_or_default();
    let mut rows = [
        (
            &orch,
            format!(
                "    {orch:<18} {:<9} sess-1  /wt/a  sim/a  w1:p1  open",
                "executor"
            ),
        ),
        (
            &rev,
            format!(
                "    {rev:<18} {:<9} sess-2  /wt/b  (detached)  -  closed",
                "reviewer"
            ),
        ),
    ];
    rows.sort(); // started together: by handle
    let rows: Vec<String> = rows.into_iter().map(|(_, row)| row).collect();
    assert_eq!(block, rows.join("\n"), "{shown}");
}

#[test]
fn who_is_a_tree_of_sessions_with_what_each_holds() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let exe = c.register_with("worker", &["--parent", &orch]);
    let gone = c.register("orchestrator");
    let lone = c.register_with("reviewer", &["--parent", &gone]);
    c.ok(&gone, &["leave"]); // who leaves it out, so its child stands at the top
    c.ok(&orch, &["create", "sim/a", "A"]);
    c.ok(&exe, &["claim", "sim/a"]);
    c.ok(&orch, &["role", "sim/a", "orchestrator"]);
    let sessions = c.store().sessions().unwrap();
    let branch = |h: &str| {
        let b = &sessions
            .iter()
            .find(|s| s.row.handle == h)
            .unwrap()
            .row
            .branch;
        if b.is_empty() {
            "-".to_string()
        } else {
            b.clone()
        }
    };
    let row = |indent: &str, h: &str, kind: &str, holds: &str| {
        format!(
            "{indent}{h:<20} {kind:<12} {:<6} {:<24} {holds}",
            "live",
            branch(h)
        )
    };
    let mut expected = Vec::new();
    for s in sessions.iter().filter(|s| s.health.as_str() != "left") {
        let h = &s.row.handle;
        if *h == exe {
            continue;
        }
        if *h == orch {
            expected.push(row("", h, "orchestrator", "sim/a(orchestrator)"));
            expected.push(row("  ", &exe, "worker", "sim/a(executor)"));
        } else if *h == lone {
            expected.push(row("", h, "reviewer", ""));
        }
    }
    assert_eq!(expected.len(), 3);
    assert_eq!(c.ok("", &["who"]), expected.join("\n") + "\n");
}

#[test]
fn inbox_prints_one_line_a_message_then_nothing() {
    let c = Case::new();
    let (exe, rev) = (c.register("worker"), c.register("reviewer"));
    c.ok(&exe, &["create", "sim/a", "A"]);
    c.ok(&exe, &["claim", "sim/a"]);
    c.ok(&exe, &["move", "sim/a", "review"]);
    c.ok(&rev, &["role", "sim/a", "reviewer"]);
    c.ok(&rev, &["say", "sim/a", "two\nlines", "--to", &exe]);
    c.ok(&rev, &["say", "sim/a", "to all"]);
    c.ok(&rev, &["verdict", "sim/a", "changes", "the hash moved"]);
    assert_eq!(
        c.ok(&exe, &["inbox"]),
        format!(
            "MSG sim/a from {rev} to {exe} — two lines\n\
             VERDICT sim/a from {rev} — changes: the hash moved\n"
        )
    );
    assert_eq!(c.ok(&exe, &["inbox"]), "");
}

#[test]
fn export_writes_the_snapshot_and_says_where() {
    let c = Case::new();
    let (orch, exe) = (c.register("orchestrator"), c.register("worker"));
    c.ok(
        &orch,
        &[
            "create",
            "sim/a",
            "Alpha",
            "--link",
            "docs/SPEC.md#Jump",
            "--link",
            "notes.md",
        ],
    );
    c.ok(
        &orch,
        &[
            "create",
            "sim/b",
            "Beta",
            "--depends",
            "sim/a",
            "--priority",
            "1",
        ],
    );
    c.ok(&exe, &["claim", "sim/a"]);
    let out = scratch("export-says").join("deep").join("tasks.md");
    let path = out.to_str().unwrap();
    assert_eq!(
        c.ok("", &["export", "--out", path]),
        format!("wrote {path}\n")
    );
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        [
            "# Task board snapshot".to_string(),
            String::new(),
            format!(
                "Generated by `just tasks export` at {}. The live board is `just tasks list` and the Tasks \
                 page of `just symphony`; this file is a record, not the source.",
                local_minute(NOW)
            ),
            String::new(),
            "## ready".into(),
            String::new(),
            "- **sim/b** — Beta (p1) · after **sim/a**".into(),
            String::new(),
            "## in_progress".into(),
            String::new(),
            format!("- **sim/a** — Alpha (p2) · executor {exe} · [[SPEC#Jump]] [[notes.md]]"),
            String::new(),
        ]
        .join("\n")
    );
}

#[test]
fn a_refusal_is_its_reason_after_a_cross_and_exit_1() {
    let c = Case::new();
    let a = c.register("worker");
    let ran = c.run_as(&a, &["claim", "sim/none"], "");
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (1, "", "✖ no task 'sim/none'\n")
    );
    c.ok(&a, &["create", "sim/a", "A"]);
    for args in [
        &["parent", "sim/a"][..],
        &["parent", "sim/a", "sim/b", "--none"],
    ] {
        let ran = c.run_as(&a, args, "");
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err.as_str()),
            (1, "", "✖ name the parent, or pass --none\n"),
            "{args:?}"
        );
    }
}

#[test]
fn no_session_is_exit_2_saying_how_to_get_one() {
    let c = Case::new();
    for args in [
        &["claim", "sim/a"][..],
        &["launch", "sim/a"],
        &["list", "--mine"],
        &["next"],
        &["inbox"],
        &["claim", "sim/a", "--as="],
    ] {
        let ran = c.run_as("", args, "");
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err.as_str()),
            (
                2,
                "",
                "✖ no session: pass --as HANDLE or set TASKS_SESSION (get one with `just tasks-attach` \
                 or `just tasks register`)\n"
            ),
            "{args:?}"
        );
    }
}

#[test]
fn a_usage_error_is_argparses_and_exit_2() {
    let c = Case::new();
    let ran = c.run_as("", &["claim"], "");
    assert_eq!((ran.code, ran.out.as_str()), (2, ""));
    assert_eq!(
        ran.err,
        "usage: just tasks claim [-h] [--as ACTOR] slug\n\
         just tasks claim: error: the following arguments are required: slug\n"
    );
    let ran = c.run_as("", &["launch", "sim/a", "--model"], "");
    assert_eq!(
        (ran.code, ran.err.lines().last().unwrap_or_default()),
        (
            2,
            "just tasks launch: error: argument --model: expected one argument"
        )
    );
    let ran = c.run_as("", &["claim", "--help"], "");
    assert_eq!(
        (ran.code, ran.out.lines().next().unwrap_or_default()),
        (0, "usage: just tasks claim [-h] [--as ACTOR] slug")
    );
}

#[test]
fn the_board_unreachable_is_exit_2_in_one_line() {
    let mut c = Case::new();
    c.env
        .vars
        .insert("TASKS_SERVER".into(), "127.0.0.1:1".into());
    let ran = c.run_as("", &["list"], "");
    assert_eq!((ran.code, ran.out.as_str()), (2, ""));
    assert!(
        ran.err.starts_with("✖ ")
            && ran.err.contains("127.0.0.1:1")
            && ran.err.matches('\n').count() == 1,
        "{}",
        ran.err
    );
}

#[test]
fn register_records_where_it_was_run() {
    let mut c = Case::new();
    let repo = scratch("register-where").join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "sim/here"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "x"]);
    c.env.cwd = repo.clone();
    let handle = c.register_with("worker", &["--affinity", "sim, view,,", "--model", "opus"]);
    let s = c.store().session(&handle).unwrap().row;
    assert_eq!(
        (
            s.worktree.as_str(),
            s.branch.as_str(),
            s.model.as_str(),
            s.affinity.as_str(),
            s.pid,
            s.host.as_str()
        ),
        (
            repo.to_str().unwrap(),
            "sim/here",
            "opus",
            "sim,view",
            None,
            ""
        )
    );
}
