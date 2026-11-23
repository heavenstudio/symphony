//! Each plugin call measured: timed into the project's record, answering as the plugin does
//! linked, seam by seam.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use symphony::plugins::{Plugins, Reply};
use symphony::project::{PluginCalls, Project};
use symphony_plugin::authority::{Operation, Snapshot};
use symphony_plugin::execution::{Principal, PrincipalKind};
use symphony_plugin::vocabulary::FieldValue;
use symphony_plugin::{Card, Docs, File, Page, Plugin, Sender, Submission, TaskRef, ThreadLine};

use super::Tree;
use super::compiled::naming;
use super::every::EVERY;

/// The record at `path`, each line read as JSON.
pub(super) fn lines(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Each call timed.
const TIMING: PluginCalls = PluginCalls { timing: true };

/// Every call of every seam, as the record names it (`seam.call`).
const EVERY_CALL: [&str; 30] = [
    "components.render",
    "components.resolve",
    "components.cache_version",
    "card_kinds.recognises",
    "card_kinds.pictures",
    "card_kinds.slot",
    "card_kinds.triage_category",
    "card_kinds.auto_approvable",
    "card_kinds.gates",
    "card_kinds.approval_needs_agent",
    "card_kinds.chosen",
    "renderers.draw",
    "routes.serve",
    "home_regions.region",
    "board_actions.offer",
    "board_actions.submit",
    "board_fields.validate",
    "gates.answer",
    "gates.kept_while",
    "authorities.attest",
    "describers.describe",
    "watches.changed",
    "watches.forget",
    "doc_checks.check",
    "doc_checks.components",
    "prose_rules.check",
    "commands.run",
    "transports.register",
    "transports.deliver",
    "transports.alive",
];

/// The calls the record at `path` names, each once (`seam.call`).
fn recorded_calls(path: &Path) -> BTreeSet<String> {
    lines(path)
        .iter()
        .map(|line| {
            format!(
                "{}.{}",
                line["seam"].as_str().unwrap(),
                line["call"].as_str().unwrap()
            )
        })
        .collect()
}

/// `plugins` linked and the same timed, its record read as each call writes it.
struct Both {
    linked: Plugins,
    timed: Plugins,
    record: PathBuf,
    /// The record's lines already read.
    read: Mutex<usize>,
}

impl Both {
    /// `plugin` linked and timed.
    fn new(plugin: &'static dyn Plugin, tree: &Tree) -> Both {
        let record = tree.path("artifacts/plugin-calls.jsonl");
        Both {
            linked: Plugins::new(vec![plugin]),
            timed: Plugins::new(vec![plugin]).measured(TIMING, &record),
            read: Mutex::new(lines(&record).len()),
            record,
        }
    }

    /// That `call` answers the same timed as linked, and that it was recorded.
    fn same<T: PartialEq + Debug>(&self, what: &str, call: impl Fn(&Plugins) -> T) {
        let linked = call(&self.linked);
        let timed = call(&self.timed);
        let lines = lines(&self.record);
        let mut read = self.read.lock().unwrap();
        let written = &lines[*read..];
        *read = lines.len();
        assert!(!written.is_empty(), "{what}: no call was recorded");
        assert_eq!(linked, timed, "{what}");
    }
}

fn card_lines() -> Vec<(bool, Option<&'static str>, &'static str)> {
    vec![
        (true, None, "A or B? \"quoted\""),
        (false, Some("approved"), "B, with ✅ and a tab\t"),
    ]
}

/// What `read` makes of a card with `text`.
pub(super) fn with_card<R>(text: &str, read: impl FnOnce(&Card<'_>) -> R) -> R {
    let thread: Vec<ThreadLine<'_>> = card_lines()
        .into_iter()
        .map(|(agent, verdict, text)| ThreadLine {
            agent,
            verdict,
            text,
        })
        .collect();
    read(&Card {
        slug: "pick-a-widget",
        doc: "docs/WIDGETS.md",
        topic: "Widgets · \\ back",
        text,
        thread: &thread,
    })
}

/// An operation `principal` takes on sim/jump from `snapshot`, binding `revisions`.
fn operation(principal: Principal, snapshot: Snapshot, revisions: &[(&str, &str)]) -> Operation {
    Operation::new(
        "symphony".into(),
        "sim/jump".into(),
        "accept · ✅".into(),
        "approved \"twice\"".into(),
        snapshot,
        revisions
            .iter()
            .map(|(input, revision)| (input.to_string(), revision.to_string()))
            .collect(),
        principal,
    )
}

/// The operations [`EVERY`]'s authority is asked to attest: each kind of principal, the largest
/// numbers a snapshot holds and none, input revisions and none.
fn operations() -> Vec<Operation> {
    vec![
        operation(
            Principal::new(PrincipalKind::Human, "local".into(), "owner ✅".into()),
            Snapshot::new(
                "sha256:d1".into(),
                u64::MAX,
                u64::MAX,
                "verify".into(),
                "awaiting".into(),
            ),
            &[("brief", "r7"), ("diff ü", "")],
        ),
        operation(
            Principal::new(PrincipalKind::Agent, "board".into(), "amber-heron".into()),
            Snapshot::new(String::new(), 0, 0, String::new(), String::new()),
            &[],
        ),
        operation(
            Principal::new(PrincipalKind::Service, String::new(), "ci\t1".into()),
            Snapshot::new("d2".into(), 2, 3, "land".into(), "ready".into()),
            &[("tip", "c0ffee")],
        ),
    ]
}

#[test]
fn every_seam_answers_the_same_linked_and_timed_and_its_line_holds_its_time_alone() {
    let tree = Tree::new();
    let both = Both::new(&EVERY, &tree);
    every_seam_answers_the_same(&both, &tree);
    // Every call of every seam was timed, and the declaration was not: it is no call.
    let every: BTreeSet<String> = EVERY_CALL.into_iter().map(String::from).collect();
    assert_eq!(recorded_calls(&both.record), every);
    for line in lines(&both.record) {
        let mut keys: Vec<&str> = line
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "at_ms", "call", "name", "path", "pid", "plugin", "seam", "us"
            ],
            "{line}"
        );
        assert_eq!(line["path"], "linked", "{line}");
    }
}

