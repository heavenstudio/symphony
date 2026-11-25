//! CLI text output and JSON record values consumed by agents and scripts.

use crate::common::NOW;
use crate::{Case, git, scratch};
use std::fs;
use symphony_board::sys::local_minute;
use symphony_board::usage::Prices;
use symphony_board::{Message, NewLaunch, Role, Session, Task};

#[test]
fn each_command_says_what_it_did_in_one_line() {
    let mut c = Case::new();
    // Past its first live line, orch stays once it holds sim/a, so the board gives it sim/b too.
    let (orch, exe, rev) = (
        c.register_settled("orchestrator"),
        c.register("worker"),
        c.register("reviewer"),
    );
    let says = |who: &str, args: &[&str], said: &str| assert_eq!(c.ok(who, args), said, "{args:?}");
    says(
        &orch,
        &["create", "sim/a", "A"],
        &format!("created sim/a (ready); {orch} orchestrates it\n"),
    );
    says(
        &exe,
        &["create", "sim/b", "B", "--stage", "design"],
        &format!("created sim/b (design); {orch} orchestrates it\n"),
    );
    says(
        &orch,
        &["role", "sim/b", "orchestrator"],
        "orchestrator on sim/b\n",
    );
    says(
        &exe,
        &["claim", "sim/a"],
        "claimed sim/a: its work goes on the branch sim/a, which the land takes\n",
    );
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
    // Done is not landed: outside a checkout the default landing is named.
    says(
        &rev,
        &["verdict", "sim/a", "approve"],
        "sim/a is approved: done on the board, not on the default branch until it lands; `just \
         land sim/a --as HANDLE` lands it\n",
    );
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
    // An instruction subtask's slug is never a branch: its claim names none.
    says(&exe, &["claim", "sim/b/m"], "claimed sim/b/m\n");
    says(&exe, &["done", "sim/b/m", "4 ms"], "sim/b/m is done\n");
    // Nor is a slice's, which works on its parent's branch and lands with it: its claim names no
    // branch.
    c.ok(&orch, &["create", "sim/b/s", "S", "--parent", "sim/b"]);
    says(&exe, &["claim", "sim/b/s"], "claimed sim/b/s\n");
    // A subtask never goes to review, but a task put under another while in review is one: its
    // approve names no land of its own.
    c.ok(&orch, &["create", "sim/c", "C"]);
    c.ok(&exe, &["claim", "sim/c"]);
    c.ok(&exe, &["move", "sim/c", "review"]);
    c.ok(&rev, &["role", "sim/c", "reviewer"]);
    c.ok(&orch, &["parent", "sim/c", "sim/b"]);
    says(
        &rev,
        &["verdict", "sim/c", "approve"],
        "sim/c is approved: done on the board; its work lands when the task it is under, sim/b, \
         does\n",
    );
    says(&exe, &["next"], "sim/b  p2  B\n");
    says(&exe, &["leave"], "left\n");
}

/// `show --json`'s `stages` for a task made and claimed at `NOW` by `by`, shown at `NOW`.
fn stages(by: &str) -> String {
    STAGES.replace("BY", by)
}

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
  "segments": [
   {
    "status": "ready",
    "from": 1800000000.0,
    "to": 1800000000.0,
    "by": "BY"
   },
   {
    "status": "in_progress",
    "from": 1800000000.0,
    "to": null,
    "by": "BY"
   }
  ],
  "unclaimed_seconds": 0.0,
  "first_claimed": 1800000000.0,
  "bounces": 0,
  "current": {
   "status": "in_progress",
   "since": 1800000000.0
  },
  "closed": null
 }"#;

/// `show --json`'s `tokens` for a task none of whose sessions' beats spent any: every sum zero,
/// priced at the table every build embeds, which says the day its prices were read.
fn no_tokens() -> String {
    NO_TOKENS.replace("READ", &Prices::embedded().read)
}

