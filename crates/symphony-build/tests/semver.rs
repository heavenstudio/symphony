//! The project-local semver tool launcher: pinned downloads, reuse, refusal and argv forwarding.
//! Refusal tests use owned archives. The real checker uses the project-cached tool and tiny
//! offline API fixtures, without changing any workspace API or installing a tool on PATH.

use std::path::{Path, PathBuf};
use std::process::Output;

use symphony::children::{command, output};

#[path = "../../../tools/test-support/script_board.rs"]
mod fixture_script;
#[path = "../../symphony/tests/common/tree.rs"]
mod tree;

fn launcher() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/semver.sh")
}

fn bash(script: &str, args: &[&Path]) -> Output {
    let mut cmd = command("bash");
    cmd.args(["-c", script, "semver-fixture"]).arg(launcher());
    cmd.args(args);
    output(&mut cmd).unwrap()
}

fn text(done: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    )
}

/// An archive containing a stand-in checker, and its SHA-256 (computed by the real launcher).
fn archive(tree: &tree::Tree, body: &str) -> (PathBuf, String) {
    let path = tree.path("input/cargo-semver-checks");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Pack an owned real file rather than a link to the shared fixture script.
    tree.write("input/cargo-semver-checks", body);
    let archive = tree.path("tool.tar.gz");
    let packed = output(
        command("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(tree.path("input"))
            .arg("cargo-semver-checks"),
    )
    .unwrap();
    assert!(packed.status.success(), "{}", text(&packed));
    let hash = bash("source \"$1\"; sha256 \"$2\"", &[&archive]);
    assert!(hash.status.success(), "{}", text(&hash));
    (
        archive,
        String::from_utf8(hash.stdout).unwrap().trim().into(),
    )
}

#[test]
fn platform_aliases_select_four_pinned_archives_and_unknown_hosts_fail_closed() {
    let done = bash(
        r#"source "$1"
for os in Darwin Linux; do
    for arch in arm64 aarch64 x86_64 amd64; do
        name="$(platform_name "$os" "$arch")"
        hash="$(pinned "$name")"
        printf '%s %s\n' "$name" "$hash"
    done
done
"#,
        &[],
    );
    assert!(done.status.success(), "{}", text(&done));
    let said = String::from_utf8(done.stdout).unwrap();
    assert_eq!(said.lines().count(), 8);
    for line in said.lines() {
        let (name, hash) = line.split_once(' ').unwrap();
        assert!(name.ends_with("apple-darwin") || name.ends_with("unknown-linux-gnu"));
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    }
    for (os, arch) in [("Windows", "arm64"), ("Darwin", "riscv64")] {
        let done = bash(
            "source \"$1\"; platform_name \"$2\" \"$3\"",
            &[Path::new(os), Path::new(arch)],
        );
        assert_eq!(done.status.code(), Some(2));
        assert!(text(&done).contains("no cargo-semver-checks 0.51.0 build pinned"));
    }
}

#[test]
fn verified_archive_is_published_atomically_reused_and_scratch_is_removed() {
    let tree = tree::Tree::new();
    let (archive, hash) = archive(&tree, "#!/bin/sh\nexit 0\n");
    let cache = tree.path("cache with spaces");
    let done = bash(
        r#"source "$1"
archive="$2"; hash="$3"; cache="$4"
fetch() { cp "$archive" "$2"; }
pinned() { echo "$hash"; }
first="$(ensure "$cache" fixture)"
fetch() { echo 'cache unexpectedly fetched' >&2; return 22; }
second="$(ensure "$cache" fixture)"
[ "$first" = "$second" ]
[ -x "$first" ]
printf '%s\n' "$first"
"#,
        &[&archive, Path::new(&hash), &cache],
    );
    assert!(done.status.success(), "{}", text(&done));
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        format!("{}/0.51.0/fixture/cargo-semver-checks\n", cache.display())
    );
    assert_eq!(
        tree.names("cache with spaces/0.51.0/fixture"),
        ["cargo-semver-checks"]
    );
}

#[test]
fn concurrent_fetches_publish_one_complete_executable_without_shared_scratch() {
    let tree = tree::Tree::new();
    let (archive, hash) = archive(&tree, "#!/bin/sh\nexit 0\n");
    let cache = tree.path("concurrent");
    let run = || {
        bash(
            r#"source "$1"
archive="$2"; hash="$3"
fetch() { cp "$archive" "$2"; }
pinned() { echo "$hash"; }
ensure "$4" fixture
"#,
            &[&archive, Path::new(&hash), &cache],
        )
    };
    std::thread::scope(|scope| {
        let one = scope.spawn(run);
        let two = scope.spawn(run);
        let one = one.join().unwrap();
        let two = two.join().unwrap();
        assert!(one.status.success(), "{}", text(&one));
        assert!(two.status.success(), "{}", text(&two));
        assert_eq!(one.stdout, two.stdout);
    });
    assert_eq!(
        tree.names("concurrent/0.51.0/fixture"),
        ["cargo-semver-checks"]
    );
    assert_eq!(
        tree.read("concurrent/0.51.0/fixture/cargo-semver-checks"),
        "#!/bin/sh\nexit 0\n"
    );
}

