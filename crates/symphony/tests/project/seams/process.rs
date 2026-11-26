//! A plugin answering from a process of its own (`[plugins.process]`): the seam tests' plugin
//! served by the `symphony-test-plugin` binary, asked as a project's plugins are and answering as
//! it does linked, its gates kept as a linked plugin's are; its process killed, a call after the
//! pause starts it again (the lib's
//! `plugins::process` tests show the calls meanwhile, with a pause longer than the test); its
//! process ending with its input whatever its calls do; and what its calls cost linked and from its
//! process, and what a register read costs linked, card by card from its process and in one
//! request.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::time::{Duration, Instant};

use symphony::gates::{Asked, Kept, theirs};
use symphony::plugins::Plugins;
use symphony::plugins::process::{self, PAUSE};
use symphony::project::{PluginCalls, PluginProcess, Project};
use symphony::render::Render;
use symphony_plugin::protocol::{Envelope, PageData, Request};
use symphony_plugin::vocabulary::{Block, Gate, GateState, Span};
use symphony_plugin::{
    CardKind, Command, Docs, File, Gates, Page, Plugin as _, Renderer, Submission, TaskRef,
};

use super::Tree;
use super::calls::{lines, median, with_card};
use super::every::PIPED;

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
    "Pick every widget.\n*Slot:* North · cover",
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

/// Waits until `holds`, with the backstop of five seconds.
fn until(what: &str, holds: impl Fn() -> bool) {
    test_time::until_true(what, test_time::backstop(Duration::from_secs(5)), holds)
        .unwrap_or_else(|stall| panic!("{stall}"));
}

#[test]
fn a_plugins_process_answers_its_components_and_card_kinds_as_it_does_linked() {
    let tree = Tree::new();
    let project = project(&tree, "timing = true\n");
    let (linked, piped) = (
        Plugins::new(vec![&PIPED]),
        Plugins::at(&project, &tree.root),
    );
    assert!(process::remote(piped.list()[0]).is_some());
    assert_eq!(piped.component_names(), ["task", "every", "again", "slow"]);
    let root = tree.root.as_path();
    // An artifacts folder of its own, so one lost on the wire reads otherwise.
    let page = || Page::new(root, "docs/ünï code.md").in_artifacts("out/ä");
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
    assert_eq!(
        piped.render(&page(), "slow", "5", None),
        linked.render(&page(), "slow", "5", None)
    );
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

/// A register of `count` cards as the board reads it: every other card the test plugin's kind
/// recognises, each third with a slot, each fifth checked, each with a question and its answer.
fn register(count: usize) -> Vec<symphony::board::Doc> {
    let mut lines = Vec::new();
    for n in 0..count {
        let (what, slot) = match (n % 2, n % 3) {
            (0, 0) => ("Pick every widget", " *Slot:* North · cover"),
            (0, _) => ("Pick every widget", ""),
            _ => ("Hold the broken one", ""),
        };
        let done = if n % 5 == 4 { "x" } else { " " };
        lines.push(format!("- [{done}] **pick-{n}** — {what} {n}.{slot}"));
        lines.push(format!("  ↳ A or B for {n}?"));
        lines.push("  B please".to_string());
    }
    vec![symphony::board::Doc {
        path: "docs/WIDGETS.md".into(),
        comments: Vec::new(),
        decisions: symphony::register::parse_decisions(&lines),
    }]
}

/// Every card-kind answer the board asks of each card of `docs` from `plugins`, as one value.
fn read_of(plugins: &Plugins, root: &Path, docs: &[symphony::board::Doc]) -> Vec<String> {
    let mut read = Vec::new();
    for doc in docs {
        let on = doc.path.as_str();
        for card in &doc.decisions {
            read.push(format!(
                "{:?}",
                (
                    plugins.slot(card),
                    plugins.pictures(root, on, card),
                    plugins.approval_needs_agent(card),
                    plugins.triage_category(on, card),
                    plugins.chosen(card),
                    plugins.auto_approvable(card),
                    plugins.gates(on, card),
                )
            ));
        }
    }
    read
}

#[test]
fn a_register_read_of_the_plugins_process_is_one_request_and_answers_as_linked() {
    let tree = Tree::new();
    let project = project(&tree, "timing = true\n");
    let root = tree.root.as_path();
    let docs = register(12);
    let linked = read_of(&Plugins::new(vec![&PIPED]), root, &docs);
    assert!(linked.iter().any(|read| read.contains("North")));
    let piped = Plugins::at(&project, root).reading(root, &docs);
    assert_eq!(read_of(&piped, root, &docs), linked);
    // One request for the read, and no call of a card after it.
    let record = lines(&symphony::plugins::record(&project, root));
    let calls: Vec<String> = record
        .iter()
        .map(|line| format!("{}.{}", line["seam"], line["call"]).replace('"', ""))
        .collect();
    assert_eq!(calls, ["card_kinds.read"]);
    assert_eq!(record[0]["name"], "every");
    assert!(record[0].get("failed").is_none(), "{}", record[0]);
}

/// The test plugin linked, and as the project at `tree` has it from its process, its calls
/// recorded when `timing`.
fn linked_and_piped(tree: &Tree, timing: bool) -> (Plugins, Plugins, Project) {
    let project = project(tree, if timing { "timing = true\n" } else { "" });
    let piped = Plugins::at(&project, &tree.root);
    assert!(process::remote(piped.list()[0]).is_some());
    (Plugins::new(vec![&PIPED]), piped, project)
}

/// A renderer as a page sees it, with no call: its name, extensions and content type.
fn renderer_said(renderer: &dyn Renderer) -> (String, Vec<String>, String) {
    let extensions = renderer
        .extensions()
        .iter()
        .map(|e| e.to_string())
        .collect();
    (
        renderer.name().into(),
        extensions,
        renderer.content_type().into(),
    )
}

#[test]
fn a_plugins_process_draws_its_renderers_files_as_it_does_linked() {
    let tree = Tree::new();
    let (linked, piped, _) = linked_and_piped(&tree, false);
    let (theirs, ours) = (linked.list()[0].renderers(), piped.list()[0].renderers());
    let said = |renderers: &[&dyn Renderer]| -> Vec<_> {
        renderers.iter().map(|r| renderer_said(*r)).collect()
    };
    assert_eq!(said(&ours), said(&theirs));
    assert_eq!(ours.len(), 1);
    let page = || Page::new(&tree.root, "docs/ünï code.md").in_artifacts("out/ä");
    for (src, label) in [("/docs/a.every?v=2#top", ""), ("/docs/b.every", "a widget")] {
        let file = File { src, label };
        let drawn = |plugins: &Plugins| {
            symphony::renderers::renderer_of(plugins, src)
                .unwrap()
                .draw(&page(), &file)
        };
        assert_eq!(drawn(&piped), drawn(&linked), "{src}");
    }
    // The file a page links is drawn by the process's renderer, and served as its type.
    let found = symphony::renderers::renderer_of(&piped, "/docs/b.EVERY").unwrap();
    assert!(std::ptr::addr_eq(found, ours[0]));
    assert_eq!(
        symphony::renderers::content_type(&piped, "docs/b.every"),
        Some("model/every")
    );
}

#[test]
fn an_entrypoint_names_a_type_of_a_plugins_process() {
    let tree = Tree::new();
    let settings = |types: &str| {
        format!(
            "[plugins]\ncrates = [\"every\"]\n\n[plugins.process]\nevery = [{PLUGIN:?}]\n\n\
             [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\ntypes = [{types}]\n"
        )
    };
    let log = tree.path("artifacts/plugins/every.log");
    tree.write(
        symphony::project::FILE,
        &settings("\"markdown\", \"every\""),
    );
    let project = Project::load(&tree.root).unwrap();
    let types: Vec<(&str, &[&str])> = project.entrypoints[0]
        .renderers()
        .map(|renderer| (renderer.name, renderer.extensions))
        .collect();
    assert_eq!(
        types,
        [("markdown", &["md"][..]), ("every", &["every"][..])]
    );
    assert!(log.exists());
    // A type nothing handles is refused, naming the ones there are, its process's among them.
    tree.write(symphony::project::FILE, &settings("\"none\""));
    let refused = Project::load(&tree.root).unwrap_err();
    assert!(
        refused.ends_with("(the renderers are markdown, html, media, every)"),
        "{refused}"
    );
}

#[test]
fn a_plugins_process_that_cannot_say_its_renderers_is_named_by_the_entrypoint_that_needs_one() {
    let tree = Tree::new();
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"every\"]\n\n[plugins.process]\nevery = [\"./no-such-plugin\"]\n\n\
         [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\ntypes = [\"every\"]\n",
    );
    let refused = Project::load(&tree.root).unwrap_err();
    assert!(
        refused.starts_with("symphony.toml: [plugins.process] every cannot say its renderers: "),
        "{refused}"
    );
}

