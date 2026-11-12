use std::path::Path;
use std::sync::atomic::AtomicUsize;

use symphony_plugin::protocol::DeclaredKind;
use symphony_plugin::vocabulary::Span;
use symphony_plugin::{Card, ThreadLine};

use super::*;
use crate::test_tree::Tree;

/// A plugin with a component and a card kind, each answer made of what it is given.
struct Tiny;

static TINY: Tiny = Tiny;

impl Plugin for Tiny {
    fn name(&self) -> &str {
        "tiny"
    }
    fn components(&self) -> Vec<&dyn Component> {
        vec![self]
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![self]
    }
}

impl Component for Tiny {
    fn name(&self) -> &str {
        "tiny"
    }
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        match argument {
            "panic" => panic!("tiny cannot draw {argument}"),
            "bad" => Err(format!("tiny refuses {argument} on {}", page.doc_path)),
            _ => Ok(vec![Block::Paragraph {
                spans: vec![Span::Text {
                    text: format!(
                        "{argument} {option:?} on {} in {}",
                        page.doc_path,
                        page.root.display()
                    ),
                }],
            }]),
        }
    }
    fn resolve(&self, _: &Page<'_>, _: &str, option: Option<&str>) -> Result<(), String> {
        option.map_or(Ok(()), |option| {
            Err(format!("tiny takes no option, not {option:?}"))
        })
    }
    fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        Some(format!("v1 {}", page.doc_path))
    }
}

impl CardKind for Tiny {
    fn name(&self) -> &str {
        "tiny"
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        card.text.contains("tiny")
    }
    fn pictures(&self, root: &Path, card: &Card<'_>) -> Vec<Picture> {
        card.thread
            .iter()
            .map(|line| Picture {
                src: Some(format!("{}/{}.png", root.display(), card.slug)),
                caption: line.text.into(),
                waiting: line.verdict.is_none(),
            })
            .collect()
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        let (_, after) = card.text.split_once("*Slot:* ")?;
        Some(after.lines().next().unwrap_or_default().to_string())
    }
    fn markers(&self) -> &[&str] {
        &["Slot"]
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        Some(format!("{} · {}", card.doc, card.topic))
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        !card.text.contains("hold")
    }
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        Some((card.thread.len(), card.slug.into()))
    }
    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        card.thread.iter().any(|line| line.agent)
    }
    fn chosen(&self, card: &Card<'_>) -> Result<Option<String>, String> {
        if card.text.contains("broken") {
            return Err(format!("{} names no candidate", card.slug));
        }
        Ok(Some(card.thread.len().to_string()))
    }
}

/// A card kind with only what a kind requires: every other call its default.
struct BareKind;

impl CardKind for BareKind {
    fn name(&self) -> &str {
        "bare"
    }
    fn recognises(&self, _: &Card<'_>) -> bool {
        false
    }
}

