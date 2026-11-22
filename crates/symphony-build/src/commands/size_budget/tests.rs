use super::*;
use crate::test_tree::Tree;
use std::time::Duration;

/// A checkout with `budget` as its budget file, the client files `client` (path, bytes) and the
/// release binaries `built` (name, bytes) in `release/`, written after the client.
fn checkout(budget: &str, client: &[(&str, usize)], built: &[(&str, usize)]) -> Tree {
    let tree = Tree::new();
    fill(&tree, budget, client, built);
    tree
}

/// A checkout as [`checkout`] makes it whose client's `builds.json` names `builds`, newest first:
/// each the names of its files under `assets/`, its shell first.
fn kept_checkout(
    budget: &str,
    builds: &[&[&str]],
    client: &[(&str, usize)],
    built: &[(&str, usize)],
) -> Tree {
    let tree = Tree::new();
    tree.write(&format!("{CLIENT}/{KEPT}"), &builds_json(builds));
    fill(&tree, budget, client, built);
    tree
}

fn fill(tree: &Tree, budget: &str, client: &[(&str, usize)], built: &[(&str, usize)]) {
    tree.write(BUDGET, budget);
    for (path, bytes) in client {
        tree.write(&format!("{CLIENT}/{path}"), &"c".repeat(*bytes));
    }
    for (name, bytes) in built {
        tree.write(&format!("release/{name}"), &"b".repeat(*bytes));
    }
}

/// `builds.json` as `symphony-build publish-client` writes it, naming `builds` newest first.
fn builds_json(builds: &[&[&str]]) -> String {
    let builds: Vec<_> = (builds.iter())
        .map(|files| serde_json::json!({"shell": files.first(), "files": files}))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({"keep": 3, "builds": builds})).unwrap() + "\n"
}

/// A budget file: `client` for the client, and `each` for every shipped binary but those `lines`
/// sets.
fn budget(client: u64, each: u64, lines: &[(&str, u64)]) -> String {
    let mut text = format!("client = {client}\n\n[binaries]\n");
    for name in BUILDS {
        let most = (lines.iter().find(|(line, _)| *line == name)).map_or(each, |(_, most)| *most);
        text.push_str(&format!("{name} = {most}\n"));
    }
    text
}