#[test]
fn a_project_whose_plugins_process_could_not_declare_is_read_again_until_it_loads() {
    let tree = Tree::new();
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"every\"]\n\n[plugins.process]\nevery = [\"./bin/plugin\"]\n\n\
         [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\ntypes = [\"markdown\", \"every\"]\n",
    );
    tree.write("models/a.every", "");
    // Its program is not there at the first read: the plain project, saying why and where its log
    // is.
    let started = Instant::now();
    let mut read = vec![Project::of(&tree.root)];
    assert!(read[0].plugins.is_empty());
    let why = Project::problem(&tree.root).unwrap();
    let log = tree.path("artifacts/plugins/every.log");
    assert!(
        why.starts_with("symphony.toml: [plugins.process] every cannot say its renderers: "),
        "{why}"
    );
    assert!(
        why.ends_with(&format!("(its log: {})", log.display())),
        "{why}"
    );
    // Read again once a pause at most while it fails, never at each read: every project read is
    // kept, so a new one is a new load.
    read.extend((0..100).map(|_| Project::of(&tree.root)));
    let loads = read
        .iter()
        .enumerate()
        .filter(|(at, project)| *at == 0 || !std::sync::Arc::ptr_eq(&read[at - 1], project))
        .count();
    let pauses = started.elapsed().as_secs_f64() / symphony::project::AGAIN.as_secs_f64();
    assert!(
        loads as f64 <= 1.0 + pauses,
        "{loads} loads in {pauses} pauses"
    );
    // Its program comes, with no change to the project's file: a read after the pause loads it.
    std::fs::create_dir_all(tree.path("bin")).unwrap();
    std::os::unix::fs::symlink(PLUGIN, tree.path("bin/plugin")).unwrap();
    until("the project loads with its plugin's process", || {
        Project::of(&tree.root).plugins == ["every"]
    });
    let project = Project::of(&tree.root);
    let urls: Vec<&str> = project.entrypoints.iter().map(|e| e.url.as_str()).collect();
    assert_eq!(urls, ["models"]);
    assert_eq!(Project::problem(&tree.root), None);
    // Its later reads are the one project loaded, with no read again.
    assert!(std::sync::Arc::ptr_eq(&project, &Project::of(&tree.root)));
}

#[test]
fn a_plugins_process_serves_its_route_as_it_does_linked() {
    let tree = Tree::new();
    let (linked, piped, _) = linked_and_piped(&tree, false);
    assert!(piped.route("none").is_none());
    for rest in ["", "data.json", "models/a b.stl"] {
        let served = |plugins: &Plugins| plugins.route("every").unwrap().serve(&tree.root, rest);
        assert_eq!(served(&piped), served(&linked), "{rest}");
    }
    assert!(
        piped
            .route("every")
            .unwrap()
            .serve(&tree.root, "data.json")
            .is_some()
    );
}

#[test]
fn a_plugins_process_shows_its_home_region_as_it_does_linked() {
    let tree = Tree::new();
    let (linked, piped, _) = linked_and_piped(&tree, false);
    let ids = |plugins: &Plugins| -> Vec<String> {
        plugins
            .home_regions()
            .iter()
            .map(|r| r.id().to_string())
            .collect()
    };
    assert_eq!(ids(&piped), ids(&linked));
    let region = |plugins: &Plugins| plugins.home_regions()[0].region(&tree.root);
    assert_eq!(region(&piped), region(&linked));
    assert!(region(&piped).is_some());
}

#[test]
fn a_plugins_process_checks_the_documents_as_it_does_linked() {
    let tree = Tree::new();
    let (linked, piped, _) = linked_and_piped(&tree, false);
    tree.write("docs/A.md", "# A\n\nevery word\n");
    tree.write("docs/B.md", "# B\n");
    let docs = Docs::read(&tree.root);
    let (ours, theirs) = (piped.doc_checks()[0], linked.doc_checks()[0]);
    assert_eq!(ours.name(), theirs.name());
    assert_eq!(ours.inputs(), theirs.inputs());
    assert_eq!(ours.check(&docs), theirs.check(&docs));
    assert_eq!(ours.components(&docs), theirs.components(&docs));
    assert!(!ours.check(&docs).is_empty());
    // `symphony status --lint` runs it, as the project's own.
    let expected: Vec<String> = theirs
        .check(&docs)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        symphony::lint::check_findings(&tree.root),
        (expected, vec!["every".to_string()])
    );
}

