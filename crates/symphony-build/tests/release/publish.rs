//! A green release publishes on crates.io only while `[releases] publish` says so, after its tag
//! is pushed: the packages whose version the registry lacks, named in one `cargo publish`, through
//! a stand-in for cargo and a registry folder of the test's own. With the step off, under
//! `--no-push`, or on the tests' clock without the stand-ins, no `cargo publish` runs, through the
//! variable or through `cargo` on the PATH; a failed upload leaves the release tagged, and
//! `--refresh` finishes what it left.

use std::fs;
use std::path::PathBuf;

use super::refresh::executable;
use super::{CHECK, DAY, Fixture, NOW};

/// A Cargo workspace in the fixture's checkout: `alpha`; `bravo` building with it; `charlie`
/// building with `bravo`; each at 1.0.0.
fn workspace(fixture: &Fixture) {
    fixture.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"alpha\", \"bravo\", \"charlie\"]\n",
    );
    for (name, deps) in [
        ("alpha", ""),
        (
            "bravo",
            "alpha = { path = \"../alpha\", version = \"1.0.0\" }\n",
        ),
        (
            "charlie",
            "bravo = { path = \"../bravo\", version = \"1.0.0\" }\n",
        ),
    ] {
        fixture.write(
            &format!("{name}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\
                 description = \"{name}\"\nlicense = \"MIT\"\n\n[dependencies]\n{deps}"
            ),
        );
        fixture.write(&format!("{name}/src/lib.rs"), "\n");
    }
}

/// What a publication runs, in a folder of the fixture's own: a registry folder (`registry`); a
/// stand-in for the cargo that publishes (`publisher`), run as `publish --locked -p NAME …`, which
/// lists each package it uploads in the registry at 1.0.0 and, while the file `fail-bravo` is
/// beside it, fails at bravo, as cargo uploads nothing when one package does not build; and a
/// folder (`path`) whose `cargo` runs the real one but for `publish`, which it refuses. Each
/// stand-in writes the command lines it ran to a log of its own.
struct Stand {
    registry: PathBuf,
    publisher: PathBuf,
    path: String,
    folder: PathBuf,
}

impl Stand {
    fn new(fixture: &Fixture) -> Stand {
        let folder = fixture.root.join("stand-in");
        let registry = folder.join("registry");
        let index = registry.join("index/al/ph");
        fs::create_dir_all(&index).unwrap();
        fs::write(
            index.join("alpha"),
            "{\"name\":\"alpha\",\"vers\":\"1.0.0\"}\n",
        )
        .unwrap();
        let publisher = folder.join("publisher");
        executable(
            &publisher,
            &format!(
                r#"#!/bin/sh
echo "$*" >> '{folder}/published.log'
case " $* " in *" bravo "*) if [ -e '{folder}/fail-bravo' ]; then
    echo 'error: failed to verify package bravo' >&2
    exit 101
fi;; esac
shift 2
for name in "$@"; do
[ "$name" = -p ] && continue
dir='{registry}/index/'"$(printf %s "$name" | cut -c1-2)/$(printf %s "$name" | cut -c3-4)"
mkdir -p "$dir" && printf '{{"name":"%s","vers":"1.0.0"}}\n' "$name" >> "$dir/$name"
done
"#,
                folder = folder.display(),
                registry = registry.display(),
            ),
        );
        let bin = folder.join("bin");
        fs::create_dir_all(&bin).unwrap();
        executable(
            &bin.join("cargo"),
            &format!(
                "#!/bin/sh\nif [ \"$1\" = publish ]; then echo \"$*\" >> '{}/path.log'; exit 101; \
                 fi\nexec '{}' \"$@\"\n",
                folder.display(),
                env!("CARGO")
            ),
        );
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Stand {
            registry,
            publisher,
            path,
            folder,
        }
    }

    /// The lines the stand-in named `log` ran.
    fn ran(&self, log: &str) -> Vec<String> {
        fs::read_to_string(self.folder.join(log))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The environment of a run that publishes through both stand-ins.
    fn both(&self) -> Vec<(&str, &str)> {
        vec![
            ("SYMPHONY_CRATES_IO", self.registry.to_str().unwrap()),
            ("SYMPHONY_RELEASE_CARGO", self.publisher.to_str().unwrap()),
            ("PATH", self.path.as_str()),
        ]
    }
}

/// A release run with the stand-in check at `now` and `extra` in its environment, which must end
/// with `code`: its stdout.
fn run(fixture: &Fixture, now: f64, args: &[&str], extra: &[(&str, &str)], code: i32) -> String {
    let (said, out, err) = fixture.release_with(now, args, extra);
    assert_eq!(
        said, code,
        "release {args:?}\nstdout:\n{out}\nstderr:\n{err}"
    );
    out
}