/// Every shipped binary, `each` bytes long but those `sizes` sets.
fn built(each: usize, sizes: &[(&'static str, usize)]) -> Vec<(&'static str, usize)> {
    (BUILDS.into_iter())
        .map(|name| {
            let bytes = sizes.iter().find(|(size, _)| *size == name);
            (name, bytes.map_or(each, |(_, bytes)| *bytes))
        })
        .collect()
}

/// `symphony-build size-budget ROOT ROOT/release` on `tree`: its exit code, stdout and stderr.
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

/// What the hint past a binary's line says after its name.
const HINT: &str = "` (`cargo install cargo-bloat --root DIR`) beside the same at a commit within \
                    it: the last release (`git tag -l 'release-*'`), or before the first one, the \
                    commit that last set the budget (`git log -1 -- tools/size-budget.toml`). A \
                    reduction takes the bytes back first; a line is raised only in a commit of \
                    its own that names the growth and why no reduction will do, to the size plus \
                    4 % and never higher, on a fresh reviewer's approve (docs/workflow/CHECKS.md, \"A red \
                    at the size check\").\n";

#[test]
fn every_line_within_its_budget_prints_each_size_beside_it_and_exits_zero() {
    // Each binary at its line: a line the rule would put no lower. The client: one build of 70
    // bytes, its page of 30 and its builds.json.
    let page = 30 + builds_json(&[&["a.wasm"]]).len();
    let tree = kept_checkout(
        &budget(1_000, 1, &[("tasks", 50), ("symphony-server", 30)]),
        &[&["a.wasm"]],
        &[("index.html", 30), ("assets/a.wasm", 70)],
        &built(1, &[("tasks", 50), ("symphony-server", 70 + page + 30)]),
    );
    let (code, out, err) = run(&tree);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(
        out,
        format!(
            "size-budget: agent-send       0.0 MiB of 0.0 MiB\n\
             size-budget: symphony         0.0 MiB of 0.0 MiB\n\
             size-budget: symphony-server  0.0 MiB of 0.0 MiB (0.0 MiB with the embedded client)\n\
             size-budget: tasks            0.0 MiB of 0.0 MiB\n\
             size-budget: embedded client  0.0 MiB of 0.0 MiB (1 build of 70 bytes; {} bytes \
             embedded in 3 files)\n\
             size-budget: every shipped binary and the embedded client within \
             tools/size-budget.toml\n",
            70 + page
        )
    );
}

#[test]
fn a_line_is_its_binarys_size_plus_four_percent_rounded_down_to_ten_thousand_bytes() {
    // Sizes of the order of symphony's and of the server's less its client.
    assert_eq!(line_for(17_883_360), 18_590_000);
    assert_eq!(line_for(18_358_694), 19_090_000);
    // Rounded down, never up: 4 % of 250,000 is the rounding's own 10,000.
    assert_eq!(line_for(250_000), 260_000);
    assert_eq!(line_for(259_615), 260_000);
    assert_eq!(line_for(1_000_000), 1_040_000);
    assert_eq!(line_for(1_009_615), 1_040_000);
    assert_eq!(line_for(1_009_616), 1_050_000);
    assert_eq!(line_for(9_999), 10_000);
    // Never below the size: a binary too small for the rounding keeps its own size.
    for bytes in [0, 1, 9_615, 240_000] {
        assert_eq!(line_for(bytes), bytes, "{bytes}");
    }
    assert_eq!(line_for(u64::MAX), u64::MAX);
}

#[test]
fn a_binarys_line_above_where_the_rule_puts_it_says_where_it_comes_down_to() {
    // symphony at 300,000 bytes under a line of 400,000: the rule puts it at 310,000. The server's
    // own 260,000 (its client aside) at 270,000, where the rule puts it: nothing to say. The
    // client's line is never told to come down: a reduction of the newest build reaches what the
    // line counts in full only once the builds before it are no longer kept.
    let tree = checkout(
        &budget(
            1_000_000,
            1,
            &[
                ("symphony", 400_000),
                ("symphony-server", 270_000),
                ("tasks", 1),
            ],
        ),
        &[("index.html", 30)],
        &built(1, &[("symphony", 300_000), ("symphony-server", 260_030)]),
    );
    let (code, out, err) = run(&tree);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[1],
        "size-budget: symphony         0.3 MiB of 0.4 MiB; its line comes down to 310,000 (300,000 \
         bytes plus 4 %)"
    );
    assert_eq!(
        lines[2],
        "size-budget: symphony-server  0.2 MiB of 0.3 MiB (0.2 MiB with the embedded client)"
    );
    assert_eq!(
        lines[4],
        "size-budget: embedded client  0.0 MiB of 1.0 MiB (no build; 30 bytes embedded in 1 files)"
    );
    // A line already under the rule's, as growth leaves it, is never told to go up.
    let tree = checkout(
        &budget(100, 1, &[("symphony", 300_000)]),
        &[],
        &built(1, &[("symphony", 299_000)]),
    );
    let (code, out, _) = run(&tree);
    assert_eq!(code, 0);
    assert!(!out.contains("comes down"), "{out}");
}

#[test]
fn a_binary_a_byte_past_its_budget_says_its_size_the_budget_and_how_to_see_what_grew() {
    let tree = checkout(
        &budget(100, 50, &[]),
        &[("index.html", 10)],
        &built(1, &[("tasks", 51), ("symphony-server", 10)]),
    );
    let (code, out, err) = run(&tree);
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(
        err,
        format!(
            "size-budget: tasks is 0.0 MiB (51 bytes), 0.0 MiB past its budget of 0.0 MiB (50 \
             bytes) in tools/size-budget.toml. See what grew: `cargo bloat --release --crates \
             --bin tasks{HINT}"
        )
    );
    // The lines within their budget still print; the closing line does not.
    for name in [
        "agent-send",
        "symphony",
        "symphony-server",
        "embedded client",
    ] {
        assert!(out.contains(&format!("size-budget: {name} ")), "{out}");
    }
    assert!(!out.contains("every shipped binary"), "{out}");
}

#[test]
fn the_clients_line_counts_each_kept_build_whole_and_the_page_once() {
    // Two builds that share a stylesheet, as a landing that changed none leaves them, the newest
    // naming it twice: 10 + 100 + 20 and 10 + 90 + 20 bytes, and the page beside them.
    let builds: [&[&str]; 2] = [
        &["shell-2.js", "shared-2.js", "kind.css", "kind.css"],
        &["shell-1.js", "shared-1.js", "kind.css"],
    ];
    let client = [
        ("index.html", 5),
        ("assets/shell-2.js", 10),
        ("assets/shared-2.js", 100),
        ("assets/kind.css", 20),
        ("assets/shell-1.js", 10),
        ("assets/shared-1.js", 90),
    ];
    let page = 5 + builds_json(&builds).len();
    let (embedded, counted) = (230 + page, 130 + 120 + page);
    let checked = |line: usize| {
        kept_checkout(
            &budget(line as u64, 50, &[]),
            &builds,
            &client,
            &built(1, &[("symphony-server", embedded + 40)]),
        )
    };
    let tree = checked(10_000);
    let lines = measure(&tree.root, &tree.path("release")).unwrap();
    assert_eq!(
        lines[4],
        Measured {
            what: What::Client {
                builds: vec![130, 120],
                embedded: embedded as u64,
                files: 7
            },
            bytes: counted as u64,
            budget: 10_000
        }
    );
    // The server's own bytes are its file less what it embeds, the shared sheet once.
    assert_eq!(lines[2].bytes, 40);
    let (code, out, err) = run(&tree);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.contains(&format!(
            "size-budget: embedded client  0.0 MiB of 0.0 MiB (2 builds of 130 and 120 bytes, \
             newest first; {} bytes embedded in 7 files)\n",
            grouped(embedded as u64)
        )),
        "{out}"
    );
    // A line a byte under the builds counted whole is past, though what the server embeds is
    // within it.
    assert!(embedded < counted - 1);
    let (code, out, err) = run(&checked(counted - 1));
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(
        err,
        format!(
            "size-budget: the embedded client (apps/symphony-server/client/public: 2 builds of 130 \
             and 120 bytes, newest first) is 0.0 MiB ({} bytes), 0.0 MiB past its budget of 0.0 \
             MiB ({} bytes) in tools/size-budget.toml. Its line counts each build \
             `symphony-build publish-client` keeps whole, a file builds share once for each, and \
             the files beside assets/ once; the server embeds {} bytes in 7 files of them. See \
             what the newest build grew by beside the one before it: the files `jq -r \
             '.builds[0].files[]' apps/symphony-server/client/public/builds.json` names in \
             apps/symphony-server/client/public/assets, beside `.builds[1]`'s. A reduction takes \
             the bytes back first; a line is raised only in a commit of its own that names the \
             growth and why no reduction will do, to the size plus 4 % and never higher, on a \
             fresh reviewer's approve (docs/workflow/CHECKS.md, \"A red at the size check\").\n",
            grouped(counted as u64),
            grouped(counted as u64 - 1),
            grouped(embedded as u64)
        )
    );
}

