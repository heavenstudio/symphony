//! `launch`: the handle, the worktree and its branch, the terminal with its agent and first message,
//! where the Herdr app shows it, how the launch is confirmed, and the record on the task; what a
//! failure undoes.

use crate::common::refused;
use crate::{Hooked, SPACE, World, git};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use symphony_board::Error;
use symphony_board::confine::MARKER;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::{Ask, Bounds, CONFIRM_LOOKS, LaunchError, PROMPT_WAIT};
use symphony_board::launch::{Started, launch};

fn flag_value<'a>(call: &'a [String], flag: &str) -> &'a str {
    let at = call
        .iter()
        .position(|a| a == flag)
        .expect("the flag is there");
    &call[at + 1]
}

/// The values of every `flag` of a call.
fn flag_values<'a>(call: &'a [String], flag: &str) -> Vec<&'a str> {
    call.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .collect()
}

/// The one `hc run` of a world.
fn the_run(w: &World) -> Vec<String> {
    let [run] = <[_; 1]>::try_from(w.calls_of(&["run"])).unwrap();
    run
}

/// What `hc run` was asked to start, after its `--`.
fn started(run: &[String]) -> &[String] {
    let at = run.iter().position(|a| a == "--").unwrap();
    &run[at + 1..]
}

/// The worker sessions' healths: a failed launch leaves the handle it registered.
fn workers(w: &World) -> Vec<String> {
    w.store()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|s| s.row.kind == "worker")
        .map(|s| s.health.as_str().to_string())
        .collect()
}

#[test]
fn an_executor_gets_a_branch_named_for_the_slug_in_a_worktree_under_wt() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let path = w.wt.join("sim-jump");
    assert_eq!(s.worktree, path.display().to_string());
    assert_eq!(s.branch, "sim/jump");
    assert_eq!(
        git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "sim/jump"
    );
}

#[test]
fn a_reviewer_gets_a_detached_checkout_of_main() {
    let w = World::new();
    let s = w.review("sim/jump").row;
    let path = Path::new(&s.worktree);
    assert_eq!(s.branch, "");
    assert_eq!(git(path, &["rev-parse", "--abbrev-ref", "HEAD"]), "HEAD");
    assert_eq!(
        git(path, &["rev-parse", "HEAD"]),
        git(&w.repo, &["rev-parse", "main"])
    );
}

#[test]
fn the_detached_flag_gives_an_executor_a_detached_checkout_too() {
    let w = World::new();
    let ask = Ask {
        detached: true,
        ..w.ask("sim/jump")
    };
    assert_eq!(w.try_launch(&ask).unwrap().row.branch, "");
}

#[test]
fn an_explicit_branch_is_used_for_the_worktree() {
    let w = World::new();
    let ask = Ask {
        branch: Some("tools/other".into()),
        ..w.ask("sim/jump")
    };
    let s = w.try_launch(&ask).unwrap().row;
    assert_eq!(s.branch, "tools/other");
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "tools/other"
    );
}

#[test]
fn an_existing_branch_is_checked_out_rather_than_made() {
    let w = World::new();
    git(&w.repo, &["branch", "sim/jump"]);
    let s = w.launch("sim/jump").row;
    assert_eq!(
        git(
            Path::new(&s.worktree),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        "sim/jump"
    );
}

#[test]
fn a_given_worktree_is_used_as_it_is() {
    let w = World::new();
    let given = w.dir().join("already");
    fs::create_dir(&given).unwrap();
    let ask = Ask {
        worktree: Some(given.clone()),
        ..w.ask("sim/jump")
    };
    let s = w.try_launch(&ask).unwrap().row;
    assert_eq!(s.worktree, given.display().to_string());
    assert!(!w.wt.join("sim-jump").exists());
    assert_eq!(flag_value(&the_run(&w), "--cwd"), s.worktree);
}

#[test]
fn a_given_worktree_that_does_not_exist_is_refused() {
    let w = World::new();
    let nowhere = w.dir().join("nowhere");
    let ask = Ask {
        worktree: Some(nowhere.clone()),
        ..w.ask("sim/jump")
    };
    assert_eq!(
        refused(w.try_launch(&ask)),
        format!("--worktree {} is not a directory", nowhere.display())
    );
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
}

#[test]
fn a_given_worktree_records_the_branch_it_has_checked_out_or_none_when_detached() {
    let w = World::new();
    let (given, detached) = (w.dir().join("given"), w.dir().join("detached"));
    git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "other/work",
            &given.display().to_string(),
            "main",
        ],
    );
    git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &detached.display().to_string(),
            "main",
        ],
    );
    w.task("sim/dock", None);
    for (slug, path, branch) in [
        ("sim/jump", given, "other/work"),
        ("sim/dock", detached, ""),
    ] {
        let ask = Ask {
            worktree: Some(path),
            ..w.ask(slug)
        };
        assert_eq!(w.try_launch(&ask).unwrap().row.branch, branch, "{slug}");
    }
}

// -- through `just tasks`, which an agent runs unasked -----------------------------------------

/// `ask` launched by a command run unasked in the repository.
fn unasked(w: &World, ask: &Ask) -> Result<Started, Error> {
    launch(w.store(), &w.hc(), &w.unasked(), ask)
}

