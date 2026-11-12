//! A plugin answering from a process of its own (`[plugins.process]`): the seam tests' plugin
//! served by the `symphony-test-plugin` binary, asked as a project's plugins are and answering as
//! it does linked; its process killed, the page shows it meanwhile and a call after the pause
//! starts it again; and what its calls cost linked, through the loopback and from its process.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use symphony::plugins::Plugins;
use symphony::plugins::process::{self, PAUSE};
use symphony::project::{PluginCalls, PluginProcess, Project};
use symphony::render::Render;
use symphony_plugin::vocabulary::{Block, Span};
use symphony_plugin::{CardKind, Page};

use super::Tree;
use super::calls::{by_call, lines, median, with_card};
use super::every::EVERY;

/// The test plugin's binary.
const PLUGIN: &str = env!("CARGO_BIN_EXE_symphony-test-plugin");

/// The project at `tree` whose plugin `every` answers from the test plugin's process, its
/// `[plugins]` table holding `settings` too.
fn project(tree: &Tree, settings: &str) -> Project {
    tree.write(
        symphony::project::FILE,
        &format!(
            "[plugins]\ncrates = [\"every\"]\n{settings}\n[plugins.process]\nevery = [{PLUGIN:?}]\n"
        ),
    );
    Project::load(&tree.root).unwrap()
}

/// The cards each card kind test reads.
const CARDS: [&str; 3] = [
    "Pick every hull.\n*Slot:* North · cover",
    "hold every broken one",
    "",
];

/// That `kind` answers each call of each of [`CARDS`] as `linked` does.
fn kinds_answer_alike(kind: &dyn CardKind, linked: &dyn CardKind, root: &Path) {
    assert_eq!(kind.name(), linked.name());
    assert_eq!(kind.markers(), linked.markers());
    for text in CARDS {
        with_card(text, |card| {
            assert_eq!(kind.recognises(card), linked.recognises(card), "{text}");
            assert_eq!(
                kind.pictures(root, card),
                linked.pictures(root, card),
                "{text}"
            );
            assert_eq!(kind.slot(card), linked.slot(card), "{text}");
            assert_eq!(
                kind.triage_category(card),
                linked.triage_category(card),
                "{text}"
            );
            assert_eq!(
                kind.auto_approvable(card),
                linked.auto_approvable(card),
                "{text}"
            );
            assert_eq!(kind.gates(card), linked.gates(card), "{text}");
            assert_eq!(
                kind.approval_needs_agent(card),
                linked.approval_needs_agent(card),
                "{text}"
            );
            assert_eq!(kind.chosen(card), linked.chosen(card), "{text}");
        });
    }
}