/// That every call of every seam of [`EVERY`] answers the same through `both`'s measured plugins
/// as linked, the project's checkout at `tree`.
fn every_seam_answers_the_same(both: &Both, tree: &Tree) {
    tree.write("docs/A.md", "# A\n\nevery word\n");
    tree.write("docs/B.md", "# B\n");
    let root = tree.root.as_path();
    // An artifacts folder of its own, so a wrapper that handed the plugin another page answers
    // otherwise.
    let page = || Page::new(root, "docs/ünï code.md").in_artifacts("out/ä");
    for (argument, option) in [("lantern", Some("wide")), ("", None), ("bad", None)] {
        both.same("render", |p| p.render(&page(), "every", argument, option));
        both.same("resolve", |p| {
            p.component("every")
                .unwrap()
                .resolve(&page(), argument, option)
        });
        both.same("resolve again", |p| {
            p.component("again")
                .unwrap()
                .resolve(&page(), argument, option)
        });
    }
    both.same("cache_version", |p| p.cache_version(&page()));
    for text in [
        "Pick every widget.\n*Slot:* North · cover",
        "hold every broken one",
        "",
    ] {
        both.same("recognises", |p| {
            with_card(text, |card| p.card_kinds()[0].recognises(card))
        });
        both.same("pictures", |p| {
            with_card(text, |card| p.card_kinds()[0].pictures(root, card))
        });
        both.same("slot", |p| {
            with_card(text, |card| p.card_kinds()[0].slot(card))
        });
        both.same("triage_category", |p| {
            with_card(text, |card| p.card_kinds()[0].triage_category(card))
        });
        both.same("auto_approvable", |p| {
            with_card(text, |card| p.card_kinds()[0].auto_approvable(card))
        });
        both.same("gates", |p| {
            with_card(text, |card| p.card_kinds()[0].gates(card))
        });
        both.same("approval_needs_agent", |p| {
            with_card(text, |card| p.card_kinds()[0].approval_needs_agent(card))
        });
        both.same("chosen", |p| {
            with_card(text, |card| p.card_kinds()[0].chosen(card))
        });
    }
    for (src, label) in [("/docs/a.every?v=2#top", ""), ("/docs/b.every", "a widget")] {
        let file = File { src, label };
        both.same("draw", |p| {
            symphony::renderers::renderer_of(p, src)
                .unwrap()
                .draw(&page(), &file)
        });
    }
    for rest in ["", "data.json", "models/a b.stl"] {
        both.same("serve", |p| p.route("every").unwrap().serve(root, rest));
    }
    both.same("region", |p| p.home_regions()[0].region(root));
    for status in ["review", "in_progress"] {
        let task = TaskRef {
            slug: "sim/jump",
            status,
            branch: "sim/jump-2",
        };
        both.same("offer", |p| p.board_actions()[0].offer(root, &task));
        for payload in ["sim/jump@sim/jump-2", "stale"] {
            let submission = Submission {
                payload: payload.into(),
                note: "send it ✅".into(),
            };
            both.same("submit", |p| {
                p.board_actions()[0].submit(root, &task, &submission)
            });
            // Who sent it, timed as the same call and answered alike.
            let sender = Sender::new("ana", "Ana Lima", vec!["ana-l".into()]);
            both.same("submit from", |p| {
                p.board_actions()[0].submit_from(root, &task, &submission, &sender)
            });
        }
        for tip in [Some("c0ffee"), None] {
            both.same("answer", |p| {
                p.list()[0].gates()[0].answer(root, &task, tip)
            });
        }
        both.same("kept_while", |p| {
            p.list()[0].gates()[0].kept_while(root, &task)
        });
    }
    // The authority as the host finds it for a guard's adapter; no time at all it refuses.
    for operation in operations() {
        for within in [
            Duration::from_secs(10),
            Duration::from_millis(1),
            Duration::ZERO,
        ] {
            both.same("attest", |p| {
                p.authority("every")
                    .unwrap()
                    .attest(root, &operation, within)
            });
        }
    }
    // Each describer, the one that cannot say among them.
    for at in 0..2 {
        both.same("describe", |p| p.describers()[at].1.describe(root));
    }
    for value in [
        FieldValue::Text(" 3.5 ".into()),
        FieldValue::Number(0.1 + 0.2),
        FieldValue::Text("many".into()),
    ] {
        both.same("validate", |p| {
            p.board_field("every", "effort").unwrap().validate(&value)
        });
    }
    both.same("changed", |p| p.watches()[0].changed(root));
    both.same("forget", |p| p.watches()[0].forget(root));
    let forgot = EVERY.forgot.lock().unwrap();
    assert_eq!(forgot.iter().filter(|forgot| *forgot == root).count(), 2);
    drop(forgot);
    let docs = Docs::read(root);
    both.same("doc check", |p| p.doc_checks()[0].check(&docs));
    both.same("doc components", |p| p.doc_checks()[0].components(&docs));
    both.same("prose", |p| {
        p.prose_rules()[0].check("docs/A.md", "# A\n\nevery line\r\nnot this\n")
    });
    for args in [
        vec![],
        vec!["--all".to_string(), "a b".into(), String::new()],
    ] {
        both.same("run", |p| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = p.list()[0].commands()[0].run(&args, &mut out, &mut err);
            (code, out, err)
        });
    }
    let transport = |p: &Plugins| p.list()[0].transports()[0];
    for (handle, reported) in [("amber-heron", "/tmp/s.sock"), ("x", "")] {
        both.same("register", |p| transport(p).register(handle, reported));
    }
    for message in ["PULL\n", ""] {
        both.same("deliver", |p| transport(p).deliver("/tmp/s.sock", message));
    }
    both.same("alive", |p| transport(p).alive("relative"));
}

