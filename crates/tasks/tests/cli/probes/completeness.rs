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

const LISTINGS: [Listing; 15] = [
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
    let lost_after =
        symphony_board::health::LIVE_SECONDS + symphony_board::health::STALE_SECONDS + 600.0;
    c.env
        .vars
        .insert("TASKS_NOW".into(), (after + lost_after).to_string());
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
}
