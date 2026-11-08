//! The real process boundary: overlapping conversions, complete publication, failure and timeout.

use std::time::{Duration, Instant};
use symphony_board::sys::Command;

use crate::test_tree::Tree;

use super::convert;

fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/bash");
    command.args(["-c", script, "converter"]);
    command
}

#[test]
fn overlapping_conversions_publish_complete_outputs_from_distinct_files() {
    let tree = Tree::new();
    let dest = tree.write("thumb.jpg", "previous complete image");
    let release = tree.path("release");
    let script = r#"
        part="$3"
        printf partial > "$part"
        printf ready > "$part.ready"
        while [ ! -f "$1" ]; do sleep 0.01; done
        printf complete > "$part"
    "#;
    std::thread::scope(|scope| {
        let start = || {
            let mut command = shell(script);
            command.arg(&release);
            convert(command, &dest, Duration::from_secs(10))
        };
        let first = scope.spawn(start);
        let second = scope.spawn(start);
        let deadline = Instant::now() + Duration::from_secs(5);
        let ready = loop {
            let ready: Vec<String> = tree
                .names("")
                .into_iter()
                .filter(|name| name.ends_with(".ready"))
                .collect();
            if ready.len() == 2 || Instant::now() >= deadline {
                break ready;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // Let the children finish even if an assertion below fails.
        let before = tree.read("thumb.jpg");
        tree.write("release", "go");
        let results = (first.join().unwrap(), second.join().unwrap());
        assert_eq!(
            ready.len(),
            2,
            "each call must have its own part file: {ready:?}"
        );
        assert_eq!(
            before, "previous complete image",
            "no partial output is published"
        );
        assert_eq!(results, (true, true));
    });
    assert_eq!(tree.read("thumb.jpg"), "complete");
    assert!(tree.names("").iter().all(|name| !name.ends_with(".part")));
}

#[test]
fn a_failed_missing_or_empty_converter_preserves_the_previous_thumbnail() {
    let tree = Tree::new();
    let dest = tree.write("thumb.jpg", "previous");
    for command in [
        shell("printf partial > \"$2\"; exit 1"),
        Command::new(tree.path("missing-converter")),
        shell("exit 0"),
    ] {
        assert!(!convert(command, &dest, Duration::from_secs(1)));
        assert_eq!(tree.read("thumb.jpg"), "previous");
        assert_eq!(tree.names(""), ["thumb.jpg"]);
    }
    assert!(!convert(
        shell("exit 0"),
        &tree.path("missing/thumb.jpg"),
        Duration::from_secs(1)
    ));
    let directory = tree.path("directory.jpg");
    std::fs::create_dir(&directory).unwrap();
    assert!(!convert(
        shell("printf complete > \"$2\""),
        &directory,
        Duration::from_secs(1)
    ));
    assert!(directory.is_dir());
    assert!(tree.names("").iter().all(|name| !name.ends_with(".part")));
}

#[test]
fn the_system_converter_receives_sips_arguments_and_publishes_its_output() {
    use std::os::unix::fs::PermissionsExt;

    const CASE: &str = "SF_SYMPHONY_SIPS_CASE";
    if let Some(root) = std::env::var_os(CASE) {
        let root = std::path::PathBuf::from(root);
        assert!(super::sips_available());
        // The `sips` found is the script written below, and macOS holds a new file's first exec
        // until syspolicyd has assessed it (seconds under load, past 30 s at a load of 100 to 150),
        // after the spawn has returned. The production budget would race that; this waits for the
        // converter's own exit.
        assert!(super::sips_convert_within(
            &root.join("source.png"),
            &root.join("thumb.jpg"),
            540,
            Duration::MAX
        ));
        return;
    }
    let tree = Tree::new();
    tree.write("source.png", "source bytes");
    let script = tree.write(
        "sips",
        "#!/bin/bash\nprintf '%s\\n' \"$@\" > \"$SF_SYMPHONY_SIPS_ARGS\"\nprintf jpeg > \"${!#}\"\n",
    );
    std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let done = symphony_board::sys::output(Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "images::conversion_tests::the_system_converter_receives_sips_arguments_and_publishes_its_output"])
        .env(CASE, &tree.root)
        .env("PATH", &tree.root)
        .env("SF_SYMPHONY_SIPS_ARGS", tree.path("args")))
        .unwrap();
    assert!(
        done.status.success(),
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    let args = tree.read("args");
    let args: Vec<_> = args.lines().collect();
    assert_eq!(
        &args[..8],
        [
            "-s",
            "format",
            "jpeg",
            "-s",
            "formatOptions",
            "70",
            "--resampleWidth",
            "540"
        ]
    );
    assert_eq!(args[8], tree.path("source.png").to_str().unwrap());
    assert_eq!(args[9], "--out");
    assert!(args[10].ends_with(".part"));
    assert_eq!(tree.read("thumb.jpg"), "jpeg");
    assert!(tree.names("").iter().all(|name| !name.ends_with(".part")));
}

#[test]
fn a_timed_out_converter_is_reaped_and_its_partial_file_is_removed() {
    let tree = Tree::new();
    let dest = tree.write("thumb.jpg", "previous");
    let mut command = shell("printf partial > \"$3\"; printf %s \"$$\" > \"$1\"; exec sleep 30");
    command.arg(tree.path("pid"));
    let start = Instant::now();
    assert!(!convert(command, &dest, Duration::from_secs(1)));
    assert!(start.elapsed() < Duration::from_secs(10));
    let pid: i32 = tree.read("pid").parse().unwrap();
    // SAFETY: kill with signal 0 checks only the child PID written by this command.
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "the timed out child must be reaped"
    );
    assert_eq!(tree.read("thumb.jpg"), "previous");
    assert_eq!(tree.names(""), ["pid", "thumb.jpg"]);
}