/// What `read` makes of a card with `text`.
fn with_card<R>(text: &str, read: impl FnOnce(&Card<'_>) -> R) -> R {
    let thread = [
        ThreadLine {
            agent: true,
            verdict: None,
            text: "A or B? \"quoted\"",
        },
        ThreadLine {
            agent: false,
            verdict: Some("approved"),
            text: "B ✅",
        },
    ];
    read(&Card {
        slug: "pick-a-hull",
        doc: "docs/SHIPS.md",
        topic: "Hulls",
        text,
        thread: &thread,
    })
}

/// The cards each card kind test reads.
const CARDS: [&str; 3] = [
    "Pick a tiny hull.\n*Slot:* North",
    "hold the broken one",
    "",
];

/// How the plugin `name` would be started in `tree`: its command never run, as a test's peer
/// stands for it.
fn process_in(tree: &Tree, name: &str) -> PluginProcess {
    PluginProcess {
        command: vec![format!("{name}-plugin"), "--serve".into()],
        root: tree.root.clone(),
        log: tree.path(&format!("artifacts/plugins/{name}.log")),
    }
}

/// A start of `plugin` served on a thread of this process, as its process serves it, counted in
/// `starts`.
fn peer(plugin: &'static dyn Plugin, starts: Arc<AtomicUsize>) -> Start {
    Box::new(move || {
        starts.fetch_add(1, Ordering::SeqCst);
        let (input, host_input) = io::pipe()?;
        let (host_output, output) = io::pipe()?;
        let serving = std::thread::spawn(move || {
            symphony_plugin::serve_on(plugin, BufReader::new(input), output)
        });
        Ok(Spawned {
            input: Box::new(host_input),
            output: Box::new(host_output),
            pid: None,
            ended: Box::new(move || {
                let _ = serving.join();
                "exited with code 0".into()
            }),
        })
    })
}

/// What a hand-made peer does with a request: the lines it writes, or its end.
enum Said {
    Lines(Vec<String>),
    End,
}

/// The line answering call `id` with the `answered` member `answered`.
fn answering(id: u64, answered: &str) -> String {
    format!("{{\"id\":{id},\"answered\":{answered}}}")
}

/// What a hand-made peer answers each request but the declaration with: the call's id and the
/// request member as written.
type Respond = Arc<dyn Fn(u64, &str) -> Said + Send + Sync>;

/// A start of a peer that declares `declared`, answers each other request as `respond` says, and
/// keeps each line it is sent in `heard`; once its input or `respond` ends it, its end says
/// `how`.
fn hand_made(
    declared: Declaration,
    respond: Respond,
    heard: Arc<Mutex<Vec<String>>>,
    how: &'static str,
) -> Start {
    Box::new(move || {
        let (input, host_input) = io::pipe()?;
        let (host_output, mut output) = io::pipe()?;
        let (declared, respond, heard) = (declared.clone(), respond.clone(), heard.clone());
        let serving = std::thread::spawn(move || {
            for line in BufReader::new(input).lines() {
                let Ok(line) = line else {
                    break;
                };
                heard.lock().unwrap().push(line.clone());
                let Ok(read) = serde_json::from_str::<Raw>(&line) else {
                    continue;
                };
                let Some(request) = read.request else {
                    continue;
                };
                let said = if request.get().contains("\"plugin.declare\"") {
                    let answered = serde_json::to_string(&Answered::Answer(&declared)).unwrap();
                    Said::Lines(vec![answering(read.id, &answered)])
                } else {
                    respond(read.id, request.get())
                };
                match said {
                    Said::Lines(lines) => {
                        for line in lines {
                            writeln!(output, "{line}").unwrap();
                        }
                        output.flush().unwrap();
                    }
                    Said::End => break,
                }
            }
        });
        Ok(Spawned {
            input: Box::new(host_input),
            output: Box::new(host_output),
            pid: None,
            ended: Box::new(move || {
                let _ = serving.join();
                how.into()
            }),
        })
    })
}

/// What [`TINY`] declares.
fn tiny_declared() -> Declaration {
    Declaration::of(&TINY)
}

/// A declaration of the plugin `old` in `protocol`: a component and a card kind, both `old`.
fn old_declared(protocol: Version) -> Declaration {
    Declaration {
        protocol,
        name: "old".into(),
        components: vec!["old".into()],
        card_kinds: vec![DeclaredKind {
            name: "old".into(),
            markers: vec!["Slot".into()],
        }],
        ..Declaration::default()
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

/// The remote's one component and one card kind.
fn seams(remote: &Remote) -> (&dyn Component, &dyn CardKind) {
    let (components, kinds) = (remote.components(), remote.card_kinds());
    assert_eq!((components.len(), kinds.len()), (1, 1));
    (components[0], kinds[0])
}

#[test]
fn a_plugins_process_answers_its_components_and_card_kinds_as_it_does_linked() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        peer(&TINY, starts.clone()),
        PAUSE,
        WAIT,
    );
    assert_eq!(remote.declaration(), Ok(&tiny_declared()));
    let (component, kind) = seams(remote);
    let (linked, linked_kind) = (TINY.components()[0], TINY.card_kinds()[0]);
    assert_eq!(component.name(), "tiny");
    assert_eq!((kind.name(), kind.markers()), ("tiny", &["Slot"][..]));
    let page = Page::new(&tree.root, "docs/ünï code.md");
    for (argument, option) in [("lantern", Some("wide")), ("", None), ("bad", None)] {
        assert_eq!(
            component.render(&page, argument, option),
            linked.render(&page, argument, option)
        );
        assert_eq!(
            component.resolve(&page, argument, option),
            linked.resolve(&page, argument, option)
        );
    }
    assert_eq!(component.cache_version(&page), linked.cache_version(&page));
    for text in CARDS {
        with_card(text, |card| {
            assert_eq!(kind.recognises(card), linked_kind.recognises(card));
            assert_eq!(
                kind.pictures(&tree.root, card),
                linked_kind.pictures(&tree.root, card)
            );
            assert_eq!(kind.slot(card), linked_kind.slot(card));
            assert_eq!(
                kind.triage_category(card),
                linked_kind.triage_category(card)
            );
            assert_eq!(
                kind.auto_approvable(card),
                linked_kind.auto_approvable(card)
            );
            assert_eq!(kind.gates(card), linked_kind.gates(card));
            assert_eq!(
                kind.approval_needs_agent(card),
                linked_kind.approval_needs_agent(card)
            );
            assert_eq!(kind.chosen(card), linked_kind.chosen(card));
        });
    }
    // One process answered every call, and the remote stood for the plugin by its name.
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(remote.name(), "tiny");
    assert!(remote.down().is_none());
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains("symphony: started: tiny-plugin --serve"),
        "{log}"
    );
}

