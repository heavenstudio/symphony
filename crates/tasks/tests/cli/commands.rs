//! Board command exit codes, body-file input and subtask operations.

use crate::{Case, Ran, git, repository, scratch};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn slugs(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|t| t["slug"].as_str().unwrap())
        .collect()
}

#[test]
fn register_prints_only_the_handle() {
    let out = Case::new().ok("", &["register", "--kind", "worker"]);
    let handle = out.strip_suffix('\n').unwrap();
    let words: Vec<&str> = handle.split('-').collect();
    let lower = |w: &&str| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase());
    let number = |w: &&str| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit());
    let shaped = match words.as_slice() {
        [a, b] => lower(a) && lower(b),
        [a, b, n] => lower(a) && lower(b) && number(n),
        _ => false,
    };
    assert!(shaped, "{out:?}");
}

#[test]
fn register_keeps_a_session_url_and_refuses_one_it_cannot_link_registering_nothing() {
    let c = Case::new();
    let web = "https://claude.ai/code/session_01abc";
    let handle = c.register_with("worker", &["--session-url", web]);
    assert_eq!(
        c.store()
            .session_urls()
            .unwrap()
            .get(&handle)
            .map(String::as_str),
        Some(web)
    );
    let before = c.store().sessions().unwrap().len();
    let ran = c.run_as(
        "",
        &[
            "register",
            "--kind",
            "worker",
            "--session-url",
            "file:///etc",
        ],
        "",
    );
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(
        ran.err
            .contains("a session URL starts with https:// or http://"),
        "{}",
        ran.err
    );
    assert_eq!(c.store().sessions().unwrap().len(), before);
    // An empty one registers with none.
    let none = c.register_with("worker", &["--session-url", ""]);
    assert!(!c.store().session_urls().unwrap().contains_key(&none));
}

#[test]
fn callback_sets_or_clears_the_session_url_alone_or_beside_a_channel() {
    let c = Case::new();
    let me = c.register("worker");
    let web = "https://claude.ai/code/session_01abc";
    let urls = || c.store().session_urls().unwrap();
    // No channel given or detected: the URL alone is set, and no refusal.
    assert_eq!(
        c.ok(&me, &["callback", "--session-url", web]),
        format!("SESSION URL {me} {web}\n")
    );
    assert_eq!(urls().get(&me).map(String::as_str), Some(web));
    assert!(c.store().callbacks().unwrap().is_empty());
    // Beside a channel, both are kept.
    let out = c.ok(
        &me,
        &["callback", "codex:abcdef01", "--session-url", "https://h/2"],
    );
    assert!(
        out.starts_with(&format!(
            "SESSION URL {me} https://h/2\nCALLBACK {me} codex:abcdef01\n"
        )),
        "{out}"
    );
    assert!(c.store().callbacks().unwrap().contains_key(&me));
    assert_eq!(
        c.ok(&me, &["callback", "--session-url", ""]),
        format!("SESSION URL {me} none\n")
    );
    assert!(urls().is_empty());
    let ran = c.run_as(
        &me,
        &["callback", "--session-url", "javascript:alert(1)"],
        "",
    );
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.contains("starts with https:// or http://"),
        "{}",
        ran.err
    );
    // Without the flag, a callback with nothing to register is refused as before.
    let ran = c.run_as(&me, &["callback"], "");
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.contains("no native callback detected"),
        "{}",
        ran.err
    );
}

#[test]
fn a_plain_orchestrator_callback_says_it_hears_chat_only_as_a_fallback() {
    let c = Case::new();
    let me = c.register("orchestrator");
    assert_eq!(
        c.ok(&me, &["callback", "codex:abcdef01"]),
        format!(
            "CALLBACK {me} codex:abcdef01\n\
             CHAT {me} (fallback: an orchestrator with a callback; --chat takes the seat)\n"
        )
    );
    assert_eq!(
        c.ok(&me, &["callback", "codex:abcdef01", "--chat"]),
        format!("CALLBACK {me} codex:abcdef01\nCHAT {me}\n")
    );
    // A worker's plain callback hears no chat, and nobody else is there to.
    let worker = c.register("worker");
    c.ok(&me, &["callback", "--off"]);
    assert_eq!(
        c.ok(&worker, &["callback", "codex:abcdef02"]),
        format!("CALLBACK {worker} codex:abcdef02\nCHAT nobody registered\n")
    );
}

#[test]
fn a_refused_callback_leaves_the_session_url_as_it_was() {
    let c = Case::new();
    let me = c.register("worker");
    let other = c.register("worker");
    let kept = "https://claude.ai/code/session_01abc";
    c.ok(&me, &["callback", "--session-url", kept]);
    c.ok(&other, &["callback", "codex:abcdef02"]);
    let held = format!("callback address already held by {other}");
    for (args, why) in [
        (
            vec!["callback", "bogus:1", "--session-url", "https://h/new"],
            "callback must be uds:/absolute/socket or codex:THREAD-UUID",
        ),
        (
            vec![
                "callback",
                "codex:abcdef02",
                "--session-url",
                "https://h/new",
            ],
            held.as_str(),
        ),
        (
            vec![
                "callback",
                "--off",
                "--chat",
                "--session-url",
                "https://h/new",
            ],
            "register a callback before receiving chat",
        ),
        (
            vec!["callback", "bogus:1", "--session-url", ""],
            "callback must be uds:/absolute/socket or codex:THREAD-UUID",
        ),
    ] {
        let ran = c.run_as(&me, &args, "");
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.out);
        assert!(ran.err.contains(why), "{args:?}: {}", ran.err);
        assert!(!ran.out.contains("SESSION URL"), "{args:?}: {}", ran.out);
        assert_eq!(
            c.store()
                .session_urls()
                .unwrap()
                .get(&me)
                .map(String::as_str),
            Some(kept),
            "{args:?}"
        );
    }
    assert!(!c.store().callbacks().unwrap().contains_key(&me));
    // An address it cannot link is refused before any registration is made.
    let ran = c.run_as(
        &me,
        &[
            "callback",
            "codex:abcdef03",
            "--session-url",
            "javascript:x",
        ],
        "",
    );
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(!c.store().callbacks().unwrap().contains_key(&me));
}

