//! The board's clippy rule for its children (the crate's clippy.toml, `disallowed-methods`): a
//! child made past `sys::command` and `sys::command_with`, or a cwd scan whose `lsof` keeps the
//! session, is refused however its path is spelled, imported, aliased or gated. The toolchain's own
//! clippy, run over a probe crate that links the same `process-cwd` and reads a copy of the board's
//! clippy.toml, says where it refuses one.
//!
//! A nested build: the release run runs this target, the fast plan never does
//! (`tools/test_budget.json`). The settings' prefix check is the library's, in the fast plan.

use std::fs;
use std::path::{Path, PathBuf};
use symphony_board::sys;

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// A line clippy must refuse in a probe file, and the method it names there.
struct Refused {
    line: usize,
    method: &'static str,
}

/// A probe file: the module it is in the probe crate, its text, and what clippy must refuse there.
struct Probe {
    module: &'static str,
    text: &'static str,
    refused: &'static [Refused],
}

const NEW: &str = "process_cwd::spawn::Command::new";
const STD_NEW: &str = "std::process::Command::new";

/// Each spelling of a child made past the two helpers that clippy must refuse, and the one place a
/// child may be made: an item that says so, which clippy lets pass.
const PROBES: [Probe; 9] = [
    Probe {
        module: "spaced",
        text: "pub fn start() {\n    let _ = process_cwd::spawn::Command :: new(\"true\").spawn();\n}\n",
        refused: &[Refused {
            line: 2,
            method: NEW,
        }],
    },
    Probe {
        module: "line_broken",
        text: "pub fn start() {\n    let _ = process_cwd::spawn::Command::\n        new(\"true\").spawn();\n}\n",
        refused: &[Refused {
            line: 2,
            method: NEW,
        }],
    },
    Probe {
        module: "renamed",
        text: "use process_cwd::Command as Proc;\n\npub fn start() {\n    let _ = Proc::new(\"true\").spawn();\n}\n",
        refused: &[Refused {
            line: 4,
            method: NEW,
        }],
    },
    Probe {
        module: "after_a_test_module",
        text: "#[cfg(test)]\nmod tests {}\n\npub fn start() {\n    let _ = process_cwd::Command::new(\"true\").spawn();\n}\n",
        refused: &[Refused {
            line: 5,
            method: NEW,
        }],
    },
    Probe {
        module: "aliased",
        text: "type Proc = std::process::Command;\n\npub fn start() {\n    let _ = Proc::new(\"true\").status();\n}\n",
        refused: &[Refused {
            line: 4,
            method: STD_NEW,
        }],
    },
    Probe {
        module: "as_a_value",
        text: "pub fn start() {\n    let _ = [\"true\"].map(std::process::Command::new);\n}\n",
        refused: &[Refused {
            line: 2,
            method: STD_NEW,
        }],
    },
    Probe {
        module: "cleared",
        text: "pub fn clear(ours: &mut process_cwd::Command, theirs: &mut std::process::Command) {\n    \
               ours.env_clear();\n    theirs.env_clear();\n}\n",
        refused: &[
            Refused {
                line: 2,
                method: "process_cwd::spawn::Command::env_clear",
            },
            Refused {
                line: 3,
                method: "std::process::Command::env_clear",
            },
        ],
    },
    Probe {
        module: "scans",
        text: "pub fn scan() {\n    let _ = process_cwd::process_cwds();\n    \
               let _ = process_cwd::occupied(std::path::Path::new(\"/\"));\n}\n",
        refused: &[
            Refused {
                line: 2,
                method: "process_cwd::process_cwds",
            },
            Refused {
                line: 3,
                method: "process_cwd::occupied",
            },
        ],
    },
    Probe {
        module: "made_here",
        text: "#[expect(clippy::disallowed_methods, reason = \"the helper\")]\n\
               pub fn command(program: &str) -> process_cwd::Command {\n    \
               process_cwd::Command::new(program)\n}\n",
        refused: &[],
    },
];