#[test]
fn answers_go_to_their_calls_by_id_whatever_order_they_come_in() {
    let tree = Tree::new();
    // The first render is held until the second comes; the second answered first.
    let held: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let respond: Respond = Arc::new(move |id, request| {
        let argument = if request.contains("\"argument\":\"a\"") {
            "a"
        } else {
            "b"
        };
        let answered = |id: u64, argument: &str| {
            let blocks = vec![Block::Paragraph {
                spans: vec![Span::Text {
                    text: argument.into(),
                }],
            }];
            let answer: Answered<Result<Vec<Block>, String>> = Answered::Answer(Ok(blocks));
            answering(id, &serde_json::to_string(&answer).unwrap())
        };
        let mut held = held.lock().unwrap();
        match held.take() {
            None => {
                *held = Some(id);
                Said::Lines(Vec::new())
            }
            Some(first) => {
                let other = if argument == "a" { "b" } else { "a" };
                Said::Lines(vec![answered(id, argument), answered(first, other)])
            }
        }
    });
    let heard = Arc::new(Mutex::new(Vec::new()));
    let remote = Remote::leaked(
        "old",
        process_in(&tree, "old"),
        hand_made(old_declared(PROTOCOL), respond, heard, "exited with code 0"),
        PAUSE,
        WAIT,
    );
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    let text = |blocks: Result<Vec<Block>, String>| match blocks.unwrap().as_slice() {
        [Block::Paragraph { spans }] => match spans.as_slice() {
            [Span::Text { text }] => text.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    std::thread::scope(|scope| {
        let a = scope.spawn(|| text(component.render(&page, "a", None)));
        let b = scope.spawn(|| text(component.render(&page, "b", None)));
        assert_eq!(a.join().unwrap(), "a");
        assert_eq!(b.join().unwrap(), "b");
    });
}

#[test]
fn a_call_newer_than_the_minor_agreed_is_not_sent_and_the_seams_own_default_answers_it() {
    let tree = Tree::new();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let respond: Respond = Arc::new(|_, request| panic!("{request} was sent"));
    let older = Version { major: 0, minor: 0 };
    let remote = Remote::leaked(
        "old",
        process_in(&tree, "old"),
        hand_made(old_declared(older), respond, heard.clone(), "exited"),
        PAUSE,
        WAIT,
    );
    let (component, kind) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    let missing = Err("plugin old: it speaks protocol 0.0, which has no components.render".into());
    assert_eq!(component.render(&page, "a", None), missing);
    // A component's resolve is its render, by default.
    assert_eq!(component.resolve(&page, "a", Some("o")), missing.map(drop));
    assert_eq!(component.cache_version(&page), None);
    assert_eq!(kind.markers(), ["Slot"]);
    for text in CARDS {
        with_card(text, |card| {
            assert!(!kind.recognises(card));
            assert_eq!(
                kind.pictures(&tree.root, card),
                BareKind.pictures(&tree.root, card)
            );
            assert_eq!(kind.slot(card), BareKind.slot(card));
            assert_eq!(kind.triage_category(card), BareKind.triage_category(card));
            assert_eq!(kind.auto_approvable(card), BareKind.auto_approvable(card));
            assert_eq!(kind.gates(card), BareKind.gates(card));
            assert_eq!(
                kind.approval_needs_agent(card),
                BareKind.approval_needs_agent(card)
            );
            assert_eq!(kind.chosen(card), BareKind.chosen(card));
        });
    }
    // Nothing was sent but the declaration.
    let heard = heard.lock().unwrap();
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert!(heard[0].contains("\"plugin.declare\""), "{heard:?}");
}

#[test]
fn a_plugin_of_another_major_is_refused_naming_what_to_update_and_declares_nothing() {
    let tree = Tree::new();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let respond: Respond = Arc::new(|_, request| panic!("{request} was sent"));
    let newer = Version { major: 1, minor: 0 };
    let remote = Remote::leaked(
        "old",
        process_in(&tree, "old"),
        hand_made(old_declared(newer), respond, heard, "exited with code 0"),
        PAUSE,
        WAIT,
    );
    let refused = remote.declaration().unwrap_err();
    assert!(
        refused.starts_with("its process did not declare: "),
        "{refused}"
    );
    assert!(
        refused.contains("install a Symphony that speaks protocol 1"),
        "{refused}"
    );
    assert!(remote.components().is_empty());
    assert!(remote.card_kinds().is_empty());
    assert!(remote.down().is_some_and(|why| why == refused));
    // Its input was closed, so the process ended.
    until("the refused process ended", || {
        tree.read("artifacts/plugins/old.log")
            .contains("symphony: its process exited with code 0")
    });
}

#[test]
fn a_process_that_ends_fails_its_waiting_calls_and_starts_again_after_its_pause() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let heard = Arc::new(Mutex::new(Vec::new()));
    // The first start ends at its first call; the next serves as the plugin does.
    let dying = hand_made(
        tiny_declared(),
        Arc::new(|_, _| Said::End),
        heard,
        "was killed by signal 9",
    );
    let serving = peer(&TINY, starts.clone());
    let counted = starts.clone();
    let start: Start = Box::new(move || {
        if counted.load(Ordering::SeqCst) == 0 {
            counted.fetch_add(1, Ordering::SeqCst);
            return dying();
        }
        serving()
    });
    let pause = Duration::from_millis(300);
    let remote = Remote::leaked("tiny", process_in(&tree, "tiny"), start, pause, WAIT);
    let (component, kind) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    assert_eq!(
        component.render(&page, "a", None),
        Err("plugin tiny: its process was killed by signal 9".into())
    );
    until("the remote is down", || remote.down().is_some());
    assert_eq!(remote.pid(), None);
    // Meanwhile each call says so, and a card kind answers as one that says nothing.
    assert_eq!(
        component.render(&page, "a", None),
        Err("plugin tiny: its process was killed by signal 9; it starts again in a moment".into())
    );
    with_card(CARDS[0], |card| {
        assert!(!kind.recognises(card));
        assert_eq!(kind.slot(card), None);
    });
    std::thread::sleep(pause);
    assert_eq!(
        component.render(&page, "a", None),
        TINY.components()[0].render(&page, "a", None)
    );
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    assert!(remote.down().is_none());
    let log = tree.read("artifacts/plugins/tiny.log");
    assert_eq!(log.matches("symphony: started: ").count(), 2, "{log}");
    assert_eq!(
        log.matches("symphony: its process was killed by signal 9")
            .count(),
        1,
        "{log}"
    );
}

#[test]
fn a_plugin_that_cannot_start_is_tried_five_times_in_the_window_then_left_down() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let counted = starts.clone();
    let start: Start = Box::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::other("no such program"))
    });
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        start,
        Duration::ZERO,
        WAIT,
    );
    for _ in 0..STARTS {
        assert_eq!(
            remote.declaration(),
            Err("tiny-plugin cannot start: no such program".into())
        );
    }
    assert_eq!(
        remote.declaration(),
        Err(
            "tiny-plugin cannot start: no such program; started 5 times in 180 s, it is not \
             started again for now"
                .into()
        )
    );
    assert_eq!(starts.load(Ordering::SeqCst), STARTS);
    assert!(remote.components().is_empty());
    let log = tree.read("artifacts/plugins/tiny.log");
    assert_eq!(
        log.matches("symphony: tiny-plugin cannot start: no such program")
            .count(),
        STARTS,
        "{log}"
    );
}