#[test]
fn a_command_without_a_session_says_how_to_get_one() {
    let ran = Case::new().run_as("", &["claim", "sim/jump"], "");
    assert_eq!(ran.code, 2);
    assert!(ran.err.contains("TASKS_SESSION"), "{}", ran.err);
}

#[test]
fn the_as_flag_overrides_the_environment() {
    let c = Case::new();
    let (a, b) = (c.register("worker"), c.register("worker"));
    c.ok(&a, &["create", "sim/jump", "Jump"]);
    c.ok(&a, &["claim", "sim/jump", "--as", &b]);
    assert_eq!(
        c.store().get("sim/jump").unwrap().holders("executor"),
        [b.as_str()]
    );
}

#[test]
fn create_claim_move_review_approve() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let exe = c.register_with("worker", &["--parent", &orch]);
    let rev = c.register_with("reviewer", &["--parent", &orch]);
    c.ok(
        &orch,
        &[
            "create",
            "sim/jump",
            "Jump departure",
            "--priority",
            "1",
            "--link",
            "docs/SPEC.md#Jump",
            "--body",
            "Make the jump leave cleanly.",
        ],
    );
    c.ok(&orch, &["role", "sim/jump", "orchestrator"]);
    c.ok(&exe, &["claim", "sim/jump"]);
    c.ok(&exe, &["move", "sim/jump", "review"]);
    c.ok(&rev, &["role", "sim/jump", "reviewer"]);
    c.ok(
        &rev,
        &["verdict", "sim/jump", "approve", "hash chain holds"],
    );
    let task = c.store().get("sim/jump").unwrap();
    assert_eq!(
        (task.status.as_str(), task.priority, task.links),
        ("done", 1, vec!["docs/SPEC.md#Jump".to_string()])
    );
}

/// Every open task has an orchestrator: one an orchestrator files is its own, one filed under a
/// held task the tree's, one filed `--no-role` the live orchestrator `--for` names, and any other
/// filer's the board gives (its parent's here, else the orchestrator that stays holding fewest).
/// Its child's claim leaves the role where it is.
#[test]
fn create_by_an_orchestrator_takes_the_role_and_any_other_filer_is_given_one() {
    let mut c = Case::new();
    let orch = c.register("orchestrator");
    let other = c.register("orchestrator");
    let kid = c.register_with("worker", &["--parent", &orch]);
    let worker = c.register("worker");
    let holders = |slug: &str| {
        c.store()
            .get(slug)
            .unwrap()
            .holders("orchestrator")
            .join(" ")
    };
    assert_eq!(
        c.ok(&orch, &["create", "sim/jump", "Jump"]),
        format!("created sim/jump (ready); {orch} orchestrates it\n")
    );
    assert_eq!(holders("sim/jump"), orch);
    // `--no-role` names the orchestrator that takes it, a live one, and reserves nothing.
    let alone = c.run_as(&orch, &["create", "sim/free", "Free", "--no-role"], "");
    assert_eq!(
        (alone.code, alone.err.as_str()),
        (
            1,
            "✖ --no-role files a task for another orchestrator: name a live one with --for \
             HANDLE (every open task has an orchestrator)\n"
        )
    );
    let to_worker = ["create", "sim/free", "Free", "--no-role", "--for", &worker];
    let refused = c.run_as(&orch, &to_worker, "");
    assert!(
        refused
            .err
            .contains(&format!("{worker} is a session of kind worker")),
        "{}",
        refused.err
    );
    assert!(
        c.store().get("sim/free").is_err(),
        "a refusal files nothing"
    );
    assert_eq!(
        c.ok(
            &orch,
            &["create", "sim/free", "Free", "--no-role", "--for", &other]
        ),
        format!("created sim/free (ready); {other} orchestrates it\n")
    );
    assert_eq!(holders("sim/free"), other);
    assert!(
        c.store()
            .get("sim/free")
            .unwrap()
            .holders("reserved")
            .is_empty()
    );
    let part = ["create", "sim/jump/part", "A part", "--parent", "sim/jump"];
    assert_eq!(
        c.ok(&orch, &part),
        format!("created sim/jump/part (ready); {orch} orchestrates it\n")
    );
    assert_eq!(holders("sim/jump/part"), "", "the tree's");
    assert_eq!(
        c.ok(&kid, &["create", "sim/kids", "Kids"]),
        format!("created sim/kids (ready); {orch} orchestrates it\n")
    );
    // Past the first live line, both beating, each holding a task stays: orch holds two now,
    // other one, so the board gives a worker's to other.
    let later = crate::common::NOW + symphony_board::health::LIVE_SECONDS + 1.0;
    c.env.vars.insert("TASKS_NOW".into(), later.to_string());
    for handle in [&orch, &other] {
        c.store().touch(handle).unwrap();
    }
    assert_eq!(
        c.ok(&worker, &["create", "sim/theirs", "Theirs"]),
        format!("created sim/theirs (ready); {other} orchestrates it\n")
    );
    c.ok(&kid, &["claim", "sim/jump"]);
    let task = c.store().get("sim/jump").unwrap();
    assert_eq!(
        (task.holders("orchestrator"), task.holders("executor")),
        (vec![orch.as_str()], vec![kid.as_str()])
    );
}