#[test]
fn a_plugins_process_offers_and_takes_its_board_action_as_it_does_linked() {
    let tree = Tree::new();
    let (linked, piped, _) = linked_and_piped(&tree, false);
    let names = |plugins: &Plugins| -> Vec<String> {
        let actions = plugins.board_actions();
        actions.iter().map(|a| a.name().to_string()).collect()
    };
    assert_eq!(names(&piped), names(&linked));
    for status in ["review", "in_progress"] {
        let task = TaskRef {
            slug: "sim/jump",
            status,
            branch: "sim/jump-2",
        };
        let offered = |plugins: &Plugins| plugins.board_actions()[0].offer(&tree.root, &task);
        assert_eq!(offered(&piped), offered(&linked), "{status}");
        for payload in ["sim/jump@sim/jump-2", "stale"] {
            let submission = Submission {
                payload: payload.into(),
                note: "send it ✅".into(),
            };
            let taken = |plugins: &Plugins| {
                plugins.board_actions()[0].submit(&tree.root, &task, &submission)
            };
            assert_eq!(taken(&piped), taken(&linked), "{status} {payload}");
            // Who sent it crosses the pipe whole, and the plugin answers by it.
            let sender = symphony_plugin::Sender::new("ana", "Ana Lima", vec!["ana-l".into()]);
            let sent = |plugins: &Plugins| {
                plugins.board_actions()[0].submit_from(&tree.root, &task, &submission, &sender)
            };
            assert_eq!(sent(&piped), sent(&linked), "{status} {payload} from ana");
            if let Ok(done) = sent(&piped) {
                assert!(
                    done.said.ends_with(", from ana (Ana Lima, ana-l)"),
                    "{}",
                    done.said
                );
            }
        }
    }
}

#[test]
fn a_plugins_process_warms_and_forgets_its_watch_as_it_does_linked() {
    let tree = Tree::new();
    let (linked, piped, project) = linked_and_piped(&tree, true);
    let paths = |plugins: &Plugins| -> Vec<Vec<String>> {
        plugins.watches().iter().map(|w| w.paths()).collect()
    };
    assert_eq!(paths(&piped), paths(&linked));
    let watch = piped.watches()[0];
    assert_eq!(
        watch.changed(&tree.root),
        linked.watches()[0].changed(&tree.root)
    );
    watch.forget(&tree.root);
    // Both calls went to the process, named by the watch's paths, and each was answered.
    let record = lines(&symphony::plugins::record(&project, &tree.root));
    let calls: Vec<String> = record
        .iter()
        .map(|line| format!("{}.{} {}", line["seam"], line["call"], line["name"]).replace('"', ""))
        .collect();
    assert_eq!(
        calls,
        [
            "watches.changed data/every, pictures",
            "watches.forget data/every, pictures"
        ]
    );
    assert!(record.iter().all(|line| line.get("failed").is_none()));
}

/// The task each gates test asks of: a branch's, in review.
const TASK: TaskRef<'static> = TaskRef {
    slug: "tools/a",
    status: "review",
    branch: "tools/a-2",
};

