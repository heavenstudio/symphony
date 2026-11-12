use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::protocol::tests::{Declaring, card, every_request};
use crate::protocol::{
    Ask, CacheVersion, CardData, CardGates, Chosen, Declare, PROTOCOL, Recognises, Render, Slot,
    Validate,
};
use crate::vocabulary::{Block, FieldValue, Span};
use crate::{Card, CardKind, Component};

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
                kind: "hull".into(),
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
        format!(r#"{{"id":1,"request":{{"card_kinds.colour":{{"kind":"hull","card":{card}}}}}}}"#),
        // A field missing.
        r#"{"id":2,"request":{"card_kinds.slot":{"kind":"hull"}}}"#.to_string(),
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
    assert_eq!(refused, "moody panicked: pick-a-hull made it panic");
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
fn a_process_keeps_the_newest_renders_pages_and_loads_an_older_ones_again() {
    let pages = Pages::default();
    let data = |render| PageData {
        root: "/r".into(),
        doc_path: "docs/A.md".into(),
        render,
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
        run(&["command", "ship-art", "--all"], ""),
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
                    kind: "hull".into(),
                    card: card.clone(),
                },
            ),
            asking(
                2,
                Chosen {
                    kind: "hull".into(),
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