/// With no orchestrator that stays a filed task waits, said so once any orchestrator is on the
/// board: a board with none, a new project's, says nothing of it. An orchestrator that only
/// registers is given nothing; a lone new one waiting is given it once it has waited past the
/// first live line, at a look of its `tasks wait`, told in one GIVEN line.
#[test]
fn a_task_filed_with_no_orchestrator_that_stays_waits_for_a_new_one_to_wait() {
    let mut c = Case::new();
    let worker = c.register("worker");
    // No orchestrator on the board: nothing is said of one.
    assert_eq!(
        c.ok(&worker, &["create", "sim/loose", "Loose"]),
        "created sim/loose (ready)\n"
    );
    let shown = c.ok(&worker, &["show", "sim/loose"]);
    assert!(!shown.contains("orchestrator"), "{shown}");
    // Registering says the handle alone and gives nothing, nor does its first wait.
    let registered = c.run_as("", &["register", "--kind", "orchestrator"], "");
    assert_eq!((registered.code, registered.err.as_str()), (0, ""));
    let handle = registered.out.trim_end().to_string();
    assert!(
        !handle.is_empty() && !handle.contains(' '),
        "{:?}",
        registered.out
    );
    // One is on the board now, which does not stay yet: the task is said to wait for one.
    let waits = format!(
        "  waits for an orchestrator under its limit that stays, for 0 min: the first live past \
         its first live line that waits, watches or holds a task, and orchestrates fewer than 16 \
         open tasks ([limits] orchestrator_tasks), is given it, and any such orchestrator takes \
         it with: {} role sim/loose orchestrator\n",
        c.board()
    );
    // (`create` says it waits the same way: tests/cli/pull/wait.rs.)
    let shown = c.ok(&worker, &["show", "sim/loose"]);
    assert!(shown.contains(&waits), "{shown}");
    let first = c.ok(&handle, &["wait", "--timeout", "0"]);
    assert!(!first.contains("GIVEN"), "{first}");
    assert!(
        c.store()
            .get("sim/loose")
            .unwrap()
            .holders("orchestrator")
            .is_empty()
    );
    // Waiting on past the first live line, a look gives it, told once.
    let later = crate::common::NOW + symphony_board::health::LIVE_SECONDS + 1.0;
    c.env.vars.insert("TASKS_NOW".into(), later.to_string());
    let back = c.ok(&handle, &["wait", "--timeout", "0"]);
    assert!(
        back.starts_with("GIVEN 1 open task is yours to orchestrate: sim/loose\n"),
        "{back}"
    );
    let task = c.store().get("sim/loose").unwrap();
    assert_eq!(task.holders("orchestrator"), [handle.as_str()]);
    let shown = c.ok(&worker, &["show", "sim/loose"]);
    assert!(!shown.contains("waits for an orchestrator"), "{shown}");
    let again = c.ok(&handle, &["wait", "--timeout", "0"]);
    assert!(!again.contains("GIVEN"), "{again}");
}

/// No command files a legacy mark, and one in an old row gives no approval privilege.
#[test]
fn a_fast_fix_is_neither_filed_nor_approved_by_its_orchestrator() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let exe = c.register_with("worker", &["--parent", &orch]);
    let why = "a one-line regex fix, its test posted";
    let refused = c.run_as(&orch, &["create", "fix/new", "New", "--fast-fix", why], "");
    assert_eq!(refused.code, 2, "{}", refused.err);
    assert!(
        refused.err.contains("unrecognized arguments: --fast-fix"),
        "{}",
        refused.err
    );
    assert!(c.store().get("fix/new").is_err(), "nothing is filed");
    c.store()
        .create(&symphony_board::NewTask::new("fix/regex", "Regex", &orch))
        .unwrap();
    c.store()
        .raw()
        .row("task", "fix/regex")
        .set(serde_json::json!({
            "fields._board.fast_fix": why
        }))
        .unwrap();
    c.ok(&orch, &["role", "fix/regex", "orchestrator"]);
    c.ok(&exe, &["claim", "fix/regex"]);
    c.ok(&exe, &["move", "fix/regex", "review"]);
    let own = c.run_as(&exe, &["verdict", "fix/regex", "approve", "mine"], "");
    assert_eq!(own.code, 1);
    assert!(own.err.contains("executed fix/regex"), "{}", own.err);
    let own = c.run_as(&orch, &["verdict", "fix/regex", "approve", "mine"], "");
    assert_eq!(own.code, 1);
    assert!(own.err.contains("orchestrated fix/regex"), "{}", own.err);
    let rev = c.register("reviewer");
    c.ok(&rev, &["role", "fix/regex", "reviewer"]);
    c.ok(
        &rev,
        &["verdict", "fix/regex", "approve", "its test reads right"],
    );
    assert_eq!(c.store().get("fix/regex").unwrap().status, "done");
    // Back to review only from a checkout of its repository, which says whether its tip is on the
    // default branch already (`tests/sweep/land_approval.rs` sends one back from there).
    let back = c.run_as(&exe, &["move", "fix/regex", "review"], "");
    assert_eq!(back.code, 1);
    assert_eq!(
        back.err.trim_end().lines().last().unwrap(),
        "✖ fix/regex is done: it goes back to review only from a checkout of its repository, \
         which says whether its tip is on the default branch already",
        "{}",
        back.err
    );
    assert_eq!(c.store().get("fix/regex").unwrap().status, "done");
}

/// The orchestrator of a task, which filed it here, asks a fresh reviewer for its verdict: the
/// reviewer's role and the verdict are refused it at once, exit 1, naming the command a fresh
/// reviewer takes the role with; the task stays in review until that reviewer approves it.
#[test]
fn its_orchestrator_is_refused_the_reviewers_role_and_the_verdict_and_told_who_gives_it() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let [exe, rev] = ["worker", "reviewer"].map(|kind| c.register_with(kind, &["--parent", &orch]));
    c.ok(&orch, &["create", "sim/jump", "Jump"]);
    c.ok(&orch, &["role", "sim/jump", "orchestrator"]);
    c.ok(&exe, &["claim", "sim/jump"]);
    c.ok(&exe, &["move", "sim/jump", "review"]);
    let board = c.board();
    let ask = format!(
        "the verdict is a fresh reviewer's, one that did not; ask a fresh reviewer, which takes \
         the role from a session of its own: {board} role sim/jump reviewer"
    );
    for (args, said) in [
        (
            &["role", "sim/jump", "reviewer"][..],
            format!("✖ {orch} orchestrated sim/jump and cannot review it: {ask}\n"),
        ),
        (
            &["verdict", "sim/jump", "approve", "fine"],
            format!("✖ {orch} orchestrated sim/jump, and {ask}\n"),
        ),
    ] {
        let ran = c.run_as(&orch, args, "");
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err.as_str()),
            (1, "", said.as_str()),
            "{args:?}"
        );
    }
    let task = c.store().get("sim/jump").unwrap();
    assert_eq!(task.status, "review");
    assert!(task.holders("reviewer").is_empty());
    c.ok(&rev, &["role", "sim/jump", "reviewer"]);
    c.ok(&rev, &["verdict", "sim/jump", "approve", "its tests pass"]);
    assert_eq!(c.store().get("sim/jump").unwrap().status, "done");
}

