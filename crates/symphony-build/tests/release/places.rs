//! The release run works in a checkout it made, nowhere else: a link in its place, another worktree
//! named for it, or a folder it did not make is refused and left exactly as it was.

use std::fs;
use std::os::unix::fs::symlink;

use super::{CHECK, Fixture, NOW, git};

#[test]
fn an_agents_worktree_behind_a_link_or_named_outright_is_never_reset() {
    let fixture = Fixture::new();
    // An agent's worktree beside the release run's place, with work of its own.
    let agent = fixture.root.join("wt/agent-x");
    git(
        &fixture.main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "agent-x",
            &agent.to_string_lossy(),
        ],
    );
    fs::write(agent.join("precious.txt"), "uncommitted\n").unwrap();
    fs::write(agent.join("symphony.toml"), "edited\n").unwrap();
    let status = git(&agent, &["status", "--porcelain"]);
    let files = Fixture::files(&agent);

    // A folder in the checkout's place that the run did not make: never taken over, and the
    // board hears of it.
    let found = fixture.worktree().join("notes.txt");
    fs::create_dir_all(fixture.worktree()).unwrap();
    fs::write(&found, "someone's\n").unwrap();
    let (code, out, err) = fixture.release_at(NOW, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    let said = format!(
        "{} is not the release run's own checkout (not a worktree of this repository); it is left as \
         it is: move it away or name another --worktree",
        fixture.worktree().display()
    );
    assert!(err.contains(&said), "{err}");
    assert!(!out.contains("release: verify"), "no check ran: {out}");
    assert_eq!(fs::read_to_string(&found).unwrap(), "someone's\n");
    assert!(fixture.remote_tags().is_empty());
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["result"], "error", "{run}");
    assert_eq!(run["error"], said.as_str(), "{run}");
    assert!(
        out.contains("release: filed tests/release-red-run"),
        "{out}"
    );
    let reds = fixture.reds();
    assert_eq!(reds.keys().collect::<Vec<_>>(), ["tests/release-red-run"]);
    assert!(reds["tests/release-red-run"].body.contains(&said));
    assert!(reds["tests/release-red-run"].red());
    fs::remove_dir_all(fixture.worktree()).unwrap();

    // A link in the checkout's place, to that worktree: the run stops, and says so on the same red.
    symlink(&agent, fixture.worktree()).unwrap();
    let (code, out, err) = fixture.release_at(NOW + 10.0, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!("{} is a link", fixture.worktree().display())),
        "{err}"
    );
    assert!(!out.contains("release: verify"), "nothing ran: {out}");
    assert!(
        out.contains("release: updated tests/release-red-run"),
        "{out}"
    );
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/release-red-run"]
    );
    fs::remove_file(fixture.worktree()).unwrap();

    // Named by hand: refused, nothing recorded.
    let runs = fixture.runs().len();
    let (code, out, err) = fixture.release_at(
        NOW + 20.0,
        &["--check", CHECK, "--worktree", &agent.to_string_lossy()],
    );
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains("(a worktree the release run did not lock)"),
        "{err}"
    );
    assert_eq!(fixture.runs().len(), runs, "a refusal records nothing");

    // Or the folder holding it.
    let (code, _, err) = fixture.release_at(
        NOW + 30.0,
        &[
            "--check",
            CHECK,
            "--worktree",
            &fixture.root.join("wt").to_string_lossy(),
        ],
    );
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("holds the checkout"), "{err}");

    assert_eq!(Fixture::files(&agent), files, "not a byte of it changed");
    assert_eq!(git(&agent, &["status", "--porcelain"]), status);
    assert_eq!(
        git(&agent, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "agent-x",
        "its branch is still checked out"
    );
    assert!(fixture.remote_tags().is_empty());

    // Its own place free, the release run makes its checkout there, beside the agent's, and closes
    // the red.
    let out = fixture.night(NOW + 60.0, 0);
    assert!(
        out.contains("release: closed tests/release-red-run"),
        "{out}"
    );
    assert_eq!(fixture.reds()["tests/release-red-run"].status, "cancelled");
    assert_eq!(Fixture::files(&agent), files);
}