/// `path`, as a line says it.
fn said(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn only_the_unasked_recipes_marker_bounds_where_an_agent_starts() {
    let mut w = World::new();
    let bounds = |w: &World| Bounds::of(&w.env, &w.repo, &w.wt);
    assert_eq!(bounds(&w), None);
    // An empty value is no mark, as for every variable the board reads.
    w.set(MARKER, "");
    assert_eq!(bounds(&w), None);
    w.set(MARKER, "1");
    assert_eq!(bounds(&w), Some(Bounds::new(&w.repo, &w.repo, &w.wt)));
}

#[test]
fn unasked_a_worktree_given_outside_the_project_is_refused_and_starts_nothing() {
    let w = World::new();
    let elsewhere = w.dir().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, w.repo.join("out-link")).unwrap();
    let kept = w.wt.join("kept");
    git(
        &w.repo,
        &["worktree", "add", "-q", "--detach", &said(&kept), "main"],
    );
    for (given, place) in [
        // Anywhere else: a home folder, another repository.
        (
            elsewhere.clone(),
            format!("--worktree {} is", said(&elsewhere)),
        ),
        // However it is reached: by a link in the project, or a `..` out of it.
        (
            w.repo.join("out-link"),
            format!(
                "--worktree {} leads to {}, which is",
                said(&w.repo.join("out-link")),
                said(&elsewhere)
            ),
        ),
        (
            w.repo.join("../elsewhere"),
            format!(
                "--worktree {} leads to {}, which is",
                said(&w.repo.join("../elsewhere")),
                said(&elsewhere)
            ),
        ),
        // A git folder: the repository's own, and a worktree's `.git` under the base.
        (
            w.repo.join(".git"),
            format!("--worktree {} is", said(&w.repo.join(".git"))),
        ),
        (
            kept.join(".git"),
            format!("--worktree {} is", said(&kept.join(".git"))),
        ),
        // The base itself, which holds every worktree of the project.
        (w.wt.clone(), format!("--worktree {} is", said(&w.wt))),
        // A path that is not absolute names no one folder.
        (
            PathBuf::from("elsewhere"),
            "--worktree elsewhere is no absolute path, so it is".into(),
        ),
    ] {
        let ask = Ask {
            worktree: Some(given.clone()),
            ..w.ask("sim/jump")
        };
        assert_eq!(
            refused(unasked(&w, &ask)),
            w.outside(&place),
            "{}",
            given.display()
        );
    }
    // A link in the project that leads nowhere: git would make whatever it names.
    let dangling = w.repo.join("dangling");
    symlink(w.dir().join("nowhere"), &dangling).unwrap();
    let ask = Ask {
        worktree: Some(dangling.clone()),
        ..w.ask("sim/jump")
    };
    assert_eq!(
        refused(unasked(&w, &ask)),
        format!(
            "--worktree {0} goes through {0}, a link that leads nowhere: a write there would make \
             what it names, wherever that is",
            said(&dangling)
        )
    );
    assert_eq!(w.calls_of(&["run"]), Vec::<Vec<String>>::new());
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
    assert_eq!(workers(&w), Vec::<String>::new());
    // The binary run by itself starts it there.
    let ask = Ask {
        worktree: Some(elsewhere.clone()),
        ..w.ask("sim/jump")
    };
    assert_eq!(w.try_launch(&ask).unwrap().row.worktree, said(&elsewhere));
}

#[test]
fn unasked_a_folder_in_the_project_or_under_its_worktree_base_is_still_taken() {
    let w = World::new();
    let given = w.wt.join("given");
    git(
        &w.repo,
        &["worktree", "add", "-q", "--detach", &said(&given), "main"],
    );
    let plain = w.wt.join("plain");
    fs::create_dir(&plain).unwrap();
    symlink(&plain, w.repo.join("wt-link")).unwrap();
    fs::create_dir(w.repo.join("docs")).unwrap();
    for (slug, folder, recorded) in [
        // The checkout the command runs in, and a folder in it.
        ("sim/a", w.repo.clone(), w.repo.clone()),
        ("sim/b", w.repo.join("docs"), w.repo.join("docs")),
        // A folder under the project's worktree base, given as it is or by a link leading there.
        ("sim/c", given.clone(), given.clone()),
        ("sim/d", w.repo.join("wt-link"), plain.clone()),
    ] {
        w.task(slug, None);
        let ask = Ask {
            worktree: Some(folder),
            ..w.ask(slug)
        };
        let s = unasked(&w, &ask).expect("the launch goes").row;
        assert_eq!(s.worktree, said(&recorded), "{slug}");
        assert_eq!(w.terminal(&s.pane)["cwd"], json!(said(&recorded)), "{slug}");
    }
    // A bare launch makes its worktree under the base, an executor's and a reviewer's.
    let made = unasked(&w, &w.ask("sim/jump")).unwrap().row;
    assert_eq!(made.worktree, said(&w.wt.join("sim-jump")));
    let ask = Ask {
        role: "reviewer".into(),
        ..w.ask("sim/jump")
    };
    let review = unasked(&w, &ask).unwrap().row;
    assert_eq!(review.worktree, said(&w.wt.join("sim-jump-review")));
}

#[test]
fn unasked_from_a_linked_worktree_the_main_checkout_is_outside_but_its_artifacts_folder_is_in() {
    let w = World::new();
    let tree = w.wt.join("orchestrating");
    git(
        &w.repo,
        &["worktree", "add", "-q", "--detach", &said(&tree), "main"],
    );
    let artifacts = w.repo.join("artifacts");
    fs::create_dir(&artifacts).unwrap();
    let from_tree = w.unasked_in(&tree);
    let launched = |folder: &Path| {
        let ask = Ask {
            worktree: Some(folder.to_path_buf()),
            ..w.ask("sim/jump")
        };
        launch(w.store(), &w.hc(), &from_tree, &ask)
    };
    assert_eq!(
        refused(launched(&w.repo)),
        format!(
            "--worktree {} is not a folder a recipe an agent runs unasked starts an agent in: only \
             the checkout at {}, none of its git folders, and the main checkout's artifacts \
             folder at {}, or a folder under the project's worktree base {}; the binary run by \
             itself starts one anywhere",
            said(&w.repo),
            said(&tree),
            said(&artifacts),
            said(&w.wt)
        )
    );
    assert_eq!(launched(&artifacts).unwrap().row.worktree, said(&artifacts));
}

