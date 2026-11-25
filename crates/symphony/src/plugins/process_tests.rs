use std::io::BufRead as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize};

use symphony_plugin::protocol::{DeclaredCommand, DeclaredKind};
use symphony_plugin::vocabulary::Span;
use symphony_plugin::{Card, ThreadLine};

use super::*;
use crate::plugins::Plugins;
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
        slug: "pick-a-widget",
        doc: "docs/WIDGETS.md",
        topic: "Widgets",
        text,
        thread: &thread,
    })
}

/// The cards each card kind test reads.
const CARDS: [&str; 3] = [
    "Pick a tiny widget.\n*Slot:* North",
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

/// A wait for an answer that only a stall outlasts: the backstop of a call's own ([`WAIT`]).
const PATIENT: Duration = test_time::backstop(WAIT);

/// The limits with `wait` for each call's wait and for the wait after its cancel, `pause` for the
/// pause before a restart, and a declaration waited for as long as only a stall outlasts, so a
/// test's short wait is its calls' alone.
fn limits(wait: Duration, pause: Duration) -> Limits {
    Limits {
        wait,
        declare: PATIENT,
        cancel: wait,
        pause,
        ..Limits::default()
    }
}

/// The limits a test whose calls are answered gives them: each waited for as long as only a stall
/// outlasts.
fn patient() -> Limits {
    limits(PATIENT, PAUSE)
}

/// A peer served on a thread of this test, standing for a process: killed, it is ended at once, as
/// a killed process is, and its thread left to end with its pipes. `kills` counts the kills.
struct Thread {
    serving: std::thread::JoinHandle<()>,
    how: &'static str,
    killed: bool,
    kills: Arc<AtomicUsize>,
}

impl Handle for Thread {
    fn kill(&mut self) {
        self.killed = true;
        self.kills.fetch_add(1, Ordering::SeqCst);
    }
    fn ended(&mut self) -> Option<String> {
        if self.killed {
            return Some("was killed by signal 9".into());
        }
        self.serving.is_finished().then(|| self.how.into())
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
            let _ = symphony_plugin::serve_on(plugin, BufReader::new(input), output);
        });
        Ok(Spawned {
            input: Box::new(host_input),
            output: Box::new(host_output),
            errors: None,
            pid: None,
            process: Box::new(Thread {
                serving,
                how: "exited with code 0",
                killed: false,
                kills: Arc::default(),
            }),
        })
    })
}

/// What a hand-made peer does with a request: the lines it writes, bytes it writes as they are, or
/// its end.
enum Said {
    Lines(Vec<String>),
    Bytes(Vec<u8>),
    End,
}

/// The line answering call `id` with the `answered` member `answered`.
fn answering(id: u64, answered: &str) -> String {
    format!("{{\"id\":{id},\"answered\":{answered}}}")
}

/// What a hand-made peer answers each request but the declaration with: the call's id and the
/// request member as written.
type Respond = Arc<dyn Fn(u64, &str) -> Said + Send + Sync>;

/// What a hand-made peer's test sees of it: each line it was sent, and how often it was killed.
#[derive(Clone, Default)]
struct Seen {
    heard: Arc<Mutex<Vec<String>>>,
    kills: Arc<AtomicUsize>,
}

impl Seen {
    fn heard(&self) -> Vec<String> {
        self.heard.lock().unwrap().clone()
    }
    fn kills(&self) -> usize {
        self.kills.load(Ordering::SeqCst)
    }
}

/// A start of a peer that declares `declared`, answers each other request as `respond` says, and
/// keeps what `seen` sees; once its input or `respond` ends it, its end says `how`.
fn hand_made(declared: Declaration, respond: Respond, seen: Seen, how: &'static str) -> Start {
    Box::new(move || {
        let (input, host_input) = io::pipe()?;
        let (host_output, mut output) = io::pipe()?;
        let (declared, respond, heard) = (declared.clone(), respond.clone(), seen.heard.clone());
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
                let written = match said {
                    Said::Lines(lines) => lines
                        .iter()
                        .try_for_each(|line| writeln!(output, "{line}"))
                        .and_then(|()| output.flush()),
                    Said::Bytes(bytes) => output.write_all(&bytes).and_then(|()| output.flush()),
                    Said::End => break,
                };
                if written.is_err() {
                    break;
                }
            }
        });
        Ok(Spawned {
            input: Box::new(host_input),
            output: Box::new(host_output),
            errors: None,
            pid: None,
            process: Box::new(Thread {
                serving,
                how,
                killed: false,
                kills: seen.kills.clone(),
            }),
        })
    })
}

/// What [`TINY`] declares.
fn tiny_declared() -> Declaration {
    Declaration::of(&TINY)
}

/// The line declaring [`TINY`] as the answer to call `id`.
fn tiny_declaring(id: u64) -> String {
    answering(
        id,
        &serde_json::to_string(&Answered::Answer(&tiny_declared())).unwrap(),
    )
}

/// A declaration of the plugin `old` in `protocol`: a component and a card kind, both `old`.
fn old_declared(protocol: Version) -> Declaration {
    let mut kind = DeclaredKind::default();
    kind.name = "old".into();
    kind.markers = vec!["Slot".into()];
    let mut declared = Declaration::default();
    declared.protocol = protocol;
    declared.name = "old".into();
    declared.components = vec!["old".into()];
    declared.card_kinds = vec![kind];
    declared
}

/// The answer of a render, written: a paragraph saying `text`.
fn rendered(text: &str) -> String {
    let blocks = vec![Block::Paragraph {
        spans: vec![Span::Text { text: text.into() }],
    }];
    let answer: Answered<Result<Vec<Block>, String>> = Answered::Answer(Ok(blocks));
    serde_json::to_string(&answer).unwrap()
}

/// The text of the one paragraph a render answered.
fn text(blocks: Result<Vec<Block>, String>) -> String {
    match blocks.unwrap().as_slice() {
        [Block::Paragraph { spans }] => match spans.as_slice() {
            [Span::Text { text }] => text.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

/// Waits until `holds`, a wait of five seconds on a quiet machine, which only a stall ends
/// otherwise, failing with the load average then (RULES.md `tests/no-test-fails-on-load`).
fn until(what: &str, holds: impl Fn() -> bool) {
    let backstop = test_time::backstop(Duration::from_secs(5));
    test_time::until_true(what, backstop, holds).unwrap_or_else(|stall| panic!("{stall}"));
}

/// A stdin standing for a process that answers only what it has read: each line written to it is
/// answered with the next of its answers, on `output`, its stdout's other end, which it lets go of
/// once it has said them all.
struct Answering {
    answers: std::collections::VecDeque<String>,
    output: Option<io::PipeWriter>,
}

impl Answering {
    fn new(answers: impl IntoIterator<Item = String>, output: io::PipeWriter) -> Self {
        Self {
            answers: answers.into_iter().collect(),
            output: Some(output),
        }
    }
}

impl Write for Answering {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for _ in buf.iter().filter(|byte| **byte == b'\n') {
            let Some(answer) = self.answers.pop_front() else {
                break;
            };
            if let Some(output) = &mut self.output {
                output.write_all(format!("{answer}\n").as_bytes())?;
            }
            if self.answers.is_empty() {
                self.output = None;
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The remote's one component and one card kind.
fn seams(remote: &Remote) -> (&dyn Component, &dyn CardKind) {
    let (components, kinds) = (remote.components(), remote.card_kinds());
    assert_eq!((components.len(), kinds.len()), (1, 1));
    (components[0], kinds[0])
}

/// Whether the process `pid` still runs (or is still a child not reaped).
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[test]
fn a_plugins_process_answers_its_components_and_card_kinds_as_it_does_linked() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        peer(&TINY, starts.clone()),
        patient(),
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
    assert_eq!(remote.process(), &process_in(&tree, "tiny"));
    assert!(remote.down().is_none());
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains("symphony: started: tiny-plugin --serve"),
        "{log}"
    );
}

#[test]
fn a_newer_plugins_kinds_leave_the_rest_of_its_page_intact() {
    let tree = Tree::new();
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        hand_made(
            tiny_declared(),
            Arc::new(|id, _| {
                Said::Lines(vec![answering(
                    id,
                    &serde_json::json!({"answer":{"Ok":[
                        {"type":"rule"},
                        {"type":"future-block","unread":{"type":7}},
                        {"type":"paragraph","spans":[
                            {"type":"text","text":"before"},
                            {"type":"future-span","unread":false},
                            {"type":"media","media":{"kind":"future-media","unread":null}},
                            {"type":"text","text":"after"}
                        ]},
                        {"type":"media","media":{"kind":"future-media"}},
                        {"type":"rule"}
                    ]}})
                    .to_string(),
                )])
            }),
            Seen::default(),
            "exited with code 0",
        ),
        patient(),
    );
    let page = Page::new(&tree.root, "docs/NEW.md");
    let blocks = Plugins::new(vec![remote]).render(&page, "tiny", "", None);
    assert_eq!(
        blocks,
        vec![
            Block::Rule,
            Block::Unknown {
                tag: "future-block".into(),
                text: String::new()
            },
            Block::Paragraph {
                spans: vec![
                    Span::Text {
                        text: "before".into()
                    },
                    Span::Unknown {
                        tag: "future-span".into(),
                        text: String::new()
                    },
                    Span::Unknown {
                        tag: "media: future-media".into(),
                        text: String::new()
                    },
                    Span::Text {
                        text: "after".into()
                    },
                ]
            },
            Block::Unknown {
                tag: "media: future-media".into(),
                text: String::new()
            },
            Block::Rule,
        ]
    );
    assert!(remote.down().is_none());
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
        let mut held = held.lock().unwrap();
        match held.take() {
            None => {
                *held = Some(id);
                Said::Lines(Vec::new())
            }
            Some(first) => {
                let other = if argument == "a" { "b" } else { "a" };
                Said::Lines(vec![
                    answering(id, &rendered(argument)),
                    answering(first, &rendered(other)),
                ])
            }
        }
    });
    let remote = Remote::leaked(
        "old",
        process_in(&tree, "old"),
        hand_made(
            old_declared(PROTOCOL),
            respond,
            Seen::default(),
            "exited with code 0",
        ),
        patient(),
    );
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
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
    let seen = Seen::default();
    let respond: Respond = Arc::new(|_, request| panic!("{request} was sent"));
    let older = Version::new(0, 0);
    let remote = Remote::leaked(
        "old",
        process_in(&tree, "old"),
        hand_made(old_declared(older), respond, seen.clone(), "exited"),
        patient(),
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
    let heard = seen.heard();
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert!(heard[0].contains("\"plugin.declare\""), "{heard:?}");
}

#[test]
fn a_plugin_of_the_previous_minor_is_served_and_reads_a_page_of_this_one_as_its_own() {
    /// A render as a plugin of 0.1 reads it: its page as 0.1 wrote one, with no artifacts folder.
    #[derive(serde::Deserialize)]
    struct RenderOf01 {
        component: String,
        page: PageOf01,
        argument: String,
    }
    #[derive(serde::Deserialize)]
    struct PageOf01 {
        doc_path: String,
        #[serde(default)]
        render: u64,
    }
    let tree = Tree::new();
    let seen = Seen::default();
    let respond: Respond = Arc::new(|id, request| {
        let mut asked: BTreeMap<String, RenderOf01> = serde_json::from_str(request).unwrap();
        let asked = asked.remove("components.render").unwrap();
        let said = format!(
            "{} {} on {}, render {}",
            asked.component,
            asked.argument,
            asked.page.doc_path,
            u8::from(asked.page.render > 0)
        );
        Said::Lines(vec![answering(id, &rendered(&said))])
    });
    let previous = Version::new(PROTOCOL.major, PROTOCOL.minor - 1);
    let remote = Remote::leaked(
        "old",
        process_in(&tree, "old"),
        hand_made(old_declared(previous), respond, seen.clone(), "exited"),
        patient(),
    );
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md").in_artifacts("out");
    assert_eq!(
        text(component.render(&page, "a", None)),
        "old a on docs/A.md, render 1"
    );
    // The page went with its project's folder, which that plugin's reader left unread.
    let heard = seen.heard();
    assert!(
        heard
            .iter()
            .any(|line| line.contains("\"artifacts\":\"out\"")),
        "{heard:?}"
    );
}

#[test]
fn a_plugin_of_another_major_is_refused_killed_and_not_started_again_until_its_program_changes() {
    let tree = Tree::new();
    let program = tree.write("bin/old", "a build\n");
    let process = PluginProcess {
        command: vec![program.display().to_string()],
        ..process_in(&tree, "old")
    };
    let seen = Seen::default();
    let starts = Arc::new(AtomicUsize::new(0));
    let refusing = hand_made(
        old_declared(Version::new(1, 0)),
        Arc::new(|_, request| panic!("{request} was sent")),
        seen.clone(),
        "exited with code 0",
    );
    let counted = starts.clone();
    let start: Start = Box::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        refusing()
    });
    let remote = Remote::leaked("old", process, start, limits(PATIENT, Duration::ZERO));
    let refused = remote.declaration().unwrap_err();
    assert_eq!(
        refused,
        "its process is refused: the plugin speaks protocol 1.0 and this Symphony 0.6: install a \
         Symphony that speaks protocol 1; it is not started again until its program or the \
         project's settings change"
    );
    assert!(remote.components().is_empty());
    assert!(remote.card_kinds().is_empty());
    assert_eq!(remote.down(), Some(refused.clone()));
    // Killed at once, and with no pause it is still not started again: the same program.
    assert_eq!(seen.kills(), 1);
    for _ in 0..3 {
        assert_eq!(remote.declaration(), Err(refused.clone()));
    }
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let log = tree.read("artifacts/plugins/old.log");
    assert!(
        log.contains("symphony: its process was ended: it is refused: the plugin speaks"),
        "{log}"
    );
    // A new build of its program is started, and refused again.
    tree.write("bin/old", "another build, a longer one\n");
    assert_eq!(remote.declaration(), Err(refused));
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    assert_eq!(seen.kills(), 2);
}

