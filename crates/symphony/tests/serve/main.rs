//! `symphony serve`: the register's calls, one JSON line per call, and each field an answer carries.

mod asks;
#[path = "../common/mod.rs"]
mod common;

use serde_json::{Value, json};

use common::{Tree, plain};

fn call(request: Value) -> Value {
    symphony::cli::call(&request)
}

fn ok(request: Value) -> Value {
    let answer = call(request);
    answer
        .get("ok")
        .cloned()
        .unwrap_or_else(|| panic!("no answer: {answer}"))
}

#[test]
fn a_card_comes_back_with_everything_the_python_read() {
    let lines = [
        "### T",
        "- [x] **a** — one.",
        "  [2026-09-24 10:00] owner · approved — at least 21",
        "  [2026-09-24 11:00] agent — Done.",
    ];
    let cards = ok(json!({"fn": "parse_decisions", "lines": lines}));
    assert_eq!(
        cards,
        json!([{
            "line": 2, "slug": "a", "state": "approved", "text": "one.", "topic": "T",
            "note": ["[2026-09-24 10:00] owner · approved — at least 21", "[2026-09-24 11:00] agent — Done."],
            "raw": [lines[2], lines[3]],
            "entries": [
                {"ts": "2026-09-24 10:00", "who": "owner", "verdict": "approved", "text": "at least 21"},
                {"ts": "2026-09-24 11:00", "who": "agent", "verdict": null, "text": "Done."}
            ],
            "reply": "Done.", "posted_at": "2026-09-24 10:00", "decided_at": "2026-09-24 10:00",
            "unanswered": [], "open": false, "asks_owner": false, "resolvable": true, "group": "resolved"
        }])
    );
}

#[test]
fn a_comment_comes_back_with_its_state() {
    let comments =
        ok(json!({"fn": "parse_comments", "lines": ["> [!dislike] Grim", "> Lighten it."]}));
    assert_eq!(
        comments,
        json!([{"line": 1, "kind": "dislike", "title": "Grim", "body": ["Lighten it."], "resolution": null, "raw": [], "open": true}])
    );
}

#[test]
fn the_register_edits_answer_their_text_and_slugs() {
    let text = "- [ ] **old** — 1\n- [ ] **new** — *Replaces:* old.\n";
    let (out, changed) = symphony::register::supersede(text, "2026-09-26 08:00");
    assert_eq!(
        ok(json!({"fn": "supersede", "text": text, "now": "2026-09-26 08:00"})),
        json!([out, changed])
    );
    assert_eq!(
        ok(json!({"fn": "resolve", "text": "- [x] **a** — 1\n", "now": "t"})),
        json!(["- [r] **a** — 1\n  [t] agent · resolved\n", ["a"]])
    );
    assert_eq!(
        ok(json!({"fn": "card_end", "lines": ["- [ ] **a** — 1", "  x", "", "y"], "index": 0})),
        json!(2)
    );
    assert_eq!(
        ok(json!({"fn": "sort_register", "text": "# x\n"})),
        json!("# x\n")
    );
    assert_eq!(
        ok(json!({"fn": "check_nothing_lost", "before": "a\n", "after": "a\n", "doing": "x"})),
        Value::Null
    );
}

#[test]
fn a_refusal_names_its_kind() {
    let refused = call(
        json!({"fn": "sort_register", "text": "<!-- register:start -->\nprose\n<!-- register:end -->\n"}),
    );
    assert_eq!(refused["error"], "review");
    assert!(
        refused["message"].as_str().unwrap().contains("'prose'"),
        "{refused}"
    );
    let refused = call(
        json!({"fn": "set_decision", "text": "", "slug": "a", "state": "approved", "note": "", "now": "t"}),
    );
    assert_eq!(
        refused,
        json!({"error": "edit", "message": "no decision with slug 'a'"})
    );
    let refused = call(
        json!({"fn": "is_repeat_decision", "text": "- [x] **a** — 1\n  [2026-10-06 09:00] owner · approved\n",
                              "slug": "a", "state": "approved", "note": "", "now": "soon"}),
    );
    assert_eq!(refused["error"], "value");
    assert_eq!(call(json!({"fn": "nothing"}))["error"], "value");
    assert_eq!(
        call(json!({"fn": "add_message", "text": ""}))["error"],
        "value"
    );
}