#[test]
fn what_a_measured_plugin_declares_is_the_plugins_own() {
    let tree = Tree::new();
    let both = Both::new(&EVERY, &tree);
    let declared = |p: &Plugins| symphony_plugin::protocol::Declaration::of(p.list()[0]);
    assert_eq!(declared(&both.linked), declared(&both.timed));
    assert_eq!(
        symphony::landing::with_plugins(&Project::plain(&tree.root), &both.timed),
        symphony::landing::with_plugins(&Project::plain(&tree.root), &both.linked)
    );
    assert_eq!(both.timed.nav(), EVERY.nav());
    assert_eq!(both.timed.theme("every"), EVERY.theme());
    assert_eq!(both.timed.component_names(), ["task", "every", "again"]);
    assert_eq!(
        symphony::renderers::content_type(&both.timed, "/a.every"),
        Some("model/every")
    );
}

#[test]
fn timing_records_each_call_with_its_plugin_seam_object_and_time() {
    let tree = naming(&["first"]);
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"first\"]\ntiming = true\n",
    );
    let plugins = Plugins::of(&tree.root);
    let page = Page::new(&tree.root, "docs/A.md");
    plugins.render(&page, "word", "x", None);
    plugins.watches()[0].changed(&tree.root);
    let lines = lines(&tree.path("artifacts/plugin-calls.jsonl"));
    let said: Vec<(String, String, String, String)> = lines
        .iter()
        .map(|line| {
            let text = |key: &str| line[key].as_str().unwrap().to_string();
            (text("plugin"), text("seam"), text("name"), text("call"))
        })
        .collect();
    let said: Vec<(&str, &str, &str, &str)> = said
        .iter()
        .map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), d.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("first", "components", "word", "render"),
            ("first", "watches", "data/one", "changed")
        ]
    );
    for line in &lines {
        assert!(line["us"].as_f64().unwrap() >= 0.0, "{line}");
        assert!(line.get("round_trip_us").is_none(), "{line}");
    }
}

