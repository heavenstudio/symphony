use super::*;
use crate::test_tree::Tree;

/// A checkout with `budget` as its budget file, the client files `client` (path, bytes) and the
/// release binaries `built` (name, bytes) in `release/`.
fn checkout(budget: &str, client: &[(&str, usize)], built: &[(&str, usize)]) -> Tree {
    let tree = Tree::new();
    tree.write(BUDGET, budget);
    for (path, bytes) in client {
        tree.write(&format!("{CLIENT}/{path}"), &"c".repeat(*bytes));
    }
    for (name, bytes) in built {
        tree.write(&format!("release/{name}"), &"b".repeat(*bytes));
    }
    tree
}

/// `symphony size-budget ROOT ROOT/release` on `tree`: its exit code, stdout and stderr.
fn run(tree: &Tree) -> (i32, String, String) {
    let args = [
        tree.root.to_str().unwrap().to_string(),
        tree.path("release").to_str().unwrap().to_string(),
    ];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

const BUDGET_OF_TWO: &str = "client = 100\n\n[binaries]\ntasks = 50\nsymphony = 40\n";

#[test]
fn every_line_within_its_budget_prints_each_size_beside_it_and_exits_zero() {
    let tree = checkout(
        BUDGET_OF_TWO,
        &[("index.html", 30), ("assets/a.wasm", 70)],
        &[("tasks", 50), ("symphony", 1)],
    );
    let (code, out, err) = run(&tree);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(
        out,
        "size-budget: symphony         0.0 MiB of 0.0 MiB\n\
         size-budget: tasks            0.0 MiB of 0.0 MiB\n\
         size-budget: embedded client  0.0 MiB of 0.0 MiB (2 files)\n\
         size-budget: every shipped binary and the embedded client within tools/size-budget.toml\n"
    );
}

#[test]
fn a_binary_a_byte_past_its_budget_says_its_size_the_budget_and_how_to_see_what_grew() {
    let tree = checkout(
        BUDGET_OF_TWO,
        &[("index.html", 10)],
        &[("tasks", 51), ("symphony", 40)],
    );
    let (code, out, err) = run(&tree);
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(
        err,
        "size-budget: tasks is 0.0 MiB (51 bytes), 0.0 MiB past its budget of 0.0 MiB (50 bytes) \
         in tools/size-budget.toml. See what grew: `cargo bloat --release --crates --bin tasks` \
         (`cargo install cargo-bloat --root DIR`), beside the same at the last release \
         (`git tag -l 'release-*'`); a budget is raised only in the commit that needs it, saying \
         why.\n"
    );
    // The lines within their budget still print; the closing line does not.
    assert!(out.contains("size-budget: symphony "), "{out}");
    assert!(out.contains("size-budget: embedded client "), "{out}");
    assert!(!out.contains("every shipped binary"), "{out}");
}

#[test]
fn the_embedded_client_past_its_budget_counts_every_file_under_the_distribution() {
    let tree = checkout(
        BUDGET_OF_TWO,
        &[
            ("index.html", 40),
            ("assets/one.wasm", 40),
            ("assets/deeper/two.js", 21),
        ],
        &[("tasks", 1), ("symphony", 1)],
    );
    let (code, _, err) = run(&tree);
    assert_eq!(code, 1, "{err}");
    assert_eq!(
        err,
        "size-budget: the embedded client (apps/symphony-server/client/public, 3 files) is 0.0 MiB \
         (101 bytes), 0.0 MiB past its budget of 0.0 MiB (100 bytes) in tools/size-budget.toml. \
         See what it holds: `ls -lS apps/symphony-server/client/public/assets`; every generation \
         `symphony publish-client` keeps there is embedded in the server.\n"
    );
}

#[test]
fn the_servers_line_is_its_file_less_the_client_it_embeds() {
    let budget = "client = 100\n\n[binaries]\nsymphony-server = 50\n";
    // 130 bytes of file holding 90 of client: 40 of its own, within 50.
    let tree = checkout(budget, &[("index.html", 90)], &[("symphony-server", 130)]);
    let (code, out, err) = run(&tree);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.starts_with(
            "size-budget: symphony-server  0.0 MiB of 0.0 MiB (0.0 MiB with the embedded client)\n"
        ),
        "{out}"
    );
    // The same client in a file of 141: 51 of its own, past 50, though the file's 141 is within
    // the two lines' 150; and the client past its line is said on the client's line alone.
    let tree = checkout(budget, &[("index.html", 90)], &[("symphony-server", 141)]);
    let (code, _, err) = run(&tree);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "size-budget: symphony-server, less its embedded client (its file is 141 bytes), is 0.0 \
         MiB (51 bytes), 0.0 MiB past its budget of 0.0 MiB (50 bytes) in \
         tools/size-budget.toml. See what grew: `cargo bloat --release --crates --bin \
         symphony-server` (`cargo install cargo-bloat --root DIR`), beside the same at the last \
         release (`git tag -l 'release-*'`); a budget is raised only in the commit that needs \
         it, saying why.\n"
    );
    let tree = checkout(budget, &[("index.html", 101)], &[("symphony-server", 140)]);
    let (code, out, err) = run(&tree);
    assert_eq!(code, 1);
    assert!(out.contains("size-budget: symphony-server "), "{out}");
    assert!(
        err.starts_with("size-budget: the embedded client ") && err.lines().count() == 1,
        "{err}"
    );
    // What the lines count, and a file smaller than the client it is said to embed counts nothing
    // of its own.
    for (file, own) in [(141, 51), (90, 0), (10, 0)] {
        let tree = checkout(budget, &[("index.html", 90)], &[("symphony-server", file)]);
        assert_eq!(
            measure(&tree.root, &tree.path("release")).unwrap(),
            [
                Measured {
                    what: What::Binary {
                        name: "symphony-server".into(),
                        whole: Some(file as u64)
                    },
                    bytes: own,
                    budget: 50
                },
                Measured {
                    what: What::Client { files: 1 },
                    bytes: 90,
                    budget: 100
                }
            ]
        );
    }
}