#[test]
fn a_call_with_no_answer_in_its_wait_is_given_up_and_cancelled() {
    let tree = Tree::new();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        hand_made(
            tiny_declared(),
            Arc::new(|_, _| Said::Lines(Vec::new())),
            heard.clone(),
            "exited with code 0",
        ),
        PAUSE,
        Duration::from_millis(50),
    );
    let (component, kind) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    assert_eq!(
        component.render(&page, "a", None),
        Err("plugin tiny: no answer in 50ms".into())
    );
    // The call it gave up was cancelled by its id, and the process kept.
    until("the cancel was sent", || heard.lock().unwrap().len() == 3);
    let heard = heard.lock().unwrap().clone();
    let render: Raw = serde_json::from_str(&heard[1]).unwrap();
    assert!(
        render
            .request
            .is_some_and(|r| r.get().contains("components.render"))
    );
    assert_eq!(
        heard[2],
        format!("{{\"id\":{},\"cancel\":{{}}}}", render.id)
    );
    assert!(remote.down().is_none());
    with_card(CARDS[0], |card| assert_eq!(kind.slot(card), None));
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains("symphony: components.render of tiny: no answer in 50ms"),
        "{log}"
    );
}

#[test]
fn a_refusal_or_an_answer_that_does_not_read_is_shown_where_the_answer_goes() {
    let tree = Tree::new();
    let respond: Respond = Arc::new(|id, request| {
        let answered = if request.contains("components.render") {
            r#"{"answer":42}"#
        } else {
            r#"{"refused":"no slot today"}"#
        };
        Said::Lines(vec![
            "not a line of the protocol".into(),
            format!("{{\"id\":{id},\"cancel\":{{}}}}"),
            format!("{{\"id\":{id}}}"),
            answering(id, answered),
        ])
    });
    let heard = Arc::new(Mutex::new(Vec::new()));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        hand_made(tiny_declared(), respond, heard, "exited with code 0"),
        PAUSE,
        WAIT,
    );
    let (component, kind) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    let unread = component.render(&page, "a", None).unwrap_err();
    assert!(
        unread.starts_with("plugin tiny: its answer to components.render cannot be read: "),
        "{unread}"
    );
    with_card(CARDS[0], |card| assert_eq!(kind.slot(card), None));
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains(
            "symphony: card_kinds.slot of tiny: it refused card_kinds.slot: no slot today"
        ),
        "{log}"
    );
    assert!(
        log.contains("symphony: a line that answers no call (")
            && log.contains("): not a line of the protocol"),
        "{log}"
    );
    // A line of the host's members, or of none, answers nothing either.
    assert!(log.contains("a plugin sends answers only"), "{log}");
    assert!(
        log.contains("holds none of request, cancel and answered"),
        "{log}"
    );
}