#[test]
fn a_plugin_declaring_another_name_or_a_seam_a_process_does_not_answer_is_refused_naming_it() {
    let tree = Tree::new();
    let refused = |declared: Declaration| {
        let seen = Seen::default();
        let respond: Respond = Arc::new(|_, request| panic!("{request} was sent"));
        let remote = Remote::leaked(
            "tiny",
            process_in(&tree, "tiny"),
            hand_made(declared, respond, seen.clone(), "exited with code 0"),
            patient(),
        );
        let why = remote.declaration().unwrap_err();
        assert_eq!(seen.kills(), 1, "{why}");
        why
    };
    assert_eq!(
        refused(old_declared(PROTOCOL)),
        "its process is refused: it declares the plugin \"old\", not \"tiny\", which the project \
         names; it is not started again until its program or the project's settings change"
    );
    // Seams a process answers beside ones it does not: refused, naming only those.
    let mut wider = tiny_declared();
    wider.routes = vec!["/x".into()];
    wider.gates = vec!["landing".into()];
    wider.prose_rules = vec!["plain".into()];
    wider.transports = vec!["mail".into()];
    wider.theme = Some(symphony_plugin::Theme::default());
    assert_eq!(
        refused(wider),
        "its process is refused: it declares prose rules, transports, which a plugin's process \
         does not answer: link the plugin, or serve one without them; it is not started again \
         until its program or the project's settings change"
    );
}

#[test]
fn the_seams_a_process_does_not_answer_are_each_named() {
    use symphony_plugin::protocol::{DeclaredCheck, DeclaredGenerator, DeclaredRenderer};
    assert!(unanswered(&tiny_declared()).is_empty());
    let mut every = tiny_declared();
    every.renderers = vec![DeclaredRenderer::default()];
    every.routes = vec!["/r".into()];
    every.nav = vec![symphony_plugin::vocabulary::NavItem {
        label: "n".into(),
        title: "N".into(),
        href: "/n".into(),
        document: "docs/N.md".into(),
    }];
    every.home_regions = vec!["h".into()];
    every.board_actions = vec!["a".into()];
    // A board field as a process declares it, on the wire.
    every.board_fields =
        vec![serde_json::from_str(r#"{"name":"f","kind":"Text","required":false}"#).unwrap()];
    every.gates = vec!["g".into()];
    every.watches = vec![vec!["w".into()]];
    every.doc_checks = vec![DeclaredCheck::default()];
    every.prose_rules = vec!["p".into()];
    every.generators = vec![DeclaredGenerator::default()];
    every.commands = vec![DeclaredCommand::default()];
    every.transports = vec!["t".into()];
    every.theme = Some(symphony_plugin::Theme::default());
    assert_eq!(
        unanswered(&every),
        ["board fields", "prose rules", "generators", "transports"]
    );
}

/// What [`TINY`] declares, and one object at each seam a process answers besides its components
/// and card kinds, with its navigation and theme.
fn wide_declared() -> Declaration {
    use symphony_plugin::protocol::{DeclaredCheck, DeclaredRenderer};
    let mut renderer = DeclaredRenderer::default();
    renderer.name = "model".into();
    renderer.extensions = vec!["stl".into(), "obj".into()];
    renderer.content_type = "model/stl".into();
    let mut check = DeclaredCheck::default();
    check.name = "links".into();
    check.inputs = vec!["data/links.json".into()];
    let mut declared = tiny_declared();
    declared.renderers = vec![renderer];
    declared.routes = vec!["shelf".into()];
    declared.nav = vec![symphony_plugin::vocabulary::NavItem {
        label: "shelf".into(),
        title: "Shelf".into(),
        href: "/x/shelf".into(),
        document: "docs/SHELF.md".into(),
    }];
    declared.home_regions = vec!["progress".into()];
    declared.board_actions = vec!["land".into()];
    declared.gates = vec!["landing".into()];
    declared.watches = vec![
        vec!["data".into()],
        vec!["models".into(), "pictures".into()],
    ];
    declared.doc_checks = vec![check];
    declared.theme = Some(symphony_plugin::Theme {
        package: "tiny".into(),
        name: "Tiny".into(),
        ..symphony_plugin::Theme::default()
    });
    declared
}

#[test]
fn a_seam_whose_process_does_not_answer_is_shown_where_its_answer_goes() {
    use symphony_plugin::{Docs, File, Problem};
    let tree = Tree::new();
    // A pause longer than the test, so each call after the first finds it down.
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        hand_made(
            wide_declared(),
            Arc::new(|_, _| Said::End),
            Seen::default(),
            "was killed by signal 9",
        ),
        limits(PATIENT, Duration::from_secs(600)),
    );
    // What it declared answers with no call.
    let renderer = remote.renderers()[0];
    assert_eq!(renderer.name(), "model");
    assert_eq!(renderer.extensions(), ["stl", "obj"]);
    assert_eq!(renderer.content_type(), "model/stl");
    assert_eq!(remote.routes()[0].path(), "shelf");
    assert_eq!(remote.home_regions()[0].id(), "progress");
    assert_eq!(remote.board_actions()[0].name(), "land");
    assert_eq!(remote.gates()[0].name(), "landing");
    let watched: Vec<Vec<String>> = remote.watches().iter().map(|w| w.paths()).collect();
    assert_eq!(watched, wide_declared().watches);
    assert_eq!(remote.doc_checks()[0].inputs(), ["data/links.json"]);
    assert_eq!(remote.nav(), wide_declared().nav);
    assert_eq!(remote.theme(), wide_declared().theme);
    let page = Page::new(&tree.root, "docs/A.md");
    let file = File {
        src: "/docs/a.stl",
        label: "",
    };
    // A drawing is the renderer's error in the file's place.
    assert_eq!(
        renderer.draw(&page, &file),
        Err("plugin tiny: its process was killed by signal 9".into())
    );
    until("the remote is down", || remote.down().is_some());
    let down = "its process was killed by signal 9; it starts again in a moment";
    assert_eq!(
        renderer.draw(&page, &file),
        Err(format!("plugin tiny: {down}"))
    );
    // A route's answer is not found, a region none, a check's components none.
    assert_eq!(remote.routes()[0].serve(&tree.root, "a.json"), None);
    assert_eq!(remote.home_regions()[0].region(&tree.root), None);
    // A board action refuses, so the board offers nothing and moves nothing.
    let task = symphony_plugin::TaskRef {
        slug: "a",
        status: "review",
        branch: "a",
    };
    let action = remote.board_actions()[0];
    assert_eq!(
        action.offer(&tree.root, &task),
        Err(format!("plugin tiny: {down}"))
    );
    let submission = symphony_plugin::Submission::default();
    assert_eq!(
        action.submit(&tree.root, &task, &submission),
        Err(format!("plugin tiny: {down}"))
    );
    // A gates answer is the answerer's error, and what it is kept while unwinds with it.
    let gates = remote.gates()[0];
    assert_eq!(
        gates.answer(&tree.root, &task, Some("c0ffee")),
        Err(format!("plugin tiny: {down}"))
    );
    assert_eq!(
        untold(gates, &tree.root, &task),
        format!("plugin tiny: {down}")
    );
    // A watch's warming says so in the server's log, and its forgetting says nothing.
    let watch = remote.watches()[1];
    assert_eq!(
        watch.changed(&tree.root),
        Some(format!(
            "plugin tiny: its watch of models, pictures did not run: {down}"
        ))
    );
    watch.forget(&tree.root);
    let docs = Docs::read(&tree.root);
    assert!(remote.doc_checks()[0].components(&docs).is_empty());
    // A check that did not run is a problem, never a pass.
    assert_eq!(
        remote.doc_checks()[0].check(&docs),
        [Problem::new(
            "symphony.toml",
            0,
            format!("plugin tiny: its doc check links did not run: {down}")
        )]
    );
}

/// The command `every` of the plugin `tiny` whose program is `program`, its process never started.
fn command_of(tree: &Tree, program: &[&str]) -> seams::RemoteCommand {
    let remote = Remote::leaked(
        "tiny",
        PluginProcess {
            command: program.iter().map(|&word| word.into()).collect(),
            ..process_in(tree, "tiny")
        },
        Box::new(|| -> io::Result<Spawned> { panic!("a command started the plugin's process") }),
        patient(),
    );
    let command = seams::RemoteCommand {
        remote,
        name: "every".into(),
        summary: "print its arguments".into(),
    };
    assert_eq!(
        (command.name(), command.summary()),
        ("every", "print its arguments")
    );
    command
}

/// The command `every` of the plugin `tiny` whose program is `program` run with `args`: its exit
/// code and what was written to the `out` and `err` it was given.
fn commanded(tree: &Tree, program: &[&str], args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|&arg| arg.into()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = command_of(tree, program).run(&args, &mut out, &mut err);
    let text = |bytes| String::from_utf8(bytes).unwrap();
    (code, text(out), text(err))
}

/// A stream that keeps what it is given until it is flushed, then appends it to its file.
struct Held {
    file: std::path::PathBuf,
    held: Vec<u8>,
}

impl io::Write for Held {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.held.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        file.write_all(&std::mem::take(&mut self.held))
    }
}

