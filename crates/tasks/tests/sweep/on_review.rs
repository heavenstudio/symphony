//! A task moving to review starts the command `[board] on_review` names in the main checkout's
//! symphony.toml (src/on_review.rs): detached, from the main checkout, with the slug, the branch and
//! its tip appended, its output in `sf-tasks/on-review.log`. A test's board starts none unless told
//! (`TASKS_ON_REVIEW=on`). The command here is a script that records where it ran and what it was
//! given; the test waits for its record, never for the move, which returns at once.

use crate::common::Seed;
use crate::{Repo, git, write};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::cli::Env;
use symphony_board::on_review::{LOG, MANIFEST, Started};
use symphony_board::sweep::trigger::folder;

/// `tasks ARGS` as `handle`, from `cwd`, the review command let start on this board, with `change`
/// made to the environment: what it printed (it must succeed).
fn run(
    r: &Repo,
    handle: &str,
    cwd: &Path,
    args: &[&str],
    change: impl FnOnce(&mut BTreeMap<String, String>),
) -> String {
    let mut vars = r.vars();
    for (key, value) in [
        ("TASKS_ON_REVIEW", "on"),
        ("TASKS_SWEEP", "off"),
        ("TASKS_SESSION", handle),
    ] {
        vars.insert(key.into(), value.into());
    }
    change(&mut vars);
    let env = Env {
        vars,
        cwd: cwd.to_path_buf(),
    };
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, &env, &mut "".as_bytes(), &mut out, &mut err);
    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!(code, 0, "{args:?}: {out}{err}");
    out
}

fn log(r: &Repo) -> PathBuf {
    folder(&r.root.join(".git")).join(LOG)
}

/// The file at `path` once it exists: its text.
fn written(path: &Path) -> String {
    let began = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(path)
            && text.ends_with('\n')
        {
            return text;
        }
        assert!(
            began.elapsed() < Duration::from_secs(60),
            "nothing written to {} within a minute",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The main checkout's manifest naming `command` as the review command.
fn manifest(r: &Repo, command: &[&str]) {
    let quoted: Vec<String> = command.iter().map(|word| format!("{word:?}")).collect();
    write(
        &r.root,
        MANIFEST,
        &format!(
            "[project]\nname = \"Star Foundry\"\n\n[board]\nexport = \"docs/tasks.md\"\non_review = [{}]\n",
            quoted.join(", ")
        ),
    );
}

/// A script under `dir` at `path` that records its working directory and arguments in `record`,
/// then says `said` on its output.
fn recorder(dir: &Path, path: &str, record: &Path, said: &str) {
    write(
        dir,
        path,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$*\" > '{}'\necho '{said}'\n",
            record.display()
        ),
    );
    let file = dir.join(path);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A branch `name` off main with one commit, in a worktree of its own: its tip.
fn branch(r: &Repo, name: &str) -> String {
    let wt = r.add(&name.replace('/', "-"), Some(name), "main");
    r.commit(&wt, &format!("extends RefCounted\n# {name}\n"));
    git(&wt, &["rev-parse", "HEAD"])
}

/// A board with `handle` executing `slug`, in progress.
fn executing(slug: &str, handle: &str) -> Repo {
    Repo::with(
        Seed::default()
            .session(handle)
            .task(slug, "in_progress")
            .role(slug, handle, "executor"),
    )
}

#[test]
fn a_task_moved_to_review_starts_the_manifests_command_from_the_main_checkout_with_the_tip() {
    let r = executing("sim/jump", "amber-heron");
    let tip = branch(&r, "sim/jump");
    let record = r.dir.join("record");
    recorder(&r.root, "tools/on-review.sh", &record, "verify started");
    manifest(&r, &["tools/on-review.sh", "--quiet"]);
    // Moved from the branch's worktree: the command still runs from the main checkout.
    let out = run(
        &r,
        "amber-heron",
        &r.dir.join("sim-jump"),
        &["move", "sim/jump", "review"],
        |_| {},
    );
    assert_eq!(
        out,
        format!(
            "sim/jump is review\n→ the tip's verify started detached: tools/on-review.sh --quiet sim/jump sim/jump {tip}\n"
        )
    );
    let main = r.root.canonicalize().unwrap();
    assert_eq!(
        written(&record),
        format!("{}\n--quiet sim/jump sim/jump {tip}\n", main.display())
    );
    let text = written(&log(&r));
    let header = text.lines().next().unwrap();
    assert!(
        header.starts_with('[')
            && header.ends_with(&format!(
                "] sim/jump is in review: sim/jump at {tip}; tools/on-review.sh --quiet sim/jump sim/jump {tip}"
            )),
        "{text}"
    );
    assert!(text.contains("\nverify started\n"), "{text}");
}

#[test]
fn a_review_task_names_its_branch_and_a_bare_program_name_is_found_on_the_path() {
    let r = executing("review/sim/jump", "amber-heron");
    let tip = branch(&r, "sim/jump");
    let record = r.dir.join("record");
    let bin = r.dir.join("bin");
    recorder(&bin, "on-review-hook", &record, "ran");
    manifest(&r, &["on-review-hook"]);
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "review/sim/jump", "review"],
        |vars| {
            let path = vars.get("PATH").cloned().unwrap_or_default();
            vars.insert("PATH".into(), format!("{}:{path}", bin.display()));
        },
    );
    assert_eq!(
        written(&record),
        format!(
            "{}\nreview/sim/jump sim/jump {tip}\n",
            r.root.canonicalize().unwrap().display()
        )
    );
}