/// A refused move of a done task names the move back to review only while its approve stands:
/// once its landing closed it, that move is refused too, and what it needs is a new task.
#[test]
fn a_done_task_names_the_move_back_to_review_only_until_its_landing_closes_it() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let [exe, rev] = ["worker", "reviewer"].map(|kind| c.register_with(kind, &["--parent", &orch]));
    c.ok(&orch, &["create", "sim/jump", "Jump"]);
    c.ok(&orch, &["role", "sim/jump", "orchestrator"]);
    c.ok(&exe, &["claim", "sim/jump"]);
    c.ok(&exe, &["move", "sim/jump", "review"]);
    c.ok(&rev, &["role", "sim/jump", "reviewer"]);
    c.ok(&rev, &["verdict", "sim/jump", "approve", "good"]);
    let board = c.board();
    let refused = |said: &str| {
        let ran = c.run_as(&exe, &["move", "sim/jump", "ready"], "");
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err.as_str()),
            (
                1,
                "",
                format!("✖ sim/jump cannot go from done to ready; {said}\n").as_str()
            )
        );
    };
    refused(&format!(
        "a done task goes only back to review, and only one approved that no landing has closed: \
         {board} move sim/jump review"
    ));
    c.store()
        .landed(
            "sim/jump",
            &orch,
            "landed sim/jump on main at 3e0c66192471",
            true,
            &["done"],
        )
        .unwrap();
    refused(&format!(
        "a done task no approve stands on, as a landing leaves it, moves no further: what it still \
         needs goes in a new task, {board} create SLUG TITLE"
    ));
    assert_eq!(c.store().get("sim/jump").unwrap().status, "done");
}

/// A reviewer that claimed an instruction subtask made `--for` it, which puts nothing on the task's
/// branch, still approves the task; the executor of a work slice of it is refused, in these words.
#[test]
fn the_reviewer_of_a_review_instruction_approves_and_a_slices_executor_is_refused() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let [exe, part, rev] =
        ["worker", "worker", "reviewer"].map(|kind| c.register_with(kind, &["--parent", &orch]));
    c.ok(&orch, &["create", "sim/jump", "Jump"]);
    c.ok(&orch, &["role", "sim/jump", "orchestrator"]);
    c.ok(&exe, &["claim", "sim/jump"]);
    let under = |slug: &str, whom: &str, more: &[&str]| {
        let mut args = vec![
            "create", slug, "A step", "--parent", "sim/jump", "--for", whom,
        ];
        args.extend(more);
        c.ok(&orch, &args);
        c.ok(whom, &["claim", slug]);
    };
    under("sim/jump/part", &part, &[]);
    c.ok(
        &part,
        &["done", "sim/jump/part", "integrated on the branch"],
    );
    c.ok(&exe, &["move", "sim/jump", "review"]);
    under("sim/jump/review", &rev, &["--instruction"]);
    for who in [&part, &rev] {
        c.ok(who, &["role", "sim/jump", "reviewer"]);
    }
    let slice = c.run_as(&part, &["verdict", "sim/jump", "approve", "mine"], "");
    assert_eq!(slice.code, 1);
    assert_eq!(
        slice.err.trim_end().lines().last().unwrap(),
        format!(
            "✖ {part} executed sim/jump: its verdict is a fresh reviewer's, one that did not \
             write or execute it"
        ),
        "{}",
        slice.err
    );
    c.ok(
        &rev,
        &[
            "verdict",
            "sim/jump",
            "approve",
            "3e0c66192471: its tests pass",
        ],
    );
    c.ok(&rev, &["done", "sim/jump/review", "approved the tip"]);
    assert_eq!(c.store().get("sim/jump").unwrap().status, "done");
}

#[test]
fn a_body_file_is_read() {
    let c = Case::new();
    let a = c.register("worker");
    let body = scratch("body-file").join("brief.md");
    fs::write(&body, "# Brief\n\nDo it.\n").unwrap();
    c.ok(
        &a,
        &[
            "create",
            "sim/jump",
            "Jump",
            "--body-file",
            body.to_str().unwrap(),
        ],
    );
    assert_eq!(
        c.store().get("sim/jump").unwrap().body,
        "# Brief\n\nDo it.\n"
    );
}

#[test]
fn a_refusal_is_exit_1_with_the_reason() {
    let c = Case::new();
    let (a, b) = (c.register("worker"), c.register("worker"));
    c.ok(&a, &["create", "sim/jump", "Jump"]);
    c.ok(&a, &["claim", "sim/jump"]);
    let ran = c.run_as(&b, &["claim", "sim/jump"], "");
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains(&a), "{}", ran.err);
}

#[test]
fn owner_actors_are_refused_by_flag_and_environment_without_changing_the_board() {
    let c = Case::new();
    let agent = c.register("worker");
    c.ok(&agent, &["create", "sim/jump", "Jump"]);
    c.ok(&agent, &["claim", "sim/jump"]);
    let before = c.store().dump().unwrap();
    for command in [
        vec!["priority", "sim/jump", "0"],
        vec!["move", "sim/jump", "cancelled"],
        vec!["create", "sim/forged", "Forged"],
        vec!["say", "sim/jump", "Forged owner message"],
        vec!["claim", "sim/jump"],
    ] {
        for (who, prefix, suffix) in [
            (agent.as_str(), vec!["--as", "owner"], vec![]),
            (agent.as_str(), vec![], vec!["--as=owner"]),
            ("owner", vec![], vec![]),
            ("owner", vec![], vec!["--as="]),
        ] {
            let mut args = prefix;
            args.extend_from_slice(&command);
            args.extend_from_slice(&suffix);
            let ran = c.run_as(who, &args, "");
            assert_eq!(ran.code, 1, "{args:?} as {who}: {}", ran.err);
            assert!(ran.out.is_empty(), "{args:?}: {}", ran.out);
            assert!(
                ran.err.contains("the owner acts from the Symphony"),
                "{args:?}: {}",
                ran.err
            );
            assert_eq!(c.store().dump().unwrap(), before, "{args:?} as {who}");
        }
    }
    // The effective actor is the flag's agent, even when the environment says owner.
    c.ok("owner", &["priority", "sim/jump", "4", "--as", &agent]);
    assert_eq!(c.store().get("sim/jump").unwrap().priority, 4);
    assert_eq!(
        c.store().thread("sim/jump").unwrap().last().unwrap().sender,
        agent
    );
}