#[test]
fn what_the_clients_line_counts_does_not_move_with_how_many_files_its_builds_share() {
    // Three builds of a 10-byte shell and a 100-byte shared chunk each: sharing one chunk, as
    // landings that changed no shared module leave them, or each with its own, as three that did
    // leave them, every chunk that imports the shared one renamed with it. The server embeds 200
    // bytes more of the second; the line counts the two alike.
    let shared: [&[&str]; 3] = [
        &["sh-3.js", "s-0.js"],
        &["sh-2.js", "s-0.js"],
        &["sh-1.js", "s-0.js"],
    ];
    let apart: [&[&str]; 3] = [
        &["sh-3.js", "s-3.js"],
        &["sh-2.js", "s-2.js"],
        &["sh-1.js", "s-1.js"],
    ];
    let client = |builds: &[&[&str]]| -> Measured {
        let mut files = vec![("index.html".to_string(), 5)];
        for build in builds {
            files.push((format!("assets/{}", build[0]), 10));
            files.push((format!("assets/{}", build[1]), 100));
        }
        let files: Vec<(&str, usize)> = (files.iter())
            .map(|(path, bytes)| (path.as_str(), *bytes))
            .collect();
        let tree = kept_checkout(
            &budget(10_000, 50, &[]),
            builds,
            &files,
            &built(1, &[("symphony-server", 1_000)]),
        );
        measure(&tree.root, &tree.path("release"))
            .unwrap()
            .remove(4)
    };
    assert_eq!(builds_json(&shared).len(), builds_json(&apart).len());
    let page = 5 + builds_json(&shared).len() as u64;
    let (sharing, not) = (client(&shared), client(&apart));
    assert_eq!((sharing.bytes, not.bytes), (330 + page, 330 + page));
    assert_eq!(
        sharing.what,
        What::Client {
            builds: vec![110; 3],
            embedded: 130 + page,
            files: 6
        }
    );
    assert_eq!(
        not.what,
        What::Client {
            builds: vec![110; 3],
            embedded: 330 + page,
            files: 8
        }
    );
}

