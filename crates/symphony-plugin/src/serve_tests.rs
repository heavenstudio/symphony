use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::protocol::tests::{Declaring, card, every_request};
use crate::protocol::{
    Ask, CacheVersion, CardData, CardGates, Chosen, Declare, PROTOCOL, ReadCards, Recognises,
    Render, Slot, Validate,
};
use crate::vocabulary::TaskOffer;
use crate::vocabulary::{Block, FieldValue, Picture, Span};
use crate::{BoardAction, Card, CardKind, Component, Sender, Submission, Submitted, TaskRef};

/// What `plugin` answers to `lines`, by id: each answer line read raw, with the plugin's time.
fn served(plugin: &dyn Plugin, lines: &[String]) -> BTreeMap<u64, (String, Option<f64>)> {
    let input = lines.join("\n");
    let mut output = Vec::new();
    serve_on(plugin, input.as_bytes(), &mut output).unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| {
            let read: Raw = serde_json::from_str(line).unwrap();
            let id = read.id;
            let us = read.us;
            let Ok(Member::Answered(raw)) = read.member() else {
                panic!("{line} is no answer");
            };
            (id, (raw.get().to_string(), us))
        })
        .collect()
}

/// The host's line asking `request` as the call `id`.
fn asking(id: u64, request: impl Into<Request>) -> String {
    serde_json::to_string(&Envelope::<Request, ()>::asking(id, request.into())).unwrap()
}

/// `answer` as the wire carries it.
fn answer_text<A: Ask>(answer: &A::Answer) -> String {
    serde_json::to_string(&Answered::Answer(answer)).unwrap()
}