#[test]
fn a_command_runs_as_the_plugins_program_with_this_process_streams_and_its_exit_code() {
    let tree = Tree::new();
    let said = tree.path("said");
    // The program's own words first, then `command NAME ARGS…`, each argument as it was given;
    // what it writes goes where it writes it, after what was written before is flushed.
    let script = r#"said=$1; shift
test "$1 $2" = "command every" || exit 99
shift 2
for arg; do printf '%s;' "$arg" >> "$said"; done
exit "$#""#;
    let command = command_of(
        &tree,
        &["/bin/sh", "-c", script, "sh", said.to_str().unwrap()],
    );
    let mut out = Held {
        file: said.clone(),
        held: b"before;".to_vec(),
    };
    let mut err = Vec::new();
    let args = ["--all", "a b", ""].map(String::from);
    assert_eq!(command.run(&args, &mut out, &mut err), 3);
    assert_eq!(
        std::fs::read_to_string(&said).unwrap(),
        "before;--all;a b;;"
    );
    assert!(out.held.is_empty() && err.is_empty());
    assert_eq!(command.run(&[], &mut out, &mut err), 0);
    assert_eq!(
        std::fs::read_to_string(&said).unwrap(),
        "before;--all;a b;;"
    );
}

#[test]
fn a_command_that_cannot_start_or_is_killed_says_so_on_its_stderr() {
    let tree = Tree::new();
    let (code, out, err) = commanded(&tree, &["/bin/sh", "-c", "kill -9 $$", "sh"], &[]);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (
            137,
            "",
            "plugin tiny: its command every was killed by signal 9\n"
        )
    );
    let missing = tree.path("no-such-plugin");
    let (code, out, err) = commanded(&tree, &[missing.to_str().unwrap()], &["a"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(
        err.starts_with("plugin tiny: its command every cannot start: "),
        "{err}"
    );
    assert_eq!(
        commanded(&tree, &[], &[]),
        (
            1,
            String::new(),
            "plugin tiny: its command every cannot start: it names no program\n".into()
        )
    );
}

/// The plugin `tiny` in `tree`, its first process ending at its first call and the next serving as
/// the plugin does, `pause` before a restart: its remote and the count of its starts.
fn ending_once(tree: &Tree, pause: Duration) -> (&'static Remote, Arc<AtomicUsize>) {
    let starts = Arc::new(AtomicUsize::new(0));
    let dying = hand_made(
        tiny_declared(),
        Arc::new(|_, _| Said::End),
        Seen::default(),
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
    let remote = Remote::leaked(
        "tiny",
        process_in(tree, "tiny"),
        start,
        limits(PATIENT, pause),
    );
    (remote, starts)
}

#[test]
fn a_process_that_ends_fails_its_waiting_calls_and_says_so_while_its_pause_lasts() {
    let tree = Tree::new();
    // A pause longer than the test, so each call after the end finds it down.
    let (remote, starts) = ending_once(&tree, Duration::from_secs(600));
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
    assert_eq!(starts.load(Ordering::SeqCst), 1, "not started again");
}

#[test]
fn a_process_that_ends_starts_again_after_its_pause() {
    let tree = Tree::new();
    let pause = Duration::from_millis(300);
    let (remote, starts) = ending_once(&tree, pause);
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    assert_eq!(
        component.render(&page, "a", None),
        Err("plugin tiny: its process was killed by signal 9".into())
    );
    until("the remote is down", || remote.down().is_some());
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test waits out the plugin's restart pause"
    )]
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
        limits(PATIENT, Duration::ZERO),
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
fn a_call_with_no_answer_in_its_wait_is_cancelled_and_a_process_still_silent_a_wait_later_killed() {
    let tree = Tree::new();
    let seen = Seen::default();
    let wait = Duration::from_millis(50);
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        hand_made(
            tiny_declared(),
            Arc::new(|_, _| Said::Lines(Vec::new())),
            seen.clone(),
            "exited with code 0",
        ),
        limits(wait, PAUSE),
    );
    let (component, kind) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    assert_eq!(
        component.render(&page, "a", None),
        Err("plugin tiny: no answer in 50ms".into())
    );
    // The call it gave up was cancelled by its id.
    until("the cancel was sent", || seen.heard().len() == 3);
    let heard = seen.heard();
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
    // Still silent a wait after its cancel, the process is killed and the plugin down.
    let hung = format!(
        "its process was ended: call {} had no answer 50ms after it was cancelled",
        render.id
    );
    until("the hung process was ended", || {
        remote.down().is_some_and(|why| why == hung)
    });
    assert_eq!(seen.kills(), 1);
    with_card(CARDS[0], |card| assert_eq!(kind.slot(card), None));
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains("symphony: components.render of tiny: no answer in 50ms"),
        "{log}"
    );
    assert!(log.contains(&format!("symphony: {hung}")), "{log}");
}

#[test]
fn an_answer_after_its_call_was_given_up_is_logged_and_dropped_and_the_process_kept() {
    let tree = Tree::new();
    let seen = Seen::default();
    let wait = Duration::from_secs(1);
    // The first render is answered only once the test lets it, after its call was given up; the
    // others at once, an answer to no call before each.
    let (let_go, held) = crate::test_channel::channel::<()>();
    let held = Mutex::new(Some(held));
    let respond: Respond = Arc::new(move |id, _| {
        if let Some(held) = held.lock().recover().take() {
            // Until the test lets it go, or lets go of its end.
            let _ = held.recv();
            return Said::Lines(vec![answering(id, &rendered("late"))]);
        }
        Said::Lines(vec![
            answering(999, &rendered("nobody's")),
            answering(id, &rendered("on time")),
        ])
    });
    // A call given up may still be answered for as long as only a stall outlasts, so the late
    // answer comes inside that however loaded the machine.
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        hand_made(tiny_declared(), respond, seen.clone(), "exited with code 0"),
        Limits {
            cancel: test_time::backstop(wait),
            ..limits(wait, PAUSE)
        },
    );
    let (component, _) = seams(remote);
    let connection = remote.connection().unwrap();
    let page = Page::new(&tree.root, "docs/A.md");
    assert_eq!(
        component.render(&page, "a", None),
        Err("plugin tiny: no answer in 1s".into())
    );
    let_go.send(()).unwrap();
    until("the late answer was read", || {
        tree.read("artifacts/plugins/tiny.log")
            .contains("was answered after it was given up: dropped")
    });
    let render: Raw = serde_json::from_str(&seen.heard()[1]).unwrap();
    // Once the wait after its cancel has run out, the process that answered is kept.
    remote.after_cancel(&connection, render.id);
    assert!(remote.down().is_none(), "{:?}", remote.down());
    // Kept, it answers the next call: one a loaded machine leaves past its wait is answered late,
    // dropped as the first was, and asked again.
    let next =
        test_time::until(
            "the next call answered within its wait",
            PATIENT,
            || match component.render(&page, "b", None) {
                Err(why) if why == "plugin tiny: no answer in 1s" => None,
                answered => Some(answered),
            },
        )
        .unwrap_or_else(|stall| panic!("{stall}"));
    assert_eq!(text(next), "on time");
    assert_eq!(seen.kills(), 0);
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains(&format!(
            "symphony: call {} was answered after it was given up: dropped",
            render.id
        )),
        "{log}"
    );
    assert!(
        log.contains("symphony: a line that answers no call (no call 999 waits): {\"id\":999,"),
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
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        hand_made(
            tiny_declared(),
            respond,
            Seen::default(),
            "exited with code 0",
        ),
        patient(),
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
fn a_line_that_is_not_utf8_is_logged_shown_and_skipped_and_the_calls_after_it_answered() {
    let tree = Tree::new();
    // Before each answer, a line of bytes that are not text, with a control byte among them.
    let respond: Respond = Arc::new(|id, _| {
        let mut bytes = b"\xff\xfe stray \x1b[31m\n".to_vec();
        bytes.extend_from_slice(answering(id, &rendered("drawn")).as_bytes());
        bytes.push(b'\n');
        Said::Bytes(bytes)
    });
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        hand_made(
            tiny_declared(),
            respond,
            Seen::default(),
            "exited with code 0",
        ),
        patient(),
    );
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    for _ in 0..2 {
        assert_eq!(text(component.render(&page, "a", None)), "drawn");
    }
    assert!(remote.down().is_none());
    let log = tree.read("artifacts/plugins/tiny.log");
    assert_eq!(
        log.matches("): \u{fffd}\u{fffd} stray \u{fffd}[31m\n")
            .count(),
        2,
        "{log}"
    );
}

/// An output that gives what its pipe is written and then, where the pipe ends, fails.
struct Breaking {
    pipe: io::PipeReader,
}

impl io::Read for Breaking {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.pipe.read(buf)? {
            0 if !buf.is_empty() => Err(io::Error::other("the pipe broke")),
            read => Ok(read),
        }
    }
}

/// A process standing still: it ends only once killed, as `kills` counts.
struct Still {
    kills: Arc<AtomicUsize>,
}