#[test]
fn a_client_file_no_kept_build_names_or_a_build_naming_no_file_is_an_error_not_a_pass() {
    // What `size-budget` says of a client of `builds` (none: no builds.json) and `files`.
    let refused = |builds: Option<&[&[&str]]>, files: &[(&str, usize)]| -> String {
        let (budget, built) = (
            budget(10_000, 50, &[]),
            built(1, &[("symphony-server", 1_000)]),
        );
        let tree = match builds {
            Some(builds) => kept_checkout(&budget, builds, files, &built),
            None => checkout(&budget, files, &built),
        };
        let (code, out, err) = run(&tree);
        assert_eq!((code, out.as_str()), (2, ""), "{err}");
        err
    };
    let unnamed = |paths: &str| {
        format!(
            "symphony-build size-budget: apps/symphony-server/client/public: builds.json names no \
             build that holds {paths}: the line counts the builds, so the server would embed what \
             it does not count; `symphony-build publish-client` keeps no file outside its builds \
             (`just client-update` publishes the client again)\n"
        )
    };
    // Files of a build no longer kept, every one named, in order.
    let kept: &[&[&str]] = &[&["sh.js"]];
    assert_eq!(
        refused(
            Some(kept),
            &[
                ("assets/sh.js", 1),
                ("assets/stale.js", 1),
                ("assets/old.css", 1)
            ]
        ),
        unnamed("assets/old.css, assets/stale.js")
    );
    // A file below assets/, which no build's name reaches.
    assert_eq!(
        refused(
            Some(kept),
            &[("assets/sh.js", 1), ("assets/deeper/two.js", 1)]
        ),
        unnamed("assets/deeper/two.js")
    );
    // No builds.json: no build, so every file of assets/ is one none names.
    assert_eq!(
        refused(None, &[("index.html", 1), ("assets/a.js", 1)]),
        unnamed("assets/a.js")
    );
    // A build naming a file that is not there, or one that is no file of assets/.
    for (name, files) in [
        ("gone.js", &[("assets/sh.js", 1)][..]),
        ("../index.html", &[("assets/sh.js", 1), ("index.html", 1)]),
        (
            "deeper/two.js",
            &[("assets/sh.js", 1), ("assets/deeper/two.js", 1)],
        ),
        ("", &[("assets/sh.js", 1)]),
    ] {
        assert_eq!(
            refused(Some(&[&["sh.js", name]]), files),
            format!(
                "symphony-build size-budget: apps/symphony-server/client/public: builds.json: the \
                 build sh.js names {name:?}, which is no file of assets/\n"
            )
        );
    }
    // A builds.json that is not one.
    for (text, said) in [
        ("not json", "expected"),
        ("{\"keep\": 3}", "missing field `builds`"),
        (
            "{\"builds\": [{\"shell\": \"sh.js\"}]}",
            "missing field `files`",
        ),
    ] {
        let tree = checkout(&budget(10_000, 50, &[]), &[], &built(1, &[]));
        tree.write(&format!("{CLIENT}/{KEPT}"), text);
        let (code, out, err) = run(&tree);
        assert_eq!((code, out.as_str()), (2, ""), "{text}");
        assert!(
            err.starts_with(
                "symphony-build size-budget: apps/symphony-server/client/public: builds.json: "
            ) && err.contains(said),
            "{text}: {err}"
        );
    }
}

#[test]
fn the_committed_distribution_holds_no_file_outside_its_kept_builds() {
    let ours = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(CLIENT);
    let client = folder(&ours).unwrap();
    assert!(client.kept() >= client.embedded);
}