#[test]
fn every_call_is_answered_by_its_id_but_the_loopbacks_alone_which_is_refused() {
    let requests = every_request();
    let lines: Vec<String> = requests
        .iter()
        .enumerate()
        .map(|(n, (request, _))| asking(u64::try_from(n).unwrap() + 1, request.clone()))
        .collect();
    let answers = served(&Declaring, &lines);
    assert_eq!(answers.len(), requests.len(), "one answer for each request");
    for (n, (_, tag)) in requests.iter().enumerate() {
        let (answered, us) = &answers[&(u64::try_from(n).unwrap() + 1)];
        assert!(us.is_some_and(|us| us >= 0.0), "{tag}: {us:?}");
        if crate::protocol::LOOPBACK_ONLY.contains(tag) {
            assert!(
                answered.starts_with(r#"{"refused":"commands.run is not asked on a pipe"#),
                "{tag}: {answered}"
            );
        } else {
            assert!(answered.starts_with(r#"{"answer":"#), "{tag}: {answered}");
        }
    }
    // The answers are the plugin's own, as a linked call gives them.
    let declared = &answers[&1].0;
    assert_eq!(
        declared,
        &answer_text::<Declare>(&Declaration::of(&Declaring))
    );
    let card = card();
    let slot = served(
        &Declaring,
        &[asking(
            5,
            Slot {
                kind: "widget".into(),
                card: card.clone(),
            },
        )],
    );
    assert_eq!(
        slot[&5].0,
        answer_text::<Slot>(&card.read(|card| Declaring.slot(card)))
    );
    let validated = served(
        &Declaring,
        &[asking(
            6,
            Validate {
                field: "risk".into(),
                value: FieldValue::Text(" high ".into()),
            },
        )],
    );
    assert_eq!(validated[&6].0, r#"{"answer":{"Ok":"high"}}"#);
}

#[test]
fn a_request_that_cannot_be_answered_is_refused_by_its_id_naming_why() {
    let card = serde_json::to_string(&card()).unwrap();
    let lines = [
        // A call this build does not know.
        format!(
            r#"{{"id":1,"request":{{"card_kinds.colour":{{"kind":"widget","card":{card}}}}}}}"#
        ),
        // A field missing.
        r#"{"id":2,"request":{"card_kinds.slot":{"kind":"widget"}}}"#.to_string(),
        // An object the plugin does not have.
        format!(r#"{{"id":3,"request":{{"card_kinds.slot":{{"kind":"lantern","card":{card}}}}}}}"#),
        // Two members.
        r#"{"id":4,"request":{},"cancel":{}}"#.to_string(),
        // A line that is no envelope, which names no call to answer, and a blank one.
        "not json".to_string(),
        String::new(),
        r#"{"request":{}}"#.to_string(),
        // A cancel, and an answer, which a host does not send: neither is answered.
        r#"{"id":5,"cancel":{}}"#.to_string(),
        r#"{"id":6,"answered":{"answer":1}}"#.to_string(),
    ];
    let answers = served(&Declaring, &lines);
    let refusal = |id: u64| {
        let (answered, _) = &answers[&id];
        serde_json::from_str::<Answered<()>>(answered)
            .unwrap()
            .shown()
            .expect_err(answered)
    };
    assert!(
        refusal(1).contains("unknown variant `card_kinds.colour`"),
        "{}",
        refusal(1)
    );
    assert!(
        refusal(2).contains("missing field `card`"),
        "{}",
        refusal(2)
    );
    assert_eq!(refusal(3), "the plugin has no card kind \"lantern\"");
    assert_eq!(
        refusal(4),
        "the line of call 4 holds request and cancel: a line holds one"
    );
    assert_eq!(
        answers.keys().copied().collect::<Vec<_>>(),
        [1, 2, 3, 4],
        "only the four requests are answered"
    );
    // A refusal before any call is made has no time of the plugin's.
    assert_eq!(answers[&4].1, None);
}

/// A plugin whose card kind panics on a card that says so, whose component's render waits for
/// [`SLOW`] to be let go, and whose components count what they load for a page.
struct Moody;

static MOODY: Moody = Moody;

/// What a slow render waits on: a count the test raises to let it go.
static SLOW: AtomicUsize = AtomicUsize::new(0);
/// Each load of a page's shared value, numbered.
static LOADS: AtomicUsize = AtomicUsize::new(0);

impl Plugin for Moody {
    fn name(&self) -> &str {
        "moody"
    }
    fn components(&self) -> Vec<&dyn Component> {
        vec![self]
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![self]
    }
}

impl CardKind for Moody {
    fn name(&self) -> &str {
        "moody"
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        assert!(!card.text.contains("panic"), "{} made it panic", card.slug);
        true
    }
}

impl Component for Moody {
    fn name(&self) -> &str {
        "moody"
    }
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        _: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        if argument == "slow" {
            while SLOW.load(Ordering::SeqCst) == 0 {
                std::thread::yield_now();
            }
        }
        let load = page.once("moody", || LOADS.fetch_add(1, Ordering::SeqCst));
        Ok(vec![Block::Paragraph {
            spans: vec![Span::Text {
                text: format!("{argument} {load}"),
            }],
        }])
    }
}

/// A card saying `text`.
fn moody_card(text: &str) -> CardData {
    CardData {
        text: text.into(),
        ..card()
    }
}

#[test]
fn a_panic_in_a_call_is_answered_as_a_refusal_naming_it_and_the_process_goes_on() {
    let answers = served(
        &MOODY,
        &[
            asking(
                1,
                Recognises {
                    kind: "moody".into(),
                    card: moody_card("panic now"),
                },
            ),
            asking(
                2,
                Recognises {
                    kind: "moody".into(),
                    card: moody_card("calm"),
                },
            ),
        ],
    );
    let refused = serde_json::from_str::<Answered<bool>>(&answers[&1].0)
        .unwrap()
        .shown()
        .unwrap_err();
    assert_eq!(refused, "moody panicked: pick-a-widget made it panic");
    assert_eq!(answers[&2].0, r#"{"answer":true}"#);
}

#[test]
fn a_slow_call_holds_one_worker_and_the_others_answer_meanwhile() {
    let (plugin_input, mut host) = std::io::pipe().unwrap();
    let (host_output, plugin_output) = std::io::pipe().unwrap();
    let page = |render| PageData {
        root: "/r".into(),
        doc_path: "docs/A.md".into(),
        render,
        artifacts: "artifacts".into(),
    };
    let render = |argument: &str| Render {
        component: "moody".into(),
        page: page(0),
        argument: argument.into(),
        option: None,
    };
    std::thread::scope(|scope| {
        let served = scope.spawn(|| serve_on(&MOODY, BufReader::new(plugin_input), plugin_output));
        writeln!(host, "{}", asking(1, render("slow"))).unwrap();
        writeln!(host, "{}", asking(2, render("quick"))).unwrap();
        let mut answers = BufReader::new(host_output).lines();
        // The quick call is answered while the slow one still waits.
        let first: Raw = serde_json::from_str(&answers.next().unwrap().unwrap()).unwrap();
        assert_eq!(first.id, 2);
        SLOW.store(1, Ordering::SeqCst);
        let second: Raw = serde_json::from_str(&answers.next().unwrap().unwrap()).unwrap();
        assert_eq!(second.id, 1);
        // Its input ended, the process ends once every call read is answered.
        drop(host);
        served.join().unwrap().unwrap();
        assert!(answers.next().is_none());
    });
}

#[test]
fn the_requests_of_one_render_share_one_page_and_a_render_of_its_own_shares_none() {
    let request = |id: u64, render: u64, call: &str| -> String {
        let page = PageData {
            root: "/r".into(),
            doc_path: "docs/A.md".into(),
            render,
            artifacts: "artifacts".into(),
        };
        if call == "version" {
            return asking(
                id,
                CacheVersion {
                    component: "moody".into(),
                    page,
                },
            );
        }
        asking(
            id,
            Render {
                component: "moody".into(),
                page,
                argument: call.into(),
                option: None,
            },
        )
    };
    // Render 9's two requests, render 10's, render 0's two, then render 9 again.
    let answers = served(
        &MOODY,
        &[
            request(1, 9, "a"),
            request(2, 9, "b"),
            request(3, 10, "a"),
            request(4, 0, "a"),
            request(5, 0, "b"),
            request(6, 9, "c"),
        ],
    );
    let load = |id: u64| -> usize {
        let text = &answers[&id].0;
        let read: Answered<Result<Vec<Block>, String>> = serde_json::from_str(text).unwrap();
        let blocks = read.shown().unwrap().unwrap();
        let Block::Paragraph { spans } = &blocks[0] else {
            panic!("{text}");
        };
        let Span::Text { text } = &spans[0] else {
            panic!("{text}");
        };
        text.rsplit(' ').next().unwrap().parse().unwrap()
    };
    assert_eq!(load(1), load(2));
    assert_eq!(load(1), load(6));
    assert_ne!(load(1), load(3));
    assert_ne!(load(4), load(5));
    assert_ne!(load(4), load(1));
}

#[test]
fn a_page_of_its_own_or_of_a_render_holds_the_artifacts_folder_it_was_asked_with() {
    let pages = Pages::default();
    for render in [0, 7] {
        let data = PageData {
            root: "/r".into(),
            doc_path: "docs/A.md".into(),
            render,
            artifacts: "out/ä".into(),
        };
        assert_eq!(pages.with(&data, |page| page.artifact("x")), "out/ä/x");
    }
}

#[test]
fn a_process_keeps_the_newest_renders_pages_and_loads_an_older_ones_again() {
    let pages = Pages::default();
    let data = |render| PageData {
        root: "/r".into(),
        doc_path: "docs/A.md".into(),
        render,
        artifacts: "artifacts".into(),
    };
    let load = |render: u64| {
        pages.with(&data(render), |page| {
            *page.once("kept", || LOADS.fetch_add(1, Ordering::SeqCst))
        })
    };
    let first = load(1);
    assert_eq!(load(1), first);
    for render in 2..=u64::try_from(KEPT_RENDERS).unwrap() {
        load(render);
    }
    assert_eq!(load(1), first, "still among the newest");
    load(1_000);
    assert_ne!(load(1), first, "the oldest, dropped for the newest");
}

#[test]
fn a_command_runs_as_the_binary_with_its_own_streams_and_its_code() {
    let run = |args: &[&str], input: &str| {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(&Declaring, &args, input.as_bytes(), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    assert_eq!(
        run(&["command", "draw-widgets", "--all"], ""),
        (0, String::new(), String::new())
    );
    let (code, _, said) = run(&["command", "nothing"], "");
    assert_eq!(
        (code, said.as_str()),
        (2, "declaring: no command nothing\n")
    );
    let (code, _, said) = run(&["command"], "");
    assert_eq!((code, said.as_str()), (2, "usage: command NAME [ARGS…]\n"));
    let (code, _, said) = run(&["--help"], "");
    assert_eq!(code, 2);
    assert!(said.contains("\"--help\" is no argument"), "{said}");
    // With no argument, the protocol on its input and output.
    let declare = asking(1, crate::protocol::Declare { protocol: PROTOCOL });
    let (code, out, _) = run(&[], &declare);
    assert_eq!(code, 0);
    assert!(
        out.starts_with(r#"{"id":1,"answered":{"answer":{"protocol""#),
        "{out}"
    );
    // An input that fails ends the process, said on stderr.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let failing = BufReader::new(Failing);
    let code = main(&Declaring, &[], failing, &mut out, &mut err);
    assert_eq!(code, 1);
    let said = String::from_utf8(err).unwrap();
    assert_eq!(
        said,
        "declaring: the protocol's input failed: the pipe broke\n"
    );
}

/// An input whose every read fails.
struct Failing;

impl std::io::Read for Failing {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("the pipe broke"))
    }
}

#[test]
fn a_line_that_is_not_utf8_is_skipped_and_the_next_answered() {
    let declare = asking(1, crate::protocol::Declare { protocol: PROTOCOL });
    let mut input = b"\xff\xfe{\"id\":9}\n".to_vec();
    input.extend_from_slice(declare.as_bytes());
    let mut output = Vec::new();
    serve_on(&Declaring, &input[..], &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(output.lines().count(), 1, "{output}");
    assert!(
        output.starts_with(r#"{"id":1,"answered":{"answer":"#),
        "{output}"
    );
}

#[test]
fn a_line_past_the_bound_ends_the_process_and_an_answer_past_it_is_refused() {
    let declare = asking(1, crate::protocol::Declare { protocol: PROTOCOL });
    let validate = asking(
        2,
        Validate {
            field: "risk".into(),
            value: FieldValue::Text("high".into()),
        },
    );
    // The lines before the long one are answered; it ends the reading, naming the bound.
    let input = format!("{validate}\n{declare}\n{}\n{validate}\n", "x".repeat(201));
    let mut output = Vec::new();
    let failed = serving(&Declaring, input.as_bytes(), &mut output, 200).unwrap_err();
    assert_eq!(failed.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(
        failed.to_string(),
        "a line longer than the protocol's 200 bytes"
    );
    let answers: BTreeMap<u64, String> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| {
            assert!(line.len() <= 200, "{line}");
            let read: Raw = serde_json::from_str(line).unwrap();
            let id = read.id;
            let Ok(Member::Answered(raw)) = read.member() else {
                panic!("{line}");
            };
            (id, raw.get().to_string())
        })
        .collect();
    assert_eq!(answers.len(), 2, "{answers:?}");
    assert_eq!(answers[&2], r#"{"answer":{"Ok":"high"}}"#);
    // The declaration of every seam is longer than 200 bytes: refused, by its id, naming its length.
    let refused = serde_json::from_str::<Answered<()>>(&answers[&1])
        .unwrap()
        .shown()
        .unwrap_err();
    let whole = serde_json::to_vec(&Envelope::<(), Answered<Declaration>>::answering(
        1,
        Answered::Answer(Declaration::of(&Declaring)),
        None,
    ))
    .unwrap()
    .len();
    assert!(
        refused.starts_with("its answer is a line of ")
            && refused.ends_with(" bytes, longer than the protocol's 200"),
        "{refused}"
    );
    // Its length counts its time beside it, a few bytes more than the line without it.
    let said: usize = refused
        .trim_start_matches("its answer is a line of ")
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(said > whole && said < whole + 32, "{said} {whole}");
}

#[test]
fn an_input_that_ends_runs_its_ending_after_its_delay_and_only_then() {
    let ran = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = ran.clone();
    let (ending, ended) = mpsc::channel();
    let then = Box::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        let _ = ending.send(());
    });
    let mut input = Ending::new(&b"a\nb\n"[..], std::time::Duration::from_millis(20), then);
    let mut line = String::new();
    input.read_line(&mut line).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test holds past the input's ending deadline"
    )]
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "not while the input holds lines"
    );
    let mut rest = String::new();
    input.read_to_string(&mut rest).unwrap();
    assert_eq!(line + &rest, "a\nb\n");
    // Ended: once, after the delay, however often the end is read.
    let mut more = [0; 4];
    assert_eq!(std::io::Read::read(&mut input, &mut more).unwrap(), 0);
    ending_ran(&ended);
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test holds for a length of time to show nothing more runs"
    )]
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}

#[test]
fn an_input_that_fails_runs_its_ending_too() {
    let (ending, ended) = mpsc::channel();
    let then = Box::new(move || {
        let _ = ending.send(());
    });
    let mut input = Ending::new(BufReader::new(Failing), std::time::Duration::ZERO, then);
    let mut line = String::new();
    assert!(input.read_line(&mut line).is_err());
    ending_ran(&ended);
}

/// An input's ending ran, within the backstop of the wait it was given: only a stall, said with
/// the machine's load, comes before it.
fn ending_ran(ended: &mpsc::Receiver<()>) {
    ended
        .recv_timeout(test_time::backstop(std::time::Duration::from_secs(5)))
        .unwrap_or_else(|_| {
            panic!(
                "the ending did not run: a stall; {}",
                test_time::Load::now()
            )
        });
}

#[test]
fn a_commands_code_is_the_low_byte_an_exit_status_holds() {
    assert_eq!(exit_code(0), 0);
    assert_eq!(exit_code(3), 3);
    assert_eq!(exit_code(256), 0);
    assert_eq!(exit_code(-1), 255);
}

#[test]
fn a_card_kinds_gates_and_choice_answer_as_linked() {
    let card = card();
    let answers = served(
        &Declaring,
        &[
            asking(
                1,
                CardGates {
                    kind: "widget".into(),
                    card: card.clone(),
                },
            ),
            asking(
                2,
                Chosen {
                    kind: "widget".into(),
                    card: card.clone(),
                },
            ),
        ],
    );
    assert_eq!(
        answers[&1].0,
        answer_text::<CardGates>(&card.read(|c| CardKind::gates(&Declaring, c)))
    );
    assert_eq!(
        answers[&2].0,
        answer_text::<Chosen>(&card.read(|c| Declaring.chosen(c)))
    );
}

/// A card kind whose every call answers from what the card holds, so an answer made of another
/// card, or of another root, reads otherwise.
struct Tally;

static TALLY: Tally = Tally;

impl CardKind for Tally {
    fn name(&self) -> &str {
        "tally"
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        card.text.contains("Pick")
    }
    fn pictures(&self, root: &std::path::Path, card: &Card<'_>) -> Vec<Picture> {
        card.thread
            .iter()
            .map(|line| Picture {
                src: Some(format!("{}/{}", root.display(), card.slug)),
                caption: line.text.into(),
                waiting: line.verdict.is_none(),
            })
            .collect()
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        Some(card.doc.to_string()).filter(|doc| !doc.is_empty())
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        Some(card.topic.into())
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        card.thread.is_empty()
    }
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        Some((card.thread.len(), card.slug.into()))
    }
    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        card.thread.iter().any(|line| line.agent)
    }
    fn chosen(&self, card: &Card<'_>) -> Result<Option<String>, String> {
        if card.text.is_empty() {
            return Err(format!("{} says nothing", card.slug));
        }
        Ok(card.text.chars().next().map(String::from))
    }
}