#[test]
fn the_writes_answer_as_the_library_does() {
    let text = "# D\n\n- [ ] **one** — first.\n";
    assert_eq!(
        ok(
            json!({"fn": "set_decision", "text": text, "slug": "one", "state": "approved", "note": "Fine.", "now": "2026-10-06 09:00"})
        ),
        json!(
            symphony::edit::set_decision(
                text,
                "one",
                "approved",
                "Fine.",
                "2026-10-06 09:00",
                &plain()
            )
            .unwrap()
        )
    );
    assert_eq!(
        ok(
            json!({"fn": "add_message", "text": text, "slug": "one", "message": "Why?", "now": "t", "who": "agent", "verdict": "question"})
        ),
        json!("# D\n\n- [ ] **one** — first.\n  [t] agent · question — Why?\n")
    );
    assert_eq!(
        ok(json!({"fn": "add_message", "text": text, "slug": "one", "message": "Hm", "now": "t"})),
        json!("# D\n\n- [ ] **one** — first.\n  [t] owner — Hm\n")
    );
    assert_eq!(
        ok(
            json!({"fn": "insert_comment", "text": "a\n", "after_line": 1, "kind": "like", "title": "keep", "body": ""})
        ),
        json!("a\n\n> [!like] keep\n")
    );
    assert_eq!(
        ok(json!({"fn": "split_message", "message": "a\nb"})),
        json!(["a", "b"])
    );
    assert_eq!(
        ok(
            json!({"fn": "is_repeat_decision", "text": text, "slug": "one", "state": "approved", "note": "", "now": "t"})
        ),
        json!(false)
    );
    assert_eq!(
        ok(json!({"fn": "section_text", "text": "# A\n\nx\n", "anchor": "a"})),
        json!("# A\n\nx\n")
    );
    assert_eq!(
        ok(json!({"fn": "section_text", "text": "# A\n", "anchor": "b"})),
        Value::Null
    );
    assert_eq!(ok(json!({"fn": "now_stamp"})).as_str().unwrap().len(), 16);
}

#[test]
fn a_scan_answers_each_document_or_refuses_a_missing_tree() {
    let tree = Tree::new();
    tree.write("docs/x.md", "> [!question] why\n- [ ] **a** — 1\n");
    let docs = ok(json!({"fn": "scan", "root": tree.root}));
    assert_eq!(docs[0]["path"], "docs/x.md");
    assert_eq!(docs[0]["comments"][0]["kind"], "question");
    assert_eq!(docs[0]["decisions"][0]["group"], "decide");
    let missing = call(json!({"fn": "scan", "root": tree.path("nope")}));
    assert_eq!(missing["error"], "review");
}

#[test]
fn serve_answers_a_line_for_each_line_until_the_input_ends() {
    let input = "{\"fn\": \"split_message\", \"message\": \"a\"}\nnot json\n{\"fn\": \"card_end\", \"lines\": [\"x\"], \"index\": 0}\n";
    let mut out = Vec::new();
    let code = symphony::cli::serve_main(&mut input.as_bytes(), &mut out);
    assert_eq!(code, 0);
    let answers: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(answers.len(), 3);
    assert_eq!(answers[0], json!({"ok": ["a", ""]}));
    assert_eq!(answers[1]["error"], "value");
    assert_eq!(answers[2], json!({"ok": 1}));
}