#[test]
fn the_kept_builds_weights_read_newest_first() {
    assert_eq!(weighing(&[]), "no build");
    assert_eq!(weighing(&[756_886]), "1 build of 756,886 bytes");
    assert_eq!(weighing(&[2, 1]), "2 builds of 2 and 1 bytes, newest first");
    assert_eq!(
        weighing(&[756_886, 754_713, 755_142]),
        "3 builds of 756,886, 754,713 and 755,142 bytes, newest first"
    );
}

/// The server's line of `out`.
fn servers(out: &str) -> &str {
    (out.lines())
        .find(|line| line.starts_with("size-budget: symphony-server "))
        .unwrap_or_else(|| panic!("no server line: {out}"))
}

#[test]
fn the_servers_line_is_its_file_less_the_client_it_embeds() {
    // 130 bytes of file holding 90 of client: 40 of its own, within 50, and where its line comes
    // down to is of those 40.
    let tree = checkout(
        &budget(100, 50, &[]),
        &[("index.html", 90)],
        &built(1, &[("symphony-server", 130)]),
    );
    let (code, out, err) = run(&tree);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(
        servers(&out),
        "size-budget: symphony-server  0.0 MiB of 0.0 MiB (0.0 MiB with the embedded client); its \
         line comes down to 40 (40 bytes plus 4 %)"
    );
    // The same client in a file of 141: 51 of its own, past 50, though the file's 141 is within
    // the two lines' 150; and the client past its line is said on the client's line alone.
    let tree = checkout(
        &budget(100, 50, &[]),
        &[("index.html", 90)],
        &built(1, &[("symphony-server", 141)]),
    );
    let (code, _, err) = run(&tree);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        format!(
            "size-budget: symphony-server, less its embedded client (its file is 141 bytes), is \
             0.0 MiB (51 bytes), 0.0 MiB past its budget of 0.0 MiB (50 bytes) in \
             tools/size-budget.toml. See what grew: `cargo bloat --release --crates --bin \
             symphony-server{HINT}"
        )
    );
    let tree = checkout(
        &budget(100, 50, &[]),
        &[("index.html", 101)],
        &built(1, &[("symphony-server", 140)]),
    );
    let (code, out, err) = run(&tree);
    assert_eq!(code, 1);
    servers(&out);
    assert!(
        err.starts_with("size-budget: the embedded client ") && err.lines().count() == 1,
        "{err}"
    );
    // What the lines count: a file as big as its client has nothing of its own.
    for (file, own) in [(141, 51), (90, 0)] {
        let tree = checkout(
            &budget(100, 50, &[]),
            &[("index.html", 90)],
            &built(1, &[("symphony-server", file)]),
        );
        let lines = measure(&tree.root, &tree.path("release")).unwrap();
        assert_eq!(
            lines.iter().map(|line| &line.what).collect::<Vec<_>>(),
            [
                &What::Binary {
                    name: "agent-send".into(),
                    whole: None
                },
                &What::Binary {
                    name: "symphony".into(),
                    whole: None
                },
                &What::Binary {
                    name: "symphony-server".into(),
                    whole: Some(file as u64)
                },
                &What::Binary {
                    name: "tasks".into(),
                    whole: None
                },
                &What::Client {
                    builds: vec![],
                    embedded: 90,
                    files: 1
                },
            ]
        );
        assert_eq!((lines[2].bytes, lines[2].budget), (own, 50));
        assert_eq!((lines[4].bytes, lines[4].budget), (90, 100));
    }
}