/// The gates `answerers` give [`TASK`] at `tip` in the project at `root`, read through `kept` as
/// the task's page reads them.
fn read(
    kept: &Kept,
    root: &Path,
    answerers: &[(&str, &'static dyn Gates)],
    tip: Option<&str>,
) -> Vec<Gate> {
    let asked = Asked {
        slug: TASK.slug.into(),
        status: TASK.status.into(),
        branch: TASK.branch.into(),
        tip: tip.map(str::to_string),
    };
    theirs(kept, root, answerers, &asked)
}

/// A wait only a stall outlasts, for a call that is answered.
const PATIENT: Duration = test_time::backstop(process::WAIT);

/// What `ask` gives once it gives something, asked again while it gives nothing: a call in a test's
/// short wait may have no answer in it on a loaded machine, and only a stall leaves it none past
/// [`PATIENT`].
fn answered<T>(what: &str, ask: impl FnMut() -> Option<T>) -> T {
    test_time::until(what, PATIENT, ask).unwrap_or_else(|stall| panic!("{stall}"))
}

/// The seam tests' plugin's gates answerers linked, each with its plugin's name.
fn linked_answerers() -> Vec<(&'static str, &'static dyn Gates)> {
    PIPED.gates().into_iter().map(|g| ("every", g)).collect()
}

/// A worktree of the checkout at `tree`, as git links one, with the same `symphony.toml`.
fn worktree(tree: &Tree) -> std::path::PathBuf {
    tree.write(".git/worktrees/wt/commondir", "../..\n");
    tree.write(
        "wt/.git",
        &format!("gitdir: {}\n", tree.path(".git/worktrees/wt").display()),
    );
    let file = std::fs::read_to_string(tree.path(symphony::project::FILE)).unwrap();
    tree.write("wt/symphony.toml", &file);
    tree.path("wt")
}

#[test]
fn a_plugins_process_answers_its_gates_as_it_does_linked_for_a_checkout_and_its_branches() {
    let tree = Tree::checkout();
    let (_, _, project) = linked_and_piped(&tree, false);
    let root = tree.root.as_path();
    let ours = symphony::gates::answerers(&project, root);
    let names: Vec<(&str, &str)> = ours.iter().map(|(p, g)| (*p, g.name())).collect();
    assert_eq!(names, [("every", "every"), ("every", "slow")]);
    // A task on a branch at its tip, and one whose branch the repository does not hold.
    for ((_, far), (_, near)) in ours.iter().zip(linked_answerers()) {
        for tip in [Some("c0ffee"), None] {
            assert_eq!(
                far.answer(root, &TASK, tip),
                near.answer(root, &TASK, tip),
                "{} {tip:?}",
                near.name()
            );
        }
        assert_eq!(far.kept_while(root, &TASK), near.kept_while(root, &TASK));
    }
    // With no branch, `every`'s own error, as it said it.
    assert_eq!(
        ours[0].1.answer(root, &TASK, None),
        Err("tools/a-2 has no branch".into())
    );
    // A branch's checkout is answered by its main checkout's process, each call naming the
    // branch's checkout, which `every` reads.
    let branch = worktree(&tree);
    let in_branch = symphony::gates::answerers(&Project::of(&branch), &branch);
    let answering = process::remote(Plugins::of(&branch).list()[0]).unwrap();
    assert_ne!(answering.process().root, branch);
    let tip = Some("c0ffee");
    assert_eq!(
        in_branch[0].1.answer(&branch, &TASK, tip),
        linked_answerers()[0].1.answer(&branch, &TASK, tip)
    );
    assert_eq!(
        in_branch[0].1.kept_while(&branch, &TASK),
        linked_answerers()[0].1.kept_while(&branch, &TASK)
    );
}

#[test]
fn a_plugins_process_gates_are_kept_as_a_linked_ones_are_each_asked_once_a_tip() {
    let tree = Tree::new();
    let (_, _, project) = linked_and_piped(&tree, true);
    let root = tree.root.as_path();
    let ours = symphony::gates::answerers(&project, root);
    // An error kept as long as only a stall outlasts, so what is asked is a count alone.
    let kept_until_asked_again = || {
        Kept::new(symphony::gates::Limits {
            retry: PATIENT,
            ..symphony::gates::Limits::default()
        })
    };
    let (kept, kept_linked) = (kept_until_asked_again(), kept_until_asked_again());
    for tip in [Some("c0ffee"), Some("c0ffee"), Some("decade"), None, None] {
        assert_eq!(
            read(&kept, root, &ours, tip),
            read(&kept_linked, root, &linked_answerers(), tip),
            "{tip:?}"
        );
    }
    // With no branch, `every`'s error stands in for the gate it answered before, as it said it.
    let none = read(&kept, root, &ours, None);
    assert_eq!(
        (
            none[0].name.as_str(),
            none[0].state,
            none[0].detail.as_str()
        ),
        ("certificate", GateState::Unknown, "tools/a-2 has no branch")
    );
    // Each answerer was asked once a tip, its error kept, and what it is kept while at each read.
    let record = lines(&symphony::plugins::record(&project, root));
    let asked = |call: &str, name: &str| {
        let what = |line: &&serde_json::Value| {
            line["seam"] == "gates" && line["call"] == call && line["name"] == name
        };
        record.iter().filter(what).count()
    };
    for name in ["every", "slow"] {
        assert_eq!(asked("answer", name), 3, "{name}");
        assert_eq!(asked("kept_while", name), 6, "{name}");
    }
    assert!(record.iter().all(|line| line.get("failed").is_none()));
}

#[test]
fn a_plugins_process_that_fails_mid_answer_reads_unknown_and_never_as_a_pass() {
    let tree = Tree::new();
    let (_, piped, project) = linked_and_piped(&tree, false);
    let root = tree.root.as_path();
    let remote = process::remote(piped.list()[0]).unwrap();
    let slow = vec![symphony::gates::answerers(&project, root)[1]];
    let kept = Kept::default();
    let met = read(&kept, root, &slow, Some("c0ffee"));
    assert_eq!(
        (met[0].name.as_str(), met[0].state),
        ("slow", GateState::Met)
    );
    let pid = remote.pid().unwrap();
    // At the next tip its answer hangs, and its process is killed while the call is in it.
    let failed = std::thread::scope(|scope| {
        let reading = scope.spawn(|| read(&kept, root, &slow, Some("hang")));
        until("the call is in the plugin", || {
            tree.path(super::every::HANGING).exists()
        });
        let killed = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .unwrap();
        assert!(killed.success());
        reading.join().unwrap()
    });
    assert_eq!(
        (
            failed[0].name.as_str(),
            failed[0].state,
            failed[0].by.as_deref()
        ),
        ("slow", GateState::Unknown, Some("slow"))
    );
    assert_eq!(
        failed[0].detail,
        format!("plugin every: its process {pid} was killed by signal 9")
    );
}

#[test]
fn a_slow_gates_answer_of_a_plugins_process_is_kept_as_a_linked_ones_and_leaves_its_calls_a_worker()
{
    use symphony_plugin::authority::{Operation, Snapshot};
    use symphony_plugin::execution::{Principal, PrincipalKind};
    let tree = Tree::new();
    let root = tree.root.as_path();
    let process = PluginProcess {
        command: vec![PLUGIN.into()],
        root: tree.root.clone(),
        log: tree.path("artifacts/plugins/every.log"),
    };
    // A call's wait far shorter than its answers are held, and a cancel's only a stall outlasts, so
    // a call a loaded machine leaves unanswered in that wait ends nothing.
    let wait = Duration::from_millis(50);
    let limits = process::Limits {
        wait,
        cancel: PATIENT,
        declare: PATIENT,
        ..process::Limits::default()
    };
    let remote = process::Remote::leaked(
        "every",
        process.clone(),
        process::spawning(&process),
        limits,
    );
    let fifo = tree.path(super::every::HELD);
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    assert!(made.unwrap().success());
    let slow: Vec<(&str, &'static dyn Gates)> = vec![("every", remote.gates()[1])];
    let linked_slow = vec![linked_answerers()[1]];
    let pid = remote.pid().unwrap();
    let kept = Kept::new(symphony::gates::Limits {
        wait,
        retry: PATIENT,
        ceiling: PATIENT,
        ..symphony::gates::Limits::default()
    });
    let asked = |n: usize| Asked {
        slug: format!("tools/held-{n}"),
        status: "review".into(),
        branch: format!("tools/held-{n}"),
        tip: Some(format!("held-{n}")),
    };
    let held_since = Instant::now();
    // Four tasks read, each answer held: unknown while it goes on, as a linked one's slow answer.
    for n in 1..=4 {
        let read = answered("its answer is asked", || {
            let gates = theirs(&kept, root, &slow, &asked(n));
            (gates[0].detail == "slow gave no answer within 0.05 s").then_some(gates)
        });
        assert_eq!(
            (read[0].name.as_str(), read[0].state),
            ("slow", GateState::Unknown)
        );
    }
    // Three are in the plugin, holding three of its four workers; the fourth waits in the host.
    let marks = || {
        let held = |n: &usize| tree.path(&format!("{}-{n}", super::every::HELD)).exists();
        (1..=4).filter(held).count()
    };
    until("three answers are in the plugin", || marks() == 3);
    // Its other calls answer on the worker left: its authority, what an answer is kept while, a
    // component.
    let operation = Operation::new(
        "sim".into(),
        "sim/a".into(),
        "accept".into(),
        "approved".into(),
        Snapshot::new("d1".into(), 2, 3, "verify".into(), "awaiting".into()),
        BTreeMap::new(),
        Principal::new(PrincipalKind::Human, "local".into(), "owner".into()),
    );
    let within = Duration::from_millis(2500);
    let attested = answered("its authority attests", || {
        remote.authorities()[0]
            .attest(root, &operation, within)
            .ok()
    });
    assert_eq!(
        Ok(attested),
        PIPED.authorities()[0].attest(root, &operation, within)
    );
    let told = answered("what an answer is kept while is told", || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            slow[0].1.kept_while(root, &TASK)
        }))
        .ok()
    });
    assert_eq!(told, linked_slow[0].1.kept_while(root, &TASK));
    let page = Page::new(root, "docs/A.md");
    let drawn = answered("a component draws", || {
        remote.components()[0].render(&page, "lantern", None).ok()
    });
    assert_eq!(
        Ok(drawn),
        PIPED.components()[0].render(&page, "lantern", None)
    );
    // The held answers outlast a call's wait, none given up, and the fourth still waits its turn:
    // each of the four reads above gave up only once its own wait had passed.
    assert!(
        held_since.elapsed() > wait * 4,
        "{:?}",
        held_since.elapsed()
    );
    assert_eq!(marks(), 3);
    // Let go, a byte for each held: the three answer, then the fourth in its turn, at once with the
    // FIFO gone.
    let before = kept.answered();
    let mut letting_go = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&fifo)
        .unwrap();
    std::fs::remove_file(&fifo).unwrap();
    std::io::Write::write_all(&mut letting_go, &[0; 3]).unwrap();
    until("the four answers came", || kept.answered() == before + 4);
    // Each is kept, and read as a linked answer for its task, with no answer asked again.
    let kept_linked = Kept::default();
    for n in 1..=4 {
        let ours = answered("its kept answer is read", || {
            let gates = theirs(&kept, root, &slow, &asked(n));
            (gates[0].state != GateState::Unknown).then_some(gates)
        });
        assert_eq!(
            ours,
            theirs(&kept_linked, root, &linked_slow, &asked(n)),
            "{n}"
        );
    }
    assert_eq!(kept.answered(), before + 4);
    // The process was never ended: the one started first answered throughout.
    assert_eq!(remote.pid(), Some(pid));
    let log = std::fs::read_to_string(&process.log).unwrap();
    assert!(!log.contains("was ended"), "{log}");
    assert!(!log.contains("gates.answer"), "{log}");
}