const NO_TOKENS: &str = r#"{
  "total": {
   "input": 0,
   "cache_write_5m": 0,
   "cache_write_1h": 0,
   "cache_read": 0,
   "output": 0,
   "calls": 0
  },
  "cost": {
   "dollars": 0.0,
   "input": 0.0,
   "cache_write_5m": 0.0,
   "cache_write_1h": 0.0,
   "cache_read": 0.0,
   "output": 0.0,
   "unpriced": [],
   "prices": "READ"
  },
  "by_role": [],
  "by_round": [],
  "rows": []
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
    // Every title starts one column after the longest slug, a child's indent counted.
    let long = "a-very-long-slug/that-overflows-the-column";
    assert_eq!(
        c.ok("", &["list"]),
        [
            format!(
                "p1 in_progress {:<42} Alpha  [{}]\n",
                "sim/a",
                held.join(", ")
            ),
            format!("  p2 ready       {:<40} Beta\n", "sim/a/b"),
            format!("p2 ready       {long} Long  [orchestrator {orch}]\n"),
        ]
        .concat()
    );
}

#[test]
fn list_leaves_finished_tasks_out_unless_asked_and_says_when_nothing_is_open() {
    let c = Case::new();
    assert_eq!(c.ok("", &["list"]), "no tasks\n");
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", "sim/a", "Alpha"]);
    c.ok(&orch, &["create", "sim/b", "Beta"]);
    c.ok(&orch, &["move", "sim/b", "cancelled"]);
    let open = c.ok("", &["list"]);
    assert!(open.contains("sim/a") && !open.contains("sim/b"), "{open}");
    for args in [&["list", "--all"][..], &["list", "--status", "cancelled"]] {
        assert!(c.ok("", args).contains("sim/b"), "{args:?}");
    }
    let json = c.ok("", &["list", "--json"]);
    assert!(json.contains("sim/a") && !json.contains("sim/b"), "{json}");
    assert!(c.ok("", &["list", "--json", "--all"]).contains("sim/b"));
    c.ok(&orch, &["move", "sim/a", "cancelled"]);
    assert_eq!(
        c.ok("", &["list"]),
        "no open tasks (--all lists done and cancelled ones too)\n"
    );
}