#[test]
fn the_record_goes_under_the_projects_artifacts_folder() {
    let tree = naming(&["first"]);
    tree.write(
        symphony::project::FILE,
        "[project]\nartifacts = \"out/kept\"\n\n[plugins]\ncrates = [\"first\"]\ntiming = true\n",
    );
    let project = Project::of(&tree.root);
    assert_eq!(project.plugin_calls, TIMING);
    assert_eq!(
        symphony::plugins::record(&project, &tree.root),
        tree.path("out/kept/plugin-calls.jsonl")
    );
    let plugins = Plugins::of(&tree.root);
    assert_eq!(
        plugins.route("one").unwrap().serve(&tree.root, "data"),
        Some(Reply::Json("{\"one\":1}".into()))
    );
    let lines = lines(&tree.path("out/kept/plugin-calls.jsonl"));
    let serve = lines
        .iter()
        .find(|line| line["seam"] == "routes")
        .expect("the route's call is recorded");
    assert_eq!(serve["call"], "serve");
    assert!(serve["us"].as_f64().unwrap() >= 0.0, "{serve}");
    assert!(!tree.path("artifacts").exists());
}

#[test]
fn a_project_whose_file_writes_the_retired_loopback_times_as_its_timing_says() {
    // The project runs with the key left out, as any unknown one, and the rest read.
    for (file, timing) in [
        ("[plugins]\ncrates = [\"first\"]\nloopback = true\n", false),
        (
            "[plugins]\ncrates = [\"first\"]\ntiming = true\nloopback = true\n",
            true,
        ),
    ] {
        let tree = naming(&["first"]);
        tree.write(symphony::project::FILE, file);
        let project = Project::of(&tree.root);
        assert_eq!(project.plugin_calls, PluginCalls { timing }, "{file}");
        let plugins = Plugins::of(&tree.root);
        plugins.render(&Page::new(&tree.root, "docs/A.md"), "word", "x", None);
        let lines = lines(&tree.path("artifacts/plugin-calls.jsonl"));
        let paths: Vec<&serde_json::Value> = lines.iter().map(|line| &line["path"]).collect();
        let expected: &[&str] = if timing { &["linked"] } else { &[] };
        assert_eq!(paths, expected, "{file}");
    }
}

#[test]
fn timing_is_off_unless_the_file_says_so_and_then_nothing_is_recorded() {
    assert_eq!(
        Project::parse(Path::new("/r"), "").unwrap().plugin_calls,
        PluginCalls::default()
    );
    assert!(!PluginCalls::default().recorded());
    let tree = naming(&["first"]);
    let plugins = Plugins::of(&tree.root);
    assert!(std::ptr::eq(
        plugins.list()[0],
        symphony::plugins::compiled()[0]
    ));
    let page = Page::new(&tree.root, "docs/A.md");
    plugins.render(&page, "word", "x", None);
    plugins.watches()[0].changed(&tree.root);
    assert!(!tree.path("artifacts").exists());
    let timed = Project::parse(Path::new("/r"), "[plugins]\ntiming = true\n").unwrap();
    assert_eq!(timed.plugin_calls, TIMING);
    assert!(timed.plugin_calls.recorded());
    assert!(
        Project::parse(Path::new("/r"), "[plugins]\ntiming = \"yes\"\n").is_err(),
        "a setting that is not true or false is refused"
    );
}

#[test]
fn a_plugin_command_is_measured_as_the_project_it_runs_in_says() {
    let tree = naming(&[]);
    let run = |root: Option<&Path>| {
        let command = symphony::plugins::command("every", root).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = command.run(&["a".into(), "b c".into()], &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    let said = (2, "a|b c\n".to_string(), "2 arguments\n".to_string());
    assert_eq!(run(None), said);
    assert_eq!(run(Some(&tree.root)), said);
    assert!(!tree.path("artifacts").exists());
    // The retired loopback turns nothing on.
    tree.write(symphony::project::FILE, "[plugins]\nloopback = true\n");
    assert_eq!(run(Some(&tree.root)), said);
    assert!(!tree.path("artifacts").exists());
    tree.write(symphony::project::FILE, "[plugins]\ntiming = true\n");
    assert_eq!(run(Some(&tree.root)), said);
    let lines = lines(&tree.path("artifacts/plugin-calls.jsonl"));
    let run_line = lines
        .iter()
        .find(|line| line["seam"] == "commands")
        .expect("the command's run is recorded");
    assert_eq!(run_line["call"], "run");
    assert_eq!(run_line["name"], "every");
    // No plugin adds it, and none failed to say what it adds.
    assert_eq!(
        symphony::plugins::command("nobody", Some(&tree.root)).err(),
        Some(Vec::new())
    );
}

/// The environment variable naming the project the child of
/// [`with_timing_off_a_plugin_command_loads_no_project_and_says_nothing_of_its_file`] runs its
/// command in.
const QUIET_ROOT: &str = "SYMPHONY_TEST_QUIET_COMMAND_ROOT";

#[test]
fn with_timing_off_a_plugin_command_loads_no_project_and_says_nothing_of_its_file() {
    if let Some(root) = std::env::var_os(QUIET_ROOT) {
        // The child: the command found and run as `symphony NAME` finds and runs it.
        naming(&[]);
        let command = symphony::plugins::command("every", Some(Path::new(&root))).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(command.run(&["a".into()], &mut out, &mut err), 1);
        return;
    }
    // A key this build does not know, which a loaded project names on stderr.
    let run = |file: &str| {
        let tree = Tree::new();
        tree.write(symphony::project::FILE, file);
        let done = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "seams::calls::with_timing_off_a_plugin_command_loads_no_project_and_says_nothing_of_its_file",
                "--nocapture",
            ])
            .env(QUIET_ROOT, &tree.root)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&done.stdout).into_owned();
        assert!(done.status.success() && said.contains("1 passed"), "{said}");
        (
            String::from_utf8_lossy(&done.stderr).into_owned(),
            tree.path("artifacts").exists(),
        )
    };
    let (said, recorded) = run("[plugins]\ncolour = \"red\"\n");
    assert!(!said.contains("colour"), "{said}");
    assert!(!recorded);
    // The retired loopback is no setting: with it alone the file is not loaded, so not named.
    let (said, recorded) = run("[plugins]\ncolour = \"red\"\nloopback = true\n");
    assert!(
        !said.contains("colour") && !said.contains("loopback"),
        "{said}"
    );
    assert!(!recorded);
    // With timing on, the project is loaded as the server loads it, and its file named.
    let (said, recorded) = run("[plugins]\ncolour = \"red\"\ntiming = true\n");
    assert!(
        said.contains("colour") && said.contains("left out"),
        "{said}"
    );
    assert!(recorded);
}