impl Handle for Still {
    fn kill(&mut self) {
        self.kills.fetch_add(1, Ordering::SeqCst);
    }
    fn ended(&mut self) -> Option<String> {
        (self.kills.load(Ordering::SeqCst) > 0).then(|| "was killed by signal 9".into())
    }
}

#[test]
fn a_stdout_that_fails_ends_its_process_killed_and_the_plugin_down() {
    let tree = Tree::new();
    let kills = Arc::new(AtomicUsize::new(0));
    let killed = kills.clone();
    let start: Start = Box::new(move || {
        // It declares once it has read the request, as a process does, and its stdout then breaks.
        let (output, answers) = io::pipe()?;
        Ok(Spawned {
            input: Box::new(Answering::new([tiny_declaring(1)], answers)),
            output: Box::new(Breaking { pipe: output }),
            errors: None,
            pid: None,
            process: Box::new(Still {
                kills: killed.clone(),
            }),
        })
    });
    // A pause longer than the test, so the next call finds it down rather than starting it again.
    let pause = Duration::from_secs(600);
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        start,
        limits(PATIENT, pause),
    );
    assert_eq!(remote.declaration(), Ok(&tiny_declared()));
    let broke = "its process was ended: its stdout could not be read: the pipe broke";
    until("the plugin is down", || {
        remote.down().is_some_and(|why| why == broke)
    });
    assert_eq!(kills.load(Ordering::SeqCst), 1);
    let (component, _) = seams(remote);
    assert_eq!(
        component.render(&Page::new(&tree.root, "docs/A.md"), "a", None),
        Err(format!("plugin tiny: {broke}; it starts again in a moment"))
    );
}

#[test]
fn a_declaration_with_no_answer_in_its_wait_kills_the_process() {
    let tree = Tree::new();
    let kills = Arc::new(AtomicUsize::new(0));
    let killed = kills.clone();
    let start: Start = Box::new(move || {
        // A process that reads nothing and says nothing: its stdout never ends.
        let (output, silent) = io::pipe()?;
        std::mem::forget(silent);
        Ok(Spawned {
            input: Box::new(io::sink()),
            output: Box::new(output),
            errors: None,
            pid: None,
            process: Box::new(Still {
                kills: killed.clone(),
            }),
        })
    });
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        start,
        Limits {
            declare: Duration::from_millis(50),
            ..limits(Duration::from_millis(50), PAUSE)
        },
    );
    assert_eq!(
        remote.declaration(),
        Err("its process did not declare: no answer in 50ms".into())
    );
    assert_eq!(kills.load(Ordering::SeqCst), 1);
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains("symphony: its process was ended: it did not declare: no answer in 50ms"),
        "{log}"
    );
}

#[test]
fn a_line_past_the_bound_ends_the_process_and_the_plugin_starts_again_after_its_pause() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let seen = Seen::default();
    let flooding = hand_made(
        tiny_declared(),
        Arc::new(|_, _| Said::Lines(vec!["x".repeat(5000)])),
        seen.clone(),
        "exited with code 0",
    );
    let serving = peer(&TINY, Arc::default());
    let counted = starts.clone();
    let start: Start = Box::new(move || match counted.fetch_add(1, Ordering::SeqCst) {
        0 => flooding(),
        _ => serving(),
    });
    let pause = Duration::from_millis(100);
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        start,
        Limits {
            line: 4096,
            ..limits(PATIENT, pause)
        },
    );
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    let ended = component.render(&page, "a", None).unwrap_err();
    assert_eq!(
        ended,
        format!(
            "plugin tiny: its process was ended: it wrote a line longer than the protocol's 4096 \
             bytes: {}…",
            "x".repeat(SHOWN)
        )
    );
    assert_eq!(seen.kills(), 1);
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test waits out the plugin's restart pause"
    )]
    std::thread::sleep(pause);
    assert_eq!(
        component.render(&page, "a", None),
        TINY.components()[0].render(&page, "a", None)
    );
    assert_eq!(starts.load(Ordering::SeqCst), 2);
}

#[test]
fn a_request_past_the_bound_is_not_sent_and_its_process_kept() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        peer(&TINY, starts.clone()),
        Limits {
            line: 1000,
            ..patient()
        },
    );
    let (component, _) = seams(remote);
    let page = Page::new(&tree.root, "docs/A.md");
    let refused = component
        .render(&page, &"x".repeat(1000), None)
        .unwrap_err();
    assert!(
        refused.starts_with("plugin tiny: its request is a line of ")
            && refused.ends_with(" bytes, longer than the protocol's 1000"),
        "{refused}"
    );
    assert!(component.render(&page, "a", None).is_ok());
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

/// A stdin that takes what it is written into `input` until `stuck`, then takes nothing until it
/// is not.
struct Stuck {
    input: Box<dyn Write + Send>,
    stuck: Arc<AtomicBool>,
}

impl Write for Stuck {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        while self.stuck.load(Ordering::SeqCst) {
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: a test's writer looks again for a flag its test clears, where a condvar is the event"
            )]
            std::thread::sleep(Duration::from_millis(1));
        }
        self.input.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.input.flush()
    }
}

#[test]
fn a_queue_its_stdin_takes_nothing_of_ends_the_process() {
    let tree = Tree::new();
    let seen = Seen::default();
    let stuck = Arc::new(AtomicBool::new(false));
    let serving = hand_made(
        tiny_declared(),
        Arc::new(|_, _| Said::Lines(Vec::new())),
        seen.clone(),
        "exited with code 0",
    );
    let held = stuck.clone();
    let start: Start = Box::new(move || {
        let mut spawned = serving()?;
        spawned.input = Box::new(Stuck {
            input: spawned.input,
            stuck: held.clone(),
        });
        Ok(spawned)
    });
    let remote = Remote::leaked("tiny", process_in(&tree, "tiny"), start, patient());
    let connection = remote.connection().unwrap();
    stuck.store(true, Ordering::SeqCst);
    // The writer takes one line and sticks on it; the queue holds the rest, then refuses.
    let full = (0..=QUEUED + 1).find_map(|_| remote.queued(&connection, b"{}\n".to_vec()).err());
    stuck.store(false, Ordering::SeqCst);
    let full = full.expect("the queue never filled");
    assert_eq!(
        full,
        format!("its process was ended: its stdin took none of the {QUEUED} lines queued for it")
    );
    assert_eq!(seen.kills(), 1);
    assert_eq!(remote.down(), Some(full));
}

#[test]
fn a_real_process_that_stops_reading_is_ended_in_the_wait_and_holds_no_other_call_past_it() {
    let tree = Tree::new();
    // It answers the declaration, then reads nothing more, and lives on past what only a stall
    // outlasts: only the host ends it, and a call it held would be held as long.
    let declares = tiny_declaring(1);
    let wait = Duration::from_secs(1);
    let lives = test_time::backstop(wait).as_secs().to_string();
    let process = PluginProcess {
        command: [
            "sh",
            "-c",
            "read line; printf '%s\\n' \"$1\"; exec sleep \"$2\"",
            "sh",
            &declares,
            &lives,
        ]
        .map(String::from)
        .to_vec(),
        ..process_in(&tree, "tiny")
    };
    let started = process.clone();
    let remote = Remote::leaked(
        "tiny",
        process,
        Box::new(move || spawn(&started)),
        limits(wait, PAUSE),
    );
    let (component, _) = seams(remote);
    let pid = remote.pid().unwrap();
    let connection = remote.connection().unwrap();
    let page = Page::new(&tree.root, "docs/A.md");
    // Two requests, each larger than a pipe holds: the one queued first sticks part written, and
    // the other waits behind it, whichever goes first.
    let (x, y) = ("x".repeat(1 << 20), "y".repeat(1 << 20));
    let failed = |argument: &str| component.render(&page, argument, None).unwrap_err();
    let (x_failed, y_failed) = std::thread::scope(|scope| {
        // Neither call's wait begins before both calls wait: each queues its line, which begins
        // its wait, only once the test lets go of the queue.
        let queue = connection.queue.lock().recover();
        let xs = scope.spawn(|| failed(&x));
        let ys = scope.spawn(|| failed(&y));
        until("both calls wait", || {
            connection.waiting.lock().recover().calls.len() == 2
        });
        drop(queue);
        (xs.join().unwrap(), ys.join().unwrap())
    });
    let stuck = format!("plugin tiny: its process {pid} was ended: its stdin took no line in 1s");
    assert_eq!(x_failed, stuck);
    assert_eq!(y_failed, stuck);
    assert!(!alive(pid), "{pid} still runs");
}

/// A process that, once killed, is seen ended only when `released`, as a process slow to die is.
struct Lingering {
    killed: bool,
    released: Arc<AtomicBool>,
}

impl Handle for Lingering {
    fn kill(&mut self) {
        self.killed = true;
    }
    fn ended(&mut self) -> Option<String> {
        (self.killed && self.released.load(Ordering::SeqCst))
            .then(|| "was killed by signal 9".into())
    }
}

#[test]
fn every_call_that_finds_its_process_ending_is_told_how_the_first_ended_it() {
    let tree = Tree::new();
    // It declares once it has read the request, as a process does; its stdout stays open until
    // the test lets go of `answers`, so it ends only as the test ends it.
    let (output, answers) = io::pipe().unwrap();
    let declares = Mutex::new(Some(Answering::new(
        [tiny_declaring(1)],
        answers.try_clone().unwrap(),
    )));
    let released = Arc::new(AtomicBool::new(false));
    let lingering = released.clone();
    let start: Start = Box::new(move || {
        let input = declares
            .lock()
            .recover()
            .take()
            .ok_or_else(|| io::Error::other("it is started once"))?;
        Ok(Spawned {
            input: Box::new(input),
            output: Box::new(output.try_clone()?),
            errors: None,
            pid: None,
            process: Box::new(Lingering {
                killed: false,
                released: lingering.clone(),
            }),
        })
    });
    let remote = Remote::leaked("tiny", process_in(&tree, "tiny"), start, patient());
    let connection = remote.connection().unwrap();
    let first = "its process was ended: the first call's reason";
    std::thread::scope(|scope| {
        // One call ends it, and its process is not seen ended until the test lets it.
        scope.spawn(|| remote.end(&connection, Some("the first call's reason".into())));
        until("it is ending", || {
            connection.waiting.lock().recover().ending
        });
        // Another call ends it meanwhile, which leaves the first's reason standing.
        remote.end(&connection, Some("the second call's reason".into()));
        released.store(true, Ordering::SeqCst);
    });
    // Every call that finds it ending is told the first's reason (each asks `closed`).
    assert_eq!(connection.closed(), first);
    assert_eq!(remote.down().as_deref(), Some(first));
    // A request that reaches it after, or a line queued for its closed stdin, is told the same.
    let request = Render::new(
        "tiny".into(),
        PageData::of(&Page::new(&tree.root, "docs/A.md")),
        "a".into(),
        None,
    );
    let sent = remote.sent_within(&connection, request.into(), WAIT, None, &mut |_| Ok(()));
    assert_eq!(sent.err().as_deref(), Some(first));
    assert_eq!(
        remote
            .queued(&connection, b"{}\n".to_vec())
            .err()
            .as_deref(),
        Some(first)
    );
    drop(answers);
}