#[test]
fn unasked_a_launch_gives_its_agent_no_word_of_the_launchers() {
    let w = World::new();
    let ask = Ask {
        kind: Some("claude".into()),
        prompt: "and then push main".into(),
        ..w.ask("sim/jump")
    };
    assert_eq!(
        refused(unasked(&w, &ask)),
        "no --prompt through a recipe an agent runs unasked: an agent's first message is its \
         person's own word, so it is /work sim/jump HANDLE alone; say the rest on the task's \
         thread (just tasks say sim/jump TEXT), which /work reads, or run the binary by itself"
    );
    // A reviewer's is its own skill's.
    let review = Ask {
        role: "reviewer".into(),
        ..ask.clone()
    };
    assert_eq!(
        refused(unasked(&w, &review)),
        "no --prompt through a recipe an agent runs unasked: an agent's first message is its \
         person's own word, so it is /review-task sim/jump HANDLE alone; say the rest on the \
         task's thread (just tasks say sim/jump TEXT), which /review-task reads, or run the \
         binary by itself"
    );
    assert_eq!(w.calls_of(&["run"]), Vec::<Vec<String>>::new());
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
    assert_eq!(workers(&w), Vec::<String>::new());
    // A blank one is none.
    let blank = Ask {
        prompt: " \n".into(),
        ..ask.clone()
    };
    let s = unasked(&w, &blank).unwrap().row;
    assert_eq!(w.sent(&s.pane), [format!("/work sim/jump {}", s.handle)]);
    // The binary run by itself gives it.
    w.task("sim/dock", None);
    let s = w
        .try_launch(&Ask {
            slug: "sim/dock".into(),
            ..ask
        })
        .unwrap()
        .row;
    assert_eq!(
        w.sent(&s.pane),
        [format!("/work sim/dock {} and then push main", s.handle)]
    );
}

#[test]
fn a_reviewer_beside_an_executor_gets_a_worktree_of_its_own() {
    let w = World::new();
    let a = w.launch("sim/jump").row.worktree;
    let first = w.review("sim/jump").row;
    let b = first.worktree;
    // A second reviewer is launched only beside the first, saying why: refused while the first
    // still runs (the launch would resume it), naming the launch's own --beside, it leaves nothing.
    let ask = Ask {
        role: "reviewer".into(),
        kind: Some("reviewer".into()),
        ..w.ask("sim/jump")
    };
    let launched = w.store().task_sessions("sim/jump").unwrap();
    let refusal = refused(w.try_launch(&ask));
    let board = w.store().board_command();
    assert!(
        refusal.starts_with(&format!(
            "{} reviews sim/jump and still runs in terminal",
            first.handle
        )) && refusal.ends_with(&format!(
            "({board} launch sim/jump --role reviewer --beside WHY)"
        )),
        "{refusal}"
    );
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), launched);
    assert!(!w.wt.join("sim-jump-review-2").exists());
    let beside = Ask {
        beside: Some("a second stamp".into()),
        ..ask
    };
    let c = w.try_launch(&beside).unwrap().row.worktree;
    assert_eq!(
        w.store().get("sim/jump").unwrap().holders("reviewer").len(),
        2
    );
    assert_eq!(
        [a, b, c].map(|p| Path::new(&p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()),
        ["sim-jump", "sim-jump-review", "sim-jump-review-2"].map(String::from)
    );
    // Only a reviewer joins beside another.
    let executor = Ask {
        beside: Some("why".into()),
        ..w.ask("sim/jump")
    };
    assert_eq!(
        refused(w.try_launch(&executor)),
        "only a reviewer joins beside another, not 'executor'"
    );
}

#[test]
fn a_worktree_that_exists_is_refused_not_overwritten() {
    let w = World::new();
    fs::create_dir_all(w.wt.join("sim-jump")).unwrap();
    assert_eq!(
        refused(w.try_launch(&w.ask("sim/jump"))),
        format!(
            "{} exists: give it with --worktree, or remove it",
            w.wt.join("sim-jump").display()
        )
    );
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
}

#[test]
fn the_agent_runs_in_an_hc_terminal_that_outlives_its_launcher_and_is_kept() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let run = the_run(&w);
    assert_eq!(run[1], "--bg");
    assert_eq!(flag_value(&run, "--name"), s.handle);
    assert_eq!(flag_value(&run, "--cwd"), s.worktree);
    let options = &run[..run.iter().position(|a| a == "--").unwrap()];
    for flag in [
        "--no-parent",
        "--outlive-parent",
        "--keep",
        "--no-notify",
        "--json",
    ] {
        assert!(options.contains(&flag.to_string()), "{flag}: {run:?}");
    }
    let labels = flag_values(&run, "--label");
    assert_eq!(labels.len(), 4, "{labels:?}");
    assert_eq!(labels[0], format!("herdr.space={SPACE}"));
    assert!(labels[1].starts_with("herdr.tab=tb"), "{labels:?}");
    assert_eq!(
        labels[2..],
        [
            "symphony.board=lamp-works/tasks".to_string(),
            format!("symphony.handle={}", s.handle)
        ]
    );
    let t = w.terminal(&s.pane);
    assert_eq!(
        (t["keep"].clone(), t["outlive_parent"].clone()),
        (json!(true), json!(true))
    );
}

#[test]
fn a_launch_from_inside_a_terminal_is_no_child_of_it_and_shows_in_the_streams_tab() {
    let mut w = World::new();
    let caller = w.add_terminal(true, Some(&w.orch), json!({}));
    w.set("HC_TERMINAL_ID", &caller);
    let s = w.launch("sim/jump").row;
    assert_eq!(w.terminal(&s.pane)["parent"], json!(null));
    let tab = flag_values(&the_run(&w), "--label")[1].replace("herdr.tab=", "");
    assert_eq!(w.tab_name(&tab), ("lamp-works".into(), "code".into()));
}

#[test]
fn claude_starts_with_a_session_id_chosen_up_front_the_model_and_its_first_message() {
    let w = World::new();
    let s = w
        .try_launch(&Ask {
            kind: Some("claude".into()),
            ..w.ask("sim/jump")
        })
        .unwrap()
        .row;
    assert_eq!(s.claude_session, "00000001-0000-4000-8000-000000000000");
    let run = the_run(&w);
    assert_eq!(
        started(&run),
        [
            "env",
            &format!("TASKS_SESSION={}", s.handle),
            "claude",
            "--session-id",
            &s.claude_session,
            "-n",
            &s.handle,
            "--permission-mode",
            "auto",
            "--model",
            "sonnet",
        ]
    );
    assert_eq!(w.sent(&s.pane), [format!("/work sim/jump {}", s.handle)]);
}

