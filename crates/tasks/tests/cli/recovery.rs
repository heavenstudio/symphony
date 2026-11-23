//! `tasks recovery`: the board's recovery binding said, revoked and made again, on the board of a
//! fixture repository of its own outside this one (no `TASKS_SERVER`), its PostgreSQL started by
//! its first command that opens it and stopped when the case ends.

use crate::common::NOW;
use crate::{git, project_file, repository, run, scratch};
use std::collections::BTreeMap;
use std::path::PathBuf;
use symphony_board::cli::Env;
use symphony_board::recovery::Authority as _;
use symphony_board::server::Home;
use symphony_board::{Error, NewTask};

pub(crate) struct Alpha {
    pub(crate) root: PathBuf,
    machine: PathBuf,
}

impl Alpha {
    pub(crate) fn new(name: &str) -> Alpha {
        let dir = scratch(name);
        let root = repository(&dir.join("alpha"));
        std::fs::write(root.join("symphony.toml"), project_file("alpha", "")).unwrap();
        let machine = dir.join("machine");
        std::fs::create_dir_all(&machine).unwrap();
        Alpha { root, machine }
    }

    pub(crate) fn env(&self) -> Env {
        let vars: BTreeMap<String, String> = [
            ("SYMPHONY_MACHINE_DIR", self.machine.display().to_string()),
            ("TASKS_NOW", NOW.to_string()),
            ("TASKS_SWEEP", "off".to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
        Env {
            vars,
            cwd: self.root.clone(),
        }
    }

    fn home(&self) -> Home {
        Home::in_common(&self.root.join(".git"))
    }

    pub(crate) fn run(&self, args: &[&str]) -> (i32, String, String) {
        let ran = run(&self.env(), args, "");
        (ran.code, ran.out, ran.err)
    }
}

impl Drop for Alpha {
    fn drop(&mut self) {
        let _ = self.home().stop_board();
    }
}

#[test]
fn a_board_is_bound_revoked_and_bound_again_and_its_bound_writes_follow() {
    let alpha = Alpha::new("recovery-bound");
    let (code, _, err) = alpha.run(&["recovery", "revoke"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(
        alpha.home().board_server(&alpha.root, false).is_err(),
        "a revoke starts no database"
    );

    let (code, out, err) = alpha.run(&["recovery"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("the alpha board holds recovery generation "),
        "{out}"
    );
    assert!(
        out.contains(" and is bound to none: bound to none, so bound writes are refused"),
        "{out}"
    );

    let env = alpha.env();
    let store = || {
        symphony_board::cli::recovery::recovering(symphony_board::cli::connect(&env).unwrap(), &env)
    };
    store()
        .create(&NewTask::new("a/one", "One", "owner"))
        .unwrap();
    let Err(Error::Refused(why)) = store().bound("a/one") else {
        panic!("an unbound board reads no token");
    };
    assert!(why.contains("bound to no recovery generation"), "{why}");

    let (code, out, err) = alpha.run(&["recovery", "rebind", "--as", "someone"]);
    assert_eq!(code, 0, "{err}");
    let bound = alpha.home().recovery("tasks").bound().unwrap().unwrap();
    assert!(
        out.contains(&format!(
            "holds a new recovery generation, {bound}, and is bound to it"
        )),
        "{out}"
    );
    let (_, out, _) = alpha.run(&["recovery"]);
    assert_eq!(
        out,
        format!(
            "the alpha board holds recovery generation {bound} and is bound to {bound}: bound writes are taken\n"
        )
    );
    let at = store().bound("a/one").unwrap().bound;
    assert_eq!(at.generation, bound);

    // A copy by a store with no authority leaves the two apart, which the status says.
    let plain = symphony_board::cli::connect(&env).unwrap();
    plain.copy(&plain.dump().unwrap()).unwrap();
    let (_, out, _) = alpha.run(&["recovery"]);
    assert!(
        out.contains("the two differ (a copy or a restore came between)"),
        "{out}"
    );
    assert!(matches!(
        store().amend_bound("a/one", &at, "Stale", &[], "owner"),
        Err(Error::Refused(_))
    ));
    alpha.run(&["recovery", "rebind"]);
    let now = store().bound("a/one").unwrap().bound;
    assert_ne!(now.generation, at.generation);
    store()
        .amend_bound("a/one", &now, "Bound again", &[], "owner")
        .unwrap();

    let (code, _, _) = alpha.run(&["recovery", "revoke"]);
    assert_eq!(code, 0);
    assert_eq!(alpha.home().recovery("tasks").bound(), Ok(None));
    assert!(matches!(store().bound("a/one"), Err(Error::Refused(_))));
}

#[test]
fn a_scratch_database_has_a_binding_of_its_own() {
    let alpha = Alpha::new("recovery-scratch");
    let (code, _, err) = alpha.run(&["--database", "trial", "recovery", "rebind"]);
    assert_eq!(code, 0, "{err}");
    let home = alpha.home();
    assert!(home.recovery("trial").bound().unwrap().is_some());
    assert_eq!(
        home.recovery("tasks").bound(),
        Ok(None),
        "the project's own is not touched"
    );
    let (_, out, _) = alpha.run(&["--database", "trial", "recovery"]);
    assert!(
        out.starts_with("the scratch database trial of the alpha board holds"),
        "{out}"
    );
    assert!(out.ends_with("bound writes are taken\n"), "{out}");
}

#[test]
fn the_projects_own_board_is_rebound_and_revoked_only_from_its_main_checkout() {
    let alpha = Alpha::new("recovery-main");
    alpha.run(&["recovery", "rebind"]);
    let bound = alpha.home().recovery("tasks").bound().unwrap();
    let linked = alpha.root.parent().unwrap().join("linked");
    git(
        &alpha.root,
        &["worktree", "add", "-q", linked.to_str().unwrap()],
    );
    let env = Env {
        cwd: linked,
        ..alpha.env()
    };
    for act in ["revoke", "rebind"] {
        let ran = run(&env, &["recovery", act], "");
        assert_eq!(ran.code, 2, "{act}");
        assert!(
            ran.err
                .contains("changes the alpha board's binding only from its main checkout")
                && ran.err.contains("nothing was changed"),
            "{act}: {}",
            ran.err
        );
    }
    assert_eq!(alpha.home().recovery("tasks").bound().unwrap(), bound);
    let mut unnamed = alpha.env();
    unnamed
        .vars
        .insert("TASKS_DATABASE".into(), "not plain".into());
    let ran = run(&unnamed, &["recovery", "revoke"], "");
    assert_eq!(ran.code, 2);
    assert!(
        ran.err.contains("is not a plain name for a database"),
        "{}",
        ran.err
    );
    let said = run(&env, &["recovery"], "");
    assert_eq!(
        said.code, 0,
        "the binding is said from any checkout: {}",
        said.err
    );
    assert!(said.out.contains("bound writes are taken"), "{}", said.out);
}

#[test]
fn a_named_server_has_no_binding_and_words_it_does_not_know_are_refused() {
    let named = crate::env();
    for args in [
        &["recovery"][..],
        &["recovery", "revoke"],
        &["recovery", "rebind"],
    ] {
        let ran = run(&named, args, "");
        assert_eq!(ran.code, 2, "{args:?}");
        assert!(
            ran.err
                .contains("has no board folder here and so no recovery binding"),
            "{}",
            ran.err
        );
    }
    let store = symphony_board::cli::connect(&named).unwrap();
    store
        .create(&NewTask::new("a/one", "One", "owner"))
        .unwrap();
    let store = symphony_board::cli::recovery::recovering(store, &named);
    let Err(Error::Refused(why)) = store.bound("a/one") else {
        panic!("a named server's board reads no token");
    };
    assert!(why.contains("opened with no recovery authority"), "{why}");

    let ran = run(&named, &["recovery", "adopt"], "");
    assert_eq!(ran.code, 2);
    assert!(
        ran.err.starts_with("usage: just tasks recovery"),
        "{}",
        ran.err
    );
    assert!(
        ran.err.contains("unrecognized arguments: adopt"),
        "{}",
        ran.err
    );
    let help = run(&named, &["recovery", "--help"], "");
    assert_eq!(
        (help.code, help.out.as_str()),
        (0, symphony_board::cli::recovery::HELP)
    );
    assert_eq!(run(&named, &["help", "recovery"], "").out, help.out);
}