#[test]
fn a_child_dropped_while_it_runs_is_killed_and_reaped() {
    let tree = Tree::new();
    let process = PluginProcess {
        command: vec!["sleep".into(), "30".into()],
        ..process_in(&tree, "sleep")
    };
    let spawned = spawn(&process).unwrap();
    let pid = spawned.pid.unwrap();
    assert!(alive(pid));
    drop(spawned);
    assert!(!alive(pid), "{pid} still runs");
}

#[test]
fn a_declaration_changed_at_a_restart_is_noted_and_the_first_stands() {
    let tree = Tree::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let first = hand_made(
        tiny_declared(),
        Arc::new(|_, _| Said::End),
        Seen::default(),
        "exited with code 0",
    );
    let changed = hand_made(
        {
            let mut declared = tiny_declared();
            declared.components = vec!["tiny".into(), "another".into()];
            declared
        },
        Arc::new(|_, _| Said::End),
        Seen::default(),
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
        limits(PATIENT, Duration::ZERO),
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
        patient(),
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
        patient(),
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
        limits(PATIENT, Duration::ZERO),
    );
    remote.record_into(Some(Record::at(&record)));
    let request = Recognises::new("tiny".into(), with_card("", CardData::of));
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
    for absent in ["us", "request_bytes", "answer_bytes"] {
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
        patient(),
    );
    let request = symphony_plugin::protocol::RunCommand::new("every".into(), Vec::new());
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

/// A main checkout at `main` and a worktree of it at `main/wt`, as git links them, each with
/// `file` as its `symphony.toml`.
fn checkout_and_worktree(main: &Tree, main_file: &str, branch_file: &str) -> PathBuf {
    main.write("symphony.toml", main_file);
    main.write(".git/worktrees/wt/commondir", "../..\n");
    let branch = main.path("wt");
    main.write(
        "wt/.git",
        &format!("gitdir: {}\n", main.path(".git/worktrees/wt").display()),
    );
    main.write("wt/symphony.toml", branch_file);
    branch
}

#[test]
fn a_branchs_checkout_is_answered_by_its_main_checkouts_process_and_starts_none_of_its_own() {
    let main = Tree::new();
    let file = "[plugins]\ncrates = [\"tiny\"]\n\n[plugins.process]\ntiny = [\"./bin/tiny\"]\n";
    let branch = checkout_and_worktree(&main, file, file);
    let main_root = crate::builds::main_checkout(&branch).unwrap();
    let main_project = Project::of(&main_root);
    let branch_project = Project::of(&branch);
    let (in_main, in_branch) = (
        &main_project.plugin_processes["tiny"],
        &branch_project.plugin_processes["tiny"],
    );
    // The main checkout's own process; the branch's, the main checkout's too.
    assert_eq!(super::answering("tiny", in_main).as_ref(), Some(in_main));
    assert_eq!(super::answering("tiny", in_branch).as_ref(), Some(in_main));
    assert_eq!(in_main.root, main_root);
    let plugins = crate::plugins::Plugins::for_project(&branch_project);
    let remote = remote(plugins.list()[0]).unwrap();
    assert_eq!(remote.process(), in_main);
    // A branch that adds a process the main checkout does not start has none: with no plugin of
    // that name linked, none at all.
    let main = Tree::new();
    let branch = checkout_and_worktree(&main, "[plugins]\ncrates = [\"tiny\"]\n", file);
    let branch_project = Project::of(&branch);
    let in_branch = &branch_project.plugin_processes["tiny"];
    assert_eq!(super::answering("tiny", in_branch), None);
    let plugins = crate::plugins::Plugins::for_project(&branch_project);
    assert!(plugins.list().is_empty());
}

#[test]
fn a_log_past_its_bound_is_rolled_and_begun_again() {
    let tree = Tree::new();
    let start: Start = Box::new(|| Err(io::Error::other("never started")));
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        start,
        Limits {
            log: 1000,
            ..patient()
        },
    );
    for line in 0..40 {
        remote.log(&format!("line {line:02} {}", "-".repeat(40)));
    }
    let (log, rolled) = (
        tree.read("artifacts/plugins/tiny.log"),
        tree.read("artifacts/plugins/tiny.1.log"),
    );
    assert!(log.len() <= 1000 && rolled.len() <= 1000, "{log}\n{rolled}");
    assert!(log.contains("line 39 "), "{log}");
    // The two hold the newest lines, in order, and nothing older than the roll before.
    let said: Vec<&str> = rolled
        .lines()
        .chain(log.lines())
        .map(|line| line.split_once(" symphony: line ").unwrap().1)
        .collect();
    let first: usize = said[0][..2].parse().unwrap();
    assert!(first > 0, "{said:?}");
    let expected: Vec<String> = (first..40)
        .map(|line| format!("{line:02} {}", "-".repeat(40)))
        .collect();
    assert_eq!(said, expected);
}

#[test]
fn a_line_the_log_shows_is_cut_at_its_bound_with_no_control_character() {
    assert_eq!(shown(b"plain text"), "plain text");
    assert_eq!(shown(b"a\tb\x1b[0m\xff"), "a\u{fffd}b\u{fffd}[0m\u{fffd}");
    let long = vec![b'x'; SHOWN + 10];
    assert_eq!(
        shown(&long),
        format!("{}… ({} bytes)", "x".repeat(SHOWN), SHOWN + 10)
    );
    assert_eq!(shown(&long[..SHOWN]), "x".repeat(SHOWN));
}

/// A remote of the real process `command`, in `tree`.
fn spawned(tree: &Tree, name: &str, command: &[&str]) -> &'static Remote {
    let process = PluginProcess {
        command: command.iter().map(|word| (*word).to_string()).collect(),
        root: tree.root.clone(),
        log: tree.path(&format!("artifacts/plugins/{name}.log")),
    };
    let started = process.clone();
    Remote::leaked(name, process, Box::new(move || spawn(&started)), patient())
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
    // What it said on stderr comes before its end.
    let said = log
        .find("\nthe checkout\n")
        .unwrap_or_else(|| panic!("{log}"));
    let end = log
        .find(&format!("symphony: its process {pid} exited with code 3"))
        .unwrap_or_else(|| panic!("{log}"));
    assert!(said < end, "{log}");
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
fn a_real_process_whose_stderr_line_is_long_is_logged_in_pieces() {
    let tree = Tree::new();
    let remote = spawned(
        &tree,
        "long",
        &[
            "sh",
            "-c",
            "read line; head -c 10000 /dev/zero | tr '\\0' y >&2; echo >&2; exit 0",
        ],
    );
    assert!(remote.declaration().is_err());
    let log = tree.read("artifacts/plugins/long.log");
    let pieces: Vec<&str> = log.lines().filter(|line| line.starts_with('y')).collect();
    assert_eq!(pieces.len(), 3, "{log}");
    assert_eq!(pieces[0], format!("{}…", "y".repeat(ERROR_LINE)));
    assert_eq!(pieces[1], format!("{}…", "y".repeat(ERROR_LINE)));
    assert_eq!(pieces[2], "y".repeat(10_000 - 2 * ERROR_LINE));
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
    let remote = Remote::leaked("tiny", process_in(&tree, "tiny"), start, patient());
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
    symphony_board::order::sort_by(&mut said, &Ord::cmp);
    let mut every: Vec<String> = (0..8)
        .flat_map(|thread| (0..100).map(move |line| format!("thread {thread} line {line}")))
        .collect();
    symphony_board::order::sort_by(&mut every, &Ord::cmp);
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

/// A start of a peer that declares `declared` and answers every other request as `plugin`'s
/// process does, keeping what `seen` sees.
fn serving_as(plugin: &'static dyn Plugin, declared: Declaration, seen: Seen) -> Start {
    let respond: Respond = Arc::new(move |id, request| {
        let line = format!("{{\"id\":{id},\"request\":{request}}}\n");
        let mut answered = Vec::new();
        symphony_plugin::serve_on(plugin, line.as_bytes(), &mut answered).unwrap();
        Said::Bytes(answered)
    });
    hand_made(declared, respond, seen, "exited with code 0")
}

/// That `kind` answers every call of `card` as `linked` does, its pictures read at `root`.
fn kinds_alike(kind: &dyn CardKind, linked: &dyn CardKind, root: &Path, card: &Card<'_>) {
    let slug = card.slug;
    assert_eq!(kind.name(), linked.name());
    assert_eq!(kind.markers(), linked.markers());
    assert_eq!(kind.recognises(card), linked.recognises(card), "{slug}");
    assert_eq!(
        kind.pictures(root, card),
        linked.pictures(root, card),
        "{slug}"
    );
    assert_eq!(kind.slot(card), linked.slot(card), "{slug}");
    assert_eq!(
        kind.triage_category(card),
        linked.triage_category(card),
        "{slug}"
    );
    assert_eq!(
        kind.auto_approvable(card),
        linked.auto_approvable(card),
        "{slug}"
    );
    assert_eq!(kind.gates(card), linked.gates(card), "{slug}");
    assert_eq!(
        kind.approval_needs_agent(card),
        linked.approval_needs_agent(card),
        "{slug}"
    );
    assert_eq!(kind.chosen(card), linked.chosen(card), "{slug}");
}

/// `count` cards of [`TINY`]'s, `pick-1` on, each of a text long enough that a request of it
/// outweighs its answer.
fn long_cards(count: usize) -> Vec<CardData> {
    (1..=count)
        .map(|n| {
            let text = format!("Pick tiny {n}. {}", "Its long story. ".repeat(40));
            let mut card = with_card(&text, CardData::of);
            card.slug = format!("pick-{n}");
            card
        })
        .collect()
}

/// The calls of `heard`, the lines a peer was sent, that are register reads.
fn reads(heard: &[String]) -> Vec<ReadCards> {
    heard
        .iter()
        .filter_map(|line| {
            let read: Raw = serde_json::from_str(line).ok()?;
            match serde_json::from_str::<Request>(read.request?.get()).ok()? {
                Request::ReadCards(asked) => Some(asked),
                _ => None,
            }
        })
        .collect()
}

#[test]
fn a_register_read_asks_its_cards_in_as_few_requests_as_fit_a_line_and_answers_each_as_linked() {
    let tree = Tree::new();
    let cards = long_cards(7);
    // A line that holds three of them as the host counts one, its id the longest an id is.
    let three = serde_json::to_vec(&Envelope::<Request, ()>::asking(
        u64::MAX,
        ReadCards::new(tree.root.clone(), cards[..3].to_vec()).into(),
    ))
    .unwrap()
    .len();
    let seen = Seen::default();
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        serving_as(&TINY, tiny_declared(), seen.clone()),
        Limits {
            line: three,
            ..patient()
        },
    );
    let read = remote.read_cards(&tree.root, &cards).unwrap();
    // Seven cards, three to a line: three requests, in the cards' order, none past the bound.
    let asked = reads(&seen.heard());
    let sent: Vec<Vec<CardData>> = asked.iter().map(|read| read.cards.clone()).collect();
    assert_eq!(
        sent,
        [
            cards[..3].to_vec(),
            cards[3..6].to_vec(),
            cards[6..].to_vec()
        ]
    );
    assert!(asked.iter().all(|read| read.root == tree.root));
    for line in seen.heard() {
        assert!(line.len() <= three, "{} > {three}", line.len());
    }
    // Every call of every card is answered from the read as linked, with no request more.
    let heard = seen.heard().len();
    let (kind, linked) = (read.card_kinds()[0], TINY.card_kinds()[0]);
    for card in &cards {
        card.read(|card| kinds_alike(kind, linked, &tree.root, card));
    }
    assert_eq!(seen.heard().len(), heard);
    assert!(read.of(remote) && !read.of(&TINY));
    // A card the read does not hold, and the pictures of another checkout, are asked one by one.
    let elsewhere = Path::new("/elsewhere");
    let other = long_cards(8).pop().unwrap();
    other.read(|card| assert_eq!(kind.slot(card), linked.slot(card)));
    cards[0].read(|card| {
        assert_eq!(
            kind.pictures(elsewhere, card),
            linked.pictures(elsewhere, card)
        );
    });
    assert_eq!(seen.heard().len(), heard + 2);
    assert_eq!(reads(&seen.heard()).len(), 3);
}