#[test]
fn failed_download_bad_checksum_and_bad_archive_never_leave_an_executable() {
    let tree = tree::Tree::new();
    let (archive, hash) = archive(&tree, "#!/bin/sh\nexit 0\n");
    for failure in ["download", "checksum", "archive", "missing", "symlink"] {
        let cache = tree.path(failure);
        let done = bash(
            r#"source "$1"
archive="$2"; hash="$3"; cache="$4"; failure="$5"
fetch() {
    case "$failure" in
    download) return 22 ;;
    archive) echo 'not a tarball' > "$2" ;;
    missing) tar -czf "$2" -T /dev/null ;;
    symlink)
        mkdir "$cache/link"
        ln -s /bin/sh "$cache/link/cargo-semver-checks"
        tar -czf "$2" -C "$cache/link" cargo-semver-checks
        rm -f "$cache/link/cargo-semver-checks"
        rmdir "$cache/link"
        ;;
    *) cp "$archive" "$2" ;;
    esac
    if [ "$failure" = archive ] || [ "$failure" = missing ] || [ "$failure" = symlink ]; then
        sha256 "$2" > "$cache/hash"
    fi
}
pinned() {
    case "$failure" in
    checksum) echo wrong ;;
    archive | missing | symlink) cat "$cache/hash" ;;
    *) echo "$hash" ;;
    esac
}
ensure "$cache" fixture
"#,
            &[&archive, Path::new(&hash), &cache, Path::new(failure)],
        );
        assert_eq!(done.status.code(), Some(2), "{failure}: {}", text(&done));
        let expected = match failure {
            "download" => "allow HTTPS to GitHub and retry",
            "checksum" => "not the pinned wrong",
            _ => "holds no cargo-semver-checks executable",
        };
        assert!(text(&done).contains(expected), "{failure}: {}", text(&done));
        assert!(tree.names(&format!("{failure}/0.51.0/fixture")).is_empty());
    }
}

#[test]
fn an_unwritable_cache_fails_and_both_checksum_utilities_agree() {
    let tree = tree::Tree::new();
    let file = tree.write("file", "abc");
    let refused = bash("source \"$1\"; ensure \"$2\" fixture", &[&file]);
    assert_eq!(refused.status.code(), Some(2), "{}", text(&refused));
    for available in ["yes", "no"] {
        let hashed = bash(
            r#"source "$1"
available="$3"
command() {
    if [ "$1" = -v ] && [ "$2" = sha256sum ] && [ "$available" = no ]; then return 1; fi
    builtin command "$@"
}
sha256 "$2"
"#,
            &[&file, Path::new(available)],
        );
        assert!(hashed.status.success(), "{}", text(&hashed));
        assert_eq!(
            String::from_utf8(hashed.stdout).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
        );
    }
}

#[test]
fn launcher_resolves_relative_target_and_forwards_each_argument_and_the_exit_status() {
    let mut tree = tree::Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let platform = bash(
        "source \"$1\"; platform_name \"$(uname -s)\" \"$(uname -m)\"",
        &[],
    );
    assert!(platform.status.success(), "{}", text(&platform));
    let platform = String::from_utf8(platform.stdout).unwrap();
    let binary = tree.path(&format!(
        "build with spaces/tools/cargo-semver-checks/0.51.0/{}/cargo-semver-checks",
        platform.trim()
    ));
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    fixture_script::write(
        &binary,
        "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\nexit 7\n",
        false,
    );
    let invoke = |args: &[&str]| {
        output(
            command("bash")
                .arg(launcher())
                .args(args)
                .current_dir(&tree.root)
                .env("CARGO_TARGET_DIR", "build with spaces"),
        )
        .unwrap()
    };
    let found = invoke(&[]);
    assert!(found.status.success(), "{}", text(&found));
    assert_eq!(
        String::from_utf8(found.stdout).unwrap().trim(),
        binary.to_str().unwrap()
    );
    let done = invoke(&["--package", "name with spaces", "--all-features"]);
    assert_eq!(done.status.code(), Some(7));
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        "<semver-checks>\n<--package>\n<name with spaces>\n<--all-features>\n"
    );
}

/// A stand-in checker that counts its runs in `runs` and says `body` after.
fn counting(tree: &tree::Tree, body: &str) -> (PathBuf, PathBuf) {
    let (binary, runs) = (tree.path("checker"), tree.path("runs"));
    fixture_script::write(
        &binary,
        &format!(
            "#!/bin/sh\necho run >> '{0}'\nn=$(wc -l < '{0}' | tr -d ' ')\necho \"out $n\"\n{body}\n",
            runs.display()
        ),
        false,
    );
    (binary, runs)
}