#[test]
fn a_plugins_process_declaring_gates_commands_and_an_authority_is_accepted_and_each_answers() {
    use symphony_plugin::authority::{Operation, Snapshot};
    use symphony_plugin::execution::{Principal, PrincipalKind};
    let tree = Tree::new();
    let (linked, piped, project) = linked_and_piped(&tree, false);
    let declared = process::remote(piped.list()[0])
        .unwrap()
        .declaration()
        .unwrap();
    assert_eq!(declared.gates, ["every", "slow"]);
    assert_eq!(declared.authorities, ["every"]);
    let commands: Vec<&str> = declared.commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(commands, ["every"]);
    assert_eq!(symphony::plugins::problems(&project), Vec::<String>::new());
    // Its command is the project's, as `symphony NAME` finds it.
    let found = symphony::plugins::command("every", Some(&tree.root)).unwrap();
    assert!(std::ptr::addr_eq(found, piped.list()[0].commands()[0]));
    // Its authority attests as linked, found by its adapter as the owner's take finds it.
    let operation = Operation::new(
        "sim".into(),
        "sim/a".into(),
        "accept".into(),
        "approved".into(),
        Snapshot::new("d1".into(), 2, 3, "verify".into(), "awaiting".into()),
        BTreeMap::new(),
        Principal::new(PrincipalKind::Human, "local".into(), "owner".into()),
    );
    for within in [Duration::from_millis(2500), Duration::ZERO] {
        let attested = |plugins: &Plugins| {
            let authority = plugins.authority("every").unwrap();
            authority.attest(&tree.root, &operation, within)
        };
        assert_eq!(attested(&piped), attested(&linked), "{within:?}");
    }
    let signed = piped.authority("every").unwrap().attest(
        &tree.root,
        &operation,
        Duration::from_millis(2500),
    );
    assert!(signed.is_ok(), "{signed:?}");
}

#[test]
fn a_plugins_process_gives_its_navigation_and_theme_from_its_declaration_with_no_call() {
    let tree = Tree::new();
    tree.write(
        symphony::project::FILE,
        &format!(
            "[plugins]\ncrates = [\"every\"]\ntiming = true\n\n[plugins.process]\nevery = \
             [{PLUGIN:?}]\n\n[theme]\npackage = \"every\"\n"
        ),
    );
    let project = Project::load(&tree.root).unwrap();
    let (linked, piped) = (
        Plugins::new(vec![&PIPED]),
        Plugins::at(&project, &tree.root),
    );
    assert_eq!(piped.nav(), linked.nav());
    assert_eq!(piped.nav().len(), 1);
    assert_eq!(piped.theme("every"), linked.theme("every"));
    assert!(piped.theme("other").is_none());
    // The project's theme package is the process's.
    assert_eq!(symphony::theme::Package::of(&project).name, "Every");
    // A name the package gives is the theme's own: the header opens with it.
    assert_eq!(
        symphony::theme::Package::of(&project).wordmark(),
        Some("Every")
    );
    assert_eq!(project.view().wordmark, "Every");
    let package = symphony::theme::Package::of(&project);
    assert_eq!(package.css(), ":root{--background:white;--primary:red;}\n");
    assert_eq!(
        package.layouts,
        std::collections::BTreeMap::from([("header".into(), "compact".into())])
    );
    assert_eq!(symphony::theme::problems(&project).len(), 2);
    assert!(
        symphony::theme::problems(&project)
            .iter()
            .any(|problem| problem.contains("unsafe"))
    );
    assert!(
        symphony::theme::problems(&project)
            .iter()
            .any(|problem| problem.contains("unknown"))
    );
    let mut overridden = project.clone();
    overridden
        .theme
        .tokens
        .insert("primary".into(), "blue".into());
    overridden
        .theme
        .layouts
        .insert("header".into(), "default".into());
    let package = symphony::theme::Package::of(&overridden);
    assert_eq!(package.css(), ":root{--background:white;--primary:blue;}\n");
    assert!(package.layouts.is_empty());
    assert!(lines(&symphony::plugins::record(&project, &tree.root)).is_empty());
}

#[test]
fn a_plugins_process_declares_its_command_and_symphony_finds_it_in_the_project() {
    let tree = Tree::new();
    let (linked, piped, project) = linked_and_piped(&tree, true);
    let (theirs, ours) = (linked.list()[0].commands(), piped.list()[0].commands());
    let said = |commands: &[&dyn Command]| -> Vec<(String, String)> {
        let said = |c: &&dyn Command| (c.name().to_string(), c.summary().to_string());
        commands.iter().map(said).collect()
    };
    assert_eq!(said(&ours), said(&theirs));
    assert_eq!(ours.len(), 1);
    // `symphony NAME` in the project finds the process's command, and its listing names it once.
    let found = symphony::plugins::command("every", Some(&tree.root)).unwrap();
    assert!(std::ptr::addr_eq(found, ours[0]));
    let listed = symphony::plugins::commands_of(Some(&tree.root));
    assert_eq!(
        listed.listed.iter().filter(|c| c.name() == "every").count(),
        1
    );
    assert!(listed.failed.is_empty());
    // A command is no call on the pipe, so none is recorded.
    assert!(lines(&symphony::plugins::record(&project, &tree.root)).is_empty());
}