#[test]
fn a_card_too_long_for_a_line_alone_is_left_to_its_own_calls_and_the_others_read() {
    let tree = Tree::new();
    let mut cards = long_cards(3);
    let one = serde_json::to_vec(&Envelope::<Request, ()>::asking(
        u64::MAX,
        ReadCards::new(tree.root.clone(), cards[..1].to_vec()).into(),
    ))
    .unwrap()
    .len();
    cards[1].text = format!(
        "Pick tiny 2.\n*Slot:* North\n{}",
        "Its long story. ".repeat(45)
    );
    let seen = Seen::default();
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        serving_as(&TINY, tiny_declared(), seen.clone()),
        Limits {
            line: one,
            ..patient()
        },
    );
    let read = remote.read_cards(&tree.root, &cards).unwrap();
    let sent: Vec<Vec<CardData>> = reads(&seen.heard())
        .into_iter()
        .map(|read| read.cards)
        .collect();
    assert_eq!(sent, [vec![cards[0].clone()], vec![cards[2].clone()]]);
    // The long card's own request is past the bound too: it is answered as the kind's default.
    let kind = read.card_kinds()[0];
    cards[1].read(|card| {
        assert_eq!(TINY.slot(card).as_deref(), Some("North"));
        assert_eq!(kind.slot(card), None);
    });
    cards[2].read(|card| assert_eq!(kind.slot(card), TINY.slot(card)));
    let log = tree.read("artifacts/plugins/tiny.log");
    assert!(
        log.contains("symphony: card_kinds.slot of tiny: its request is a line of "),
        "{log}"
    );
}

/// Registers of [`TINY`]'s cards, two documents' worth, read as the board reads them.
fn registers() -> Vec<crate::board::Doc> {
    let doc = |path: &str, lines: &[&str]| crate::board::Doc {
        path: path.into(),
        comments: Vec::new(),
        decisions: crate::register::parse_decisions(lines),
    };
    vec![
        doc(
            "docs/WIDGETS.md",
            &[
                "- [ ] **pick-a** — Pick a tiny widget. *Slot:* North",
                "  ↳ A or B?",
                "  B please",
                "- [x] **pick-b** — hold the broken one",
                "  ↳ done",
            ],
        ),
        doc("docs/OTHER.md", &["- [ ] **plain** — Nothing tiny here."]),
    ]
}

/// That `plugins` answers every card of `docs` as `linked` does, as the board asks of a card.
fn read_alike(plugins: &Plugins, linked: &Plugins, root: &Path, docs: &[crate::board::Doc]) {
    for doc in docs {
        let on = doc.path.as_str();
        for card in &doc.decisions {
            assert_eq!(plugins.slot(card), linked.slot(card));
            assert_eq!(
                plugins.pictures(root, on, card),
                linked.pictures(root, on, card)
            );
            assert_eq!(
                plugins.approval_needs_agent(card),
                linked.approval_needs_agent(card)
            );
            assert_eq!(
                plugins.triage_category(on, card),
                linked.triage_category(on, card)
            );
            assert_eq!(plugins.chosen(card), linked.chosen(card));
            assert_eq!(plugins.auto_approvable(card), linked.auto_approvable(card));
            assert_eq!(plugins.gates(on, card), linked.gates(on, card));
        }
    }
}

#[test]
fn a_read_of_the_registers_is_one_request_and_answers_every_card_as_linked() {
    let tree = Tree::new();
    let seen = Seen::default();
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        serving_as(&TINY, tiny_declared(), seen.clone()),
        patient(),
    );
    let docs = registers();
    let plugins = Plugins::new(vec![remote]).reading(&tree.root, &docs);
    let asked = reads(&seen.heard());
    assert_eq!(asked.len(), 1, "one request for the read");
    // Each card as it is asked: with its document, and with none.
    let docs_sent: Vec<(&str, &str)> = asked[0]
        .cards
        .iter()
        .map(|card| (card.slug.as_str(), card.doc.as_str()))
        .collect();
    assert_eq!(
        docs_sent,
        [
            ("pick-a", "docs/WIDGETS.md"),
            ("pick-a", ""),
            ("pick-b", "docs/WIDGETS.md"),
            ("pick-b", ""),
            ("plain", "docs/OTHER.md"),
            ("plain", ""),
        ]
    );
    let heard = seen.heard().len();
    read_alike(&plugins, &Plugins::new(vec![&TINY]), &tree.root, &docs);
    assert_eq!(seen.heard().len(), heard, "every answer from the read");
    // The plugins as they were are asked card by card still.
    let unread = Plugins::new(vec![remote]);
    read_alike(&unread, &Plugins::new(vec![&TINY]), &tree.root, &docs);
    assert!(seen.heard().len() > heard);
    // A read of plugins none of which answers from a process asks nothing.
    let linked = Plugins::new(vec![&TINY]).reading(&tree.root, &docs);
    assert!(linked.read.is_empty());
}

#[test]
fn a_plugin_of_the_previous_minor_is_asked_a_reads_cards_one_by_one_and_answers_the_same() {
    let tree = Tree::new();
    let seen = Seen::default();
    let previous = Version::new(PROTOCOL.major, ReadCards::SINCE - 1);
    let remote = Remote::leaked(
        "tiny",
        process_in(&tree, "tiny"),
        serving_as(
            &TINY,
            {
                let mut declared = tiny_declared();
                declared.protocol = previous;
                declared
            },
            seen.clone(),
        ),
        patient(),
    );
    let docs = registers();
    let plugins = Plugins::new(vec![remote]).reading(&tree.root, &docs);
    assert!(plugins.read.is_empty());
    read_alike(&plugins, &Plugins::new(vec![&TINY]), &tree.root, &docs);
    let heard = seen.heard();
    assert!(reads(&heard).is_empty(), "{heard:?}");
    assert!(
        heard
            .iter()
            .any(|line| line.contains("\"card_kinds.slot\"")),
        "{heard:?}"
    );
}

#[test]
fn a_read_whose_process_is_down_answers_each_card_as_a_kind_that_says_nothing() {
    let tree = Tree::new();
    let (remote, _) = ending_once(&tree, Duration::from_secs(600));
    let docs = registers();
    // Its declaration came at its start; the read is its first call, which finds it ending.
    assert!(remote.declaration().is_ok());
    let plugins = Plugins::new(vec![remote]).reading(&tree.root, &docs);
    assert!(plugins.read.is_empty());
    let bare: &'static dyn Plugin = &BarePlugin;
    read_alike(&plugins, &Plugins::new(vec![bare]), &tree.root, &docs);
}

/// A plugin whose one card kind is [`BareKind`], named as [`TINY`]'s is.
struct BarePlugin;

impl Plugin for BarePlugin {
    fn name(&self) -> &str {
        "tiny"
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![&BareKind]
    }
}