#[test]
fn a_declaration_changed_at_a_restart_is_noted_and_the_first_stands() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let heard = Arc::new(Mutex::new(Vec::new()));
    let first = hand_made(
        tiny_declared(),
        Arc::new(|_, _| Said::End),
        heard.clone(),
        "exited with code 0",
    );
    let changed = hand_made(
        old_declared(PROTOCOL),
        Arc::new(|_, _| Said::End),
        heard,
        "exited with code 0",
    );
    let counted = starts.clone();
    let start: Start = Box::new(move || match counted.fetch_add(1, Ordering::SeqCst) {
        0 => first(),
        _ => changed(),
    });
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        start,
        Duration::ZERO,
        WAIT,
    );
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    assert!(component.render(&page, "a", None).is_err());
    until("the first process ended", || remote.down().is_some());
    assert!(component.render(&page, "a", None).is_err());
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    assert_eq!(remote.declaration(), Ok(&tiny_declared()));
    assert_eq!(component.name(), "tiny");
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains("symphony: its declaration changed since its first start"),
        "{log}"
    );
}

#[test]
fn a_panic_in_the_plugin_is_shown_as_its_refusal_and_its_process_answers_on() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        peer(&TINY, starts.clone()),
        PAUSE,
        WAIT,
    );
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    assert_eq!(
        component.render(&page, "panic", None),
        Err(
            "plugin tiny: it refused components.render: tiny panicked: tiny cannot draw panic"
                .into()
        )
    );
    assert!(component.render(&page, "a", None).is_ok());
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[test]
fn a_call_is_recorded_as_a_process_call_with_the_plugins_time_and_the_bytes_both_ways() {
    let tree = Tree::new();
    let record = tree.path("artifacts/plugin-calls.jsonl");
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        peer(&TINY, Arc::new(AtomicUsize::new(0))),
        PAUSE,
        WAIT,
    );
    remote.record_into(Some(Record::at(&record)));
    let (component, kind) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    component.render(&page, "a", None).unwrap();
    with_card(CARDS[0], |card| kind.slot(card));
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&record)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // The declaration is the start's, not a call of a seam: it is not recorded.
    assert_eq!(lines.len(), 2, "{lines:?}");
    for (line, (seam, call, name)) in lines.iter().zip([
        ("components", "render", "tiny"),
        ("card_kinds", "slot", "tiny"),
    ]) {
        assert_eq!(line["path"], "process", "{line}");
        assert_eq!(line["plugin"], "tiny", "{line}");
        assert_eq!(
            (&line["seam"], &line["call"], &line["name"]),
            (&seam.into(), &call.into(), &name.into()),
            "{line}"
        );
        assert_eq!(line["pid"], std::process::id(), "{line}");
        let us = line["us"].as_f64().unwrap();
        let round_trip = line["round_trip_us"].as_f64().unwrap();
        assert!(us >= 0.0 && round_trip >= us, "{line}");
        assert!(line["request_bytes"].as_u64().unwrap() > 0, "{line}");
        assert!(line["answer_bytes"].as_u64().unwrap() > 0, "{line}");
        assert!(line.get("failed").is_none(), "{line}");
    }
    // Recorded no more, the next call writes nothing.
    remote.record_into(None);
    component.render(&page, "a", None).unwrap();
    assert_eq!(std::fs::read_to_string(&record).unwrap().lines().count(), 2);
}