#[test]
fn a_slug_naming_no_branch_starts_nothing_and_the_log_says_so() {
    let r = executing("docs/plan", "amber-heron");
    let record = r.dir.join("record");
    recorder(&r.root, "tools/on-review.sh", &record, "ran");
    manifest(&r, &["tools/on-review.sh"]);
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "docs/plan", "review"],
        |_| {},
    );
    let text = written(&log(&r));
    assert!(
        text.contains("] docs/plan is in review\nno command: no branch docs/plan in "),
        "{text}"
    );
    assert!(!record.exists());
}

#[test]
fn a_manifest_without_the_key_a_test_board_not_told_and_off_start_none() {
    for (manifest_text, on_review) in [
        (Some("[board]\nexport = \"docs/tasks.md\"\n"), Some("on")),
        (None, Some("on")),
        (
            Some("[board]\non_review = [\"tools/on-review.sh\"]\n"),
            None,
        ),
        (
            Some("[board]\non_review = [\"tools/on-review.sh\"]\n"),
            Some("off"),
        ),
    ] {
        let r = executing("sim/jump", "amber-heron");
        branch(&r, "sim/jump");
        let record = r.dir.join("record");
        recorder(&r.root, "tools/on-review.sh", &record, "ran");
        if let Some(text) = manifest_text {
            write(&r.root, MANIFEST, text);
        }
        run(
            &r,
            "amber-heron",
            &r.root,
            &["move", "sim/jump", "review"],
            |vars| {
                match on_review {
                    Some(value) => vars.insert("TASKS_ON_REVIEW".into(), value.into()),
                    None => vars.remove("TASKS_ON_REVIEW"),
                };
            },
        );
        // A command that starts opens the log before the move returns; none did.
        assert!(!log(&r).exists(), "{manifest_text:?} {on_review:?}");
        assert!(!record.exists(), "{manifest_text:?} {on_review:?}");
    }
}

#[test]
fn a_move_elsewhere_than_review_starts_none_and_a_broken_manifest_is_logged() {
    let r = executing("sim/jump", "amber-heron");
    branch(&r, "sim/jump");
    let record = r.dir.join("record");
    recorder(&r.root, "tools/on-review.sh", &record, "ran");
    manifest(&r, &["tools/on-review.sh"]);
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "sim/jump", "blocked"],
        |_| {},
    );
    assert!(!log(&r).exists());
    write(
        &r.root,
        MANIFEST,
        "[board]\non_review = \"tools/on-review.sh\"\n",
    );
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "sim/jump", "review"],
        |_| {},
    );
    let text = written(&log(&r));
    assert!(
        text.contains("] sim/jump is in review\nno command: ")
            && text.contains("symphony.toml: [board] on_review:"),
        "{text}"
    );
    assert!(!record.exists());
}

#[test]
fn the_trigger_names_what_it_started_and_refuses_from_a_worktree_too() {
    let r = executing("sim/jump", "amber-heron");
    let tip = branch(&r, "sim/jump");
    let record = r.dir.join("record");
    recorder(&r.root, "tools/on-review.sh", &record, "ran");
    manifest(&r, &["tools/on-review.sh"]);
    let mut vars = r.vars();
    vars.insert("TASKS_ON_REVIEW".into(), "on".into());
    let env = Env {
        vars,
        cwd: r.dir.join("sim-jump"),
    };
    assert_eq!(
        symphony_board::on_review::trigger(&env, &env.cwd, "sim/jump"),
        Started::Command(vec![
            "tools/on-review.sh".into(),
            "sim/jump".into(),
            "sim/jump".into(),
            tip.clone()
        ])
    );
    written(&record);
    assert_eq!(
        symphony_board::on_review::trigger(&env, &env.cwd, "sim/gone"),
        Started::Refused(format!(
            "no branch sim/gone in {}",
            r.root.canonicalize().unwrap().display()
        ))
    );
    let mut off = env.clone();
    off.vars.insert("TASKS_ON_REVIEW".into(), "off".into());
    assert_eq!(
        symphony_board::on_review::trigger(&off, &off.cwd, "sim/jump"),
        Started::None
    );
}