#[test]
fn an_asked_effort_goes_to_claudes_effort_flag_and_codexs_reasoning_effort() {
    for (kind, effort, expected) in [
        (
            "claude",
            Some("max"),
            &["--model", "sonnet", "--effort", "max"][..],
        ),
        ("claude", Some(""), &["--model", "sonnet"][..]),
        (
            "codex",
            Some("high"),
            &["model_reasoning_effort=\"high\""][..],
        ),
        ("codex", None, &["model_reasoning_effort=\"xhigh\""][..]),
    ] {
        let w = World::new();
        w.try_launch(&Ask {
            kind: Some(kind.into()),
            effort: effort.map(str::to_string),
            ..w.ask("sim/jump")
        })
        .unwrap();
        let run = the_run(&w);
        let program = started(&run);
        assert!(
            program.windows(expected.len()).any(|part| part == expected),
            "{kind} {effort:?}: {program:?}"
        );
        if kind == "claude" && expected.len() == 2 {
            assert!(!program.iter().any(|a| a == "--effort"), "{program:?}");
            assert_eq!(program.last().map(String::as_str), Some("sonnet"));
        }
    }
}

#[test]
fn codex_is_the_default_without_its_daemon_and_its_thread_is_recorded_once_it_registers() {
    for (model, expected) in [("", "gpt-6.1-sol"), ("chosen-model", "chosen-model")] {
        let w = World::new();
        let ask = Ask {
            model: model.into(),
            ..w.ask("sim/jump")
        };
        let s = w.try_launch(&ask).unwrap().row;
        assert_eq!(s.claude_session, "codex:");
        let run = the_run(&w);
        let handle = &s.handle;
        let wrapper = symphony_board::launch::hc::IN_ITS_TERMINAL;
        assert_eq!(
            started(&run),
            [
                "env",
                &format!("TASKS_SESSION={handle}"),
                wrapper[0],
                wrapper[1],
                wrapper[2],
                "codex",
                "--no-daemon",
                "-s",
                "workspace-write",
                "-m",
                expected,
                "-c",
                "model_reasoning_effort=\"xhigh\"",
                "-c",
                "sandbox_workspace_write.network_access=true",
                "-c",
                &format!("shell_environment_policy.set.TASKS_SESSION=\"{handle}\""),
            ]
        );
        assert_eq!(w.sent(&s.pane), [format!("$work sim/jump {handle}")]);
        // The thread is the agent's to say: its callback fills the record, from its terminal.
        w.store()
            .callback_in(handle, Some("codex:0abc-12"), false, false, Some(&s.pane))
            .unwrap();
        let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!(read.claude_session, "codex:0abc-12");
        assert_eq!(
            w.store().callbacks().unwrap()[handle].terminal.as_deref(),
            Some(s.pane.as_str())
        );
    }
}

#[test]
fn a_launched_agent_is_given_the_board_its_launcher_reaches() {
    for kind in [None, Some("claude".to_string())] {
        let w = World::new();
        let mut machine = w.machine();
        machine.board_env = vec![
            ("TASKS_DATABASE".into(), "scratch".into()),
            ("TASKS_SERVER".into(), "postgresql:/scratch/socket".into()),
        ];
        let ask = Ask {
            kind: kind.clone(),
            ..w.ask("sim/jump")
        };
        let s = symphony_board::launch::launch(w.store(), &w.hc(), &machine, &ask)
            .unwrap()
            .row;
        let run = the_run(&w);
        let program = started(&run);
        assert_eq!(
            program[..4],
            [
                "env".to_string(),
                format!("TASKS_SESSION={}", s.handle),
                "TASKS_DATABASE=scratch".into(),
                "TASKS_SERVER=postgresql:/scratch/socket".into()
            ]
        );
        if kind.is_none() {
            let set = flag_values(program, "-c");
            assert!(
                set.contains(&"shell_environment_policy.set.TASKS_DATABASE=\"scratch\""),
                "{set:?}"
            );
        }
    }
}

#[test]
fn no_model_starts_claude_with_its_own() {
    let w = World::new();
    let ask = Ask {
        kind: Some("claude".into()),
        model: String::new(),
        ..w.ask("sim/jump")
    };
    w.try_launch(&ask).unwrap();
    let run = the_run(&w);
    assert!(!run.contains(&"--model".to_string()), "{run:?}");
}

#[test]
fn the_first_message_is_work_slug_handle_alone_or_followed_by_a_given_prompt() {
    for (kind, work) in [(None, "$work"), (Some("claude".into()), "/work")] {
        let w = World::new();
        let s = w
            .try_launch(&Ask {
                kind: kind.clone(),
                ..w.ask("sim/jump")
            })
            .unwrap()
            .row;
        w.task("sim/dock", None);
        let ask = Ask {
            kind,
            prompt: "  Read the brief.  ".into(),
            ..w.ask("sim/dock")
        };
        let dock = w.try_launch(&ask).unwrap().row;
        assert_eq!(w.sent(&s.pane), [format!("{work} sim/jump {}", s.handle)]);
        assert_eq!(
            w.sent(&dock.pane),
            [format!("{work} sim/dock {} Read the brief.", dock.handle)]
        );
    }
}

#[test]
fn a_reviewers_first_message_is_the_review_skill_never_work() {
    for (kind, review) in [
        (Some("codex".into()), "$review-task"),
        (Some("claude".into()), "/review-task"),
    ] {
        let w = World::new();
        let s = w
            .try_launch(&Ask {
                role: "reviewer".into(),
                kind,
                prompt: "Judge the tip.".into(),
                ..w.ask("sim/jump")
            })
            .unwrap()
            .row;
        assert_eq!(
            w.sent(&s.pane),
            [format!("{review} sim/jump {} Judge the tip.", s.handle)]
        );
    }
}

#[test]
fn everything_is_recorded_on_the_task_and_the_session() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let [read] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(read, s);
    assert_eq!(
        (
            read.role.as_str(),
            read.model.as_str(),
            read.started,
            read.closed
        ),
        ("executor", "sonnet", w.f.now(), None)
    );
    assert_eq!(w.live(), [read.pane.as_str()]);
    let board = w.store().session(&s.handle).unwrap().row;
    assert_eq!(
        (
            board.kind.as_str(),
            board.worktree.as_str(),
            board.branch.as_str(),
            board.parent
        ),
        (
            "worker",
            s.worktree.as_str(),
            "sim/jump",
            Some(w.orch.clone())
        )
    );
    assert!(
        w.f.said("sim/jump")
            .iter()
            .any(|m| m.contains("launched") && m.contains(&s.handle)),
        "{:?}",
        w.f.said("sim/jump")
    );
}