#[test]
fn list_json_and_its_status_filter() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["create", "sim/b", "B", "--backlog"]);
    let data = json(&c.ok("", &["list", "--json", "--status", "ready"]));
    assert_eq!(slugs(&data["tasks"]), ["sim/a"]);
}

#[test]
fn create_can_start_at_a_stage() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(
        &a,
        &["create", "docs/lamps", "Lamp briefs", "--stage", "concept"],
    );
    assert_eq!(c.store().get("docs/lamps").unwrap().status, "concept");
}

/// A session and the data task `content/lineup-stretch` it made: the session's handle.
fn data_task(c: &Case) -> String {
    let a = c.register("worker");
    c.ok(&a, &["create", "content/lineup-stretch", "Lineup"]);
    a
}

#[test]
fn create_refuses_a_brief_importing_from_the_worktree_of_a_dependency_not_done() {
    let c = Case::new();
    let a = data_task(&c);
    let ran = c.run_as(
        &a,
        &[
            "create",
            "pictures/lamps",
            "Lamps",
            "--depends",
            "content/lineup-stretch",
            "--body",
            "Import content/x.json from .claude/worktrees/content-lineup-stretch/content/x.json.",
        ],
        "",
    );
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.contains("workflow/shared-data-lands-first"),
        "{}",
        ran.err
    );
    assert!(
        ran.err.contains(".claude/worktrees/content-lineup-stretch"),
        "{}",
        ran.err
    );
    let named: String = ran
        .err
        .split(".claude/worktrees/")
        .nth(1)
        .unwrap()
        .chars()
        .take(25)
        .collect();
    assert!(
        !named.contains("stretch."),
        "the worktree is named with its sentence's stop: {}",
        ran.err
    );
    assert!(c.store().get("pictures/lamps").is_err());
}

#[test]
fn create_takes_a_review_or_a_cwd_brief_naming_a_worktree_it_does_not_depend_on() {
    let c = Case::new();
    let a = data_task(&c);
    c.ok(
        &a,
        &[
            "create",
            "review/x",
            "Review",
            "--body",
            "Read the diff in /r/.claude/worktrees/content-lineup-stretch.",
        ],
    );
    c.ok(
        &a,
        &[
            "create",
            "pictures/other",
            "Other",
            "--depends",
            "content/lineup-stretch",
            "--body",
            "Work in .claude/worktrees/picture-other, on main's data.",
        ],
    );
}

#[test]
fn create_takes_artifact_overlays_under_a_dependencys_worktree() {
    let c = Case::new();
    let a = data_task(&c);
    c.ok(
        &a,
        &[
            "create",
            "pictures/lamps",
            "Lamps",
            "--depends",
            "content/lineup-stretch",
            "--body",
            "See .claude/worktrees/content-lineup-stretch/artifacts/lamp.png",
        ],
    );
}

#[test]
fn create_takes_the_worktree_of_a_dependency_that_is_done() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "content/parent", "Parent"]);
    c.ok(
        &a,
        &[
            "create",
            "content/lineup-stretch",
            "Lineup",
            "--parent",
            "content/parent",
            "--instruction",
        ],
    );
    let store = c.store();
    store.claim("content/lineup-stretch", &a).unwrap();
    store.done("content/lineup-stretch", &a, "landed").unwrap();
    c.ok(
        &a,
        &[
            "create",
            "pictures/lamps",
            "Lamps",
            "--depends",
            "content/lineup-stretch",
            "--body",
            "Old notes in .claude/worktrees/content-lineup-stretch/notes.md",
        ],
    );
}

/// A board whose commands run in a repository (in scratch folder `name`) holding the data task's work in
/// worktree `folder` (under it) on its slug's branch: the board, the data task's session and the worktree.
fn data_task_in(name: &str, folder: &str) -> (Case, String, PathBuf) {
    let repo = repository(&scratch(name).join("repo"));
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "content/lineup-stretch",
            folder,
        ],
    );
    let mut c = Case::new();
    c.env.cwd = repo.clone();
    let a = data_task(&c);
    (c, a, repo.join(folder))
}

/// `create pictures/lamps`, depending on the data task, with `body`: the run.
fn create_depending(c: &Case, a: &str, body: &str) -> Ran {
    c.run_as(
        a,
        &[
            "create",
            "pictures/lamps",
            "Lamps",
            "--depends",
            "content/lineup-stretch",
            "--body",
            body,
        ],
        "",
    )
}

/// That `ran` was refused naming `worktree` as the data task's, and made no task.
fn refused_naming(c: &Case, ran: &Ran, worktree: &Path) {
    assert_eq!(ran.code, 1, "{}", ran.err);
    let said = format!(
        "the brief names {}, the worktree of content/lineup-stretch",
        worktree.display()
    );
    assert!(ran.err.contains(&said), "{}", ran.err);
    assert!(c.store().get("pictures/lamps").is_err());
}

#[test]
fn create_refuses_a_brief_importing_from_a_dependencys_agent_worktree() {
    let (c, a, worktree) = data_task_in("agent-refused", ".claude/worktrees/agent-a1b2");
    let ran = create_depending(
        &c,
        &a,
        "Import content/x.json from .claude/worktrees/agent-a1b2/content/x.json.",
    );
    refused_naming(&c, &ran, &worktree);
}

#[test]
fn create_refuses_a_brief_importing_from_a_dependencys_codex_worktree() {
    let (c, a, worktree) = data_task_in("codex-refused", ".claude/worktrees/codex-lineup-pilot");
    let body = format!("Import {}/content/x.json", worktree.display());
    let ran = create_depending(&c, &a, &body);
    refused_naming(&c, &ran, &worktree);
}