#[test]
fn a_server_built_from_another_client_than_the_one_beside_it_is_an_error_naming_the_rebuild() {
    // Smaller than the client it is compared with: it cannot embed it.
    let tree = checkout(
        &budget(100, 50, &[]),
        &[("index.html", 90)],
        &built(1, &[("symphony-server", 89)]),
    );
    let (code, out, err) = run(&tree);
    assert_eq!((code, out.as_str()), (2, ""));
    assert_eq!(
        err,
        format!(
            "symphony-build size-budget: symphony-server: {} is 89 bytes, less than the 90 of \
             apps/symphony-server/client/public it embeds, so it embeds another client; rebuild \
             it (`just size-check`)\n",
            tree.path("release/symphony-server").display()
        )
    );
    // Big enough, but built before a client file last changed, as a hand run after
    // `client-update` finds it: its own bytes cannot be told.
    let tree = checkout(
        &budget(100, 50, &[]),
        &[("index.html", 30), ("about.html", 30)],
        &built(1, &[("symphony-server", 100)]),
    );
    let server = tree.path("release/symphony-server");
    let built_at = fs::metadata(&server).unwrap().modified().unwrap();
    let client = tree.path(&format!("{CLIENT}/about.html"));
    let later = fs::File::options().write(true).open(&client).unwrap();
    later
        .set_modified(built_at + Duration::from_secs(1))
        .unwrap();
    let (code, out, err) = run(&tree);
    assert_eq!((code, out.as_str()), (2, ""));
    assert_eq!(
        err,
        format!(
            "symphony-build size-budget: symphony-server: {} was built before \
             apps/symphony-server/client/public last changed, so it embeds another client; \
             rebuild it (`just size-check`)\n",
            server.display()
        )
    );
    // Built after it, as Cargo rebuilds it: within.
    let rebuilt = fs::File::options().write(true).open(&server).unwrap();
    rebuilt
        .set_modified(built_at + Duration::from_secs(2))
        .unwrap();
    assert_eq!(run(&tree).0, 0);
}

#[test]
fn every_line_past_its_budget_is_said_not_only_the_first() {
    let tree = checkout(
        &budget(100, 50, &[]),
        &[("index.html", 101)],
        &built(51, &[("symphony-server", 152)]),
    );
    let (code, out, err) = run(&tree);
    assert_eq!(code, 1);
    assert_eq!(out, "");
    let said: Vec<&str> = err
        .lines()
        .map(|line| line.split_whitespace().nth(1).unwrap())
        .collect();
    assert_eq!(
        said,
        ["agent-send", "symphony", "symphony-server,", "tasks", "the"],
        "{err}"
    );
}

#[test]
fn a_checkout_with_no_client_distribution_embeds_none() {
    let tree = checkout(&budget(100, 50, &[]), &[], &built(1, &[]));
    let (code, out, err) = run(&tree);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(
        out.contains("embedded client  0.0 MiB of 0.0 MiB (no build; 0 bytes embedded in 0 files)"),
        "{out}"
    );
    let lines = measure(&tree.root, &tree.path("release")).unwrap();
    assert_eq!(
        (lines[2].bytes, lines[4].bytes),
        (1, 0),
        "the server's own is its whole file"
    );
}

#[test]
fn every_shipped_binary_has_a_line_and_every_line_is_a_shipped_binarys() {
    // A line gone: the check is of every shipped binary, so it is no pass.
    let without = budget(100, 50, &[]).replace("tasks = 50\n", "");
    let tree = checkout(&without, &[], &built(1, &[]));
    let (code, out, err) = run(&tree);
    assert_eq!((code, out.as_str()), (2, ""));
    assert_eq!(
        err,
        "symphony-build size-budget: tools/size-budget.toml: [binaries] has no line for tasks, which \
         this workspace delivers (`BUILDS` in crates/symphony/src/init.rs): every shipped binary \
         has a budget\n"
    );
    // Every one it lacks is named, in the order the shipped set gives them; an empty table lacks
    // all four.
    let tree = checkout("client = 1\n[binaries]\n", &[], &built(1, &[]));
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert!(
        err.contains("has no line for symphony, tasks, agent-send, symphony-server, which"),
        "{err}"
    );
    // A line for a binary the workspace does not deliver, built or not: a binary added to the
    // budget is added to the shipped set first.
    let extra = format!("{}tasks-search = 10\nshelf = 1\n", budget(100, 50, &[]));
    for release in [built(1, &[]), {
        let mut also = built(1, &[]);
        also.push(("tasks-search", 1));
        also
    }] {
        let tree = checkout(&extra, &[], &release);
        let (code, out, err) = run(&tree);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(
            err,
            "symphony-build size-budget: tools/size-budget.toml: [binaries] names shelf, tasks-search, \
             which this workspace does not deliver (`BUILDS` in crates/symphony/src/init.rs): a \
             line is for a shipped binary\n"
        );
    }
    // The shipped set is the four the workspace builds and the budget file of this checkout
    // holds a line for each of them, and none else.
    let ours = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(BUDGET),
    )
    .unwrap();
    shipped(&toml::from_str(&ours).unwrap()).unwrap();
}