#[test]
fn a_call_with_no_answer_is_recorded_with_why_and_without_the_plugins_time() {
    let tree = Tree::new();
    let record = tree.path("artifacts/plugin-calls.jsonl");
    let start: Start = Box::new(|| Err(io::Error::other("no such program")));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        start,
        Duration::ZERO,
        WAIT,
    );
    remote.record_into(Some(Record::at(&record)));
    let request = Recognises {
        kind: "tiny".into(),
        card: with_card("", CardData::of),
    };
    assert_eq!(
        remote.ask("tiny", request),
        Err(Failure::Down(
            "tiny-plugin cannot start: no such program".into()
        ))
    );
    let line: serde_json::Value =
        serde_json::from_str(std::fs::read_to_string(&record).unwrap().trim_end()).unwrap();
    assert_eq!(line["failed"], "tiny-plugin cannot start: no such program");
    assert_eq!(
        (&line["seam"], &line["call"]),
        (&"card_kinds".into(), &"recognises".into())
    );
    for absent in ["us", "request_bytes", "answer_bytes", "lost"] {
        assert!(line.get(absent).is_none(), "{absent}: {line}");
    }
    assert!(line["round_trip_us"].as_f64().is_some(), "{line}");
}

#[test]
fn the_loopbacks_own_call_is_not_asked_of_a_process() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        peer(&TINY, starts.clone()),
        PAUSE,
        WAIT,
    );
    let request = symphony_plugin::protocol::RunCommand {
        command: "every".into(),
        args: Vec::new(),
    };
    assert_eq!(
        remote.ask("every", request),
        Err(Failure::Down(
            "commands.run is not asked of a plugin's process".into()
        ))
    );
    // Not even started for it.
    assert_eq!(starts.load(Ordering::SeqCst), 0);
}

#[test]
fn made_keeps_one_remote_for_a_name_its_command_and_its_checkout() {
    let (tree, other) = (Tree::new(), Tree::new());
    let process = process_in(&tree, "kept");
    let first = made("kept", &process);
    assert!(std::ptr::eq(first, made("kept", &process)));
    assert!(!std::ptr::eq(
        first,
        made("kept", &process_in(&other, "kept"))
    ));
    let mut moved = process.clone();
    moved.command = vec!["another".into()];
    assert!(!std::ptr::eq(first, made("kept", &moved)));
    assert!(!std::ptr::eq(first, made("other", &process)));
    // A remote is known for one when its plugins are listed; a linked plugin is none.
    assert!(remote(first).is_some_and(|found| std::ptr::eq(found, first)));
    assert!(remote(&TINY).is_none());
    // Made, it is not started: nothing was run, nothing logged.
    assert!(!tree.path("artifacts/plugins/kept.log").exists());
}