/// A folder of the test's own under the temporary folder, removed when it drops.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The probe crate in `root`: a package of its own that links this workspace's `process-cwd` at
/// the versions its lockfile pins, with the board's clippy.toml and one module per probe.
fn probe_crate(root: &Path) {
    let process_cwd = crate_dir().join("../process-cwd").canonicalize().unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"board-child-probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             publish = false\n\n[dependencies]\nprocess-cwd = {{ path = {:?} }}\n\n[workspace]\n",
            process_cwd.display().to_string()
        ),
    )
    .unwrap();
    fs::copy(
        crate_dir().join("../../Cargo.lock"),
        root.join("Cargo.lock"),
    )
    .unwrap();
    fs::copy(crate_dir().join("clippy.toml"), root.join("clippy.toml")).unwrap();
    let mut lib = String::new();
    for probe in &PROBES {
        fs::write(
            root.join("src").join(format!("{}.rs", probe.module)),
            probe.text,
        )
        .unwrap();
        lib.push_str(&format!("pub mod {};\n", probe.module));
    }
    fs::write(root.join("src/lib.rs"), lib).unwrap();
}

/// What clippy said of `src/`, one `file:line: method` per finding, sorted; a finding of anything
/// but a disallowed method keeps its whole message.
fn findings(said: &str) -> Vec<String> {
    let mut found: Vec<String> = said
        .lines()
        .filter_map(|line| line.strip_prefix("src/"))
        .map(|line| {
            let mut parts = line.splitn(4, ':');
            let (file, at) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let message = parts.nth(1).unwrap_or("").trim();
            let method = message
                .strip_prefix("warning: use of a disallowed method `")
                .and_then(|rest| rest.strip_suffix('`'))
                .unwrap_or(message);
            format!("{file}:{at}: {method}")
        })
        .collect();
    symphony_board::order::sort_by(&mut found, &Ord::cmp);
    found
}

#[test]
fn findings_are_read_as_file_line_and_method() {
    let said = "   Checking board-child-probe v0.0.0\n\
                src/a.rs:2:13: warning: use of a disallowed method `std::process::Command::new`\n\
                src/b.rs:7:1: warning: this expectation is unfulfilled\n\
                warning: `board-child-probe` (lib) generated 2 warnings\n";
    assert_eq!(
        findings(said),
        [
            "a.rs:2: std::process::Command::new",
            "b.rs:7: warning: this expectation is unfulfilled",
        ]
    );
}

/// Every spelling the probes hold is refused on its own line, naming the method, and nothing else
/// is said: an item that expects the lint passes, and every path in the settings resolves.
#[test]
fn clippy_refuses_a_child_made_past_the_helpers_however_it_is_spelled() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("board-child-probe-{}", std::process::id())));
    let _ = fs::remove_dir_all(&scratch.0);
    probe_crate(&scratch.0);
    let mut cargo = sys::command(env!("CARGO"));
    cargo
        .args([
            "clippy",
            "--offline",
            "--quiet",
            "--message-format",
            "short",
        ])
        .current_dir(&scratch.0)
        .env("CARGO_TARGET_DIR", scratch.0.join("target"));
    for flags in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CLIPPY_CONF_DIR",
        "CLIPPY_ARGS",
    ] {
        cargo.env_remove(flags);
    }
    let done = sys::output(&mut cargo).unwrap();
    let said = String::from_utf8_lossy(&done.stderr);
    assert!(done.status.success(), "{said}");
    let mut expected: Vec<String> = PROBES
        .iter()
        .flat_map(|probe| {
            probe.refused.iter().map(move |refused| {
                format!("{}.rs:{}: {}", probe.module, refused.line, refused.method)
            })
        })
        .collect();
    symphony_board::order::sort_by(&mut expected, &Ord::cmp);
    assert_eq!(findings(&said), expected, "{said}");
    assert!(!said.contains("clippy.toml"), "{said}");
}