/// A project's approval authorities: `approvals` signs the guard it is asked of with the time it
/// was given, `closed` cannot say, `broken` panics.
struct Approver(&'static str);

static APPROVERS: Approvers = Approvers;

struct Approvers;

impl Plugin for Approvers {
    fn name(&self) -> &str {
        "approvers"
    }
    fn authorities(&self) -> Vec<&dyn symphony_plugin::Authority> {
        static EACH: [Approver; 3] = [
            Approver("approvals"),
            Approver("closed"),
            Approver("broken"),
        ];
        EACH.iter()
            .map(|a| -> &dyn symphony_plugin::Authority { a })
            .collect()
    }
}

impl symphony_plugin::Authority for Approver {
    fn adapter(&self) -> &str {
        self.0
    }
    fn attest(
        &self,
        _: &Path,
        operation: &symphony_plugin::authority::Operation,
        within: Duration,
    ) -> Result<symphony_plugin::authority::Signed, String> {
        match self.0 {
            "approvals" => Ok(symphony_plugin::authority::Signed::new(
                format!("{} within {} ms", operation.guard, within.as_millis()),
                "ab".repeat(64),
            )),
            "closed" => Err("the ledger is closed".into()),
            _ => panic!("the ledger's file is gone"),
        }
    }
}

#[test]
fn a_plugins_process_answers_its_authorities_as_it_does_linked_and_shows_a_panic_as_its_refusal() {
    use symphony_plugin::authority::{Operation, Snapshot};
    use symphony_plugin::execution::{Principal, PrincipalKind};
    let tree = Tree::new();
    let remote = Remote::leaked(
        "approvers",
        process_in(&tree, "approvers"),
        peer(&APPROVERS, Arc::default()),
        patient(),
    );
    assert_eq!(
        remote.declaration().map(|d| d.authorities.clone()),
        Ok(vec!["approvals".into(), "closed".into(), "broken".into()])
    );
    let operation = Operation::new(
        "sim".into(),
        "sim/a".into(),
        "accept".into(),
        "approved".into(),
        Snapshot::new("d1".into(), 2, 3, "verify".into(), "awaiting".into()),
        std::collections::BTreeMap::new(),
        Principal::new(PrincipalKind::Human, "local".into(), "owner".into()),
    );
    let within = Duration::from_millis(2500);
    let (remote_ones, linked_ones) = (remote.authorities(), APPROVERS.authorities());
    for (far, near) in remote_ones.iter().zip(&linked_ones).take(2) {
        assert_eq!(far.adapter(), near.adapter());
        assert_eq!(
            far.attest(&tree.root, &operation, within),
            near.attest(&tree.root, &operation, within)
        );
    }
    assert_eq!(
        remote_ones[0].attest(&tree.root, &operation, within),
        Ok(symphony_plugin::authority::Signed::new(
            "approved within 2500 ms".into(),
            "ab".repeat(64)
        ))
    );
    assert_eq!(
        remote_ones[2].attest(&tree.root, &operation, within),
        Err(
            "plugin approvers: it refused authorities.attest: approvers panicked: the ledger's \
             file is gone"
                .into()
        )
    );
}

/// A project's Settings page describers: `lamp` describes the root it is asked of, `shelf` cannot
/// say, `broken` panics.
struct Lamp(&'static str);

static LAMPS: Lamps = Lamps;

struct Lamps;

impl Plugin for Lamps {
    fn name(&self) -> &str {
        "lamps"
    }
    fn describers(&self) -> Vec<&dyn symphony_plugin::Describer> {
        static EACH: [Lamp; 3] = [Lamp("lamp"), Lamp("shelf"), Lamp("broken")];
        EACH.iter()
            .map(|d| -> &dyn symphony_plugin::Describer { d })
            .collect()
    }
}

impl symphony_plugin::Describer for Lamp {
    fn id(&self) -> &str {
        self.0
    }
    fn describe(&self, root: &Path) -> Result<symphony_plugin::vocabulary::SettingsEntry, String> {
        match self.0 {
            "lamp" => Ok(symphony_plugin::vocabulary::SettingsEntry {
                id: "lamp".into(),
                name: "Lamp".into(),
                about: root.display().to_string(),
                state: None,
                needs: Vec::new(),
                settings: Vec::new(),
                source: None,
            }),
            "shelf" => Err("shelf.toml does not read".into()),
            _ => panic!("the lamp's file is gone"),
        }
    }
}

#[test]
fn a_plugins_process_answers_its_describers_as_it_does_linked_and_shows_a_panic_as_its_refusal() {
    let tree = Tree::new();
    let remote = Remote::leaked(
        "lamps",
        process_in(&tree, "lamps"),
        peer(&LAMPS, Arc::default()),
        patient(),
    );
    assert_eq!(
        remote.declaration().map(|d| d.describers.clone()),
        Ok(vec!["lamp".into(), "shelf".into(), "broken".into()])
    );
    let (remote_ones, linked_ones) = (remote.describers(), LAMPS.describers());
    for (far, near) in remote_ones.iter().zip(&linked_ones).take(2) {
        assert_eq!(far.id(), near.id());
        assert_eq!(far.describe(&tree.root), near.describe(&tree.root));
    }
    assert_eq!(
        remote_ones[0].describe(&tree.root).map(|entry| entry.about),
        Ok(tree.root.display().to_string())
    );
    assert_eq!(
        remote_ones[2].describe(&tree.root),
        Err(
            "plugin lamps: it refused describers.describe: lamps panicked: the lamp's file is gone"
                .into()
        )
    );
}

#[test]
fn a_describer_declared_under_a_minor_before_the_call_is_never_sent_it() {
    use symphony_plugin::protocol::Describe;
    let tree = Tree::new();
    let seen = Seen::default();
    let mut declared = Declaration::of(&LAMPS);
    declared.protocol = Version::new(PROTOCOL.major, Describe::SINCE - 1);
    let remote = Remote::leaked(
        "lamps",
        process_in(&tree, "lamps"),
        serving_as(&LAMPS, declared, seen.clone()),
        patient(),
    );
    let said = remote.describers()[0].describe(&tree.root).unwrap_err();
    assert!(
        said.contains("it speaks protocol 0.4, which has no describers.describe"),
        "{said}"
    );
    let heard = seen.heard();
    assert!(
        !heard.iter().any(|line| line.contains(Describe::TAG)),
        "{heard:?}"
    );
}

/// What `gates` unwinds [`Gates::kept_while`] with for `task` at `root`: why it has no answer.
fn untold(gates: &dyn Gates, root: &Path, task: &symphony_plugin::TaskRef<'_>) -> String {
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        gates.kept_while(root, task)
    }))
    .expect_err("kept_while had an answer");
    match unwound.downcast::<crate::gates::Untold>() {
        Ok(untold) => untold.0,
        Err(_) => panic!("kept_while unwound with something other than why it has no answer"),
    }
}

/// A project's gates answerers: `landing` meets its gate at the tip it is asked and says it is
/// kept while the task's status holds, `closed` cannot say, `broken` panics in both calls.
struct Landing(&'static str);

static LANDINGS: Landings = Landings;

struct Landings;

impl Plugin for Landings {
    fn name(&self) -> &str {
        "landings"
    }
    fn gates(&self) -> Vec<&dyn Gates> {
        static EACH: [Landing; 3] = [Landing("landing"), Landing("closed"), Landing("broken")];
        EACH.iter().map(|g| -> &dyn Gates { g }).collect()
    }
}

impl Gates for Landing {
    fn name(&self) -> &str {
        self.0
    }
    fn answer(
        &self,
        root: &Path,
        task: &symphony_plugin::TaskRef<'_>,
        tip: Option<&str>,
    ) -> Result<Vec<symphony_plugin::vocabulary::Gate>, String> {
        use symphony_plugin::vocabulary::{Gate, GateState};
        match self.0 {
            "landing" => Ok(vec![Gate {
                name: "certificate".into(),
                state: if tip.is_some() {
                    GateState::Met
                } else {
                    GateState::Missing
                },
                by: Some(task.slug.into()),
                at: Some(0.1 + 0.2),
                commit: tip.map(str::to_string),
                detail: root.display().to_string(),
                ..Gate::default()
            }]),
            "closed" => Err(format!("{} has no certificate yet", task.branch)),
            _ => panic!("the certificate's file is gone"),
        }
    }
    fn kept_while(&self, root: &Path, task: &symphony_plugin::TaskRef<'_>) -> String {
        match self.0 {
            "broken" => panic!("the stamp's file is gone"),
            _ => format!("{} {} in {}", self.0, task.status, root.display()),
        }
    }
}

/// The task each gates test asks of.
const TASK: symphony_plugin::TaskRef<'static> = symphony_plugin::TaskRef {
    slug: "tools/a",
    status: "review",
    branch: "tools/a-2",
};

#[test]
fn a_plugins_process_answers_its_gates_as_it_does_linked_and_shows_a_refusal_as_its_error() {
    let tree = Tree::new();
    let remote = Remote::leaked(
        "landings",
        process_in(&tree, "landings"),
        peer(&LANDINGS, Arc::default()),
        patient(),
    );
    // Declared, it is not refused, and its answerers stand for it in its order.
    assert_eq!(
        remote.declaration().map(|d| d.gates.clone()),
        Ok(vec!["landing".into(), "closed".into(), "broken".into()])
    );
    let (remote_ones, linked_ones) = (remote.gates(), LANDINGS.gates());
    // On a branch at its tip and with none, its gates or its own error, as it said them.
    for (far, near) in remote_ones.iter().zip(&linked_ones).take(2) {
        assert_eq!(far.name(), near.name());
        for tip in [Some("c0ffee"), None] {
            assert_eq!(
                far.answer(&tree.root, &TASK, tip),
                near.answer(&tree.root, &TASK, tip),
                "{} {tip:?}",
                far.name()
            );
        }
        assert_eq!(
            far.kept_while(&tree.root, &TASK),
            near.kept_while(&tree.root, &TASK)
        );
    }
    assert_eq!(
        remote_ones[1].answer(&tree.root, &TASK, Some("c0ffee")),
        Err("tools/a-2 has no certificate yet".into())
    );
    // A panic in the plugin is its refusal, said as the answerer's error in both calls.
    assert_eq!(
        remote_ones[2].answer(&tree.root, &TASK, Some("c0ffee")),
        Err(
            "plugin landings: it refused gates.answer: landings panicked: the certificate's file \
             is gone"
                .into()
        )
    );
    assert_eq!(
        untold(remote_ones[2], &tree.root, &TASK),
        "plugin landings: it refused gates.kept_while: landings panicked: the stamp's file is gone"
    );
}

#[test]
fn a_plugins_gates_are_kept_and_read_as_a_linked_ones_are_its_refusal_unknown() {
    use crate::gates::{Asked, Kept, theirs};
    use symphony_plugin::vocabulary::GateState;
    let tree = Tree::new();
    let remote = Remote::leaked(
        "landings",
        process_in(&tree, "landings"),
        peer(&LANDINGS, Arc::default()),
        patient(),
    );
    let answerers = |plugin: &'static dyn Plugin| -> Vec<(&'static str, &'static dyn Gates)> {
        plugin
            .gates()
            .into_iter()
            .map(|g| ("landings", g))
            .collect()
    };
    let (far, near) = (answerers(remote), answerers(&LANDINGS));
    let (kept_far, kept_near) = (Kept::default(), Kept::default());
    let asked = |tip: Option<&str>| Asked {
        slug: TASK.slug.into(),
        status: TASK.status.into(),
        branch: TASK.branch.into(),
        tip: tip.map(str::to_string),
    };
    // The two that answer read alike; the one that panics is unknown in its place either way.
    for tip in [Some("c0ffee"), Some("c0ffee"), Some("decade"), None] {
        let (from_far, from_near) = (
            theirs(&kept_far, &tree.root, &far, &asked(tip)),
            theirs(&kept_near, &tree.root, &near, &asked(tip)),
        );
        assert_eq!(from_far[..2], from_near[..2], "{tip:?}");
        assert_eq!(
            (from_far[2].name.as_str(), from_far[2].state),
            ("broken", GateState::Unknown),
            "{tip:?}"
        );
        assert_eq!(
            from_far[2].detail,
            "plugin landings: it refused gates.kept_while: landings panicked: the stamp's file is \
             gone"
        );
    }
    // The error `closed` said, kept as it said it.
    let read = theirs(&kept_far, &tree.root, &far, &asked(Some("c0ffee")));
    assert_eq!(
        (
            read[1].name.as_str(),
            read[1].state,
            read[1].detail.as_str()
        ),
        (
            "closed",
            GateState::Unknown,
            "tools/a-2 has no certificate yet"
        )
    );
}