/// A remote of the real process `command`, in `tree`.
fn spawned(tree: &Tree, name: &str, command: &[&str]) -> &'static Remote {
    let process = PluginProcess {
        command: command.iter().map(|word| (*word).to_string()).collect(),
        root: tree.root.clone(),
        log: tree.path(&format!("artifacts/plugins/{name}.log")),
    };
    let started = process.clone();
    Remote::leaked(
        name,
        process,
        Box::new(move || spawn(&started)),
        PAUSE,
        WAIT,
    )
}

#[test]
fn a_real_process_runs_in_the_checkout_its_stderr_and_its_end_in_its_log() {
    let tree = Tree::new();
    tree.write("here.txt", "the checkout\n");
    // It reads the declaration, says what it sees on stderr, and ends.
    let remote = spawned(
        &tree,
        "shell",
        &["sh", "-c", "read line; cat here.txt >&2; exit 3"],
    );
    let refused = remote.declaration().unwrap_err();
    let pid = refused
        .strip_prefix("its process ")
        .and_then(|rest| rest.split_once(' '))
        .map(|(pid, _)| pid.to_string())
        .unwrap_or_else(|| panic!("{refused}"));
    assert_eq!(
        refused,
        format!("its process {pid} did not declare: its process {pid} exited with code 3")
    );
    let log = tree.read("artifacts/plugins/shell.log");
    assert!(
        log.contains(&format!(
            "symphony: started {pid}: sh -c read line; cat here.txt >&2; exit 3"
        )),
        "{log}"
    );
    assert!(log.contains("\nthe checkout\n"), "{log}");
    assert!(
        log.contains(&format!("symphony: its process {pid} exited with code 3")),
        "{log}"
    );
}

#[test]
fn a_real_process_killed_says_by_which_signal_and_a_missing_program_cannot_start() {
    let tree = Tree::new();
    let killed = spawned(&tree, "killed", &["sh", "-c", "read line; kill -9 $$"]);
    let refused = killed.declaration().unwrap_err();
    assert!(refused.ends_with(" was killed by signal 9"), "{refused}");
    let missing = spawned(&tree, "missing", &["./no-such-plugin"]);
    let refused = missing.declaration().unwrap_err();
    assert!(
        refused.starts_with("./no-such-plugin cannot start: "),
        "{refused}"
    );
}

#[test]
fn a_process_with_no_program_cannot_start() {
    let tree = Tree::new();
    let none = spawned(&tree, "none", &[]);
    assert_eq!(
        none.declaration(),
        Err("its command cannot start: it names no program".into())
    );
}

#[test]
fn lines_logged_by_threads_at_once_are_each_whole() {
    let tree = Tree::new();
    let start: Start = Box::new(|| Err(io::Error::other("never started")));
    let remote = Remote::leaked("tiny", process_in(&tree, "tiny"), start, PAUSE, WAIT);
    std::thread::scope(|scope| {
        for thread in 0..8 {
            scope.spawn(move || {
                for line in 0..100 {
                    remote.log(&format!("thread {thread} line {line}"));
                }
            });
        }
    });
    let log = tree.read("artifacts/plugins/tiny.log");
    let mut said: Vec<&str> = log
        .lines()
        .map(|line| {
            let (_, said) = line
                .split_once(" symphony: ")
                .unwrap_or_else(|| panic!("a line cut: {line:?}"));
            said
        })
        .collect();
    said.sort_unstable();
    let mut every: Vec<String> = (0..8)
        .flat_map(|thread| (0..100).map(move |line| format!("thread {thread} line {line}")))
        .collect();
    every.sort_unstable();
    assert_eq!(said, every);
}

#[test]
fn how_a_process_ended_is_its_code_or_its_signal() {
    use std::os::unix::process::ExitStatusExt as _;
    use std::process::ExitStatus;
    assert_eq!(ended(ExitStatus::from_raw(3 << 8)), "exited with code 3");
    assert_eq!(ended(ExitStatus::from_raw(0)), "exited with code 0");
    assert_eq!(ended(ExitStatus::from_raw(9)), "was killed by signal 9");
}