#[test]
fn symphony_runs_a_command_of_a_plugins_process_with_its_streams_as_it_runs_one_linked() {
    let tree = Tree::new();
    project(&tree, "");
    tree.write("docs/a/.keep", "");
    // Run from a folder of the project, which it finds above.
    let symphony = |args: &[&str]| {
        let done = std::process::Command::new(env!("CARGO_BIN_EXE_symphony"))
            .args(args)
            .current_dir(tree.path("docs/a"))
            .output()
            .unwrap();
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        (
            done.status.code().unwrap(),
            text(done.stdout),
            text(done.stderr),
        )
    };
    let linked = symphony_plugin::Plugin::commands(&super::every::EVERY)[0];
    for args in [&[][..], &["--all", "a b", "", "ünï"]] {
        let owned: Vec<String> = args.iter().map(|&arg| arg.into()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = linked.run(&owned, &mut out, &mut err);
        let text = |bytes| String::from_utf8(bytes).unwrap();
        let ran = symphony(&[&["every"][..], args].concat());
        assert_eq!(ran, (code, text(out), text(err)), "{args:?}");
    }
    // Its listing names the command with the summary the process declared.
    let (code, listing, _) = symphony(&["help"]);
    assert_eq!(code, 0);
    let line = |line: &&str| line.split_whitespace().next() == Some("every");
    let named: Vec<&str> = listing.lines().filter(line).collect();
    assert_eq!(named.len(), 1, "{listing}");
    assert!(named[0].ends_with("  print its arguments"), "{listing}");
}

#[test]
fn a_plugin_the_project_gives_a_process_that_does_not_start_has_no_linked_command() {
    // This process holds the plugin linked, and runs its command where no process answers for it.
    let tree = super::compiled::naming(&["every"]);
    let linked = symphony::plugins::command("every", Some(&tree.root)).unwrap();
    let own = symphony_plugin::Plugin::commands(&super::every::EVERY)[0];
    assert!(std::ptr::addr_eq(linked, own));
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"every\"]\n\n[plugins.process]\nevery = [\"/no/such/plugin\"]\n",
    );
    // A copy this process holds is not the project's plugin, so it does not stand in for it: the
    // command is not found, naming the plugin whose process failed and its log.
    let failed = symphony::plugins::command("every", Some(&tree.root))
        .err()
        .unwrap();
    let said = "the plugin every's process failed, so its commands are not known: /no/such/plugin \
                cannot start: ";
    let log = format!(
        "(its log: {})",
        tree.path("artifacts/plugins/every.log").display()
    );
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(
        failed[0].starts_with(said) && failed[0].ends_with(&log),
        "{failed:?}"
    );
    let listed = symphony::plugins::commands_of(Some(&tree.root));
    assert!(listed.listed.iter().all(|c| c.name() != "every"));
    assert_eq!(listed.failed.len(), 1, "{:?}", listed.failed);
    assert!(listed.failed[0].starts_with(said), "{:?}", listed.failed);
    // Nor while the project does not load, an entrypoint needing a renderer of that process: the
    // command is not found, the project's problem saying why, naming the plugin and its log.
    tree.write("models/a.md", "# A\n");
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"every\"]\n\n[plugins.process]\nevery = [\"/no/such/plugin\"]\n\n\
         [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\ntypes = [\"markdown\", \"every\"]\n",
    );
    let failed = symphony::plugins::command("every", Some(&tree.root))
        .err()
        .unwrap();
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(
        failed[0].starts_with(
            "the project does not load, so its plugins' commands are not known: symphony.toml: \
             [plugins.process] every cannot say its renderers: /no/such/plugin cannot start: "
        ) && failed[0].ends_with(&log),
        "{failed:?}"
    );
    let listed = symphony::plugins::commands_of(Some(&tree.root));
    assert!(listed.listed.iter().all(|c| c.name() != "every"));
    assert_eq!(listed.failed.len(), 1, "{:?}", listed.failed);
}

#[test]
fn symphony_names_a_plugins_process_that_failed_for_a_command_it_does_not_know() {
    let tree = Tree::new();
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"shelf\"]\n\n[plugins.process]\nshelf = [\"./bin/shelf-plugin\"]\n",
    );
    let symphony = |args: &[&str]| {
        let done = std::process::Command::new(env!("CARGO_BIN_EXE_symphony"))
            .args(args)
            .current_dir(&tree.root)
            .output()
            .unwrap();
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        (
            done.status.code().unwrap(),
            text(done.stdout),
            text(done.stderr),
        )
    };
    // The project as `symphony` finds it, from the folder it runs in.
    let root = tree.root.canonicalize().unwrap();
    let failed = format!(
        "symphony: the plugin shelf's process failed, so its commands are not known: {} cannot \
         start: ",
        root.join("bin/shelf-plugin").display()
    );
    let log = format!(
        "(its log: {})\n",
        root.join("artifacts/plugins/shelf.log").display()
    );
    // A command it may add is refused naming the plugin, why and its log, then as unknown: exit 1.
    let (code, out, err) = symphony(&["render-models", "a", "b"]);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    let (first, rest) = err.split_once('\n').unwrap();
    assert!(first.starts_with(&failed), "{err}");
    assert!(format!("{first}\n").ends_with(&log), "{err}");
    assert!(
        rest.starts_with("symphony: unknown command \"render-models\""),
        "{err}"
    );
    // `help` lists the commands there are and says the same after them; `help NAME` refuses as the
    // command does.
    let (code, listing, _) = symphony(&["help"]);
    assert_eq!(code, 0);
    let note = listing.rsplit_once("\n\n").unwrap().1;
    assert!(
        note.starts_with(&failed) && note.ends_with(&log),
        "{listing}"
    );
    let (code, _, err) = symphony(&["help", "render-models"]);
    assert_eq!(code, 1);
    assert!(err.starts_with(&failed), "{err}");
    // A project that does not load, an entrypoint needing a renderer of that process, has no
    // plugin's command: the refusal says why, naming the plugin and its log, and exits 1; `help`
    // says the same.
    tree.write("models/a.md", "# A\n");
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"shelf\"]\n\n[plugins.process]\nshelf = [\"./bin/shelf-plugin\"]\n\n\
         [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\ntypes = [\"markdown\", \"shelf\"]\n",
    );
    let refused = format!(
        "symphony: the project does not load, so its plugins' commands are not known: \
         symphony.toml: [plugins.process] shelf cannot say its renderers: {} cannot start: ",
        root.join("bin/shelf-plugin").display()
    );
    let (code, _, err) = symphony(&["render-models", "a", "b"]);
    assert_eq!(code, 1, "{err}");
    let said: Vec<&str> = err
        .lines()
        .filter(|line| line.starts_with("symphony: "))
        .collect();
    assert_eq!(said.len(), 2, "{err}");
    assert!(
        said[0].starts_with(&refused) && format!("{}\n", said[0]).ends_with(&log),
        "{err}"
    );
    assert!(
        said[1].starts_with("symphony: unknown command \"render-models\""),
        "{err}"
    );
    let (code, listing, _) = symphony(&["help"]);
    assert_eq!(code, 0);
    let note = listing.rsplit_once("\n\n").unwrap().1;
    assert!(
        note.starts_with(&refused) && note.ends_with(&log),
        "{listing}"
    );
    // A project whose processes all declared refuses an unknown command naming no plugin: exit 2.
    project(&tree, "");
    let (code, _, err) = symphony(&["render-models"]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.starts_with("symphony: unknown command \"render-models\""),
        "{err}"
    );
}

