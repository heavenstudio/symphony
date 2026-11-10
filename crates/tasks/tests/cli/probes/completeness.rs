//! Real CLI reads of scratch lsof output: positive evidence and proof of absence stay distinct.

use super::*;
use std::os::unix::fs::PermissionsExt;
use symphony_board::{Beat, NewTask};

struct Working(symphony_board::sys::Child);

impl Drop for Working {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Listing {
    name: &'static str,
    output: &'static str,
    ending: &'static str,
    health: &'static str,
}

const LISTINGS: [Listing; 16] = [
    Listing {
        name: "ambiguous-caret-cwd",
        output: "printf 'p%s\\nfcwd\\nn/ambiguous^A\\n' \"$SCAN_PID\"",
        ending: "exit 0",
        health: "unknown",
    },
    Listing {
        name: "partial-unrelated",
        output: "printf 'p%s\\nfcwd\\nn/\\n' \"$PPID\"",
        ending: "printf 'Permission denied' >&2; exit 1",
        health: "unknown",
    },
    Listing {
        name: "failed-unrelated",
        output: "printf 'p%s\\nfcwd\\nn/\\n' \"$PPID\"",
        ending: "exit 1",
        health: "unknown",
    },
    Listing {
        name: "diagnostic-unrelated",
        output: "printf 'p%s\\nfcwd\\nn/\\n' \"$PPID\"",
        ending: "printf 'Permission denied' >&2",
        health: "unknown",
    },
    Listing {
        name: "complete-unrelated",
        output: "printf 'p%s\\nfcwd\\nn/\\n' \"$PPID\"",
        ending: "exit 0",
        health: "lost",
    },
    Listing {
        name: "complete-empty",
        output: "",
        ending: "exit 0",
        health: "lost",
    },
    Listing {
        name: "failed-empty",
        output: "",
        ending: "exit 1",
        health: "unknown",
    },
    Listing {
        name: "malformed",
        output: "printf 'pbad\\nn/omitted\\n'",
        ending: "exit 0",
        health: "unknown",
    },
    Listing {
        name: "partial-owned",
        output: "printf 'p%s\\nfcwd\\nn%s\\n' \"$SCAN_PID\" \"$SCAN_CWD\"",
        ending: "printf 'Permission denied' >&2; exit 1",
        health: "live",
    },
    Listing {
        name: "valid-owned",
        output: "printf 'p%s\\nfcwd\\nn%s\\n' \"$SCAN_PID\" \"$SCAN_CWD\"",
        ending: "exit 0",
        health: "live",
    },
    Listing {
        name: "relative-cwd",
        output: "printf 'p%s\\nfcwd\\nnrelative/path\\n' \"$SCAN_PID\"",
        ending: "exit 0",
        health: "unknown",
    },
    Listing {
        name: "unknown-cwd",
        output: "printf 'p%s\\nfcwd\\nnunknown\\n' \"$SCAN_PID\"",
        ending: "exit 0",
        health: "unknown",
    },
    Listing {
        name: "non-cwd-descriptor",
        output: "printf 'p%s\\nftxt\\nn%s\\n' \"$SCAN_PID\" \"$SCAN_CWD\"",
        ending: "exit 0",
        health: "unknown",
    },
    Listing {
        name: "missing-descriptor",
        output: "printf 'p%s\\nn%s\\n' \"$SCAN_PID\" \"$SCAN_CWD\"",
        ending: "exit 0",
        health: "unknown",
    },
    Listing {
        name: "cwd-after-non-cwd",
        output: "printf 'p%s\\nftxt\\nn%s\\nfcwd\\nn%s\\n' \"$SCAN_PID\" \"$SCAN_CWD\" \"$SCAN_CWD\"",
        ending: "exit 0",
        health: "live",
    },
    Listing {
        name: "bad-owned-with-valid-unrelated-neighbor",
        output: "printf 'p%s\\nftxt\\nn%s\\np%s\\nfcwd\\nn/\\n' \"$SCAN_PID\" \"$SCAN_CWD\" \"$PPID\"",
        ending: "exit 0",
        health: "unknown",
    },
];

fn read_agent(env: &Env, root: &Path, handle: &str) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tasks"));
    command
        .args(["agents", "--json"])
        .env_clear()
        .envs(&env.vars)
        .current_dir(root);
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let done = symphony_board::sys::output(&mut command).unwrap();
    assert!(done.status.success(), "{done:?}");
    let value: Value = serde_json::from_slice(&done.stdout).unwrap();
    value["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["handle"] == handle)
        .unwrap()
        .clone()
}

fn call_in(store: &symphony_board::Store, tree: &Path) {
    store
        .agent_beat_in(
            "listing-agent",
            &Beat::default(),
            Some((tree.to_str().unwrap(), "tools/listing")),
            boot_clock().as_ref(),
        )
        .unwrap();
}

fn expire(env: &mut Env, after: f64) {
    let lost_after =
        symphony_board::health::LIVE_SECONDS + symphony_board::health::STALE_SECONDS + 600.0;
    env.vars
        .insert("TASKS_NOW".into(), (after + lost_after).to_string());
}

#[derive(Debug)]
struct NativeListing<'a> {
    recorded: &'a Path,
    working: &'a Path,
    health: &'static str,
}