#[test]
fn a_kept_pass_is_not_used_once_the_process_cannot_tell_what_it_is_kept_while() {
    use crate::gates::{Asked, Kept, theirs};
    use symphony_plugin::vocabulary::GateState;
    let tree = Tree::new();
    // It answers as its plugin does until its third call, then ends; a pause longer than the test,
    // so each call after finds it down.
    let calls = Arc::new(AtomicUsize::new(0));
    let respond: Respond = Arc::new(move |id, request| {
        if calls.fetch_add(1, Ordering::SeqCst) >= 2 {
            return Said::End;
        }
        let line = format!("{{\"id\":{id},\"request\":{request}}}\n");
        let mut answered = Vec::new();
        symphony_plugin::serve_on(&LANDINGS, line.as_bytes(), &mut answered).unwrap();
        Said::Bytes(answered)
    });
    let remote = Remote::leaked(
        "landings",
        process_in(&tree, "landings"),
        hand_made(
            Declaration::of(&LANDINGS),
            respond,
            Seen::default(),
            "was killed by signal 9",
        ),
        limits(PATIENT, Duration::from_secs(600)),
    );
    let landing: Vec<(&str, &'static dyn Gates)> = vec![("landings", remote.gates()[0])];
    let kept = Kept::default();
    let asked = Asked {
        slug: TASK.slug.into(),
        status: TASK.status.into(),
        branch: TASK.branch.into(),
        tip: Some("c0ffee".into()),
    };
    // What it is kept while, then its answer: met, and kept.
    let met = theirs(&kept, &tree.root, &landing, &asked);
    assert_eq!(
        (met[0].name.as_str(), met[0].state),
        ("certificate", GateState::Met)
    );
    // The next read finds what it is kept while has no answer: the gate it met is unknown, its why
    // the detail, and never the pass kept for the same task and tip.
    let read = theirs(&kept, &tree.root, &landing, &asked);
    assert_eq!(
        (
            read[0].name.as_str(),
            read[0].state,
            read[0].detail.as_str()
        ),
        (
            "certificate",
            GateState::Unknown,
            "plugin landings: its process was killed by signal 9"
        )
    );
    until("the remote is down", || remote.down().is_some());
    let read = theirs(&kept, &tree.root, &landing, &asked);
    assert_eq!(
        (read[0].state, read[0].detail.as_str()),
        (
            GateState::Unknown,
            "plugin landings: its process was killed by signal 9; it starts again in a moment"
        )
    );
}

#[test]
fn gates_of_an_older_minor_are_asked_and_a_minor_before_the_calls_never_sent_them() {
    use symphony_plugin::protocol::{AnswerGates, KeptWhile};
    // Both calls are 0.1's: a plugin of 0.1 is asked them and answers as linked.
    assert_eq!((AnswerGates::SINCE, KeptWhile::SINCE), (1, 1));
    let tree = Tree::new();
    let mut declared = Declaration::of(&LANDINGS);
    declared.protocol = Version::new(PROTOCOL.major, 1);
    let remote = Remote::leaked(
        "landings",
        process_in(&tree, "landings"),
        serving_as(&LANDINGS, declared, Seen::default()),
        patient(),
    );
    let (far, near) = (remote.gates()[0], LANDINGS.gates()[0]);
    assert_eq!(
        far.answer(&tree.root, &TASK, Some("c0ffee")),
        near.answer(&tree.root, &TASK, Some("c0ffee"))
    );
    assert!(far.answer(&tree.root, &TASK, Some("c0ffee")).is_ok());
    assert_eq!(
        far.kept_while(&tree.root, &TASK),
        near.kept_while(&tree.root, &TASK)
    );
    // Under a minor before them, its answer is refused naming the protocol it speaks and what it is
    // kept while is the trait's default, neither sent.
    let tree = Tree::new();
    let seen = Seen::default();
    let mut declared = Declaration::of(&LANDINGS);
    declared.protocol = Version::new(PROTOCOL.major, 0);
    let remote = Remote::leaked(
        "landings",
        process_in(&tree, "landings"),
        serving_as(&LANDINGS, declared, seen.clone()),
        patient(),
    );
    let gates = remote.gates()[0];
    assert_eq!(
        gates.answer(&tree.root, &TASK, Some("c0ffee")),
        Err("plugin landings: it speaks protocol 0.0, which has no gates.answer".into())
    );
    assert_eq!(gates.kept_while(&tree.root, &TASK), "");
    let heard = seen.heard();
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert!(heard[0].contains("\"plugin.declare\""), "{heard:?}");
}

#[test]
fn a_gates_answer_waits_its_turn_among_those_asked_at_once_and_says_so_past_its_bound() {
    let tree = Tree::new();
    let remote = Remote::leaked(
        "landings",
        process_in(&tree, "landings"),
        peer(&LANDINGS, Arc::default()),
        patient(),
    );
    let gates = remote.gates()[0];
    let linked = LANDINGS.gates()[0].answer(&tree.root, &TASK, Some("c0ffee"));
    // Asked while every turn is taken, an answer is asked and answered once one is given back.
    let mut turns: Vec<GatesTurn> = (0..GATES_AT_ONCE)
        .map(|_| remote.gates_turn().unwrap())
        .collect();
    let answered = std::thread::scope(|scope| {
        let asking = scope.spawn(|| gates.answer(&tree.root, &TASK, Some("c0ffee")));
        turns.pop();
        asking.join().unwrap()
    });
    assert_eq!(answered, linked);
    assert_eq!(*remote.gates_asked.lock().unwrap(), GATES_AT_ONCE - 1);
    // With every turn taken, an answer waits in the host for one, and past its bound is not asked
    // and says why.
    let tree = Tree::new();
    let mut limits = patient();
    limits.gates = Duration::from_millis(50);
    let remote = Remote::leaked(
        "landings",
        process_in(&tree, "landings"),
        peer(&LANDINGS, Arc::default()),
        limits,
    );
    let _turns: Vec<GatesTurn> = (0..GATES_AT_ONCE)
        .map(|_| remote.gates_turn().unwrap())
        .collect();
    assert_eq!(
        remote.gates()[0].answer(&tree.root, &TASK, Some("c0ffee")),
        Err(format!(
            "plugin landings: the {GATES_AT_ONCE} gates answers it is asked at once ran for 50ms, \
             so this one was not asked"
        ))
    );
}

/// A remote of [`LANDINGS`] served by a hand-made peer that answers each gates answer as `respond`
/// says and keeps what `seen` sees: a gates answer given up after 50 ms, and a call given up
/// waited for as long as only a stall outlasts, so only a late answer or the test resolves its
/// cancel.
fn holding_gates(tree: &Tree, respond: Respond, seen: Seen) -> &'static Remote {
    let declared = Declaration::of(&LANDINGS);
    Remote::leaked(
        "landings",
        process_in(tree, "landings"),
        hand_made(declared, respond, seen, "exited with code 0"),
        Limits {
            gates: Duration::from_millis(50),
            ..patient()
        },
    )
}

/// How many gates answers `remote` asks now, in their turns.
fn gates_asked(remote: &Remote) -> usize {
    *remote.gates_asked.lock().unwrap()
}

#[test]
fn a_gates_answer_given_up_holds_its_turn_until_its_late_answer_comes() {
    let tree = Tree::new();
    let seen = Seen::default();
    // Its gates answer runs on until the test lets it go, past its cancel, as the SDK's worker
    // runs one on.
    let (let_go, held) = crate::test_channel::channel::<()>();
    let held = Mutex::new(held);
    let respond: Respond = Arc::new(move |id, _| {
        let _ = held.lock().recover().recv();
        let late: Answered<Result<Vec<symphony_plugin::vocabulary::Gate>, String>> =
            Answered::Answer(Ok(Vec::new()));
        Said::Lines(vec![answering(id, &serde_json::to_string(&late).unwrap())])
    });
    let remote = holding_gates(&tree, respond, seen.clone());
    // The other turns taken, the answer asked in the last is given up at its bound and holds it.
    let _others: Vec<GatesTurn> = (1..GATES_AT_ONCE)
        .map(|_| remote.gates_turn().unwrap())
        .collect();
    assert_eq!(
        remote.gates()[0].answer(&tree.root, &TASK, Some("c0ffee")),
        Err("plugin landings: no answer in 50ms".into())
    );
    assert_eq!(gates_asked(remote), GATES_AT_ONCE);
    // So another answer waits in the host while the plugin still runs it, and past its bound is
    // not asked.
    assert!(remote.gates_turn().is_err());
    assert_eq!(gates_asked(remote), GATES_AT_ONCE);
    // Answered late, its cancel resolved: its turn is given back, and the process kept.
    let_go.send(()).unwrap();
    until("the late answer was read", || {
        tree.read("artifacts/plugins/landings.log")
            .contains("was answered after it was given up: dropped")
    });
    assert_eq!(gates_asked(remote), GATES_AT_ONCE - 1);
    drop(remote.gates_turn().unwrap());
    assert_eq!(remote.down(), None);
    assert_eq!(seen.kills(), 0);
    // The call it gave up was the one cancelled: the peer reads the cancel only once it has
    // written its late answer.
    until("the cancel was heard", || seen.heard().len() == 3);
    let heard = seen.heard();
    let asked: Raw = serde_json::from_str(&heard[1]).unwrap();
    assert!(
        (asked.request).is_some_and(|r| r.get().contains("gates.answer")),
        "{heard:?}"
    );
    assert_eq!(heard[2], format!("{{\"id\":{},\"cancel\":{{}}}}", asked.id));
}

#[test]
fn a_gates_answer_given_up_holds_its_turn_until_its_silent_process_is_ended() {
    let tree = Tree::new();
    let seen = Seen::default();
    // Its gates answers never come, and its cancel is not answered.
    let remote = holding_gates(
        &tree,
        Arc::new(|_, _| Said::Lines(Vec::new())),
        seen.clone(),
    );
    let gates = remote.gates()[0];
    let connection = remote.connection().unwrap();
    assert_eq!(
        gates.answer(&tree.root, &TASK, Some("c0ffee")),
        Err("plugin landings: no answer in 50ms".into())
    );
    assert_eq!(gates_asked(remote), 1);
    until("the cancel was sent", || seen.heard().len() == 3);
    assert_eq!(gates_asked(remote), 1);
    let asked: Raw = serde_json::from_str(&seen.heard()[1]).unwrap();
    // Still unanswered once the wait after its cancel has run out, its process is ended, which
    // gives the turn back.
    remote.after_cancel(&connection, asked.id);
    assert_eq!(gates_asked(remote), 0);
    assert_eq!(
        remote.down(),
        Some(format!(
            "its process was ended: call {} had no answer {PATIENT:?} after it was cancelled",
            asked.id
        ))
    );
    assert_eq!(seen.kills(), 1);
}