/// The text of the one paragraph `blocks` holds.
fn said(blocks: &[Block]) -> String {
    match blocks {
        [Block::Paragraph { spans }] => match spans.as_slice() {
            [Span::Text { text }] => text.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

/// Waits, up to five seconds, until `holds`.
fn until(what: &str, holds: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !holds() {
        assert!(Instant::now() < deadline, "{what}: not in five seconds");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_plugins_process_answers_its_components_and_card_kinds_as_it_does_linked() {
    let tree = Tree::new();
    let project = project(&tree, "timing = true\n");
    let (linked, piped) = (
        Plugins::new(vec![&EVERY]),
        Plugins::at(&project, &tree.root),
    );
    assert!(process::remote(piped.list()[0]).is_some());
    assert_eq!(piped.component_names(), ["task", "every", "again"]);
    let root = tree.root.as_path();
    let page = || Page::new(root, "docs/ünï code.md");
    for (argument, option) in [("lantern", Some("wide")), ("", None), ("bad", None)] {
        assert_eq!(
            piped.render(&page(), "every", argument, option),
            linked.render(&page(), "every", argument, option),
            "{argument}"
        );
        for name in ["every", "again"] {
            let resolve = |plugins: &Plugins| {
                plugins
                    .component(name)
                    .unwrap()
                    .resolve(&page(), argument, option)
            };
            assert_eq!(resolve(&piped), resolve(&linked), "{name} {argument}");
        }
    }
    // The page's version is none, as `again` has none; `every`'s is its own.
    assert_eq!(piped.cache_version(&page()), linked.cache_version(&page()));
    let version = |plugins: &Plugins| plugins.component("every").unwrap().cache_version(&page());
    assert_eq!(version(&piped), version(&linked));
    assert!(version(&piped).is_some());
    kinds_answer_alike(piped.card_kinds()[0], linked.card_kinds()[0], root);
    // Every call went to the process and was recorded as its call: the plugin's time and the
    // host's wait, nothing failed.
    let record = lines(&symphony::plugins::record(&project, root));
    let calls: BTreeSet<String> = record
        .iter()
        .map(|line| {
            format!(
                "{}.{}",
                line["seam"].as_str().unwrap(),
                line["call"].as_str().unwrap()
            )
        })
        .collect();
    let every: BTreeSet<String> = [
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
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(calls, every);
    for line in &record {
        assert_eq!(line["path"], "process", "{line}");
        assert!(line.get("failed").is_none(), "{line}");
        assert!(
            line["round_trip_us"].as_f64().unwrap() >= line["us"].as_f64().unwrap(),
            "{line}"
        );
    }
}

#[test]
fn a_pages_components_share_one_load_in_the_plugins_process() {
    let tree = Tree::new();
    let plugins = Plugins::at(&project(&tree, ""), &tree.root);
    let on = format!("{}|docs/A.md", tree.root.display());
    let page = Page::new(&tree.root, "docs/A.md");
    plugins.render(&page, "every", "a", None);
    assert_eq!(
        said(&plugins.render(&page, "again", "loads", None)),
        format!("loads again on {on}, loaded 1 times")
    );
    // Another page, another render: loaded again.
    let another = Page::new(&tree.root, "docs/A.md");
    assert_eq!(
        said(&plugins.render(&another, "again", "loads", None)),
        format!("loads again on {on}, loaded 2 times")
    );
}

#[test]
fn a_killed_plugin_process_is_shown_on_the_page_meanwhile_and_starts_again_after_its_pause() {
    let tree = Tree::new();
    let project = project(&tree, "");
    let plugins = Plugins::at(&project, &tree.root);
    let render = Render::new(&tree.root).with_plugins(plugins.clone());
    let doc = "# A\n\n[[every:lantern]]\n";
    let before = render.html(doc, "docs/A.md");
    assert!(before.contains("data-at="), "{before}");
    let remote = process::remote(plugins.list()[0]).unwrap();
    let pid = remote.pid().unwrap();
    let killed = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    until("the plugin is down", || remote.down().is_some());
    // Meanwhile the page shows why in the component's place, and a card kind answers as one that
    // says nothing of the card.
    let meanwhile = render.html(doc, "docs/A.md");
    assert!(
        meanwhile.contains(&format!(
            "plugin every: its process {pid} was killed by signal 9; it starts again in a moment"
        )),
        "{meanwhile}"
    );
    with_card(CARDS[0], |card| {
        assert!(!plugins.card_kinds()[0].recognises(card));
    });
    // After the pause the next call starts it again, and the page is as it was.
    std::thread::sleep(PAUSE);
    assert_eq!(render.html(doc, "docs/A.md"), before);
    let again = remote.pid().unwrap();
    assert_ne!(again, pid);
    let log = std::fs::read_to_string(&project.plugin_processes["every"].log).unwrap();
    for said in [
        format!("symphony: started {pid}: {PLUGIN}"),
        format!("symphony: its process {pid} was killed by signal 9"),
        format!("symphony: started {again}: {PLUGIN}"),
    ] {
        assert!(log.contains(&said), "{said}: {log}");
    }
}

#[test]
fn a_plugins_command_names_its_program_its_checkout_and_its_log() {
    let root = Path::new("/r");
    let parsed = |text: &str| Project::parse(root, text);
    let project = parsed(
        "[project]\nartifacts = \"out/kept\"\n\n[plugins]\ncrates = [\"every\", \"bare\"]\n\n\
         [plugins.process]\nevery = [\"./bin/every\", \"--quiet\"]\nbare = [\"bare-plugin\"]\n",
    )
    .unwrap();
    assert_eq!(project.plugins, ["every", "bare"]);
    assert_eq!(
        project.plugin_processes["every"],
        PluginProcess {
            // A relative path is the checkout's; a bare name is looked up on PATH.
            command: vec!["/r/bin/every".into(), "--quiet".into()],
            root: root.into(),
            log: "/r/out/kept/plugins/every.log".into(),
        }
    );
    assert_eq!(project.plugin_processes["bare"].command, ["bare-plugin"]);
    let absolute = parsed("[plugins]\ncrates = [\"a\"]\n[plugins.process]\na = [\"/opt/a\"]\n");
    assert_eq!(absolute.unwrap().plugin_processes["a"].command, ["/opt/a"]);
    // A plugin linked is in no table of processes.
    let linked = parsed("[plugins]\ncrates = [\"every\"]\n").unwrap();
    assert!(linked.plugin_processes.is_empty());
    assert_eq!(
        parsed("[plugins]\ncrates = [\"a\"]\n[plugins.process]\nb = [\"b\"]\n").err(),
        Some(
            "symphony.toml: [plugins.process] names b, which [plugins] crates does not list".into()
        )
    );
    for command in ["[]", "[\"\"]", "[\" \", \"x\"]"] {
        assert_eq!(
            parsed(&format!(
                "[plugins]\ncrates = [\"a\"]\n[plugins.process]\na = {command}\n"
            ))
            .err(),
            Some("symphony.toml: [plugins.process] a names no program".into()),
            "{command}"
        );
    }
    assert!(
        parsed("[plugins]\ncrates = [\"a\"]\n[plugins.process]\na = \"a\"\n")
            .unwrap_err()
            .starts_with("symphony.toml: ")
    );
}

#[test]
fn a_plugins_process_that_does_not_start_is_a_problem_of_the_project() {
    let tree = Tree::new();
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"every\", \"nowhere\"]\n\n[plugins.process]\nevery = [\"./no-such-plugin\"]\n",
    );
    let project = Project::load(&tree.root).unwrap();
    let problems = symphony::plugins::problems(&project);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert_eq!(
        problems[0],
        "symphony.toml: [plugins] crates names nowhere, which this build does not hold"
    );
    let program = tree.path("no-such-plugin");
    assert!(
        problems[1].starts_with(&format!(
            "symphony.toml: the plugin every: {} cannot start: ",
            program.display()
        )),
        "{problems:?}"
    );
    // Its components are none, so a page names the component unknown.
    let plugins = Plugins::at(&project, &tree.root);
    assert_eq!(plugins.component_names(), ["task"]);
}

/// The calls of a component and of a card kind of [`EVERY`], one each, as the measurement makes
/// them.
fn component_and_kind_calls(plugins: &Plugins, root: &Path) {
    use std::hint::black_box;
    let page = Page::new(root, "docs/A.md");
    let _ = black_box(plugins.render(&page, "every", "lantern", Some("wide")));
    let _ = black_box(
        plugins
            .component("every")
            .map(|c| c.resolve(&page, "a", None)),
    );
    let _ = black_box(plugins.cache_version(&page));
    let kind = plugins.card_kinds()[0];
    with_card(CARDS[0], |card| {
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
}

/// What each call of a component and a card kind costs linked, through the loopback and from the
/// plugin's own process: a measurement, not a check. Run alone, it prints each call's medians in
/// microseconds: linked (the call timed alone), the loopback's round trip, the process's (the
/// host's wait for its answer) and the plugin's own time there, and the bytes of the process's
/// two lines; then a page of ten components drawn linked and from the process. Best in a release
/// build: `just test-rs --release -p symphony --test project the_cost_of_a_plugins_calls --
/// --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check: run it alone with --ignored --nocapture"]
fn the_cost_of_a_plugins_calls_linked_through_the_loopback_and_from_its_process() {
    const ROUNDS: usize = 1_000;
    const PAGES: usize = 200;
    let tree = Tree::new();
    let root = tree.root.as_path();
    let (timed_record, looped_record) = (tree.path("timed.jsonl"), tree.path("looped.jsonl"));
    let calls = |timing, loopback| PluginCalls { timing, loopback };
    let timed = Plugins::new(vec![&EVERY]).measured(calls(true, false), &timed_record);
    let looped = Plugins::new(vec![&EVERY]).measured(calls(false, true), &looped_record);
    let project = project(&tree, "timing = true\n");
    let piped = Plugins::at(&project, root);
    let piped_record = symphony::plugins::record(&project, root);
    // Started before the rounds, so no call waits for the start.
    assert!(
        process::remote(piped.list()[0])
            .unwrap()
            .declaration()
            .is_ok()
    );
    // Interleaved, so the machine's load weighs on each alike.
    for _ in 0..ROUNDS {
        component_and_kind_calls(&timed, root);
        component_and_kind_calls(&looped, root);
        component_and_kind_calls(&piped, root);
    }
    let median_of = |table: &std::collections::BTreeMap<String, Vec<f64>>, name: &str| {
        median(&mut table.get(name).cloned().unwrap_or_default())
    };
    let looped_trip = by_call(&looped_record, "round_trip_us");
    let (piped_trip, piped_us) = (
        by_call(&piped_record, "round_trip_us"),
        by_call(&piped_record, "us"),
    );
    let (asked, answered) = (
        by_call(&piped_record, "request_bytes"),
        by_call(&piped_record, "answer_bytes"),
    );
    println!(
        "{ROUNDS} rounds; medians in microseconds\n{:<34} {:>9} {:>9} {:>9} {:>9} {:>7} {:>7}",
        "call", "linked", "loopback", "process", "in it", "req B", "ans B"
    );
    for (name, mut times) in by_call(&timed_record, "us") {
        println!(
            "{name:<34} {:>9.3} {:>9.3} {:>9.3} {:>9.3} {:>7} {:>7}",
            median(&mut times),
            median_of(&looped_trip, &name),
            median_of(&piped_trip, &name),
            median_of(&piped_us, &name),
            median_of(&asked, &name),
            median_of(&answered, &name),
        );
    }
    // A page of ten components, drawn whole as a document is.
    let doc = "# A\n\n".to_string() + &"[[every:lantern|wide]] [[again:b]]\n\n".repeat(5);
    let (linked_page, piped_page) = (
        Render::new(root).with_plugins(Plugins::new(vec![&EVERY])),
        Render::new(root).with_plugins(Plugins::at(&Project::load(root).unwrap(), root)),
    );
    let (mut linked_ms, mut piped_ms) = (Vec::new(), Vec::new());
    for _ in 0..PAGES {
        for (render, times) in [(&linked_page, &mut linked_ms), (&piped_page, &mut piped_ms)] {
            let started = Instant::now();
            let _ = std::hint::black_box(render.html(&doc, "docs/A.md"));
            times.push(started.elapsed().as_secs_f64() * 1e3);
        }
    }
    println!(
        "a page of 10 components, {PAGES} renders; medians in milliseconds: linked {:.3}, process {:.3}",
        median(&mut linked_ms),
        median(&mut piped_ms)
    );
}