/// Exercise the real producer and stored canonical path in the same scratch board.
fn native_listing(c: &mut crate::Case, root: &Path, handle: &str, listing: NativeListing<'_>) {
    c.env
        .vars
        .insert("TASKS_NOW".into(), wall_clock().to_string());
    let store = c.store();
    call_in(&store, listing.recorded);
    let mut cat = Command::new("/bin/cat");
    cat.current_dir(listing.working)
        .stdin(symphony_board::sys::Stdio::piped())
        .stdout(symphony_board::sys::Stdio::null());
    let mut owned = Working(cat.spawn().unwrap());
    let after = wall_clock();
    c.env.vars.insert("TASKS_NOW".into(), after.to_string());
    call_in(&c.store(), listing.recorded);
    expire(&mut c.env, after);
    let kept = c.store().agent_bindings().unwrap().remove(handle).unwrap();
    assert_eq!(kept.worktree, listing.recorded.to_str().unwrap());
    assert!(kept.in_process, "{kept:?}");
    let pid = owned.0.id().to_string();
    let mut command = symphony_board::sys::command_with("lsof", &c.env.vars);
    command.args(["-a", "-p", &pid]);
    let scan = process_cwd::pid_cwds_with(command).unwrap();
    if listing.health == "unknown" {
        assert!(!scan.complete, "ambiguous native name: {scan:?}");
        assert!(
            scan.listed.is_empty(),
            "no guessed caret ownership: {scan:?}"
        );
    } else {
        assert!(scan.complete, "{scan:?}");
        assert_eq!(
            scan.listed,
            vec![(i64::from(owned.0.id()), listing.working.to_path_buf())],
            "supported native name must decode exactly once"
        );
    }
    let agent = read_agent(&c.env, root, handle);
    assert_eq!(
        owned.0.try_wait().unwrap(),
        None,
        "owned child remains alive"
    );
    assert_eq!(agent["health"], listing.health, "{listing:?}: {agent}");
    let state = if listing.health != "lost" {
        "running_test"
    } else {
        "gone"
    };
    assert_eq!(agent["state"], state, "{agent}");
    if listing.health == "unknown" {
        assert_eq!(agent["state_detail"]["phase"], "unknown", "{agent}");
    }
    owned.0.kill().unwrap();
    owned.0.wait().unwrap();
    let stopped = read_agent(&c.env, root, handle);
    assert_eq!(stopped["health"], "lost", "native stopped: {stopped}");
    assert_eq!(stopped["state"], "gone", "{stopped}");
}