#[test]
fn a_plugins_process_killed_mid_call_fails_that_call_alone() {
    let tree = Tree::new();
    let (linked, piped, project) = linked_and_piped(&tree, false);
    let remote = process::remote(piped.list()[0]).unwrap();
    assert!(remote.declaration().is_ok());
    let pid = remote.pid().unwrap();
    let page = Page::new(&tree.root, "docs/A.md");
    let hanging = {
        let piped = piped.clone();
        let root = tree.root.clone();
        std::thread::spawn(move || {
            let page = Page::new(&root, "docs/A.md");
            format!("{:?}", piped.render(&page, "slow", "hang", None))
        })
    };
    // Killed while the call is in the plugin: once its component marked the page's root.
    until("the call is in the plugin", || {
        tree.path(super::every::HANGING).exists()
    });
    let killed = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let failed = hanging.join().unwrap();
    assert!(
        failed.contains(&format!("its process {pid} was killed by signal 9")),
        "{failed}"
    );
    // The next calls, once its pause is over, answer as linked, from a process started again.
    until("the plugin is down", || remote.down().is_some());
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test waits out the plugin's restart pause"
    )]
    std::thread::sleep(PAUSE);
    assert_eq!(
        piped.render(&page, "every", "a", None),
        linked.render(&page, "every", "a", None)
    );
    assert_ne!(remote.pid().unwrap(), pid);
    assert_eq!(
        piped.home_regions()[0].region(&tree.root),
        linked.home_regions()[0].region(&tree.root)
    );
    let log = std::fs::read_to_string(&project.plugin_processes["every"].log).unwrap();
    assert!(
        log.contains(&format!("its process {pid} was killed by signal 9")),
        "{log}"
    );
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
fn a_killed_plugin_process_starts_again_after_its_pause() {
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
    // After the pause the next call starts it again, and the page is as it was.
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test waits out the plugin's restart pause"
    )]
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
    // A relative program stays inside the checkout.
    for program in ["../a", "bin/../../a", "./../a"] {
        assert_eq!(
            parsed(&format!(
                "[plugins]\ncrates = [\"a\"]\n[plugins.process]\na = [{program:?}]\n"
            ))
            .err(),
            Some(format!(
                "symphony.toml: [plugins.process] a names {program:?}: a relative program is a \
                 path inside the checkout, with no `..`"
            )),
            "{program}"
        );
    }
}