#[test]
fn the_timing_is_read_alone_and_any_file_it_cannot_be_read_from_leaves_it_off() {
    let tree = Tree::new();
    let read = |file: &str| {
        tree.write(symphony::project::FILE, file);
        PluginCalls::read(&tree.root)
    };
    let off = PluginCalls::default();
    assert_eq!(PluginCalls::read(&tree.root), off, "no file");
    assert_eq!(read(""), off);
    assert_eq!(
        read("[project]\nlost = 1\n[plugins]\ncolour = \"red\"\ntiming = true\n"),
        TIMING
    );
    // The retired loopback is read no more, whatever it holds.
    assert_eq!(read("[plugins]\nloopback = true\n"), off);
    assert_eq!(read("[plugins]\nloopback = \"yes\"\n"), off);
    assert_eq!(read("[plugins]\ntiming = true\nloopback = true\n"), TIMING);
    assert_eq!(read("[plugins]\ntiming = \"yes\"\n"), off);
    assert_eq!(read("[plugins\ntiming = true\n"), off);
    // With it off, a project's plugins and a plugin command are the plugins' own.
    let tree = naming(&["every"]);
    let project = Project::of(&tree.root);
    assert!(std::ptr::eq(
        Plugins::at(&project, &tree.root).list()[0],
        Plugins::for_project(&project).list()[0]
    ));
    let own = EVERY.commands()[0];
    let command = symphony::plugins::command("every", Some(&tree.root)).unwrap();
    assert!(std::ptr::addr_eq(command, own));
    let unmeasured = Plugins::new(vec![&EVERY]).measured(off, &tree.path("never.jsonl"));
    assert!(std::ptr::addr_eq(unmeasured.list()[0], &EVERY));
}

#[test]
fn a_plugins_process_is_named_by_any_entry_of_its_table_and_a_file_it_cannot_be_read_from_names_none()
 {
    let tree = Tree::new();
    let named = |file: &str| {
        tree.write(symphony::project::FILE, file);
        symphony::project::PluginProcess::named_at(&tree.root)
    };
    let none: Vec<String> = Vec::new();
    assert_eq!(
        symphony::project::PluginProcess::named_at(&tree.root),
        none,
        "no file"
    );
    assert_eq!(named(""), none);
    assert_eq!(named("[plugins]\ntiming = true\n"), none);
    assert_eq!(named("[plugins.process]\n"), none, "an empty table");
    // Whatever an entry holds: the project reads and checks it, not this.
    assert_eq!(
        named("[project]\nlost = 1\n[plugins.process]\nevery = [\"/no/such/plugin\"]\n"),
        ["every"]
    );
    assert_eq!(
        named("[plugins.process]\nevery = 3\nother = { a = [1, \"b\"] }\n"),
        ["every", "other"]
    );
    assert_eq!(
        named("[plugins.process\nevery = [\"p\"]\n"),
        none,
        "no parse"
    );
    assert_eq!(named("[plugins]\nprocess = 3\n"), none, "not a table");
}

/// Each `pub trait` of `source` and the methods it declares.
fn traits(source: &str) -> BTreeMap<String, BTreeSet<String>> {
    blocks(source, |line| {
        let named = line.strip_prefix("pub trait ")?;
        Some(named.chars().take_while(|c| c.is_alphanumeric()).collect())
    })
}