#[test]
fn a_check_the_registrys_index_did_not_answer_is_run_again_and_any_other_failure_is_not() {
    const UNREAD: &str = "error: failed to retrieve index of crate versions from registry";
    let run = |body: &str| {
        let tree = tree::Tree::new();
        let (binary, runs) = counting(&tree, body);
        let said = tree.path("said");
        let done = bash(
            r#"source "$1"; checked "$2" "$3" semver-checks -p one"#,
            &[&said, &binary],
        );
        assert!(!said.exists(), "what the checker said is removed");
        let runs = std::fs::read_to_string(runs).unwrap().lines().count();
        (
            done.status.code(),
            runs,
            String::from_utf8(done.stdout).unwrap(),
            String::from_utf8(done.stderr).unwrap(),
        )
    };
    // The index unread once, then a check that passes: green, each run's output kept.
    let (status, runs, out, err) = run(&format!(
        "[ \"$n\" -gt 1 ] && exit 0\necho '{UNREAD}' >&2\nexit 101"
    ));
    assert_eq!((status, runs, out.as_str()), (Some(0), 2, "out 1\nout 2\n"));
    assert_eq!(
        err,
        format!(
            "{UNREAD}\nsemver: crates.io's index did not answer run 1 of 3; running the check \
             again\n"
        )
    );
    // Unread every time: three runs, and the checker's own status.
    let (status, runs, _, err) = run(&format!("echo '{UNREAD}' >&2\nexit 101"));
    assert_eq!((status, runs), (Some(101), 3));
    assert_eq!(err.matches("running the check again").count(), 2, "{err}");
    // A finding, or any other failure, is the answer at once, and so is a pass.
    let (status, runs, out, err) = run("echo 'error: a finding' >&2\nexit 7");
    assert_eq!(
        (status, runs, out.as_str(), err.as_str()),
        (Some(7), 1, "out 1\n", "error: a finding\n")
    );
    assert_eq!(run("exit 0").0, Some(0));
    assert_eq!(run("exit 0").1, 1);
}

/// Real rustdoc comparisons, offline: independent versions of a tiny API, never a mutation of
/// the workspace. These also document why the compile guards still hold arbitrary types.
#[test]
fn real_checker_requires_the_right_version_move_and_leaves_type_changes_to_compile_guards() {
    let tree = tree::Tree::new();
    let found = output(command("bash").arg(launcher())).unwrap();
    assert!(found.status.success(), "{}", text(&found));
    let checker = String::from_utf8(found.stdout).unwrap();
    let baseline = "pub struct Request;\nimpl Request { pub fn new(value: u8) -> Self { Self } }\n\
                    #[non_exhaustive] pub enum Word { First }\n";
    let cases = [
        (
            "unchanged",
            "1.0.0",
            baseline,
            true,
            "no semver update required",
        ),
        ("removed", "1.0.1", "", false, "struct_missing"),
        ("minor_break", "1.1.0", "", false, "struct_missing"),
        ("major", "2.0.0", "", true, "major"),
        (
            "constructor",
            "1.1.0",
            "pub struct Request; impl Request { pub fn new(value: u8, later: bool) -> Self { Self } }\n\
             #[non_exhaustive] pub enum Word { First }\n",
            false,
            "method_parameter_count_changed",
        ),
        (
            "additive",
            "1.1.0",
            "pub struct Request; impl Request { pub fn new(value: u8) -> Self { Self } }\n\
             #[non_exhaustive] pub enum Word { First, Second }\n",
            true,
            "Summary",
        ),
        (
            "additive_patch",
            "1.0.1",
            "pub struct Request; impl Request { pub fn new(value: u8) -> Self { Self } }\n\
             #[non_exhaustive] pub enum Word { First, Second }\n",
            true,
            "no semver update required",
        ),
        (
            "types",
            "1.0.1",
            "pub struct Request; impl Request { pub fn new(value: &str) -> Self { Self } }\n\
             #[non_exhaustive] pub enum Word { First }\n",
            true,
            "no semver update required",
        ),
    ];
    for (name, version, source, green, diagnostic) in cases {
        for (side, version, source) in [("old", "1.0.0", baseline), ("new", version, source)] {
            tree.write(
                &format!("{name}/{side}/Cargo.toml"),
                &format!(
                    "[workspace]\n[package]\nname = \"semver-fixture\"\nversion = \"{version}\"\nedition = \"2024\"\n"
                ),
            );
            tree.write(&format!("{name}/{side}/src/lib.rs"), source);
        }
        let done = output(
            command(checker.trim())
                .arg("semver-checks")
                .arg("--manifest-path")
                .arg(tree.path(&format!("{name}/new/Cargo.toml")))
                .arg("--baseline-root")
                .arg(tree.path(&format!("{name}/old")))
                .arg("--all-features")
                .env("CARGO_NET_OFFLINE", "true")
                .env("CARGO_TARGET_DIR", tree.path("target")),
        )
        .unwrap();
        assert_eq!(done.status.success(), green, "{name}: {}", text(&done));
        assert!(text(&done).contains(diagnostic), "{name}: {}", text(&done));
    }
}