/// Reference JSON fixtures, independent of the production serializer.
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
                ("id", t.id.to_string()),
            ],
            depth,
        )
    }

    /// A task's record as `list --json` lists it: [`task`]'s fields, then when it took its status.
    pub fn listed(t: &Task, in_status_since: f64, depth: usize) -> String {
        let record = task(t, depth);
        let closing = format!("\n{}}}", " ".repeat(depth));
        let fields = record.strip_suffix(&closing).expect("an object's end");
        format!(
            "{fields},\n{}\"in_status_since\": {}{closing}",
            " ".repeat(depth + 1),
            float(in_status_since)
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
fn json_preserves_the_reference_record_values() {
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
    // The case's clock stands still, so each task took the status it holds when it was made.
    let tasks = |status: Option<&str>| -> String {
        let all = board.tasks(status).unwrap();
        let laid: Vec<String> = all.iter().map(|t| py::listed(t, t.created, 2)).collect();
        py::object(vec![("tasks", py::list(laid, 1))], 0) + "\n"
    };
    same_json(&c.ok("", &["list", "--json"]), &tasks(None));
    same_json(
        &c.ok("", &["list", "--json", "--status", "ready"]),
        &tasks(Some("ready")),
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
            ("held", board.held_count("sim/a").unwrap().to_string()),
            ("children", py::list(children, 1)),
            ("stages", stages(&orch)),
            ("tokens", no_tokens()),
        ],
        0,
    );
    same_json(&c.ok("", &["show", "sim/a", "--json"]), &shown);
    let sessions: Vec<String> = board
        .sessions()
        .unwrap()
        .iter()
        .map(|s| py::session(s, 2))
        .collect();
    same_json(
        &c.ok("", &["who", "--json"]),
        &py::object(vec![("sessions", py::list(sessions, 1))], 0),
    );
}

fn same_json(actual: &str, expected: &str) {
    let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
    assert_eq!(parse(actual), parse(expected));
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
        // What it waits on: the executor of its slice, at work under it.
        format!("  Waits: on working: {exe} · executor of sim/a/do · 0 s"),
        "  links: docs/A.md, docs/B.md#H".into(),
        "  depends on: sim/base".into(),
        "  parent: sim/base".into(),
        "  Stages: ready 0.0 h; never claimed in 0.0 h".into(),
        "  subtasks:".into(),
        // The slugs padded to the longest of them.
        format!("    sim/a/do  in_progress executor {exe}"),
        "    sim/a/two ready       -".into(),
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
        format!("  Waits: on working: {exe} · executor · 0 s"),
        "  parent: sim/a".into(),
        "  Stages: ready 0.0 h · in_progress 0.0 h; first claimed after 0.0 h".into(),
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
        [
            format!("{indent}{h}"),
            kind.into(),
            "live".into(),
            branch(h),
            holds.into(),
        ]
    };
    let mut expected = vec![["HANDLE", "KIND", "HEALTH", "BRANCH", "ROLES"].map(String::from)];
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
    assert_eq!(expected.len(), 4);
    // A column as wide as its widest cell and two spaces between; the last unpadded.
    let widths: Vec<usize> = (0..5)
        .map(|i| expected.iter().map(|r| r[i].chars().count()).max().unwrap())
        .collect();
    let lines: Vec<String> = expected
        .iter()
        .map(|r| {
            let cells: Vec<String> = r
                .iter()
                .zip(&widths)
                .map(|(cell, width)| format!("{cell:<width$}"))
                .collect();
            cells.join("  ").trim_end().to_string()
        })
        .collect();
    assert_eq!(c.ok("", &["who"]), lines.join("\n") + "\n");
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
    // No orchestrator live: the tasks wait for one, holding none.
    let exe = c.register("worker");
    c.ok(
        &exe,
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
        &exe,
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

/// Under the marker the board's recipe sets (`just tasks`, which an agent runs unasked), the
/// snapshot goes only into the checkout it runs in, and from a linked worktree into the main
/// checkout's artifacts folder; never into a git folder, the rest of the main checkout or anywhere
/// else, however the path leads there (`..`, a link, as written), and it replaces only an earlier
/// snapshot. A refusal is one line and writes nothing; the binary run by itself still writes where
/// it is told.
#[test]
fn export_under_the_board_recipe_writes_only_into_the_project() {
    let mut c = Case::new();
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", "sim/a", "Alpha"]);
    let dir = scratch("export-confined");
    let main = crate::repository(&dir.join("shop"));
    git(&main, &["worktree", "add", "-q", "-b", "tree", "../tree"]);
    let tree = dir.join("tree");
    let out = dir.join("elsewhere");
    fs::create_dir_all(&out).unwrap();
    std::os::unix::fs::symlink(&out, tree.join("out-link")).unwrap();
    let hook = main.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    fs::create_dir_all(tree.join("notes")).unwrap();
    let own = tree.join("notes/own.md");
    fs::write(&own, "mine\n").unwrap();
    let untouched = |out: &std::path::Path| fs::read_dir(out).unwrap().next().is_none();
    c.env.cwd = tree.clone();
    c.env
        .vars
        .insert(symphony_board::confine::MARKER.into(), "1".into());

    // Its default, a folder in the worktree and the main checkout's artifacts folder pass, and so
    // does each again over the snapshot it wrote.
    let main_copy = main.join("artifacts/board.md");
    for _ in 0..2 {
        assert_eq!(c.ok("", &["export"]), "wrote docs/history/tasks.md\n");
        for path in [
            "notes/board.md".to_string(),
            main_copy.display().to_string(),
        ] {
            assert_eq!(
                c.ok("", &["export", "--out", &path]),
                format!("wrote {path}\n")
            );
        }
    }
    for written in [
        tree.join("docs/history/tasks.md"),
        tree.join("notes/board.md"),
        main_copy,
    ] {
        assert!(
            fs::read_to_string(&written)
                .unwrap()
                .starts_with("# Task board snapshot\n"),
            "{}",
            written.display()
        );
    }

    let places = format!(
        "only the checkout at {}, none of its git folders, and the main checkout's artifacts \
         folder at {}",
        tree.display(),
        main.join("artifacts").display()
    );
    let plain = out.join("board.md").display().to_string();
    for (path, place) in [
        // A git folder: the common dir's hook or config, the worktree's own `.git` file.
        (
            "../shop/.git/hooks/pre-commit".to_string(),
            format!("leads to {}, which is", hook.display()),
        ),
        (
            "../shop/.git/board.md".to_string(),
            format!(
                "leads to {}, which is",
                main.join(".git/board.md").display()
            ),
        ),
        (
            ".git".to_string(),
            format!("leads to {}, which is", tree.join(".git").display()),
        ),
        // A new `.git` in any case, which a volume that ignores case gives git as its own.
        (
            "notes/.GIT".to_string(),
            format!("leads to {}, which is", tree.join("notes/.GIT").display()),
        ),
        (
            "../shop/artifacts/.Git/board.md".to_string(),
            format!(
                "leads to {}, which is",
                main.join("artifacts/.Git/board.md").display()
            ),
        ),
        // The rest of the main checkout.
        (
            "../shop/justfile".to_string(),
            format!("leads to {}, which is", main.join("justfile").display()),
        ),
        // Anywhere else.
        (
            "../elsewhere/deep/board.md".to_string(),
            format!("leads to {}, which is", out.join("deep/board.md").display()),
        ),
        (
            "out-link/board.md".to_string(),
            format!("leads to {}, which is", out.join("board.md").display()),
        ),
        (
            "notes/../../elsewhere/board.md".to_string(),
            format!("leads to {}, which is", out.join("board.md").display()),
        ),
        // Already where it leads, it is named once.
        (plain, "is".to_string()),
    ] {
        let refused = c.run_as("", &["export", "--out", &path], "");
        assert_eq!(
            (refused.code, refused.out.as_str(), refused.err),
            (
                1,
                "",
                format!(
                    "✖ {path} {place} not a place a recipe an agent runs unasked writes: \
                     {places}; the binary run by itself writes anywhere\n"
                )
            ),
            "{path}"
        );
        assert!(untouched(&out), "{path} wrote into {}", out.display());
    }
    assert_eq!(fs::read_to_string(&hook).unwrap(), "#!/bin/sh\nexit 0\n");
    assert!(!main.join(".git/board.md").exists() && !main.join("justfile").exists());
    assert!(
        fs::read_to_string(tree.join(".git"))
            .unwrap()
            .starts_with("gitdir: ")
    );

    // A file of the checkout's own that is no snapshot stays as it is.
    let refused = c.run_as("", &["export", "--out", "notes/own.md"], "");
    assert_eq!(
        (refused.code, refused.out.as_str(), refused.err),
        (
            1,
            "",
            "✖ notes/own.md holds something other than an earlier snapshot: the board recipe, \
             which an agent runs unasked, replaces only a file whose first line is \
             `# Task board snapshot`, and the binary run by itself anything\n"
                .to_string()
        )
    );
    assert_eq!(fs::read_to_string(&own).unwrap(), "mine\n");

    // Without the marker, the binary run by itself, the snapshot goes where it is told.
    c.env.vars.remove(symphony_board::confine::MARKER);
    let elsewhere = out.join("deep/board.md");
    let path = elsewhere.display().to_string();
    for path in [path.as_str(), "notes/own.md"] {
        assert_eq!(
            c.ok("", &["export", "--out", path]),
            format!("wrote {path}\n")
        );
    }
    assert!(elsewhere.is_file());
    assert!(
        fs::read_to_string(&own)
            .unwrap()
            .starts_with("# Task board snapshot\n")
    );
}

#[test]
fn a_line_a_mistyped_flag_suggests_reaches_the_scratch_board_it_was_typed_for() {
    let c = Case::new();
    let a = c.register("worker");
    let database = c.env.vars["TASKS_DATABASE"].clone();
    let server = c.env.vars["TASKS_SERVER"].clone();
    // The line begins with the command that reaches the board however it was chosen, as the
    // board's refusals name it: typed again in a shell with neither variable, it reaches the same
    // board, never the project's.
    let suggests = |case: &Case, args: &[&str], board: &str| {
        let ran = case.run_as(&a, args, "");
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.err);
        assert!(
            ran.err
                .ends_with(&format!("did you mean `{board} list --status ready`?\n")),
            "{args:?}: {}",
            ran.err
        );
    };
    // Its database by `--database`, its server by the environment.
    suggests(
        &c,
        &["--database", &database, "list", "--stauts", "ready"],
        &c.board(),
    );
    // Both by the environment alone.
    suggests(&c, &["list", "--stauts", "ready"], &c.board());
    // `--database` wins over the environment's, as it does for the run.
    suggests(
        &c,
        &["--database", "other1", "list", "--stauts", "ready"],
        &format!("TASKS_SERVER={server} just tasks --database other1"),
    );
    // The server alone: the project's database on it, which only the server names.
    let mut on_server = Case { env: c.env.clone() };
    on_server.env.vars.remove("TASKS_DATABASE");
    suggests(
        &on_server,
        &["list", "--stauts", "ready"],
        &format!("TASKS_SERVER={server} just tasks"),
    );
}

#[test]
fn a_refusal_is_its_reason_after_a_cross_and_exit_1() {
    let c = Case::new();
    let a = c.register("worker");
    // The commands that find a task name this board.
    let board = c.board();
    let find = |slug: &str| {
        format!(
            "✖ no task {slug}: `{board} search \"WORDS\"` finds a task by its words, `{board} list \
             --all` lists every one\n"
        )
    };
    let ran = c.run_as(&a, &["claim", "sim/none"], "");
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.clone()),
        (1, "", find("'sim/none'"))
    );
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["create", "sim/alpha", "Alpha"]);
    c.ok(&a, &["create", "sim/alpine", "Alpine"]);
    // A mistyped slug names the slugs nearest it, up to three.
    for (args, said) in [
        (
            &["claim", "sim/alpah"][..],
            "✖ no task 'sim/alpah'; did you mean 'sim/alpha'?\n".to_string(),
        ),
        (
            &["show", "sim/alpne"],
            "✖ no task 'sim/alpne'; did you mean 'sim/alpine'?\n".into(),
        ),
        (
            &["show", "sim/alp"],
            "✖ no task 'sim/alp'; did you mean 'sim/a' or 'sim/alpha'?\n".into(),
        ),
        // A slug `repr` puts in double quotes is read as one too.
        (&["show", "sim/it's"], find("\"sim/it's\"")),
    ] {
        let ran = c.run_as(&a, args, "");
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err),
            (1, "", said),
            "{args:?}"
        );
    }
    // More than three as near: a search finds it better than a list of guesses.
    for slug in ["sim/b1", "sim/b2", "sim/b3"] {
        c.ok(&a, &["create", slug, "B"]);
    }
    let ran = c.run_as(&a, &["show", "sim/b"], "");
    assert_eq!((ran.code, ran.err), (1, find("'sim/b'")));
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
    // On this board: the one the environment's TASKS_SERVER and TASKS_DATABASE chose.
    let said = format!(
        "✖ no session: pass --as HANDLE or set TASKS_SESSION (get one with `{} register`)\n",
        c.board()
    );
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
            (2, "", said.as_str()),
            "{args:?}"
        );
    }
}