#[test]
fn the_binary_dispatches_and_refuses_an_unknown_command() {
    let run = |args: &[&str]| {
        let argv: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = symphony::cli::main(&argv, &mut "".as_bytes(), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    let tree = Tree::new();
    tree.write("docs/x.md", "# x\n");
    let (code, out, _) = run(&["status", "--root", tree.root.to_str().unwrap(), "--open"]);
    assert_eq!((code, out.as_str()), (0, "no open comments\n"));
    let (code, _, err) = run(&["status", "--root", tree.root.to_str().unwrap(), "--bogus"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--bogus"), "{err}");
    let (code, _, err) = run(&["frobnicate"]);
    assert_eq!(code, 2);
    assert!(err.contains("frobnicate"), "{err}");
    let (code, _, err) = run(&[]);
    assert_eq!(code, 2, "{err}");
    let (code, _, err) = run(&["reply", "only-a-path"]);
    assert_eq!(code, 2, "{err}");
}

/// The binary's whole command line, as `symphony::cli::main` takes it: (code, stdout, stderr).
fn cli(args: &[&str]) -> (i32, String, String) {
    let argv: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony::cli::main(&argv, &mut "".as_bytes(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_command_line_that_does_not_read_is_exit_two_naming_what_is_wrong() {
    let tree = Tree::new();
    tree.write("docs/x.md", "# x\n");
    let root = tree.root.to_str().unwrap();
    for (args, named) in [
        (vec!["status", "--root"], "--root"),
        (vec!["status", "--root", root, "--json=yes"], "--json"),
        (vec!["status", "--root", root, "stray"], "stray"),
        (vec!["status", "--root", root, "--now", "--open"], "--now"),
        (vec!["reply", "a.md", "slug", "text", "more"], "more"),
    ] {
        let (code, out, err) = cli(&args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}: {err}");
        assert!(err.contains(named), "{args:?}: {err}");
        assert!(err.starts_with("usage: symphony "), "{args:?}: {err}");
    }
}

#[test]
fn help_is_the_usage_on_stdout_and_exit_zero() {
    for (command, usage) in [
        (
            "status",
            "usage: symphony status [-h] [--root ROOT] [--json] [--open] [--tidy] [--lint] [--now NOW]",
        ),
        (
            "reply",
            "usage: symphony reply [-h] [--question] [--now NOW] path slug text",
        ),
        (
            "ask",
            "usage: symphony ask [-h] [--root ROOT] [--db PATH] {list,show,answer} ...",
        ),
        (
            "docs-audit",
            "usage: symphony docs-audit [-h] [--root ROOT] [--check]",
        ),
        ("blocks", "usage: symphony blocks [--root ROOT]"),
        (
            "triage",
            "usage: symphony triage [-h] [--root ROOT] {stats,queue} [n]",
        ),
    ] {
        for flag in ["--help", "-h"] {
            let (code, out, err) = cli(&[command, flag]);
            assert_eq!((code, err.as_str()), (0, ""), "{command} {flag}");
            assert_eq!(out.lines().next(), Some(usage), "{command} {flag}");
        }
    }
}

/// `verdict-tasks` retired with the owner's asks: a verdict waiting on an agent is an ask on the
/// board (`just tasks asks`), never a task.
#[test]
fn the_retired_watch_and_verdict_tasks_commands_are_unknown_even_when_asking_for_help() {
    for retired in ["watch", "verdict-tasks"] {
        for args in [vec![retired], vec![retired, "--help"], vec![retired, "-h"]] {
            let (code, out, err) = cli(&args);
            assert_eq!((code, out.as_str()), (2, ""), "{args:?}: {err}");
            assert!(
                err.starts_with(&format!("symphony: unknown command \"{retired}\"; use ")),
                "{args:?}: {err}"
            );
        }
    }
}

#[test]
fn a_reply_replaces_the_file_whole_so_a_reader_never_sees_it_cut_short() {
    let tree = Tree::new();
    let before = "- [ ] **a** — one\n";
    let page = tree.write("docs/x.md", before);
    let open = tree.open_with_mode("docs/x.md", 0o640);
    let (code, _, err) = cli(&["reply", "--now", "t", page.to_str().unwrap(), "a", "Done."]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(common::rest(open), before);
    assert_eq!(
        tree.read("docs/x.md"),
        "- [ ] **a** — one\n  [t] agent — Done.\n"
    );
    assert_eq!(tree.mode("docs/x.md"), 0o640);
    assert_eq!(tree.names("docs"), ["x.md"]);
}

#[test]
fn a_reply_may_start_with_a_dash_after_a_double_dash_or_with_a_space() {
    let tree = Tree::new();
    let page = tree.write("docs/x.md", "- [ ] **a** — one\n");
    let path = page.to_str().unwrap();
    let now = "2026-10-08 12:00";
    let (code, _, err) = cli(&["reply", "--now", now, path, "a", "- a list item"]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = cli(&["reply", "--now", now, "--", path, "a", "-dashed"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        tree.read("docs/x.md"),
        "- [ ] **a** — one\n  [2026-10-08 12:00] agent — - a list item\n  [2026-10-08 12:00] agent — -dashed\n"
    );
}

#[test]
fn a_flag_may_be_given_with_an_equals_sign_or_shortened() {
    let tree = Tree::new();
    tree.write("docs/x.md", "> [!question] why\n");
    let root = format!("--root={}", tree.root.display());
    let mut argv = vec![root.clone(), "--op".to_string()];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        symphony::cli::status_main(&argv, &mut out, &mut err),
        0,
        "{}",
        String::from_utf8_lossy(&err)
    );
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "docs/x.md:1  question why\n"
    );
    argv[1] = "--t".to_string(); // only --tidy starts with --t
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        symphony::cli::status_main(&argv, &mut out, &mut err),
        0,
        "{}",
        String::from_utf8_lossy(&err)
    );
}

/// The binary itself, serving `requests` from `dir`: its answers, one per request. It ends at the end
/// of its input, and the test waits for it.
fn serve_from(dir: &std::path::Path, requests: &[Value]) -> Vec<Value> {
    use std::io::Write;
    use symphony_board::sys::Command;
    let mut child = symphony_board::sys::spawn_piped(
        Command::new(env!("CARGO_BIN_EXE_symphony"))
            .arg("serve")
            .current_dir(dir),
    )
    .expect("the Symphony binary");
    let mut input = child.stdin.take().expect("its input");
    // Written beside the read: it answers each request as it reads it, and a pipe holds only so much.
    let output = std::thread::scope(|scope| {
        scope.spawn(move || {
            for request in requests {
                writeln!(input, "{request}").expect("a request");
            }
        });
        symphony_board::sys::wait_output(child).expect("its answers")
    });
    assert!(
        output.status.success(),
        "{:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_relative_path_is_read_from_the_callers_directory_and_named_as_given() {
    let tree = Tree::new();
    tree.write("docs/x.md", "> [!question] why\n");
    tree.write("elsewhere/.keep", "");
    let caller = tree.root.to_string_lossy().to_string();
    let away = tree.path("elsewhere").to_string_lossy().to_string();
    let answers = serve_from(
        &tree.path("elsewhere"),
        &[
            json!({"fn": "scan", "root": ".", "cwd": caller}),
            json!({"fn": "scan", "root": ".", "cwd": away}),
        ],
    );
    assert_eq!(answers[0]["ok"][0]["path"], "docs/x.md", "{answers:?}");
    assert_eq!(
        answers[1],
        json!({"error": "review", "message": "no docs/ directory under ."})
    );
}
