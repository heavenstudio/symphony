//! The model command from a linked worktree edits the main checkout; a scratch board's limits
//! command reads that edit without a verdict, a restart, or any board write by the editor.

use std::collections::BTreeMap;
use std::path::Path;

use super::Tree;
use super::board::Fixture;
use symphony_board::cli::Env;
use symphony_board::launch::Defaults;

#[path = "../common/layouts.rs"]
mod layouts;

fn model(here: &Path, machine: &Path, args: &[&str]) -> (i32, String, String) {
    let done = symphony_board::sys::output(
        symphony_board::sys::command(env!("CARGO_BIN_EXE_symphony"))
            .arg("model")
            .args(args)
            .env("SYMPHONY_MACHINE_DIR", machine)
            .current_dir(here),
    )
    .unwrap();
    (
        done.status.code().unwrap(),
        String::from_utf8(done.stdout).unwrap(),
        String::from_utf8(done.stderr).unwrap(),
    )
}

#[test]
fn the_model_command_changes_main_and_limits_reads_it_without_a_review() {
    let tree = Tree::new();
    let (main, linked) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let fixture = Fixture::empty();
    let client = fixture.other_client();
    let before = fixture.rows();
    let (code, out, err) = model(&linked.join("docs"), &machine, &["all", "unlisted-model"]);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(out.contains("all model set to \"unlisted-model\""), "{out}");
    assert!(
        !std::fs::read_to_string(linked.join("symphony.toml"))
            .unwrap()
            .contains("unlisted-model")
    );
    let defaults = Defaults::configured(&main).unwrap();
    for role in symphony_board::launch::defaults::ROLES {
        assert_eq!(
            defaults.resolve_as(Some(role), None, None).model,
            "unlisted-model"
        );
    }
    assert_eq!(
        fixture.rows(),
        before,
        "the model edit makes no task or reviewer verdict and writes no board state"
    );
    let env = Env {
        cwd: linked.clone(),
        vars: BTreeMap::from([
            ("TASKS_SERVER".into(), client.addr().into()),
            ("TASKS_DATABASE".into(), client.database().into()),
            ("SYMPHONY_MACHINE_DIR".into(), machine.display().to_string()),
        ]),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code =
        symphony_board::cli::main(&["limits".into()], &env, &mut &b""[..], &mut out, &mut err);
    let out = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    for role in symphony_board::launch::defaults::ROLES {
        assert!(
            out.lines().any(|line| line.starts_with(role)
                && line.contains("unlisted-model")
                && line.contains("[agents.models]")),
            "{out}"
        );
    }
}

#[test]
fn the_binary_reports_refusals_and_accepts_a_dashed_model_after_double_dash() {
    let tree = Tree::new();
    let (main, _) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    let before = std::fs::read_to_string(main.join("symphony.toml")).unwrap();
    for (args, named) in [
        (vec!["worker", "x"], "unknown role"),
        (vec!["executor"], "ROLE and MODEL"),
        (vec!["executor", "--bogus"], "--bogus"),
    ] {
        let (code, out, err) = model(&main, &machine, &args);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.contains(named), "{err}");
        assert_eq!(
            std::fs::read_to_string(main.join("symphony.toml")).unwrap(),
            before
        );
    }
    let (code, _, err) = model(&main, &machine, &["--", "executor", "-future-model"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        Defaults::configured(&main)
            .unwrap()
            .models
            .executor
            .as_deref(),
        Some("-future-model")
    );
    let (code, out, err) = model(&tree.root, &machine, &["executor", "m"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains("no git checkout"), "{err}");
}

#[test]
fn literal_help_flags_and_del_round_trip_through_the_binary_and_settings_readers() {
    let tree = Tree::new();
    let (main, _) = layouts::repository(&tree.root);
    let machine = tree.path("machine");
    for value in ["--help", "-h", "bad\u{7f}name", "\u{7f}"] {
        let (code, out, err) = model(&main, &machine, &["executor", "--", value]);
        assert_eq!((code, err.as_str()), (0, ""), "{out}");
        assert!(out.contains("executor model set to"), "{out}");
        let written = std::fs::read_to_string(main.join("symphony.toml")).unwrap();
        let parsed: toml::Table = written.parse().unwrap();
        assert_eq!(parsed["agents"]["models"]["executor"].as_str(), Some(value));
        assert_eq!(
            Defaults::configured(&main)
                .unwrap()
                .models
                .executor
                .as_deref(),
            Some(value)
        );
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            symphony::lint::settings(&[main.display().to_string()], &mut out, &mut err),
            0,
            "{}",
            String::from_utf8_lossy(&err)
        );
    }
    let before = std::fs::read_to_string(main.join("symphony.toml")).unwrap();
    for flag in ["--help", "-h"] {
        let (code, out, err) = model(&main, &machine, &[flag]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.starts_with("usage: symphony model ROLE MODEL"), "{out}");
        assert_eq!(
            std::fs::read_to_string(main.join("symphony.toml")).unwrap(),
            before
        );
    }
}