/// Each `impl` block of `source`, by its line (`impl Component for On<dyn Component>`), and the
/// methods it defines.
fn impls(source: &str) -> BTreeMap<String, BTreeSet<String>> {
    blocks(source, |line| {
        line.starts_with("impl ")
            .then(|| line.strip_suffix(" {"))
            .flatten()
            .map(str::to_string)
    })
}

/// The blocks of `source` that `opens` names by their first line, each with the methods at its
/// first depth (`    fn NAME`), to the `}` that closes it.
fn blocks(
    source: &str,
    opens: impl Fn(&str) -> Option<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut found = BTreeMap::new();
    let mut open: Option<(String, BTreeSet<String>)> = None;
    for line in source.lines() {
        if let Some((_, methods)) = open.as_mut() {
            if line == "}" {
                let (name, methods) = open.take().unwrap();
                found.insert(name, methods);
            } else if let Some(method) = line.strip_prefix("    fn ") {
                methods.insert(
                    method
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect(),
                );
            }
        } else if let Some(name) = opens(line) {
            open = Some((name, BTreeSet::new()));
        }
    }
    found
}

/// The file that defines the trait `name`, as the plugin crate's root (`lib`) takes it: a module of
/// its own (`pub use renderer::{…}`), another crate (`pub use symphony_docs::{…}`), or the root
/// itself.
fn defining_file(lib: &str, name: &str) -> String {
    let used = lib
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("pub use "))
        .find(|used| {
            used.split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == name)
        });
    match used.and_then(|used| Some((used, used.split_once("::")?.0))) {
        Some((used, from)) if from.starts_with("symphony_") => format!(
            "the file of crates/{}/src that defines it (the plugin crate takes it as `pub use \
             {used}`)",
            from.replace('_', "-")
        ),
        Some((_, module)) => format!("crates/symphony-plugin/src/{module}.rs"),
        None => "crates/symphony-plugin/src/lib.rs".into(),
    }
}

/// The methods of the trait `name` the source reading found, or a failure naming the trait and the
/// file to add to the files the test reads.
fn methods_of<'a>(
    traits: &'a BTreeMap<String, BTreeSet<String>>,
    name: &str,
) -> &'a BTreeSet<String> {
    traits.get(name).unwrap_or_else(|| {
        let lib = include_str!("../../../../symphony-plugin/src/lib.rs");
        panic!(
            "the seam trait {name} is in none of the files this test reads: add {} to the \
             `traits(include_str!(…))` reads at its top",
            defining_file(lib, name)
        )
    })
}

#[test]
fn a_seam_trait_the_source_reading_misses_is_named_with_the_file_to_add() {
    let lib = "pub use renderer::{Drawing, Renderer};\n    pub use symphony_api::{\n\
               pub use symphony_docs::{DocCheck, Docs};\npub trait Plugin {\n";
    assert_eq!(
        defining_file(lib, "Renderer"),
        "crates/symphony-plugin/src/renderer.rs"
    );
    assert_eq!(
        defining_file(lib, "DocCheck"),
        "the file of crates/symphony-docs/src that defines it (the plugin crate takes it as \
         `pub use symphony_docs::{DocCheck, Docs};`)"
    );
    assert_eq!(
        defining_file(lib, "Plugin"),
        "crates/symphony-plugin/src/lib.rs"
    );
    let found = BTreeMap::from([("Plugin".to_string(), BTreeSet::from(["name".to_string()]))]);
    assert_eq!(methods_of(&found, "Plugin").len(), 1);
    let missed = std::panic::catch_unwind(|| methods_of(&found, "Renderer").len());
    let said = missed.unwrap_err();
    let said = said.downcast_ref::<String>().unwrap();
    assert_eq!(
        said,
        "the seam trait Renderer is in none of the files this test reads: add \
         crates/symphony-plugin/src/renderer.rs to the `traits(include_str!(…))` reads at its top"
    );
}

/// The method a seam's declared entry is the answer of, where the entry is that answer alone: a
/// component declared by its name, a route by its path…
const DECLARED_BY: [(&str, &str); 10] = [
    ("components", "name"),
    ("routes", "path"),
    ("home_regions", "id"),
    ("board_actions", "name"),
    ("gates", "name"),
    ("authorities", "adapter"),
    ("describers", "id"),
    ("watches", "paths"),
    ("prose_rules", "name"),
    ("transports", "name"),
];

/// A seam's method that goes on the wire as another method's call, with a field of that call's a
/// later minor added: a submission with who sent it is `board_actions.submit` with its `sender`.
const CALLED_AS: [(&str, &str); 1] = [("board_actions.submit_from", "board_actions.submit")];