#[test]
fn create_takes_a_brief_naming_no_worktree_beside_a_dependencys_worktree() {
    let (c, a, _) = data_task_in("agent-taken", ".claude/worktrees/agent-a1b2");
    let ran = create_depending(&c, &a, "Build the lamps from main's content/x.json.");
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(c.store().get("pictures/lamps").is_ok());
}

#[test]
fn list_mine_shows_only_the_tasks_it_holds() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["create", "sim/b", "B"]);
    c.ok(&a, &["claim", "sim/b"]);
    let out = c.ok(&a, &["list", "--mine"]);
    assert!(out.contains("sim/b") && !out.contains("sim/a"), "{out}");
}

#[test]
fn next_prints_the_best_slug_or_that_there_is_nothing_to_take() {
    let c = Case::new();
    let a = c.register("worker");
    assert_eq!(
        c.ok(&a, &["next"]),
        "nothing to take: no concept, design or ready task is free of an executor, of another's \
         reservation and of an open dependency (`just tasks list` shows the board)\n"
    );
    c.ok(&a, &["create", "sim/a", "A"]);
    assert!(c.ok(&a, &["next"]).contains("sim/a"));
}

#[test]
fn show_prints_the_roles_and_the_thread() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "sim/a", "Alpha"]);
    c.ok(&a, &["claim", "sim/a"]);
    c.ok(&a, &["say", "sim/a", "halfway there", "--to", "owner"]);
    let out = c.ok("", &["show", "sim/a"]);
    for said in ["Alpha", &format!("executor {a}"), "halfway there"] {
        assert!(out.contains(said), "{said:?} not in {out}");
    }
}

#[test]
fn reserve_release_and_show_the_dependencies() {
    let c = Case::new();
    let (a, b) = (c.register("worker"), c.register("worker"));
    c.ok(&a, &["create", "sim/base", "Base"]);
    c.ok(
        &a,
        &[
            "create",
            "sim/top",
            "Top",
            "--depends",
            "sim/base",
            "--parent",
            "sim/base",
        ],
    );
    c.ok(&a, &["reserve", "sim/top"]);
    assert_eq!(c.run_as(&b, &["reserve", "sim/top"], "").code, 1);
    c.ok(&a, &["release", "sim/top", "--role", "reserved"]);
    c.ok(&b, &["reserve", "sim/top"]);
    let out = c.ok("", &["show", "sim/top"]);
    assert!(out.contains("depends on: sim/base"), "{out}");
    assert!(out.contains("parent: sim/base"), "{out}");
    let data = json(&c.ok("", &["show", "sim/top", "--json"]));
    assert_eq!(data["task"]["depends"], serde_json::json!(["sim/base"]));
}

#[test]
fn who_json_and_all() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["leave"]);
    assert!(!c.ok("", &["who"]).contains(&a));
    assert!(c.ok("", &["who", "--all"]).contains(&a));
    assert_eq!(
        json(&c.ok("", &["who", "--json"]))["sessions"],
        serde_json::json!([])
    );
}

#[test]
fn who_lists_the_sessions_as_a_tree_with_their_health() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let child = c.register_with("worker", &["--parent", &orch]);
    let out = c.ok("", &["who"]);
    assert!(out.contains(&orch), "{out}");
    assert!(out.contains(&format!("  {child}")), "{out}");
    assert!(out.contains("live"), "{out}");
}

#[test]
fn inbox_prints_and_acknowledges() {
    let c = Case::new();
    let (a, b) = (c.register("worker"), c.register("worker"));
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["say", "sim/a", "ping", "--to", &b]);
    assert!(c.ok(&b, &["inbox"]).contains("ping"));
    assert_eq!(c.ok(&b, &["inbox"]).trim(), "");
}

#[test]
fn leave_releases_and_ends_the_session() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["claim", "sim/a"]);
    c.ok(&a, &["leave"]);
    assert_eq!(c.store().get("sim/a").unwrap().status, "ready");
    assert_eq!(c.run_as(&a, &["claim", "sim/a"], "").code, 1);
}

#[test]
fn export_writes_a_markdown_snapshot() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(
        &a,
        &["create", "sim/a", "Alpha", "--link", "docs/SPEC.md#Jump"],
    );
    c.ok(&a, &["claim", "sim/a"]);
    let out = scratch("export-snapshot").join("tasks.md");
    c.ok("", &["export", "--out", out.to_str().unwrap()]);
    let text = fs::read_to_string(&out).unwrap();
    for said in ["## in_progress", "**sim/a**", "[[SPEC#Jump]]", &a] {
        assert!(text.contains(said), "{said:?} not in {text}");
    }
}

#[test]
fn a_body_file_of_a_dash_is_read_from_stdin() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let body = "line one\n\nline two with `code` and 'quotes' — and more\n";
    let ran = c.run_as(
        &orch,
        &["create", "a/long", "Long", "--body-file", "-"],
        body,
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(c.store().get("a/long").unwrap().body, body);
}