#[test]
fn the_executor_has_the_task_reserved_for_its_work_command() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    assert_eq!(
        w.store().get("sim/jump").unwrap().holders("reserved"),
        [s.handle.as_str()]
    );
}

#[test]
fn a_reviewer_takes_the_reviewer_role() {
    let w = World::new();
    let s = w.review("sim/jump").row;
    assert_eq!(
        w.store().get("sim/jump").unwrap().holders("reviewer"),
        [s.handle.as_str()]
    );
    assert_eq!(w.store().session(&s.handle).unwrap().row.kind, "reviewer");
}

#[test]
fn an_explicit_root_branch_checked_out_elsewhere_is_a_clear_refusal() {
    let w = World::new();
    let first = w.launch("sim/jump").row; // the root's own worktree holds sim/jump
    w.task("sim/jump/r2", Some("sim/jump"));
    let why = refused(w.try_launch(&Ask {
        branch: Some("sim/jump".into()),
        ..w.ask("sim/jump/r2")
    }));
    assert_eq!(
        why,
        format!(
            "branch sim/jump is checked out in {wt}: to continue it, launch with --worktree {wt}; \
             --branch <other> starts another branch; --detached is for read-only work",
            wt = first.worktree
        )
    );
}

#[test]
fn a_task_that_is_finished_or_unknown_is_refused_before_hc_is_asked() {
    let w = World::new();
    assert_eq!(
        refused(w.try_launch(&w.ask("sim/none"))),
        "no task 'sim/none'"
    );
    w.store().move_to("sim/jump", "cancelled", &w.orch).unwrap();
    assert_eq!(
        refused(w.try_launch(&w.ask("sim/jump"))),
        "sim/jump is cancelled"
    );
    assert_eq!(w.calls(), Vec::<Vec<String>>::new());
}

#[test]
fn a_role_that_is_no_launched_role_is_refused() {
    let w = World::new();
    let ask = Ask {
        role: "orchestrator".into(),
        ..w.ask("sim/jump")
    };
    assert_eq!(
        refused(w.try_launch(&ask)),
        "a launched session is an executor or a reviewer, not 'orchestrator'"
    );
}

#[test]
fn a_role_the_board_refuses_leaves_the_handle_and_makes_nothing() {
    let w = World::new();
    let other = w.f.session("worker");
    w.store().reserve("sim/jump", &other, 3600.0).unwrap();
    assert!(matches!(
        w.try_launch(&w.ask("sim/jump")),
        Err(Error::Refused(_))
    ));
    let left: Vec<String> = w
        .store()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|s| s.row.kind == "worker" && s.row.handle != other)
        .map(|s| s.health.as_str().to_string())
        .collect();
    assert_eq!(left, ["left"]);
    assert!(!w.wt.join("sim-jump").exists());
    assert!(w.addresses().is_empty());
}

// -- where the Herdr app shows it ----------------------------------------------------------------

#[test]
fn a_launch_with_no_stream_tab_makes_one_named_for_the_role_in_the_projects_space() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let tab = flag_values(&the_run(&w), "--label")[1]
        .strip_prefix("herdr.tab=")
        .unwrap()
        .to_string();
    assert_eq!(tab.len(), 34, "{tab}");
    assert_eq!(w.tab_name(&tab), ("lamp-works".into(), "code".into()));
    assert_eq!(
        w.calls_of(&["app", "do"]),
        [vec![
            "app",
            "do",
            "tab.rename",
            "--id",
            &tab,
            "--name",
            "code"
        ]]
    );
    assert_eq!(w.terminal(&s.pane)["labels"]["herdr.tab"], tab.as_str());
}

#[test]
fn a_reviewer_goes_to_the_review_tab_and_a_given_tab_overrides_the_role() {
    let w = World::new();
    w.review("sim/jump");
    let given = Ask {
        tab: Some("3d".into()),
        ..w.ask("sim/jump")
    };
    w.try_launch(&given).unwrap();
    let names: Vec<String> = w
        .calls_of(&["app", "do", "tab.rename"])
        .iter()
        .map(|c| c[6].clone())
        .collect();
    assert_eq!(names, ["review", "3d"]);
}

#[test]
fn a_stream_tab_with_room_is_joined_and_left_as_it_is_named() {
    let w = World::new();
    let code = w.add_tab("code", 3);
    let s = w.launch("sim/jump").row;
    assert_eq!(w.terminal(&s.pane)["labels"]["herdr.tab"], code.as_str());
    assert!(w.calls_of(&["app", "do"]).is_empty());
    assert_eq!(
        w.calls_of(&["ls", "--label"]),
        [vec![
            "ls",
            "--label",
            &format!("herdr.tab={code}"),
            "--json"
        ]]
    );
}