#[test]
fn every_method_of_every_seam_is_a_call_or_declared_and_the_wrapper_measures_each() {
    let mut seams_traits = traits(include_str!("../../../../symphony-plugin/src/lib.rs"));
    seams_traits.extend(traits(include_str!(
        "../../../../symphony-plugin/src/renderer.rs"
    )));
    seams_traits.extend(traits(include_str!(
        "../../../../symphony-plugin/src/authority.rs"
    )));
    seams_traits.extend(traits(include_str!(
        "../../../../symphony-docs/src/check.rs"
    )));
    let wrapper = impls(include_str!("../../../src/plugins/measure.rs"));
    let calls: BTreeSet<&str> = symphony_plugin::protocol::CALLS.iter().copied().collect();
    let declared =
        serde_json::to_value(symphony_plugin::protocol::Declaration::of(&EVERY)).unwrap();
    // Each seam, as `Plugin` hands its objects out: `fn card_kinds(&self) -> Vec<&dyn CardKind>`.
    let seams: Vec<(String, String)> = include_str!("../../../../symphony-plugin/src/lib.rs")
        .lines()
        .filter_map(|line| {
            let method = line.strip_prefix("    fn ")?;
            let (seam, rest) = method.split_once("(&self) -> Vec<&dyn ")?;
            Some((seam.to_string(), rest.split_once('>')?.0.to_string()))
        })
        .collect();
    assert!(seams.len() >= 14, "{seams:?}");
    let plugin = methods_of(&seams_traits, "Plugin");
    // The plugin's own methods: each declared, and each the wrapper's.
    for method in plugin {
        assert!(
            declared.get(method).is_some(),
            "Plugin::{method} is not in the declaration"
        );
    }
    assert_eq!(
        &wrapper["impl Plugin for Measured"], plugin,
        "the wrapper's Plugin methods"
    );
    for (seam, name) in &seams {
        let methods = methods_of(&seams_traits, name);
        let entry = &declared[seam.as_str()][0];
        let declares = |method: &str| match entry {
            serde_json::Value::Object(fields) => fields.contains_key(method),
            serde_json::Value::Null => panic!("EVERY declares no {seam}"),
            _ => DECLARED_BY.contains(&(seam.as_str(), method)),
        };
        let mut asked = false;
        for method in methods {
            let tag = format!("{seam}.{method}");
            let call = calls.contains(tag.as_str())
                || (CALLED_AS.iter())
                    .any(|(named, as_call)| *named == tag && calls.contains(as_call));
            asked |= call;
            assert!(
                call || declares(method),
                "{name}::{method} is neither a call of the protocol nor declared"
            );
        }
        match wrapper.get(&format!("impl {name} for On<dyn {name}>")) {
            Some(measured) => assert_eq!(measured, methods, "the wrapper's {name} methods"),
            None => assert!(
                !asked,
                "{name} has calls, and the wrapper hands the plugin's own out unmeasured"
            ),
        }
    }
    // And every call of the protocol is a seam's method, and asked of the test plugin; a batch
    // carries its seam's calls, and only a host speaking to a process sends one.
    let batches: BTreeSet<&str> = symphony_plugin::protocol::BATCHES.iter().copied().collect();
    let calls: BTreeSet<&str> = calls.difference(&batches).copied().collect();
    for tag in &calls {
        let (seam, method) = tag.split_once('.').unwrap();
        if seam == "plugin" {
            continue;
        }
        let (_, name) = seams
            .iter()
            .find(|(named, _)| named == seam)
            .unwrap_or_else(|| panic!("{tag}: no such seam"));
        assert!(
            methods_of(&seams_traits, name).contains(method),
            "{tag}: no such method"
        );
    }
    let mut every: BTreeSet<&str> = EVERY_CALL.into_iter().collect();
    every.insert("plugin.declare");
    assert_eq!(every, calls, "the test plugin is asked every call");
    for (seam, method) in DECLARED_BY {
        let (_, name) = seams.iter().find(|(named, _)| named == seam).unwrap();
        assert!(
            methods_of(&seams_traits, name).contains(method),
            "{seam} {method}"
        );
    }
}

#[test]
fn the_source_reading_finds_a_traits_methods_and_an_impls_and_nothing_else() {
    let source = "\
/// A seam.
pub trait Seam: Send {
    /// Its name.
    fn name(&self) -> &str;
    fn many_lines(
        &self,
    ) -> bool {
        // fn not_a_method()
        true
    }
}

impl Seam for On<dyn Seam> {
    fn name(&self) -> &str {
        \"x\"
    }
}
";
    let set =
        |names: &[&str]| -> BTreeSet<String> { names.iter().map(|n| n.to_string()).collect() };
    assert_eq!(
        traits(source),
        BTreeMap::from([("Seam".to_string(), set(&["name", "many_lines"]))])
    );
    assert_eq!(
        impls(source),
        BTreeMap::from([("impl Seam for On<dyn Seam>".to_string(), set(&["name"]))])
    );
}