/// A plugin of two card kinds, [`TALLY`] then [`MOODY`].
struct Kinds;

impl Plugin for Kinds {
    fn name(&self) -> &str {
        "kinds"
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![&TALLY, &MOODY]
    }
}

/// The read of `cards` at `root` asked as the call `id`.
fn reading(id: u64, root: &std::path::Path, cards: Vec<CardData>) -> String {
    asking(
        id,
        ReadCards {
            root: root.to_path_buf(),
            cards,
        },
    )
}

#[test]
fn a_reads_cards_are_each_answered_by_every_kind_in_the_plugins_order_as_linked() {
    let root = std::path::PathBuf::from("/r/ünï");
    let cards = vec![
        card(),
        CardData {
            doc: String::new(),
            thread: Vec::new(),
            ..card()
        },
        moody_card(""),
    ];
    let answers = served(
        &Kinds,
        &[
            reading(1, &root, cards.clone()),
            reading(2, &root, Vec::new()),
        ],
    );
    let read = serde_json::from_str::<Answered<Vec<Vec<KindAnswers>>>>(&answers[&1].0)
        .unwrap()
        .shown()
        .unwrap();
    let linked: Vec<Vec<KindAnswers>> = cards
        .iter()
        .map(|data| {
            data.read(|card| {
                Kinds
                    .card_kinds()
                    .iter()
                    .map(|kind| KindAnswers::of(*kind, &root, card))
                    .collect()
            })
        })
        .collect();
    assert_eq!(read, linked);
    // Each card, each kind in the plugin's order, each call its own answer.
    assert_eq!(read.len(), 3);
    let kinds: Vec<&str> = read[0].iter().map(|kind| kind.kind.as_str()).collect();
    assert_eq!(kinds, ["tally", "moody"]);
    assert_eq!(read[0][0].slot.as_deref(), Some("docs/WIDGETS.md"));
    assert_eq!(read[1][0].slot, None);
    assert_eq!(
        read[0][0].pictures[0].src.as_deref(),
        Some("/r/ünï/pick-a-widget")
    );
    assert_eq!(read[2][0].chosen, Err("pick-a-widget says nothing".into()));
    // A read of no card is answered with none.
    assert_eq!(answers[&2].0, r#"{"answer":[]}"#);
    // A kind that panics on one card refuses the read whole, naming it, and the next is answered.
    let panicked = served(
        &Kinds,
        &[
            reading(3, &root, vec![card(), moody_card("panic now")]),
            reading(4, &root, vec![card()]),
        ],
    );
    assert_eq!(
        serde_json::from_str::<Answered<Vec<Vec<KindAnswers>>>>(&panicked[&3].0).unwrap(),
        Answered::Refused("kinds panicked: pick-a-widget made it panic".into())
    );
    assert!(
        panicked[&4].0.starts_with(r#"{"answer":[[{"#),
        "{}",
        panicked[&4].0
    );
}

/// A board action that answers who sent its submission, where it is told, in the line it says.
struct Signing;

impl BoardAction for Signing {
    fn name(&self) -> &str {
        "sign"
    }
    fn offer(&self, _: &std::path::Path, _: &TaskRef<'_>) -> Result<TaskOffer, String> {
        Ok(TaskOffer::default())
    }
    fn submit(
        &self,
        _: &std::path::Path,
        _: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        Ok(Submitted {
            said: format!("signed {} by nobody", submission.payload),
            released: Vec::new(),
        })
    }
    fn submit_from(
        &self,
        _: &std::path::Path,
        _: &TaskRef<'_>,
        submission: &Submission,
        sender: &Sender,
    ) -> Result<Submitted, String> {
        Ok(Submitted {
            said: format!(
                "signed {} by {} ({}, {})",
                submission.payload,
                sender.name,
                sender.display,
                sender.git.join(" ")
            ),
            released: Vec::new(),
        })
    }
}

/// A board action written before submissions said who sent them: `submit` alone.
struct Unsigned;

impl BoardAction for Unsigned {
    fn name(&self) -> &str {
        "unsigned"
    }
    fn offer(&self, _: &std::path::Path, _: &TaskRef<'_>) -> Result<TaskOffer, String> {
        Ok(TaskOffer::default())
    }
    fn submit(
        &self,
        _: &std::path::Path,
        _: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        Ok(Submitted {
            said: format!("took {}", submission.payload),
            released: Vec::new(),
        })
    }
}

struct Acting;

impl Plugin for Acting {
    fn name(&self) -> &str {
        "acting"
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        vec![&Signing, &Unsigned]
    }
}

fn submitting(id: u64, action: &str, sender: Option<Sender>) -> String {
    let mut asked = crate::protocol::Submit::new(
        action.into(),
        "/r".into(),
        crate::protocol::TaskData::of(&TaskRef {
            slug: "a/b",
            status: "review",
            branch: "a/b",
        }),
        Submission {
            payload: "tip".into(),
            note: String::new(),
        },
    );
    asked.sender = sender;
    asking(id, asked)
}

#[test]
fn a_submission_hands_its_sender_to_submit_from_and_one_with_none_to_submit() {
    let ana = Sender::new("ana", "Ana Lima", vec!["ana-l".into(), "al".into()]);
    let answers = served(
        &Acting,
        &[
            submitting(1, "sign", Some(ana.clone())),
            submitting(2, "sign", None),
            // An action that reads no sender answers by `submit`, told or not.
            submitting(3, "unsigned", Some(ana)),
        ],
    );
    let said = |said: &str| {
        answer_text::<crate::protocol::Submit>(&Ok(Submitted {
            said: said.into(),
            released: Vec::new(),
        }))
    };
    assert_eq!(
        answers[&1].0,
        said("signed tip by ana (Ana Lima, ana-l al)")
    );
    assert_eq!(answers[&2].0, said("signed tip by nobody"));
    assert_eq!(answers[&3].0, said("took tip"));
}

#[test]
fn a_submission_from_a_host_of_an_older_minor_reads_with_no_sender() {
    let line = r#"{"id":1,"request":{"board_actions.submit":{"action":"sign","root":"/r","task":{"slug":"a/b","status":"review","branch":"a/b"},"submission":{"payload":"tip","note":""}}}}"#;
    let answers = served(&Acting, &[line.to_string()]);
    assert_eq!(
        answers[&1].0,
        answer_text::<crate::protocol::Submit>(&Ok(Submitted {
            said: "signed tip by nobody".into(),
            released: Vec::new(),
        }))
    );
}