#[test]
fn the_size_check_recipe_builds_every_shipped_binary_and_no_other() {
    let justfile =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../justfile")).unwrap();
    let recipe: Vec<&str> = (justfile.lines())
        .skip_while(|line| *line != "_size-check:")
        .skip(1)
        .take_while(|line| line.starts_with("    "))
        .collect();
    let build = (recipe.iter())
        .find(|line| {
            line.trim_start()
                .starts_with("@tools/cargo.sh build --release")
        })
        .unwrap_or_else(|| panic!("no release build in _size-check: {recipe:?}"));
    let words: Vec<&str> = build.split_whitespace().collect();
    let mut bins: Vec<&str> = (words.windows(2))
        .filter(|pair| pair[0] == "--bin")
        .map(|pair| pair[1])
        .collect();
    bins.sort_unstable();
    let mut shipped = BUILDS.to_vec();
    shipped.sort_unstable();
    assert_eq!(bins, shipped, "{build}");
    assert!(
        recipe
            .iter()
            .any(|line| line.contains("symphony-build size-budget . ")),
        "{recipe:?}"
    );
}

#[test]
fn a_binary_not_built_or_a_budget_it_cannot_read_is_an_error_not_a_verdict() {
    let mut release = built(1, &[]);
    release.retain(|(name, _)| *name != "symphony");
    let tree = checkout(&budget(100, 50, &[]), &[], &release);
    let (code, out, err) = run(&tree);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(
        err.starts_with("symphony-build size-budget: symphony: ")
            && err.ends_with("(`just size-check` builds every binary first)\n"),
        "{err}"
    );
    let lines = budget(100, 50, &[]);
    let lines = lines.trim_start_matches("client = 100\n\n");
    for (text, said) in [
        ("client = 1\n".to_string(), "missing field `binaries`"),
        (lines.to_string(), "missing field `client`"),
        (
            format!("client = 1\nserver = 2\n{lines}"),
            "unknown field `server`",
        ),
        (format!("client = -1\n{lines}"), "invalid value"),
        (
            format!(
                "client = 1\n{}",
                lines.replace("tasks = 50", "tasks = \"big\"")
            ),
            "invalid type",
        ),
    ] {
        let tree = checkout(&text, &[], &built(1, &[]));
        let (code, out, err) = run(&tree);
        assert_eq!((code, out.as_str()), (2, ""), "{text}");
        assert!(
            err.starts_with("symphony-build size-budget: tools/size-budget.toml: ")
                && err.contains(said),
            "{text}: {err}"
        );
    }
    let tree = Tree::new();
    tree.write("release/tasks", "b");
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony-build size-budget: tools/size-budget.toml: "),
        "{err}"
    );
}

#[test]
fn a_link_in_the_client_or_a_folder_in_a_binarys_place_is_an_error() {
    let mut release = built(1, &[("symphony-server", 2)]);
    release.retain(|(name, _)| *name != "symphony");
    let tree = checkout(
        &budget(100, 50, &[]),
        &[("index.html", 1), ("about.html", 1)],
        &release,
    );
    fs::create_dir_all(tree.path("release/symphony")).unwrap();
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony-build size-budget: symphony: ") && err.ends_with(" is no file\n"),
        "{err}"
    );
    fs::remove_dir(tree.path("release/symphony")).unwrap();
    tree.write("release/symphony", "b");
    assert_eq!(run(&tree).0, 0, "the folder's own files");
    fs::create_dir_all(tree.path(&format!("{CLIENT}/assets"))).unwrap();
    std::os::unix::fs::symlink(
        tree.path("release/tasks"),
        tree.path(&format!("{CLIENT}/assets/link.js")),
    )
    .unwrap();
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony-build size-budget: apps/symphony-server/client/public: ")
            && err.ends_with("link.js: client assets must be regular files\n"),
        "{err}"
    );
    // A file where the distribution's folder goes is no distribution either.
    let tree = checkout(&budget(100, 50, &[]), &[], &built(1, &[]));
    tree.write(CLIENT, "not a folder");
    let (code, _, err) = run(&tree);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "symphony-build size-budget: apps/symphony-server/client/public: the client distribution is no \
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
            (&b""[..], "usage: symphony-build size-budget ROOT RELEASE\n")
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