/// One call at every seam of [`EVERY`], as the measurement below makes it.
fn every_call(plugins: &Plugins, root: &Path, docs: &Docs) {
    use std::hint::black_box;
    let page = Page::new(root, "docs/A.md");
    let task = TaskRef {
        slug: "sim/jump",
        status: "review",
        branch: "sim/jump-2",
    };
    let plugin = plugins.list()[0];
    let _ = black_box(plugins.render(&page, "every", "lantern", Some("wide")));
    let _ = black_box(
        plugins
            .component("every")
            .map(|c| c.resolve(&page, "a", None)),
    );
    let _ = black_box(plugins.cache_version(&page));
    let kind = plugins.card_kinds()[0];
    with_card("Pick every widget.\n*Slot:* North · cover", |card| {
        let _ = black_box((
            kind.recognises(card),
            kind.pictures(root, card),
            kind.slot(card),
            kind.triage_category(card),
            kind.auto_approvable(card),
            kind.gates(card),
            kind.approval_needs_agent(card),
            kind.chosen(card),
        ));
    });
    let file = File {
        src: "/docs/b.every",
        label: "a widget",
    };
    let _ = black_box(plugin.renderers()[0].draw(&page, &file));
    let _ = black_box(plugins.route("every").map(|r| r.serve(root, "data.json")));
    let _ = black_box(plugins.home_regions()[0].region(root));
    let action = plugins.board_actions()[0];
    let _ = black_box(action.offer(root, &task));
    let submission = Submission {
        payload: "sim/jump@sim/jump-2".into(),
        note: "send it".into(),
    };
    let _ = black_box(action.submit(root, &task, &submission));
    let field = plugins.board_field("every", "effort");
    let _ = black_box(field.map(|f| f.validate(&FieldValue::Text(" 3.5 ".into()))));
    let _ = black_box(plugin.gates()[0].answer(root, &task, Some("c0ffee")));
    let _ = black_box(plugin.gates()[0].kept_while(root, &task));
    let operation = &operations()[0];
    let _ = black_box(
        plugins
            .authority("every")
            .map(|a| a.attest(root, operation, Duration::from_secs(10))),
    );
    let _ = black_box(plugins.describers()[0].1.describe(root));
    let _ = black_box(plugins.watches()[0].changed(root));
    plugins.watches()[0].forget(root);
    let _ = black_box(plugins.doc_checks()[0].check(docs));
    let _ = black_box(plugins.doc_checks()[0].components(docs));
    let _ = black_box(plugins.prose_rules()[0].check("docs/A.md", "# A\n\nevery line\n"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let _ = black_box(plugin.commands()[0].run(&["a".into()], &mut out, &mut err));
    let transport = plugin.transports()[0];
    let _ = black_box((
        transport.register("amber-heron", "/tmp/s.sock"),
        transport.deliver("/tmp/s.sock", "PULL"),
        transport.alive("/tmp/s.sock"),
    ));
}

/// The median of `values`.
pub(super) fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(f64::NAN)
}

/// Each call's values of `key` in the record at `path`, by `seam.call`.
pub(super) fn by_call(path: &Path, key: &str) -> std::collections::BTreeMap<String, Vec<f64>> {
    let mut found: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    for line in lines(path) {
        if let Some(value) = line[key].as_f64() {
            let seam = line["seam"].as_str().unwrap();
            let call = line["call"].as_str().unwrap();
            found
                .entry(format!("{seam}.{call}"))
                .or_default()
                .push(value);
        }
    }
    found
}

/// What each call of the test plugin costs, timed linked: a measurement, not a check. Run alone, it
/// prints for each call the median of its calls' `us`, best in a release build:
/// `just test-rs --release -p symphony-app --test project the_cost_of_each -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check: run it alone with --ignored --nocapture"]
fn the_cost_of_each_call_timed() {
    const ROUNDS: usize = 1_000;
    let tree = Tree::new();
    tree.write("docs/A.md", "# A\n\nevery word\n");
    tree.write("docs/B.md", "# B\n");
    let docs = Docs::read(&tree.root);
    let timed_record = tree.path("timed.jsonl");
    let timed = Plugins::new(vec![&EVERY]).measured(TIMING, &timed_record);
    for _ in 0..ROUNDS {
        every_call(&timed, &tree.root, &docs);
    }
    println!(
        "{ROUNDS} rounds; medians in microseconds\n{:<34} {:>9}",
        "call", "timed"
    );
    for (name, mut times) in by_call(&timed_record, "us") {
        println!("{name:<34} {:>9.3}", median(&mut times));
    }
}