#[test]
fn a_line_is_said_from_a_file_or_stdin_and_never_from_both_or_neither() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    assert_eq!(c.run_as(&orch, &["create", "a/long", "Long"], "").code, 0);
    let post = "tests: S1 red at 1a2b3c4\n\n- `code`, 'quotes' and $(nothing run)\n";
    let file = scratch("say-file").join("post.md");
    std::fs::write(&file, post).unwrap();
    let ran = c.run_as(
        &orch,
        &["say", "a/long", "--file", file.to_str().unwrap()],
        "",
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    let ran = c.run_as(&orch, &["say", "a/long", "--file", "-"], "from stdin\n");
    assert_eq!(ran.code, 0, "{}", ran.err);
    let said: Vec<String> = (c.store().thread("a/long").unwrap().into_iter())
        .map(|m| m.text)
        .collect();
    // Its trailing line end is the file's, not the line's.
    assert!(said.contains(&post.trim_end().to_string()), "{said:?}");
    assert!(said.contains(&"from stdin".to_string()), "{said:?}");
    for (args, why) in [
        (
            &["say", "a/long", "text", "--file", "-"][..],
            "✖ say takes its line as TEXT or from --file, never both\n",
        ),
        (
            &["say", "a/long"][..],
            "✖ say needs its line: TEXT, or --file FILE (- for stdin)\n",
        ),
    ] {
        let ran = c.run_as(&orch, args, "");
        assert_eq!((ran.code, ran.err.as_str()), (1, why), "{args:?}");
    }
    let missing = scratch("say-missing").join("missing.md");
    let missing = missing.to_str().unwrap();
    let ran = c.run_as(&orch, &["say", "a/long", "--file", missing], "");
    assert_eq!(
        (ran.code, ran.err),
        (
            1,
            format!(
                "✖ no line file {missing}: --file names it by its path from here, or - for stdin\n"
            )
        )
    );
    // A folder is no file: what the system says of it is the reason.
    let folder = scratch("say-folder");
    let folder = folder.to_str().unwrap();
    let ran = c.run_as(&orch, &["say", "a/long", "--file", folder], "");
    assert_eq!(ran.code, 1);
    assert!(
        ran.err
            .starts_with(&format!("✖ cannot read the line file {folder}: ")),
        "{}",
        ran.err
    );
    // A file of line ends alone is an empty line, which the board refuses as it refuses `say S ""`.
    let ran = c.run_as(&orch, &["say", "a/long", "--file", "-"], "\r\n\n");
    assert_eq!(
        (ran.code, ran.err.as_str()),
        (1, "✖ an empty message says nothing\n")
    );
    let before = c.store().thread("a/long").unwrap().len();
    // Windows line ends go as Unix ones do, and the lines inside the post stay as written.
    let ran = c.run_as(
        &orch,
        &["say", "a/long", "--file", "-"],
        "one\r\n\r\ntwo\r\n",
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    let thread = c.store().thread("a/long").unwrap();
    assert_eq!(thread.len(), before + 1);
    assert_eq!(thread.last().unwrap().text, "one\r\n\r\ntwo");
}

#[test]
fn a_body_file_that_cannot_be_read_is_a_refusal_naming_it() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let missing = scratch("body-missing").join("missing.md");
    let missing = missing.to_str().unwrap();
    let ran = c.run_as(
        &orch,
        &["create", "a/long", "Long", "--body-file", missing],
        "",
    );
    assert_eq!(
        (ran.code, ran.err),
        (
            1,
            format!(
                "✖ no body file {missing}: --body-file names it by its path from here, or - for \
                 stdin\n"
            )
        )
    );
    // A folder is no file: what the system says of it is the reason.
    let folder = scratch("body-folder");
    let folder = folder.to_str().unwrap();
    let ran = c.run_as(
        &orch,
        &["create", "a/long", "Long", "--body-file", folder],
        "",
    );
    assert_eq!(ran.code, 1);
    assert!(
        ran.err
            .starts_with(&format!("✖ cannot read the body file {folder}: ")),
        "{}",
        ran.err
    );
}

/// `x/a` over `x/a/b` over `x/a/b/c`, and `x/other`, made by an orchestrator; then a worker: both handles.
fn chain(c: &Case) -> (String, String) {
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", "x/a", "Top"]);
    c.ok(&orch, &["create", "x/a/b", "Middle", "--parent", "x/a"]);
    c.ok(&orch, &["create", "x/a/b/c", "Bottom", "--parent", "x/a/b"]);
    c.ok(&orch, &["create", "x/other", "Other"]);
    (orch, c.register("worker"))
}

#[test]
fn list_indents_children_under_their_parent_at_any_depth() {
    let c = Case::new();
    chain(&c);
    let out = c.ok("", &["list"]);
    let named = ["x/a", "x/a/b", "x/a/b/c", "x/other"];
    let rows: Vec<(&str, usize)> = out
        .lines()
        .map(|row| {
            let slug = row.split_whitespace().find(|w| named.contains(w)).unwrap();
            (slug, row.len() - row.trim_start_matches(' ').len())
        })
        .collect();
    let at = |slug: &str| rows.iter().position(|(s, _)| *s == slug).unwrap();
    let indent = |slug: &str| rows[at(slug)].1;
    assert_eq!(indent("x/a"), indent("x/other"), "{out}");
    assert!(indent("x/a/b") > indent("x/a"), "{out}");
    assert!(indent("x/a/b/c") > indent("x/a/b"), "{out}");
    assert_eq!(at("x/a/b/c"), at("x/a/b") + 1, "{out}");
    assert_eq!(at("x/a/b"), at("x/a") + 1, "{out}");
}

#[test]
fn list_with_a_filter_keeps_a_child_whose_parent_it_hides_at_the_top() {
    let c = Case::new();
    let (_, agent) = chain(&c);
    c.ok(&agent, &["claim", "x/a/b/c"]);
    let out = c.ok("", &["list", "--status", "in_progress"]);
    let rows: Vec<&str> = out.lines().collect();
    assert_eq!(rows.len(), 1, "{out}");
    assert!(rows[0].starts_with("p2"), "{out}");
}

#[test]
fn show_lists_the_children_with_their_status_and_holder() {
    let c = Case::new();
    let (_, agent) = chain(&c);
    c.ok(&agent, &["claim", "x/a/b"]);
    let out = c.ok("", &["show", "x/a"]);
    let below = out.split("subtasks:\n").nth(1).unwrap_or_default();
    let first: Vec<&str> = below
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    assert_eq!(first, ["x/a/b", "in_progress", "executor", &agent], "{out}");
    assert!(
        !out.contains("x/a/b/c"),
        "one level; the child's own show lists its own: {out}"
    );
    assert!(c.ok("", &["show", "x/a/b"]).contains("x/a/b/c"));
    let data = json(&c.ok("", &["show", "x/a", "--json"]));
    assert_eq!(slugs(&data["children"]), ["x/a/b"]);
}