#[test]
fn every_line_past_its_budget_is_said_not_only_the_first() {
    let tree = checkout(
        BUDGET_OF_TWO,
        &[("index.html", 101)],
        &[("tasks", 51), ("symphony", 41)],
    );
    let (code, out, err) = run(&tree);
    assert_eq!(code, 1);
    assert_eq!(out, "");
    let said: Vec<&str> = err
        .lines()
        .map(|line| line.split_whitespace().nth(1).unwrap())
        .collect();
    assert_eq!(said, ["symphony", "tasks", "the"], "{err}");
}

#[test]
fn a_checkout_with_no_client_distribution_embeds_none() {
    let tree = checkout(BUDGET_OF_TWO, &[], &[("tasks", 1), ("symphony", 1)]);
    let (code, out, err) = run(&tree);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(
        out.contains("embedded client  0.0 MiB of 0.0 MiB (0 files)"),
        "{out}"
    );
}

#[test]
fn a_binary_not_built_or_a_budget_it_cannot_read_is_an_error_not_a_verdict() {
    let tree = checkout(BUDGET_OF_TWO, &[], &[("tasks", 1)]);
    let (code, out, err) = run(&tree);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(
        err.starts_with("symphony size-budget: symphony: ")
            && err.ends_with("(`just size-check` builds every binary first)\n"),
        "{err}"
    );
    for (budget, said) in [
        ("client = 1\n", "missing field `binaries`"),
        ("client = 1\n[binaries]\n", "[binaries] names no binary"),
        ("[binaries]\ntasks = 1\n", "missing field `client`"),
        (
            "client = 1\nserver = 2\n[binaries]\ntasks = 1\n",
            "unknown field `server`",
        ),
        ("client = -1\n[binaries]\ntasks = 1\n", "invalid value"),
        ("client = 1\n[binaries]\ntasks = \"big\"\n", "invalid type"),
    ] {
        let tree = checkout(budget, &[], &[("tasks", 1)]);
        let (code, out, err) = run(&tree);
        assert_eq!((code, out.as_str()), (2, ""), "{budget}");
        assert!(
            err.starts_with("symphony size-budget: tools/size-budget.toml: ") && err.contains(said),
            "{budget}: {err}"
        );
    }
    let tree = Tree::new();
    tree.write("release/tasks", "b");
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony size-budget: tools/size-budget.toml: "),
        "{err}"
    );
}

#[test]
fn a_link_in_the_client_or_a_folder_in_a_binarys_place_is_an_error() {
    let tree = checkout(
        BUDGET_OF_TWO,
        &[("index.html", 1), ("assets/a.js", 1)],
        &[("tasks", 1)],
    );
    fs::create_dir_all(tree.path("release/symphony")).unwrap();
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony size-budget: symphony: ") && err.ends_with(" is no file\n"),
        "{err}"
    );
    fs::remove_dir(tree.path("release/symphony")).unwrap();
    tree.write("release/symphony", "b");
    assert_eq!(run(&tree).0, 0, "the folder's own files");
    std::os::unix::fs::symlink(
        tree.path("release/tasks"),
        tree.path(&format!("{CLIENT}/assets/link.js")),
    )
    .unwrap();
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony size-budget: apps/symphony-server/client/public: ")
            && err.ends_with("link.js: client assets must be regular files\n"),
        "{err}"
    );
    // A file where the distribution's folder goes is no distribution either.
    let tree = checkout(BUDGET_OF_TWO, &[], &[("tasks", 1), ("symphony", 1)]);
    tree.write(CLIENT, "not a folder");
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "symphony size-budget: apps/symphony-server/client/public: the client distribution is no \
         folder\n"
    );
}

#[test]
fn the_wrong_number_of_arguments_is_the_usage() {
    for args in [
        vec![],
        vec!["root".to_string()],
        vec!["a".into(), "b".into(), "c".into()],
    ] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(main(&args, &mut out, &mut err), 2);
        assert_eq!(
            (out.as_slice(), String::from_utf8(err).unwrap().as_str()),
            (&b""[..], "usage: symphony size-budget ROOT RELEASE\n")
        );
    }
}

#[test]
fn sizes_read_in_mib_to_a_tenth_and_bytes_with_their_thousands_apart() {
    assert_eq!(mib(0), "0.0 MiB");
    assert_eq!(mib((1 << 20) - 1), "1.0 MiB");
    assert_eq!(mib(1 << 20), "1.0 MiB");
    assert_eq!(mib(3 * (1 << 20) / 2), "1.5 MiB");
    assert_eq!(mib(275_193_028), "262.4 MiB");
    assert_eq!(mib(u64::MAX), "17592186044416.0 MiB");
    assert_eq!(grouped(0), "0");
    assert_eq!(grouped(999), "999");
    assert_eq!(grouped(1_000), "1,000");
    assert_eq!(grouped(31_234_567), "31,234,567");
    assert_eq!(grouped(u64::MAX), "18,446,744,073,709,551,615");
}