#[test]
fn partial_listings_cannot_make_a_still_running_owned_child_lost() {
    let root = crate::scratch("process-listing-completeness");
    let tree = root.join("wt");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join(".git"), "gitdir: /unused/scratch-worktree\n").unwrap();
    let tree = tree.canonicalize().unwrap();
    let mut c = crate::Case::new();
    let native_path = std::env::var("PATH").unwrap();
    c.env.vars.insert("PATH".into(), native_path.clone());
    c.env
        .vars
        .insert("TASKS_NOW".into(), wall_clock().to_string());
    c.env.vars.insert("HC_BIN".into(), "/nonexistent/hc".into());
    let store = c.store();
    let handle = c.register("worker");
    store
        .create(&NewTask::new("tools/listing", "Listing", "owner"))
        .unwrap();
    store.claim("tools/listing", &handle).unwrap();
    assert!(store.bind_agent("listing-agent", &handle, false).unwrap());
    call_in(&store, &tree);
    let mut cat = Command::new("/bin/cat");
    cat.current_dir(&tree)
        .stdin(symphony_board::sys::Stdio::piped())
        .stdout(symphony_board::sys::Stdio::null());
    let mut working = Working(cat.spawn().unwrap());
    let after = wall_clock();
    c.env.vars.insert("TASKS_NOW".into(), after.to_string());
    call_in(&c.store(), &tree);
    expire(&mut c.env, after);
    let read = |env: &Env| read_agent(env, &root, &handle);
    let live = read(&c.env);
    assert_eq!(live["health"], "live", "native owned-child control: {live}");
    assert_eq!(live["state"], "running_test", "{live}");

    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let lsof = bin.join("lsof");
    c.env.vars.insert(
        "PATH".into(),
        format!("{}:/usr/bin:/bin:/usr/sbin", bin.display()),
    );
    c.env
        .vars
        .insert("SCAN_PID".into(), working.0.id().to_string());
    c.env
        .vars
        .insert("SCAN_CWD".into(), tree.to_str().unwrap().into());
    for Listing {
        name,
        output,
        ending,
        health,
    } in LISTINGS
    {
        fs::write(&lsof, format!("#!/bin/sh\n{output}\n{ending}\n")).unwrap();
        fs::set_permissions(&lsof, fs::Permissions::from_mode(0o755)).unwrap();
        let a = read(&c.env);
        assert_eq!(a["health"], health, "{name}: {a}");
        let state = if health == "lost" {
            "gone"
        } else {
            "running_test"
        };
        assert_eq!(a["state"], state, "{name}: {a}");
        if health != "lost" {
            let phase = if health == "live" {
                "running"
            } else {
                "unknown"
            };
            assert_eq!(a["state_detail"]["phase"], phase, "{name}: {a}");
        }
        assert_eq!(
            working.0.try_wait().unwrap(),
            None,
            "{name}: owned child still running"
        );
    }
    // A readable cwd whose PID ended between listing and start lookup is excluded as before.
    working.0.kill().unwrap();
    working.0.wait().unwrap();
    fs::write(
        &lsof,
        "#!/bin/sh\nprintf 'p%s\\nfcwd\\nn%s\\n' \"$SCAN_PID\" \"$SCAN_CWD\"\n",
    )
    .unwrap();
    let ended = read(&c.env);
    assert_eq!(ended["health"], "lost", "ended PID: {ended}");
    c.env.vars.insert("PATH".into(), native_path);
    let stopped = read(&c.env);
    assert_eq!(
        stopped["health"], "lost",
        "native stopped-child control: {stopped}"
    );
    assert_eq!(stopped["state"], "gone", "{stopped}");

    native_names(&mut c, &root, &handle);
}

fn native_names(c: &mut crate::Case, root: &Path, handle: &str) {
    struct Name {
        actual: &'static str,
        lookalike: &'static str,
        health: &'static str,
    }
    // Each lookalike really exists. Existence/canonicalization of a display spelling is
    // insufficient, and a second decoding pass would attribute the literal escapes wrongly.
    for name in [
        Name {
            actual: "ordinary",
            lookalike: "unrelated",
            health: "live",
        },
        Name {
            actual: "tab\tname",
            lookalike: "tab\\tname",
            health: "live",
        },
        Name {
            actual: "café",
            lookalike: "caf\\xc3\\xa9",
            health: "live",
        },
        Name {
            actual: "literal\\t^A",
            lookalike: "literal\t^A",
            health: "live",
        },
        Name {
            actual: "literal\\x41",
            lookalike: "literalA",
            health: "live",
        },
        Name {
            actual: "control\x01",
            lookalike: "control^A",
            health: "unknown",
        },
    ] {
        let actual = root.join(name.actual).join("wt");
        let other = root.join(name.lookalike).join("wt");
        for path in [&actual, &other] {
            fs::create_dir_all(path).unwrap();
            fs::write(path.join(".git"), "gitdir: /unused/scratch-worktree\n").unwrap();
        }
        let actual = actual.canonicalize().unwrap();
        let other = other.canonicalize().unwrap();
        assert_ne!(actual, other);
        native_listing(
            c,
            root,
            handle,
            NativeListing {
                recorded: &actual,
                working: &actual,
                health: name.health,
            },
        );
        native_listing(
            c,
            root,
            handle,
            NativeListing {
                recorded: &other,
                working: &actual,
                health: if name.health == "unknown" {
                    "unknown"
                } else {
                    "lost"
                },
            },
        );
    }
}
