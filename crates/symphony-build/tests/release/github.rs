//! A green release's GitHub Release is asked of the command `[releases] github` names, through a
//! stand-in for the GitHub adapter that answers as it does: once for a tag pushed, with the
//! release's own notes as its body; not again once it is there; left pending, the release green
//! and its exit unchanged, while the stand-in does not answer, and finished by `--refresh`, the
//! next release's run or `--github`, which also publishes the releases tagged before. A project
//! that names no command runs none and says, records and keeps nothing of it.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use serde_json::Value;

use super::{CHECK, DAY, Fixture, NOW};

/// A stand-in for `symphony-github release` in a folder of the fixture's own, run as `stand-in
/// release --request FILE`: it keeps each request it is handed (`asked.jsonl`) with the folder it
/// was run in (`cwd.log`), and answers `published` for a tag it has not made, `found` for one it
/// has, and, while the file `down` is beside it, `unavailable` with nothing made.
struct Stand {
    folder: PathBuf,
    program: PathBuf,
}

impl Stand {
    fn new(fixture: &Fixture) -> Stand {
        let folder = fixture.root.join("github-stand-in");
        fs::create_dir_all(folder.join("made")).unwrap();
        let program = folder.join("stand-in");
        fs::write(
            &program,
            format!(
                r#"#!/bin/sh
here='{folder}'
[ "$1" = release ] && [ "$2" = --request ] || {{ echo "usage" >&2; exit 2; }}
cat "$3" >> "$here/asked.jsonl"
pwd >> "$here/cwd.log"
tag=$(sed -n 's/^{{"tag":"\([^"]*\)".*/\1/p' "$3")
if [ -e "$here/down" ]; then
    echo '{{"state":"unavailable","why":"GitHub answered 503 to GET /repos/o/n"}}'
elif [ -e "$here/made/$tag" ]; then
    echo '{{"state":"found","release":"'"$tag"'","url":"https://github.com/o/n/releases/tag/'"$tag"'"}}'
else
    : > "$here/made/$tag"
    echo '{{"state":"published","release":"'"$tag"'","url":"https://github.com/o/n/releases/tag/'"$tag"'"}}'
fi
"#,
                folder = folder.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Stand { folder, program }
    }

    /// The project file's `[releases]` line that names the stand-in.
    fn setting(&self) -> String {
        format!("github = [\"{}\", \"release\"]\n", self.program.display())
    }

    /// The requests it was handed, in order.
    fn asked(&self) -> Vec<Value> {
        fs::read_to_string(self.folder.join("asked.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// The releases those requests named, in order.
    fn tags(&self) -> Vec<String> {
        (self.asked().iter())
            .map(|request| request["tag"].as_str().unwrap().to_string())
            .collect()
    }

    fn down(&self, down: bool) {
        if down {
            fs::write(self.folder.join("down"), "").unwrap();
        } else {
            fs::remove_file(self.folder.join("down")).unwrap();
        }
    }
}

/// The fixture with a project file naming `stand`, committed and pushed.
fn fixture_with(releases: impl Fn(&Stand) -> String) -> (Fixture, Stand) {
    let fixture = Fixture::new();
    let stand = Stand::new(&fixture);
    fixture.write("symphony.toml", &Fixture::project_file(&releases(&stand)));
    fixture.commit("the project names its GitHub Release command");
    super::git(&fixture.main, &["push", "-q", "origin", "main"]);
    (fixture, stand)
}

/// The ledger the runs keep, as JSON.
fn ledger(fixture: &Fixture) -> Value {
    serde_json::from_str(&fs::read_to_string(fixture.records().join("github.json")).unwrap())
        .unwrap()
}

/// The lines a run said of GitHub.
fn said(out: &str) -> Vec<&str> {
    out.lines()
        .filter_map(|line| line.strip_prefix("release: GitHub: "))
        .collect()
}

const URL: &str = "https://github.com/o/n/releases/tag/";
const AGAIN: &str = "`just release --refresh`, the next release's run or `just release --github` \
                     asks again";

#[test]
fn a_green_release_is_published_once_with_its_notes_and_found_after() {
    let (fixture, stand) = fixture_with(Stand::setting);
    let out = fixture.night(NOW, 0);
    assert!(
        out.contains(&format!(
            "release: green; tagged release-2026-11-12, pushed to origin\n\
             release: crates.io: not published: [releases] publish is off\n\
             release: GitHub: published release-2026-11-12 as {URL}release-2026-11-12\n"
        )),
        "{out}"
    );
    let commit = fixture.tagged("release-2026-11-12");
    let asked = stand.asked();
    assert_eq!(asked.len(), 1);
    let run = fixture.runs()[0]["id"].as_str().unwrap().to_string();
    assert_eq!(asked[0]["tag"], "release-2026-11-12");
    assert_eq!(asked[0]["commit"], commit.as_str());
    assert_eq!(asked[0]["latest"], true);
    assert_eq!(asked[0]["by"], run.as_str());
    // Its body is the release's own notes, as `symphony release-notes` prints them.
    assert_eq!(
        asked[0]["body"],
        format!(
            "## release-2026-11-12\n\nCommit: {commit}\nThe first release.\n\nNothing a project \
             changes to take it.\n"
        )
    );
    // Run in the main checkout, where the adapter reads the committed settings.
    assert_eq!(
        fs::read_to_string(stand.folder.join("cwd.log")).unwrap(),
        format!("{}\n", fixture.main.display())
    );
    let kept = &ledger(&fixture)["releases"]["release-2026-11-12"];
    assert_eq!(kept["state"], "published");
    assert_eq!(kept["said"], format!("{URL}release-2026-11-12"));
    assert_eq!(kept["by"], run.as_str());
    let status = fixture.run(NOW + 3600.0, &["--status"], 0);
    assert!(
        status.contains(&format!(
            "GitHub releases: 1 published, 0 pending\n  newest published: release-2026-11-12 as \
             {URL}release-2026-11-12; asked by {run}, 2026-11-12 10:00 (1 h ago)\n"
        )),
        "{status}"
    );

    // The next release is asked for alone, and its notes are from the release before.
    let next = fixture.land("next.txt", Some("next"), "the next landing");
    let out = fixture.night(NOW + DAY, 0);
    assert_eq!(
        said(&out),
        [format!(
            "published release-2026-11-13 as {URL}release-2026-11-13"
        )]
    );
    assert_eq!(stand.tags(), ["release-2026-11-12", "release-2026-11-13"]);
    let body = stand.asked()[1]["body"].as_str().unwrap().to_string();
    assert!(
        body.starts_with(&format!(
            "## release-2026-11-13\n\nCommit: {next}\nFrom release-2026-11-12.\n"
        )),
        "{body}"
    );
    assert!(!body.contains("## release-2026-11-12"), "{body}");

    // A refresh asks again for the newest, which is there already: none is made twice.
    let out = fixture.run(NOW + DAY + 60.0, &["--refresh"], 0);
    assert_eq!(
        said(&out),
        [format!(
            "release-2026-11-13 is there already: {URL}release-2026-11-13"
        )]
    );
    assert_eq!(
        ledger(&fixture)["releases"]["release-2026-11-13"]["state"],
        "found"
    );
    assert_eq!(fs::read_dir(stand.folder.join("made")).unwrap().count(), 2);
    assert_eq!(
        fixture.remote_tags(),
        ["release-2026-11-12", "release-2026-11-13"]
    );
}

#[test]
fn a_github_that_does_not_answer_leaves_the_release_green_and_pending_until_asked_again() {
    let (fixture, stand) = fixture_with(Stand::setting);
    stand.down(true);
    // Green, tagged, pushed, exit 0: the publication alone is pending.
    let out = fixture.night(NOW, 0);
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin\n"),
        "{out}"
    );
    let pending = format!(
        "pending: release-2026-11-12: unavailable: GitHub answered 503 to GET /repos/o/n; {AGAIN}"
    );
    assert_eq!(said(&out), [pending.as_str()]);
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    let run = &fixture.runs()[0];
    assert_eq!(
        (&run["result"], &run["pushed"]),
        (&"green".into(), &true.into())
    );
    assert_eq!(
        ledger(&fixture)["releases"]["release-2026-11-12"]["state"],
        "pending"
    );
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains(&format!(
            "GitHub releases: 0 published, 1 pending\n  pending release-2026-11-12: unavailable: \
             GitHub answered 503 to GET /repos/o/n; asked by {}, 2026-11-12 10:00 (1 min ago); \
             {AGAIN}\n",
            run["id"].as_str().unwrap()
        )),
        "{status}"
    );
    // Still down at the refresh: still pending, and the refresh's exit is its own.
    let out = fixture.run(NOW + 120.0, &["--refresh"], 0);
    assert_eq!(said(&out), [pending.as_str()]);

    // The next release's run finishes it before its own, oldest first, and only the newest is
    // the latest.
    stand.down(false);
    fixture.land("next.txt", Some("next"), "the next landing");
    let out = fixture.night(NOW + DAY, 0);
    assert_eq!(
        said(&out),
        [
            format!("published release-2026-11-12 as {URL}release-2026-11-12"),
            format!("published release-2026-11-13 as {URL}release-2026-11-13"),
        ]
    );
    let asked = stand.asked();
    let last: Vec<(&str, bool)> = (asked[asked.len() - 2..].iter())
        .map(|request| {
            (
                request["tag"].as_str().unwrap(),
                request["latest"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        last,
        [("release-2026-11-12", false), ("release-2026-11-13", true)]
    );
    let status = fixture.run(NOW + DAY + 60.0, &["--status"], 0);
    assert!(
        status.contains("GitHub releases: 2 published, 0 pending\n"),
        "{status}"
    );
}

#[test]
fn a_refresh_finishes_a_pending_publication() {
    let (fixture, stand) = fixture_with(Stand::setting);
    stand.down(true);
    fixture.night(NOW, 0);
    stand.down(false);
    let out = fixture.run(NOW + 60.0, &["--refresh"], 0);
    assert_eq!(
        said(&out),
        [format!(
            "published release-2026-11-12 as {URL}release-2026-11-12"
        )]
    );
    assert_eq!(
        ledger(&fixture)["releases"]["release-2026-11-12"]["state"],
        "published"
    );
}

#[test]
fn a_command_that_cannot_run_or_answers_no_json_leaves_the_release_green_and_pending() {
    // No such program.
    let (fixture, _stand) =
        fixture_with(|_| "github = [\"/nowhere/symphony-github\", \"release\"]\n".into());
    let out = fixture.night(NOW, 0);
    let lines = said(&out);
    assert_eq!(lines.len(), 1, "{out}");
    assert!(
        lines[0].starts_with(
            "pending: release-2026-11-12: cannot run /nowhere/symphony-github release --request "
        ),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);

    // A program that fails, and one that says something else than an answer.
    for (script, wanted) in [
        (
            "echo 'symphony-github: no working directory' >&2; exit 2",
            ": symphony-github: no working directory; ",
        ),
        ("echo done", ": its answer is not a JSON object; "),
        (
            "echo '{\"state\":\"merged\"}'",
            ": it answered the state \"merged\"; ",
        ),
        (
            "echo '{\"state\":\"refused\",\"why\":\"turned off: the committed settings do not say enabled = true and releases = \\\"publish\\\"\"}'",
            "pending: release-2026-11-12: refused: turned off: the committed settings do not say enabled = true and releases = \"publish\"; ",
        ),
    ] {
        let (fixture, stand) = fixture_with(Stand::setting);
        fs::write(&stand.program, format!("#!/bin/sh\n{script}\n")).unwrap();
        let out = fixture.night(NOW, 0);
        let lines = said(&out);
        assert_eq!(lines.len(), 1, "{out}");
        assert!(
            lines[0].starts_with("pending: release-2026-11-12: ") && lines[0].contains(wanted),
            "{wanted}\n{out}"
        );
        assert_eq!(fixture.runs()[0]["result"], "green");
    }
}

#[test]
fn a_red_run_and_a_run_under_no_push_ask_for_nothing() {
    let (fixture, stand) = fixture_with(Stand::setting);
    fixture.land("red", Some(""), "a red landing");
    let out = fixture.night(NOW, 1);
    assert!(said(&out).is_empty(), "{out}");
    fixture.land("red", None, "its fix");
    let out = fixture.run(NOW + 60.0, &["--check", CHECK, "--no-push"], 0);
    assert!(
        out.contains("release: green; tagged release-2026-11-12"),
        "{out}"
    );
    assert!(
        said(&out).is_empty(),
        "nothing is pushed, so nothing is asked: {out}"
    );
    let out = fixture.run(NOW + 120.0, &["--refresh", "--no-push"], 0);
    assert_eq!(said(&out), ["not published: --no-push publishes nothing"]);
    assert_eq!(stand.asked().len(), 0);
    assert!(!fixture.records().join("github.json").exists());
}

#[test]
fn the_releases_already_tagged_are_published_by_hand_without_a_run() {
    // Three releases cut before the project named a command.
    let fixture = Fixture::new();
    fixture.night(NOW, 0);
    fixture.land("two.txt", Some("2"), "the second landing");
    fixture.night(NOW + 60.0, 0);
    fixture.land("three.txt", Some("3"), "the third landing");
    fixture.night(NOW + DAY, 0);
    let tags = [
        "release-2026-11-12",
        "release-2026-11-12.2",
        "release-2026-11-13",
    ];
    assert_eq!(fixture.remote_tags(), tags);
    assert!(!fixture.records().join("github.json").exists());
    // Asked by hand with no command named: refused, nothing run.
    let (code, _, err) = fixture.release_at(NOW + DAY + 60.0, &["--github"]);
    assert_eq!(code, 1);
    assert!(err.contains("[releases] github names no command"), "{err}");

    let stand = Stand::new(&fixture);
    fixture.write("symphony.toml", &Fixture::project_file(&stand.setting()));
    let runs = fixture.runs().len();
    let refs = fixture.remote_refs();
    // One of them, by name.
    let out = fixture.run(NOW + DAY + 120.0, &["--github", "release-2026-11-12.2"], 0);
    assert_eq!(
        out,
        format!("release: GitHub: published release-2026-11-12.2 as {URL}release-2026-11-12.2\n")
    );
    assert_eq!(stand.asked()[0]["latest"], false);
    assert_eq!(stand.asked()[0]["by"], "by hand");
    // All of them, oldest first: the one made is found, and only the newest is the latest.
    let out = fixture.run(NOW + DAY + 180.0, &["--github"], 0);
    assert_eq!(
        said(&out),
        [
            format!("published release-2026-11-12 as {URL}release-2026-11-12"),
            format!("release-2026-11-12.2 is there already: {URL}release-2026-11-12.2"),
            format!("published release-2026-11-13 as {URL}release-2026-11-13"),
        ]
    );
    assert_eq!(stand.tags()[1..], tags);
    assert_eq!(stand.asked()[3]["latest"], true);
    // Again: none is made twice.
    let out = fixture.run(NOW + DAY + 240.0, &["--github"], 0);
    assert!(
        said(&out)
            .iter()
            .all(|line| line.contains(" is there already: ")),
        "{out}"
    );
    assert_eq!(fs::read_dir(stand.folder.join("made")).unwrap().count(), 3);
    // It ran no check, tagged and pushed nothing, and recorded no run.
    assert_eq!(fixture.runs().len(), runs);
    assert_eq!(fixture.remote_refs(), refs);

    // A release that is not tagged here is refused before anything is asked, and one left
    // pending ends 1.
    let asked = stand.asked().len();
    let (code, _, err) = fixture.release_at(NOW + DAY + 300.0, &["--github", "release-2026-11-20"]);
    assert_eq!(code, 1);
    assert!(
        err.contains("no release release-2026-11-20 is tagged here"),
        "{err}"
    );
    assert_eq!(stand.asked().len(), asked);
    stand.down(true);
    let out = fixture.run(NOW + DAY + 360.0, &["--github", "release-2026-11-13"], 1);
    assert!(
        said(&out)[0].starts_with("pending: release-2026-11-13: unavailable: "),
        "{out}"
    );
    // `--github` is a mode of its own.
    let (code, _, err) = fixture.release_at(NOW + DAY + 420.0, &["--github", "--refresh"]);
    assert_eq!(code, 2, "{err}");
}

#[test]
fn a_project_that_names_no_command_runs_none_and_says_and_keeps_nothing_of_it() {
    // A `symphony-github` on the PATH that would say so if anything ran it.
    let fixture = Fixture::new();
    let bin = fixture.root.join("trap-bin");
    fs::create_dir_all(&bin).unwrap();
    let trap = bin.join("symphony-github");
    fs::write(
        &trap,
        format!("#!/bin/sh\necho \"$*\" >> '{}/ran.log'\n", bin.display()),
    )
    .unwrap();
    fs::set_permissions(&trap, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let env = [("PATH", path.as_str())];
    let (code, out, _) = fixture.release_with(NOW, &["--check", CHECK], &env);
    assert_eq!(code, 0, "{out}");
    let (_, refreshed, _) = fixture.release_with(NOW + 60.0, &["--refresh"], &env);
    let (_, status, _) = fixture.release_with(NOW + 120.0, &["--status"], &env);
    for text in [&out, &refreshed, &status] {
        assert!(!text.contains("GitHub"), "{text}");
    }
    assert!(!bin.join("ran.log").exists());
    let kept: Vec<String> = fs::read_dir(fixture.records())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("github"))
        .collect();
    assert!(kept.is_empty(), "{kept:?}");
    // Its record names nothing of it either.
    let record = fs::read_to_string(fixture.records().join("runs").join(format!(
        "{}.json",
        fixture.runs()[0]["id"].as_str().unwrap()
    )))
    .unwrap();
    assert!(!record.to_lowercase().contains("github"), "{record}");
}