#[test]
fn a_message_to_an_agent_with_no_channel_names_the_commands_that_reach_this_board() {
    let c = Case::new();
    let (sender, receiver) = (c.register("worker"), c.register("worker"));
    let args: Vec<String> = [receiver.as_str(), "hi", "--as", sender.as_str()]
        .map(String::from)
        .to_vec();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::agent_send::main(&args, &c.env, &mut out, &mut err);
    let err = String::from_utf8(err).unwrap();
    // The board is a server TASKS_SERVER names, on a database of its own: the commands say both.
    let board = c.board();
    assert_eq!((code, out.is_empty()), (1, true), "{err}");
    assert!(
        err.contains(&format!(
            "has no native channel; have it run {board} callback --as {receiver}, or send on a \
             task with {board} say SLUG TEXT --to {receiver} (it can read {board} inbox --as \
             {receiver})"
        )),
        "{err}"
    );
    assert!(!err.contains("rs-bin"), "{err}");
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
        .insert("TASKS_SERVER".into(), crate::UNREACHED_SERVER.into());
    let ran = c.run_as("", &["list"], "");
    assert_eq!((ran.code, ran.out.as_str()), (2, ""));
    assert!(
        ran.err.starts_with("✖ ")
            && ran.err.contains(crate::UNREACHED_FOLDER)
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