#[test]
fn a_full_stream_gets_a_second_tab_named_with_a_number_filled_before_a_third_opens() {
    let w = World::new();
    w.add_tab("code", 4);
    let second = w.add_tab("code-2", 3);
    w.task("sim/dock", None);
    let s = w.launch("sim/jump").row;
    assert_eq!(w.terminal(&s.pane)["labels"]["herdr.tab"], second.as_str());
    let d = w.launch("sim/dock").row;
    let third = w.terminal(&d.pane)["labels"]["herdr.tab"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(w.tab_name(&third).1, "code-3");
}

#[test]
fn the_owners_tabs_and_other_streams_tabs_are_never_a_destination() {
    let w = World::new();
    let owners = [
        w.add_tab("orch2", 0),
        w.add_tab("review", 0),
        w.add_tab("codex", 0),
    ];
    let s = w.launch("sim/jump").row;
    let tab = w.terminal(&s.pane)["labels"]["herdr.tab"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!owners.contains(&tab), "{tab}");
    assert_eq!(w.tab_name(&tab).1, "code");
}

#[test]
fn a_missing_space_is_made_and_named_and_another_projects_is_never_used() {
    let w = World::new();
    let mut state = w.state();
    state["layout"] = json!({"spaces": [
        {"id": "sp-bench", "name": "web-bench", "tabs": [{"id": "tb-bench", "name": "code"}]},
    ]});
    w.set_state(state);
    let s = w.launch("sim/jump").row;
    let labels = w.terminal(&s.pane)["labels"].clone();
    let (space, tab) = (
        labels["herdr.space"].as_str().unwrap().to_string(),
        labels["herdr.tab"].as_str().unwrap().to_string(),
    );
    assert!(space.starts_with("sp") && space != "sp-bench", "{space}");
    assert_ne!(tab, "tb-bench");
    assert_eq!(w.tab_name(&tab), ("lamp-works".into(), "code".into()));
    assert_eq!(w.state()["layout"]["spaces"][0]["name"], "web-bench");
}

#[test]
fn a_window_that_does_not_answer_places_by_the_cores_document_and_names_nothing() {
    let mut w = World::new();
    w.set("FAKE_HC_FAIL", "app ls,app do");
    let s = w.launch("sim/jump").row;
    assert_eq!(w.calls_of(&["connect"]).len(), 1);
    let tab = w.terminal(&s.pane)["labels"]["herdr.tab"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(w.tab_name(&tab), ("lamp-works".into(), String::new()));
    // The one look before the confirmation tried to name it.
    assert_eq!(w.calls_of(&["app", "do"]).len(), 1);
}

#[test]
fn a_layout_neither_the_window_nor_the_core_gives_fails_before_a_terminal_starts() {
    let mut w = World::new();
    w.set("FAKE_HC_FAIL", "app ls,connect");
    let why = refused(w.try_launch(&w.ask("sim/jump")));
    assert_eq!(
        why,
        "cannot find the Herdr app's spaces: hc connect: fake_failure: connect failed"
    );
    assert!(w.calls_of(&["run"]).is_empty());
    assert!(!w.wt.join("sim-jump").exists());
    assert_eq!(workers(&w), ["left"]);
}

// -- how the launch is confirmed ----------------------------------------------------------------

#[test]
fn the_first_message_is_sent_once_hc_calls_the_agent_ready_and_its_receipt_confirms_the_launch() {
    let w = World::new();
    let s = w.launch("sim/jump").row;
    let calls: Vec<Vec<String>> = w
        .calls()
        .into_iter()
        .filter(|c| c.get(1) == Some(&s.pane))
        .collect();
    let words: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();
    // One look shows it ready; then the message, never forced; nothing read, no key pressed.
    assert_eq!(words, ["get", "send"], "{calls:?}");
    assert_eq!(calls[1][3..], ["--json"], "{calls:?}");
    assert_eq!(w.sent(&s.pane), [format!("$work sim/jump {}", s.handle)]);
    assert_eq!(w.terminal(&s.pane)["status"], "working");
    assert_eq!(*w.sleeps.borrow(), [PROMPT_WAIT]);
}

/// A failed confirmation must undo every resource the new launch owned: its terminal killed and
/// removed, no record, no worktree, the handle left. The terminal's address.
fn failed_launch(w: &World, result: Result<Started, Error>, why: &str) -> String {
    let refusal = refused(result);
    let [run] = <[_; 1]>::try_from(w.calls_of(&["run"])).unwrap();
    let name = flag_value(&run, "--name").to_string();
    let address = w
        .calls_of(&["get"])
        .iter()
        .map(|c| c[1].clone())
        .find(|a| a != &name)
        .expect("the launch looked at its terminal");
    assert_eq!(refusal, why.replace("{t}", &address));
    assert!(!w.has_terminal(&address), "{address} was left");
    assert_eq!(w.calls_of(&["rm", &address]).len(), 1);
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
    assert!(!w.wt.join("sim-jump").exists());
    assert_eq!(workers(w), ["left"]);
    address
}

#[test]
fn claudes_folder_trust_dialog_fails_the_launch_unsent_naming_the_terminal_and_its_question() {
    let mut w = World::new();
    w.set("FAKE_HC_BLOCKED", "1");
    let ask = Ask {
        kind: Some("claude".into()),
        ..w.ask("sim/jump")
    };
    let seen = RefCell::new(Vec::new());
    let hc = Hooked {
        instead: Box::new(|args| {
            if args[0] != "get" || !args[1].starts_with('t') {
                return None;
            }
            let answer = w.hc().run(args);
            seen.borrow_mut().push(answer.clone());
            Some(answer)
        }),
        ..Hooked::new(&w)
    };
    let launched = symphony_board::launch::launch(w.store(), &hc, &w.machine(), &ask);
    let address = failed_launch(
        &w,
        launched,
        "the agent in terminal {t} waits on a person before it took its first message: \
         Quick safety check: Is this a project you created or one you trust?",
    );
    // hc called it blocked, and as for any run with no template, recorded no first message.
    let looked: serde_json::Value =
        serde_json::from_str(&seen.borrow()[0].clone().unwrap()).unwrap();
    assert_eq!(
        (&looked["status"]["state"], &looked["first_message"]),
        (&json!("blocked"), &json!(null)),
        "{looked}"
    );
    assert_eq!(w.calls_of(&["send"]), Vec::<Vec<String>>::new());
    assert_eq!(w.calls_of(&["keys"]), Vec::<Vec<String>>::new());
    assert_eq!(w.calls_of(&["kill", &address]).len(), 1);
    assert_eq!(*w.sleeps.borrow(), [PROMPT_WAIT]);
}

#[test]
fn a_message_the_agent_never_starts_working_on_fails_the_launch_and_is_never_sent_again() {
    let mut w = World::new();
    w.set("FAKE_HC_DEAF", "1");
    let address = failed_launch(
        &w,
        w.try_launch(&w.ask("sim/jump")),
        "the agent in terminal {t} did not start working on its first message (its status when \
         it went in: idle)",
    );
    assert_eq!(w.calls_of(&["send", &address]).len(), 1);
    assert_eq!(w.calls_of(&["kill", &address]).len(), 1);
}

/// A launch through `hc`, whose terminal hc made: (the refusal, the terminal).
fn launch_through(w: &World, hc: &Hooked<'_>) -> (String, String) {
    let launched = symphony_board::launch::launch(w.store(), hc, &w.machine(), &w.ask("sim/jump"));
    let [run] = <[_; 1]>::try_from(w.calls_of(&["run"])).unwrap();
    let name = flag_value(&run, "--name").to_string();
    let address = w.calls_of(&["rm"]).pop().map(|c| c[1].clone());
    let address = address.unwrap_or_else(|| panic!("{name}'s terminal was not removed"));
    assert!(!w.has_terminal(&address), "{address} was left");
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
    assert_eq!(workers(w), ["left"]);
    (refused(launched), address)
}

#[test]
fn a_send_hc_refuses_or_cannot_tell_of_fails_the_launch_naming_the_terminal() {
    // Blocked on a person between the look and the send: hc refuses, and it is never forced.
    let w = World::new();
    let hc = Hooked {
        after: Box::new(|args| {
            if args[0] == "get" && args[1].starts_with('t') {
                w.edit_terminal(args[1], |t| {
                    t.insert("status".into(), json!("blocked"));
                });
            }
        }),
        ..Hooked::new(&w)
    };
    let (why, t) = launch_through(&w, &hc);
    assert_eq!(
        why,
        format!(
            "terminal {t} did not take its first message: hc send {t}: blocked: it waits on a person"
        )
    );
    let [send] = <[_; 1]>::try_from(w.calls_of(&["send"])).unwrap();
    assert!(!send.contains(&"--force".to_string()), "{send:?}");
    // A program that reports no status: hc cannot say whether it took the message.
    let w = World::new();
    let hc = Hooked {
        instead: Box::new(|args| {
            (args[0] == "send").then(|| Ok(r#"{"started":null,"status":null,"turn":0}"#.into()))
        }),
        ..Hooked::new(&w)
    };
    let (why, t) = launch_through(&w, &hc);
    assert_eq!(
        why,
        format!(
            "hc cannot say whether the agent in terminal {t} took its first message: it reports \
             no status"
        )
    );
}

#[test]
fn an_agent_never_ready_for_its_first_message_fails_after_its_looks_unsent() {
    for (knob, last) in [("FAKE_HC_SILENT", "none"), ("FAKE_HC_BUSY", "working")] {
        let mut w = World::new();
        w.set(knob, "1");
        let address = failed_launch(
            &w,
            w.try_launch(&w.ask("sim/jump")),
            &format!(
                "the agent in terminal {{t}} was never ready for its first message (its last \
                 status: {last})"
            ),
        );
        assert_eq!(w.calls_of(&["get", &address]).len(), CONFIRM_LOOKS + 1);
        assert_eq!(w.sleeps.borrow().len(), CONFIRM_LOOKS);
        assert_eq!(w.calls_of(&["keys"]), Vec::<Vec<String>>::new(), "{knob}");
        assert_eq!(w.calls_of(&["send"]), Vec::<Vec<String>>::new(), "{knob}");
    }
}

#[test]
fn looks_hc_never_answers_fail_naming_the_last_problem_and_still_close_the_terminal() {
    let mut w = World::new();
    w.set("FAKE_HC_FAIL", "get");
    let address = failed_launch(
        &w,
        w.try_launch(&w.ask("sim/jump")),
        "the agent in terminal {t} was never ready for its first message \
         (its last status: unanswered: hc get {t}: fake_failure: get failed)",
    );
    assert_eq!(w.calls_of(&["kill", &address]).len(), 1);
}

#[test]
fn an_agent_that_ends_before_its_first_message_fails_at_once() {
    let mut w = World::new();
    w.set("FAKE_HC_EXIT", "1");
    let address = failed_launch(
        &w,
        w.try_launch(&w.ask("sim/jump")),
        "the agent in terminal {t} ended (exit code 1) before it took its first message",
    );
    assert_eq!(*w.sleeps.borrow(), [PROMPT_WAIT]);
    // Already ended: removed, nothing to kill.
    assert!(w.calls_of(&["kill", &address]).is_empty());
}

#[test]
fn a_terminal_hc_no_longer_knows_fails_at_once_and_leaves_nothing() {
    let w = World::new();
    let hc = Hooked {
        instead: Box::new(|args| {
            (args[0] == "get" && args[1].starts_with('t'))
                .then(|| Err(LaunchError(format!("hc get {}: not_found: gone", args[1]))))
        }),
        ..Hooked::new(&w)
    };
    let why = refused(symphony_board::launch::launch(
        w.store(),
        &hc,
        &w.machine(),
        &w.ask("sim/jump"),
    ));
    assert!(
        why.starts_with("terminal t")
            && why.ends_with(" is gone before its agent took its first message"),
        "{why}"
    );
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
    assert_eq!(workers(&w), ["left"]);
}

#[test]
fn a_look_hc_cannot_answer_is_no_evidence_and_a_later_look_finds_it_ready() {
    let mut w = World::new();
    w.set("FAKE_HC_SILENT", "1");
    let looks = Cell::new(0);
    let hc = Hooked {
        instead: Box::new(|args| {
            if args[0] != "get" || !args[1].starts_with('t') {
                return None;
            }
            looks.set(looks.get() + 1);
            (looks.get() == 1).then(|| Err(LaunchError("hc get: no answer in 120 s".into())))
        }),
        after: Box::new(|args| {
            if args[0] == "get" && looks.get() == 2 {
                w.edit_terminal(args[1], |t| {
                    t.insert("status".into(), json!("idle"));
                });
            }
        }),
        ..Hooked::new(&w)
    };
    let s =
        symphony_board::launch::launch(w.store(), &hc, &w.machine(), &w.ask("sim/jump")).unwrap();
    assert_eq!(looks.get(), 3);
    assert_eq!(w.live(), [s.row.pane.as_str()]);
    assert_eq!(w.sent(&s.row.pane).len(), 1);
}

#[test]
fn an_agent_done_is_ready_for_its_first_message_like_an_idle_one() {
    let mut w = World::new();
    w.set("FAKE_HC_SILENT", "1");
    let hc = Hooked {
        after: Box::new(|args| {
            if args[0] == "run" {
                let address = w.addresses().pop().unwrap();
                w.edit_terminal(&address, |t| {
                    t.insert("status".into(), json!("done"));
                });
            }
        }),
        ..Hooked::new(&w)
    };
    let s =
        symphony_board::launch::launch(w.store(), &hc, &w.machine(), &w.ask("sim/jump")).unwrap();
    assert_eq!(*w.sleeps.borrow(), [PROMPT_WAIT]);
    assert_eq!(
        w.sent(&s.row.pane),
        [format!("$work sim/jump {}", s.row.handle)]
    );
}

#[test]
fn a_failed_run_leaves_no_terminal_no_worktree_and_the_handle_left() {
    let mut w = World::new();
    w.set("FAKE_HC_FAIL", "run");
    assert_eq!(
        refused(w.try_launch(&w.ask("sim/jump"))),
        "hc run: fake_failure: run failed"
    );
    assert!(w.addresses().is_empty());
    assert!(!w.wt.join("sim-jump").exists());
    assert_eq!(workers(&w), ["left"]);
}

#[test]
fn a_run_that_names_no_terminal_is_a_failure() {
    let w = World::new();
    let hc = Hooked {
        instead: Box::new(|args| (args[0] == "run").then(|| Ok("{}".to_string()))),
        ..Hooked::new(&w)
    };
    assert_eq!(
        refused(symphony_board::launch::launch(
            w.store(),
            &hc,
            &w.machine(),
            &w.ask("sim/jump")
        )),
        "hc run answered no terminal address"
    );
    assert_eq!(workers(&w), ["left"]);
}

// -- Codex's hooks chooser ---------------------------------------------------------------------

/// A world whose Codex starts on the hooks chooser, item `selected` highlighted.
fn chooser(selected: usize) -> World {
    let mut w = World::new();
    let screen = include_str!("fixtures/codex-hooks.txt")
        .replace("› 1.", "  1.")
        .replace(&format!("  {selected}."), &format!("› {selected}."));
    let file = w.dir().join("chooser.txt");
    fs::write(&file, format!(">_ OpenAI Codex\n\n{screen}")).unwrap();
    w.set("FAKE_HC_START_SCREEN", file.to_str().unwrap());
    w.set("FAKE_HC_START_SELECTED", &selected.to_string());
    w
}

fn keys(w: &World) -> Vec<Vec<String>> {
    w.calls_of(&["keys"])
        .into_iter()
        .map(|c| c[2..c.len() - 1].to_vec())
        .collect()
}

#[test]
fn the_hooks_chooser_is_answered_continue_without_trusting_then_the_launch_confirms() {
    for (selected, expected) in [
        (1, vec![vec!["down", "down"], vec!["enter"]]),
        (2, vec![vec!["down"], vec!["enter"]]),
        (3, vec![vec!["enter"]]),
    ] {
        let w = chooser(selected);
        let s = w.launch("sim/jump").row;
        assert_eq!(keys(&w), expected, "{selected}");
        let t = w.terminal(&s.pane);
        assert_eq!(t["confirmed_choice"], 3);
        assert_eq!(w.calls_of(&["read", &s.pane]).len(), expected.len());
        assert_eq!(
            w.calls_of(&["read", &s.pane])[0],
            ["read", &s.pane, "--lines", "0"]
        );
    }
}

#[test]
fn a_dropped_down_never_confirms_trust_and_the_launch_fails() {
    let mut w = chooser(1);
    w.set("FAKE_HC_DROP_DOWN", "1");
    failed_launch(
        &w,
        w.try_launch(&w.ask("sim/jump")),
        "the agent in terminal {t} was never ready for its first message (its last status: none)",
    );
    // The navigation only: the look after it showed trust selected, and no enter followed.
    assert_eq!(keys(&w), [vec!["down", "down"]]);
    assert_eq!(w.calls_of(&["send"]), Vec::<Vec<String>>::new());
}

#[test]
fn a_chooser_hc_calls_blocked_is_answered_and_only_the_look_right_after_enter_waits() {
    for ready_after in [true, false] {
        let w = chooser(3);
        let gets = Cell::new(0);
        let block = |t: &str| {
            w.edit_terminal(t, |t| {
                t.insert("status".into(), json!("blocked"));
            })
        };
        let hc = Hooked {
            after: Box::new(|args| match args[0] {
                "run" => block(&w.addresses().pop().unwrap()),
                // Still blocked when the look right after enter comes.
                "keys" => block(args[1]),
                "get" if args[1].starts_with('t') => {
                    gets.set(gets.get() + 1);
                    if ready_after && gets.get() == 2 {
                        w.edit_terminal(args[1], |t| {
                            t.insert("status".into(), json!("idle"));
                        });
                    }
                }
                _ => {}
            }),
            ..Hooked::new(&w)
        };
        let launched =
            symphony_board::launch::launch(w.store(), &hc, &w.machine(), &w.ask("sim/jump"));
        assert_eq!(keys(&w), [vec!["enter"]], "{ready_after}");
        if ready_after {
            let s = launched.unwrap().row;
            assert_eq!(w.sent(&s.pane), [format!("$work sim/jump {}", s.handle)]);
        } else {
            failed_launch(
                &w,
                launched,
                // hc says nothing of what it waits on, so the refusal says what the screen showed.
                "the agent in terminal {t} waits on a person before it took its first message; \
                 its terminal showed: › Ask Codex to do anything",
            );
            assert_eq!(w.calls_of(&["send"]), Vec::<Vec<String>>::new());
        }
        assert_eq!(w.sleeps.borrow().len(), 3, "{ready_after}");
    }
}

#[test]
fn claude_and_a_codex_screen_without_the_chooser_get_no_keys() {
    for kind in [Some("claude".to_string()), None] {
        let mut w = World::new();
        w.set("FAKE_HC_SILENT", "1");
        let ask = Ask {
            kind: kind.clone(),
            ..w.ask("sim/jump")
        };
        assert!(w.try_launch(&ask).is_err());
        assert!(w.calls_of(&["keys"]).is_empty(), "{kind:?}");
        let reads = w.calls_of(&["read"]).len();
        assert_eq!(reads, if kind.is_none() { CONFIRM_LOOKS } else { 0 });
    }
}