#[test]
fn create_for_a_handle_reserves_it_and_next_offers_it_to_that_handle_alone() {
    let c = Case::new();
    let (orch, agent) = chain(&c);
    let other = c.register("worker");
    c.ok(
        &orch,
        &[
            "create",
            "x/a/review",
            "Review",
            "--parent",
            "x/a",
            "--for",
            &agent,
            "--instruction",
        ],
    );
    assert!(!c.ok(&other, &["next"]).contains("x/a/review"));
    assert!(c.ok(&agent, &["next"]).starts_with("x/a/review"));
    assert_eq!(c.run_as(&other, &["claim", "x/a/review"], "").code, 1);
    c.ok(&agent, &["claim", "x/a/review"]);
}

#[test]
fn done_closes_both_kinds_of_subtask_without_a_landing() {
    let c = Case::new();
    let (orch, agent) = chain(&c);
    c.ok(
        &orch,
        &[
            "create",
            "x/a/m",
            "Measure",
            "--parent",
            "x/a",
            "--instruction",
        ],
    );
    c.ok(&agent, &["claim", "x/a/m"]);
    assert!(
        c.ok(&agent, &["done", "x/a/m", "median 4 ms"])
            .contains("done")
    );
    assert_eq!(c.store().get("x/a/m").unwrap().status, "done");
    c.ok(&agent, &["claim", "x/a/b"]);
    let ran = c.run_as(&agent, &["done", "x/a/b", "finished"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(c.store().get("x/a/b").unwrap().status, "done");
}

#[test]
fn parent_moves_a_task_and_refuses_a_cycle() {
    let c = Case::new();
    let (orch, _) = chain(&c);
    c.ok(&orch, &["parent", "x/other", "x/a/b/c"]);
    assert_eq!(
        c.store().get("x/other").unwrap().parent.as_deref(),
        Some("x/a/b/c")
    );
    let ran = c.run_as(&orch, &["parent", "x/a", "x/other"], "");
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains("ancestor"), "{}", ran.err);
    c.ok(&orch, &["parent", "x/other", "--none"]);
    assert_eq!(c.store().get("x/other").unwrap().parent, None);
}

#[test]
fn depends_points_a_cancellation_blocked_task_at_the_successor_and_unblocks_it() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    for slug in ["sim/old", "sim/new", "sim/other"] {
        c.ok(&orch, &["create", slug, "Prerequisite"]);
    }
    c.ok(
        &orch,
        &["create", "sim/dep", "Dependent", "--depends", "sim/old"],
    );
    c.ok(&orch, &["move", "sim/old", "cancelled"]);
    assert_eq!(c.store().get("sim/dep").unwrap().status, "blocked");
    assert_eq!(
        c.ok(
            &orch,
            &["depends", "sim/dep", "--replace", "sim/old", "sim/new"]
        ),
        "sim/dep depends on sim/new and is ready\n"
    );
    let out = c.ok("", &["show", "sim/dep"]);
    for said in [
        "changed its dependencies: removed sim/old; added sim/new",
        "unblocked: no cancelled dependency left; back to ready",
    ] {
        assert!(out.contains(said), "{said:?} not in {out}");
    }
    assert_eq!(
        c.ok(
            &orch,
            &[
                "depends",
                "sim/dep",
                "--add",
                "sim/other",
                "--remove",
                "sim/new"
            ]
        ),
        "sim/dep depends on sim/other and is ready\n"
    );
    assert_eq!(
        c.ok(&orch, &["depends", "sim/dep", "--remove", "sim/other"]),
        "sim/dep depends on nothing and is ready\n"
    );
    for (args, code, why) in [
        (
            &["depends", "sim/dep", "--add", "sim/dep"][..],
            1,
            "sim/dep cannot wait on itself",
        ),
        (
            &["depends", "sim/dep"][..],
            1,
            "name a dependency to add, remove or replace",
        ),
        (
            &["depends", "sim/dep", "--replace", "sim/new"][..],
            2,
            "argument --replace: expected 2 arguments",
        ),
    ] {
        let ran = c.run_as(&orch, args, "");
        assert_eq!(ran.code, code, "{args:?}: {}", ran.err);
        assert!(ran.err.contains(why), "{args:?}: {}", ran.err);
    }
    assert!(c.store().get("sim/dep").unwrap().depends.is_empty());
}

#[test]
fn help_and_the_spellings_people_type_read_and_a_mistake_says_what_to_type_instead() {
    let c = Case::new();
    let top = c.ok("", &["help"]);
    assert!(top.starts_with("usage: just tasks "), "{top}");
    assert!(top.contains("\n  claim "), "{top}");
    assert!(
        c.ok("", &["help", "claim"])
            .contains("\n\ntake a task as its executor.\n")
    );
    let ran = c.run_as("", &["shwo", "sim/a"], "");
    assert_eq!((ran.code, ran.out.as_str()), (2, ""));
    assert!(
        ran.err.ends_with(
            "just tasks: error: no command 'shwo'; did you mean 'show'? (`just tasks help` lists \
             them)\n"
        ),
        "{}",
        ran.err
    );
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", "sim/a", "A", "--priority", "p1"]);
    assert_eq!(c.store().get("sim/a").unwrap().priority, 1);
    assert_eq!(c.ok(&orch, &["priority", "sim/a", "P0"]), "sim/a is p0\n");
    let exe = c.register("worker");
    c.ok(&exe, &["claim", "sim/a"]);
    assert!(
        c.ok("", &["list", "--status", "in-progress"])
            .contains("sim/a")
    );
    for (args, why) in [
        (
            &["create", "noslash", "X"][..],
            "✖ slug 'noslash' names no area: a task's slug is area/short-name, such as \
             tools/noslash\n",
        ),
        (
            &["say", "sim/a", "hi", "--to", "orchestrator"][..],
            "✖ no session 'orchestrator' to send to; the task's orchestrator is --to \
             @orchestrator\n",
        ),
    ] {
        let ran = c.run_as(&orch, args, "");
        assert_eq!((ran.code, ran.err.as_str()), (1, why), "{args:?}");
    }
    assert!(c.store().get("noslash").is_err());
}

#[test]
fn an_empty_who_says_so() {
    let c = Case::new();
    assert_eq!(
        c.ok("", &["who"]),
        "no sessions (--all adds the ones that left)\n"
    );
    assert_eq!(c.ok("", &["who", "--all"]), "no sessions\n");
}