#[test]
fn a_plugins_process_ends_when_its_input_does_whatever_its_calls_are_doing() {
    use symphony_board::sys::{Command, Stdio};
    let mut child = Command::new(PLUGIN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A call that never returns in this test's time, then its input ends.
    let page = PageData::of(&Page::new(Path::new("/r"), "docs/A.md"));
    let hang = symphony_plugin::protocol::Render::new("slow".into(), page, "hang".into(), None);
    let line = serde_json::to_string(&Envelope::<Request, ()>::asking(1, hang.into())).unwrap();
    let mut input = child.stdin.take().unwrap();
    // The call is read before the input's end, which the pipe gives after it.
    writeln!(input, "{line}").unwrap();
    let ended = Instant::now();
    drop(input);
    let status = test_time::until(
        "the process ended after its input",
        test_time::backstop(Duration::from_secs(10)),
        || child.try_wait().unwrap(),
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    assert_eq!(status.code(), Some(1));
    assert!(ended.elapsed() >= symphony_plugin::ENDING);
    let mut said = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut said)
        .unwrap();
    assert_eq!(
        said,
        "every: its input ended and a call still runs 1s after: the process exits\n"
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

/// The microseconds the measurement's known call spends in the plugin: a render of `slow`.
const SPENT: u64 = 500;

/// The calls of a component and of a card kind of [`PIPED`], one each, and the render of `slow`
/// that spends [`SPENT`], as the measurement makes them.
fn component_and_kind_calls(plugins: &Plugins, root: &Path) {
    use std::hint::black_box;
    let page = Page::new(root, "docs/A.md");
    let _ = black_box(plugins.render(&page, "every", "lantern", Some("wide")));
    let _ = black_box(plugins.render(&page, "slow", &SPENT.to_string(), None));
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

/// Each call's values of `key` in the record at `path`, by `seam.call` and the object's name, so
/// the render of `slow` stands apart from `every`'s.
fn by_call_and_name(path: &Path, key: &str) -> BTreeMap<String, Vec<f64>> {
    let mut found: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for line in lines(path) {
        if let Some(value) = line[key].as_f64() {
            let (seam, call, name) = (&line["seam"], &line["call"], &line["name"]);
            let (seam, call, name) = (seam.as_str(), call.as_str(), name.as_str());
            let named = format!("{}.{} {}", seam.unwrap(), call.unwrap(), name.unwrap());
            found.entry(named).or_default().push(value);
        }
    }
    found
}

/// What each call of a component and a card kind costs linked and from the plugin's own process:
/// a measurement, not a check. Run alone, it prints each call's medians in microseconds: linked
/// (the call timed alone), the process's (the host's wait for its answer) and the plugin's own
/// time there, and the bytes of the process's two lines; how much more a call costs from the
/// process than linked, over every call; the render of `slow`, which spends [`SPENT`] in the
/// plugin, so each meter is seen to move by it; and a page of ten components drawn linked and from
/// the process, neither recorded. Best in a release build on a quiet machine, run twice
/// (docs/engineering/PLUGIN_PROTOCOL.md says the command).
#[test]
#[ignore = "a measurement, not a check: run it alone with --ignored --nocapture"]
fn the_cost_of_a_plugins_calls_linked_and_from_its_process() {
    const ROUNDS: usize = 1_000;
    const PAGES: usize = 200;
    let tree = Tree::new();
    let root = tree.root.as_path();
    let timed_record = tree.path("timed.jsonl");
    let timing = PluginCalls { timing: true };
    let timed = Plugins::new(vec![&PIPED]).measured(timing, &timed_record);
    let recorded = project(&tree, "timing = true\n");
    let piped = Plugins::at(&recorded, root);
    let piped_record = symphony::plugins::record(&recorded, root);
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
        component_and_kind_calls(&piped, root);
    }
    let median_of = |table: &BTreeMap<String, Vec<f64>>, name: &str| {
        median(&mut table.get(name).cloned().unwrap_or_default())
    };
    let linked_us = by_call_and_name(&timed_record, "us");
    let (piped_trip, piped_us) = (
        by_call_and_name(&piped_record, "round_trip_us"),
        by_call_and_name(&piped_record, "us"),
    );
    let (asked, answered) = (
        by_call_and_name(&piped_record, "request_bytes"),
        by_call_and_name(&piped_record, "answer_bytes"),
    );
    println!(
        "{ROUNDS} rounds; medians in microseconds\n{:<38} {:>9} {:>9} {:>9} {:>7} {:>7}",
        "call", "linked", "process", "in it", "req B", "ans B"
    );
    let mut more = Vec::new();
    for name in linked_us.keys() {
        let (linked, process) = (median_of(&linked_us, name), median_of(&piped_trip, name));
        println!(
            "{name:<38} {linked:>9.3} {process:>9.3} {:>9.3} {:>7} {:>7}",
            median_of(&piped_us, name),
            median_of(&asked, name),
            median_of(&answered, name),
        );
        if !name.ends_with(" slow") {
            more.push((process - linked, name.clone()));
        }
    }
    symphony_board::order::sort_by(&mut more, &|a, b| a.0.total_cmp(&b.0));
    if let (Some((least, cheapest)), Some((most, dearest))) = (more.first(), more.last()) {
        println!(
            "from the process, a call costs {least:.1} µs ({cheapest}) to {most:.1} µs ({dearest}) \
             more than linked, over every call measured"
        );
    }
    // The known call: each meter moves by what it spends, the process's own time and its wait.
    let slow = "components.render slow";
    let (linked, inside, waited) = (
        median_of(&linked_us, slow),
        median_of(&piped_us, slow),
        median_of(&piped_trip, slow),
    );
    let spent = f64::from(u32::try_from(SPENT).unwrap());
    println!(
        "{slow} spends {SPENT} µs: linked {linked:.1}, in the process {inside:.1}, the process's \
         round trip {waited:.1}"
    );
    assert!(
        linked >= spent && inside >= spent && waited >= inside,
        "the meters did not move"
    );
    // Eight card-kind calls a card: what a register read would cost of each card, by the medians.
    let per_card = |table: &BTreeMap<String, Vec<f64>>| -> f64 {
        table
            .keys()
            .filter(|name| name.starts_with("card_kinds."))
            .map(|name| median_of(table, name))
            .sum()
    };
    println!(
        "a card's eight card-kind calls: linked {:.1} µs, from the process {:.1} µs; a register \
         read is that times its cards, an extrapolation, not a read measured",
        per_card(&linked_us),
        per_card(&piped_trip)
    );
    // A page of ten components, drawn whole as a document is, neither page recorded.
    let quiet = Tree::new();
    let doc = "# A\n\n".to_string() + &"[[every:lantern|wide]] [[again:b]]\n\n".repeat(5);
    let (linked_page, piped_page) = (
        Render::new(root).with_plugins(Plugins::new(vec![&PIPED])),
        Render::new(&quiet.root).with_plugins(Plugins::at(&project(&quiet, ""), &quiet.root)),
    );
    let (mut linked_ms, mut piped_ms) = (Vec::new(), Vec::new());
    for _ in 0..PAGES {
        for (render, times) in [(&linked_page, &mut linked_ms), (&piped_page, &mut piped_ms)] {
            let started = Instant::now();
            let _ = std::hint::black_box(render.html(&doc, "docs/A.md"));
            times.push(started.elapsed().as_secs_f64() * 1e3);
        }
    }
    assert!(!symphony::plugins::record(&recorded, &quiet.root).exists());
    println!(
        "a page of 10 components, {PAGES} renders, unrecorded; medians in milliseconds: linked \
         {:.3}, process {:.3}",
        median(&mut linked_ms),
        median(&mut piped_ms)
    );
}

/// What a register read of the test plugin's card kinds costs linked, from its process card by
/// card, and from its process in one request (`card_kinds.read`): a measurement, not a check. Run
/// alone, it prints each read's median in milliseconds over registers of 50 and 500 cards, the
/// reads interleaved and none recorded, and the bytes of the batch's lines. Best in a release
/// build on a quiet machine (docs/engineering/PLUGIN_PROTOCOL.md says the command).
#[test]
#[ignore = "a measurement, not a check: run it alone with --ignored --nocapture"]
fn the_cost_of_a_register_read_linked_card_by_card_from_its_process_and_batched() {
    const READS: usize = 30;
    let tree = Tree::new();
    let root = tree.root.as_path();
    let linked = Plugins::new(vec![&PIPED]);
    let piped = Plugins::at(&project(&tree, ""), root);
    assert!(
        process::remote(piped.list()[0])
            .unwrap()
            .declaration()
            .is_ok()
    );
    println!("medians in milliseconds of {READS} reads of each register, unrecorded");
    for count in [50, 500] {
        let docs = register(count);
        let expected = read_of(&linked, root, &docs);
        let (mut alone, mut by_card, mut batched) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..READS {
            let timed = |times: &mut Vec<f64>, read: &dyn Fn() -> Vec<String>| {
                let started = Instant::now();
                let answered = std::hint::black_box(read());
                times.push(started.elapsed().as_secs_f64() * 1e3);
                assert_eq!(answered, expected);
            };
            timed(&mut alone, &|| read_of(&linked, root, &docs));
            timed(&mut by_card, &|| read_of(&piped, root, &docs));
            timed(&mut batched, &|| {
                read_of(&piped.reading(root, &docs), root, &docs)
            });
        }
        println!(
            "{count} cards: linked {:.3}, from the process card by card {:.3}, batched {:.3}",
            median(&mut alone),
            median(&mut by_card),
            median(&mut batched)
        );
    }
    // The batch's lines, as a recorded read sees them.
    let recorded = Tree::new();
    let project = project(&recorded, "timing = true\n");
    let plugins = Plugins::at(&project, &recorded.root);
    let docs = register(500);
    let _ = read_of(
        &plugins.reading(&recorded.root, &docs),
        &recorded.root,
        &docs,
    );
    for line in lines(&symphony::plugins::record(&project, &recorded.root)) {
        println!(
            "500 cards in one {}.{}: request {} B, answer {} B, {} µs in the plugin, {} µs waited",
            line["seam"].as_str().unwrap(),
            line["call"].as_str().unwrap(),
            line["request_bytes"],
            line["answer_bytes"],
            line["us"],
            line["round_trip_us"]
        );
    }
}

#[test]
fn process_command_maps_keep_key_order_and_refuse_the_first_invalid_name() {
    let root = Path::new("/r");
    let valid = Project::parse(root, "[plugins]\ncrates = [\"z\", \"a\"]\n[plugins.process]\nz = [\"z\", \"--z\"]\na = [\"./bin/a\", \"--a\"]\n").unwrap();
    assert_eq!(
        valid
            .plugin_processes
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert_eq!(valid.plugin_processes["a"].command, ["/r/bin/a", "--a"]);
    assert_eq!(valid.plugin_processes["z"].command, ["z", "--z"]);
    let invalid = Project::parse(
        root,
        "[plugins]\ncrates = [\"z\", \"a\"]\n[plugins.process]\nz = []\na = []\n",
    );
    assert_eq!(
        invalid.err().as_deref(),
        Some("symphony.toml: [plugins.process] a names no program")
    );
}