#[test]
fn a_release_publishes_nothing_while_off_under_no_push_or_on_the_tests_clock_without_stand_ins() {
    let fixture = Fixture::new();
    workspace(&fixture);
    fixture.commit("a workspace that publishes");
    super::git(&fixture.main, &["push", "-q", "origin", "main"]);
    let stand = Stand::new(&fixture);
    // Off, as it is unless the project says: said, recorded, and no publish run either way.
    let out = run(&fixture, NOW, &["--check", CHECK], &stand.both(), 0);
    assert!(
        out.contains(
            "release: green; tagged release-2026-11-12, pushed to origin\n\
                      release: crates.io: not published: [releases] publish is off\n"
        ),
        "{out}"
    );
    let published = &fixture.runs()[0]["published"];
    assert_eq!(
        published,
        &serde_json::json!({"release": "release-2026-11-12", "why": "[releases] publish is off", "failed": false})
    );
    assert!(
        !fixture.message("release-2026-11-12").contains("Publishes"),
        "{}",
        fixture.message("release-2026-11-12")
    );
    let status = run(&fixture, NOW, &["--status"], &[], 0);
    assert!(
        status.contains("last publish: of release-2026-11-12 by the run ")
            && status.contains("\n  not published: [releases] publish is off\n"),
        "{status}"
    );
    // On, on the tests' clock with a registry but no stand-in for cargo: refused, red, and the
    // cargo on the PATH publishes nothing either.
    fixture.write("symphony.toml", &Fixture::project_file("publish = true\n"));
    fixture.land("next.txt", Some("next"), "the next landing");
    let registry_only = [
        ("SYMPHONY_CRATES_IO", stand.registry.to_str().unwrap()),
        ("PATH", stand.path.as_str()),
    ];
    let out = run(&fixture, NOW + DAY, &["--check", CHECK], &registry_only, 1);
    assert!(
        out.contains(
            "release: crates.io: not published: SYMPHONY_RELEASE_NOW is the tests' clock: a run on \
             it publishes only to the registry SYMPHONY_CRATES_IO names, through the stand-in for \
             cargo SYMPHONY_RELEASE_CARGO names\n"
        ),
        "{out}"
    );
    assert_eq!(
        fixture.remote_tags(),
        ["release-2026-11-12", "release-2026-11-13"]
    );
    assert!(stand.ran("published.log").is_empty());
    assert!(stand.ran("path.log").is_empty());
    // On, with both stand-ins, a refresh under --no-push publishes nothing, as a run under it
    // pushes nothing.
    let out = run(
        &fixture,
        NOW + DAY + 60.0,
        &["--refresh", "--no-push"],
        &stand.both(),
        0,
    );
    assert!(
        out.contains("release: crates.io: not published: --no-push publishes nothing\n"),
        "{out}"
    );
    assert!(stand.ran("published.log").is_empty());
    assert!(stand.ran("path.log").is_empty());
}

#[test]
fn a_release_publishes_what_the_registry_lacks_with_one_command_and_a_refresh_finishes_it() {
    let fixture = Fixture::new();
    fixture.write("symphony.toml", &Fixture::project_file("publish = true\n"));
    workspace(&fixture);
    let commit = fixture.commit("a workspace that publishes");
    super::git(&fixture.main, &["push", "-q", "origin", "main"]);
    let stand = Stand::new(&fixture);
    fs::write(stand.folder.join("fail-bravo"), "").unwrap();
    let out = run(&fixture, NOW, &["--check", CHECK], &stand.both(), 1);
    let log = fixture
        .records()
        .join("logs")
        .join(fixture.runs()[0]["id"].as_str().unwrap())
        .join("publish.log");
    for said in [
        "release: green; tagged release-2026-11-12, pushed to origin\n".to_string(),
        "release: crates.io: on crates.io already: alpha 1.0.0\n".into(),
        format!(
            "release: crates.io: not published: cargo publish failed (error: failed to verify \
             package bravo; log {}): bravo 1.0.0, charlie 1.0.0\n",
            log.display()
        ),
        "release: crates.io: the release stays tagged; `just release --refresh`, or the next \
         release, publishes what is left\n"
            .into(),
    ] {
        assert!(out.contains(&said), "{said}\n\n{out}");
    }
    assert_eq!(
        fixture.remote_tags(),
        ["release-2026-11-12"],
        "the release stands"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), commit);
    assert!(
        fixture.message("release-2026-11-12").contains(&format!(
            "\n\nPublishes to the registry {}: bravo 1.0.0, charlie 1.0.0.\n",
            stand.registry.display()
        )),
        "{}",
        fixture.message("release-2026-11-12")
    );
    let run_record = &fixture.runs()[0];
    assert_eq!(run_record["result"], "green");
    assert_eq!(run_record["published"]["failed"], true);
    assert_eq!(
        run_record["published"]["skipped"],
        serde_json::json!(["alpha 1.0.0"])
    );
    assert_eq!(
        stand.ran("published.log"),
        ["publish --locked -p bravo -p charlie"],
        "one command, naming what the registry lacks: bravo failing, nothing went up"
    );
    let status = run(&fixture, NOW, &["--status"], &[], 0);
    assert!(
        status.contains("last publish: red, of release-2026-11-12 by the run "),
        "{status}"
    );

    // The upload goes through now: `--refresh` publishes what is left, skipping what is there.
    fs::remove_file(stand.folder.join("fail-bravo")).unwrap();
    let out = run(&fixture, NOW + 60.0, &["--refresh"], &stand.both(), 0);
    assert!(
        out.contains(
            "release: crates.io: published bravo 1.0.0, charlie 1.0.0\n\
             release: crates.io: on crates.io already: alpha 1.0.0\n"
        ),
        "{out}"
    );
    assert_eq!(
        stand.ran("published.log"),
        [
            "publish --locked -p bravo -p charlie",
            "publish --locked -p bravo -p charlie"
        ]
    );
    assert!(
        stand.ran("path.log").is_empty(),
        "the cargo on the PATH publishes nothing"
    );
    let status = run(&fixture, NOW + 60.0, &["--status"], &[], 0);
    assert!(
        status.contains("last publish: of release-2026-11-12 by the run ")
            && status.contains("\n  published bravo 1.0.0, charlie 1.0.0\n"),
        "{status}"
    );
}
