use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::vocabulary::{
    GalleryItem, GatePast, GateState, Image, Media, Presentation, ProgressBar, Span, View, Viewer,
};
use crate::{
    BoardAction, BoardField, CardKind, Command, Component, DocCheck, Docs, Gates, Generator,
    HomeRegion, ProseRule, Renderer, Route, Transport, Watch,
};

/// `request` written as the wire carries it and read back, with the call it went as: the one
/// member of its object, holding its fields.
fn wire(request: impl Into<Request>) -> (Request, Request, String) {
    let request = request.into();
    let text = serde_json::to_string(&request).unwrap();
    let written: serde_json::Value = serde_json::from_str(&text).unwrap();
    let members = written.as_object().unwrap();
    assert_eq!(members.len(), 1, "{text}");
    let (call, fields) = members.iter().next().unwrap();
    assert!(fields.is_object(), "{text}");
    (request, serde_json::from_str(&text).unwrap(), call.clone())
}

/// How [`ordered`] writes an object's members.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Order {
    Sorted,
    Reversed,
}

/// `text`'s JSON written again with every object's members in `order`, at every depth: as a writer
/// that sorts its keys writes it, or the other way round. Numbers keep their text as written.
pub(crate) fn ordered(text: &str, order: Order) -> String {
    fn write(value: &serde_json::Value, order: Order, out: &mut String) {
        match value {
            serde_json::Value::Object(members) => {
                let mut keys: Vec<&String> = members.keys().collect();
                keys.sort();
                if let Order::Reversed = order {
                    keys.reverse();
                }
                out.push('{');
                for (n, key) in keys.into_iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).unwrap());
                    out.push(':');
                    write(&members[key], order, out);
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                for (n, item) in items.iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    write(item, order, out);
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other).unwrap()),
        }
    }
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    let mut out = String::new();
    write(&value, order, &mut out);
    out
}

/// That `value` reads back the same written with its members in every order.
pub(crate) fn reads_back_in_any_order<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    value: &T,
) {
    let text = serde_json::to_string(value).unwrap();
    for order in [Order::Sorted, Order::Reversed] {
        let moved = ordered(&text, order);
        let read = serde_json::from_str::<T>(&moved)
            .unwrap_or_else(|e| panic!("{order:?} {moved} does not read back: {e}"));
        assert_eq!(&read, value, "{order:?} {moved}");
    }
}

pub(crate) fn card() -> CardData {
    CardData {
        slug: "pick-a-widget".into(),
        doc: "docs/WIDGETS.md".into(),
        topic: "Widgets \"quoted\"".into(),
        text: "Pick one.\n*Slot:* North · cover — ✅ \\ back".into(),
        thread: vec![
            ThreadLineData {
                agent: true,
                verdict: None,
                text: "A or B?".into(),
            },
            ThreadLineData {
                agent: false,
                verdict: Some("approved".into()),
                text: "B\u{0}, with a tab\t".into(),
            },
        ],
    }
}

fn task() -> TaskData {
    TaskData {
        slug: "sim/jump".into(),
        status: "review".into(),
        branch: "sim/jump".into(),
    }
}

/// An operation an authority attests: the owner accepting sim/a, binding one input revision.
fn operation() -> Operation {
    Operation::new(
        "sim".into(),
        "sim/a".into(),
        "accept".into(),
        "approved".into(),
        crate::authority::Snapshot::new("d1".into(), 2, 3, "verify".into(), "awaiting".into()),
        BTreeMap::from([("design".to_string(), "r1".to_string())]),
        crate::execution::Principal::new(
            crate::execution::PrincipalKind::Human,
            "local".into(),
            "owner".into(),
        ),
    )
}

fn page() -> PageData {
    PageData {
        root: "/r/ünï code".into(),
        doc_path: "docs/A.md".into(),
        render: 7,
        artifacts: "out/ä".into(),
    }
}

/// A request of every call, with its call: two of a board field's, its value a fraction and a text.
pub(crate) fn every_request() -> Vec<(Request, &'static str)> {
    let root = PathBuf::from("/r");
    vec![
        (Declare { protocol: PROTOCOL }.into(), "plugin.declare"),
        (
            Render {
                component: "report".into(),
                page: page(),
                argument: "lantern".into(),
                option: Some("wide".into()),
            }
            .into(),
            "components.render",
        ),
        (
            Resolve {
                component: "report".into(),
                page: page(),
                argument: String::new(),
                option: None,
            }
            .into(),
            "components.resolve",
        ),
        (
            CacheVersion {
                component: "report".into(),
                page: page(),
            }
            .into(),
            "components.cache_version",
        ),
        (
            Recognises {
                kind: "widget".into(),
                card: card(),
            }
            .into(),
            "card_kinds.recognises",
        ),
        (
            Pictures {
                kind: "widget".into(),
                root: root.clone(),
                card: card(),
            }
            .into(),
            "card_kinds.pictures",
        ),
        (
            Slot {
                kind: "widget".into(),
                card: card(),
            }
            .into(),
            "card_kinds.slot",
        ),
        (
            TriageCategory {
                kind: "widget".into(),
                card: card(),
            }
            .into(),
            "card_kinds.triage_category",
        ),
        (
            AutoApprovable {
                kind: "widget".into(),
                card: card(),
            }
            .into(),
            "card_kinds.auto_approvable",
        ),
        (
            CardGates {
                kind: "widget".into(),
                card: card(),
            }
            .into(),
            "card_kinds.gates",
        ),
        (
            ApprovalNeedsAgent {
                kind: "widget".into(),
                card: card(),
            }
            .into(),
            "card_kinds.approval_needs_agent",
        ),
        (
            Chosen {
                kind: "widget".into(),
                card: card(),
            }
            .into(),
            "card_kinds.chosen",
        ),
        (
            ReadCards {
                root: root.clone(),
                cards: vec![
                    card(),
                    CardData {
                        thread: Vec::new(),
                        ..card()
                    },
                ],
            }
            .into(),
            "card_kinds.read",
        ),
        (
            Draw {
                renderer: "stl".into(),
                page: page(),
                file: FileData {
                    src: "/x/widgets/a.stl?v=1#top".into(),
                    label: String::new(),
                },
            }
            .into(),
            "renderers.draw",
        ),
        (
            Serve {
                route: "widgets".into(),
                root: root.clone(),
                rest: "a.stl".into(),
            }
            .into(),
            "routes.serve",
        ),
        (
            ShowRegion {
                region: "pictures".into(),
                root: root.clone(),
            }
            .into(),
            "home_regions.region",
        ),
        (
            Offer {
                action: "land".into(),
                root: root.clone(),
                task: task(),
            }
            .into(),
            "board_actions.offer",
        ),
        (
            Submit {
                action: "land".into(),
                root: root.clone(),
                task: task(),
                submission: Submission {
                    payload: "{\"patch\":\"diff --git\\n\"}".into(),
                    note: "send it".into(),
                },
            }
            .into(),
            "board_actions.submit",
        ),
        (
            Validate {
                field: "risk".into(),
                value: FieldValue::Number(0.1 + 0.2),
            }
            .into(),
            "board_fields.validate",
        ),
        (
            Validate {
                field: "risk".into(),
                value: FieldValue::Text("3".into()),
            }
            .into(),
            "board_fields.validate",
        ),
        (
            AnswerGates {
                answerer: "stamps".into(),
                root: root.clone(),
                task: task(),
                tip: Some("c0ffee".into()),
            }
            .into(),
            "gates.answer",
        ),
        (
            KeptWhile {
                answerer: "stamps".into(),
                root: root.clone(),
                task: task(),
            }
            .into(),
            "gates.kept_while",
        ),
        (
            Attest {
                authority: "approvals".into(),
                root: root.clone(),
                operation: operation(),
                within_ms: 10_000,
            }
            .into(),
            "authorities.attest",
        ),
        (
            Changed {
                watch: 1,
                root: root.clone(),
            }
            .into(),
            "watches.changed",
        ),
        (
            Forget {
                watch: 0,
                root: root.clone(),
            }
            .into(),
            "watches.forget",
        ),
        (
            CheckDocs {
                check: "data".into(),
                root: root.clone(),
            }
            .into(),
            "doc_checks.check",
        ),
        (
            DocComponents {
                check: "data".into(),
                root,
            }
            .into(),
            "doc_checks.components",
        ),
        (
            CheckProse {
                rule: "words".into(),
                rel: "docs/A.md".into(),
                text: "# A\n\nline\r\n".into(),
            }
            .into(),
            "prose_rules.check",
        ),
        (
            RunCommand {
                command: "draw-widgets".into(),
                args: vec!["--all".into(), String::new(), "a b".into()],
            }
            .into(),
            "commands.run",
        ),
        (
            RegisterAgent {
                transport: "uds".into(),
                handle: "amber-heron".into(),
                reported: "/tmp/s.sock".into(),
            }
            .into(),
            "transports.register",
        ),
        (
            Deliver {
                transport: "uds".into(),
                address: "/tmp/s.sock".into(),
                message: "PULL\n".into(),
            }
            .into(),
            "transports.deliver",
        ),
        (
            Alive {
                transport: "uds".into(),
                address: "/tmp/s.sock".into(),
            }
            .into(),
            "transports.alive",
        ),
    ]
}

/// The items of `kind` (`struct` or `enum`) `source` defines, a `pub KIND` line each (the requests'
/// struct and `Request` in `calls!` among them), each with whether `#[non_exhaustive]` stands among
/// the attributes above it.
fn defined(source: &str, kind: &str) -> Vec<(String, bool)> {
    let opening = format!("pub {kind} ");
    let lines: Vec<&str> = source.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(at, line)| {
            let name = line.trim().strip_prefix(opening.as_str())?;
            let name = name.split([' ', '<', '{']).next().unwrap_or(name);
            let marked = lines[..at]
                .iter()
                .rev()
                .map(|above| above.trim())
                .take_while(|above| above.starts_with("#[") || above.starts_with("///"))
                .any(|above| above == "#[non_exhaustive]");
            Some((name.to_string(), marked))
        })
        .collect()
}

#[test]
fn every_struct_of_the_protocol_is_non_exhaustive_so_a_field_a_minor_adds_breaks_no_build() {
    // The data an authority's call carries is the protocol's too.
    let mut found = defined(include_str!("protocol.rs"), "struct");
    found.extend(defined(include_str!("authority.rs"), "struct"));
    let unmarked: Vec<&str> = found
        .iter()
        .filter(|(_, marked)| !marked)
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        unmarked.is_empty(),
        "mark each `#[non_exhaustive]` and give it a constructor: {unmarked:?}"
    );
    // The requests' one in `calls!`, and every struct of data besides.
    let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    for name in [
        "$name",
        "Version",
        "Envelope",
        "Cancel",
        "PageData",
        "CardData",
        "Declaration",
        "Operation",
        "Snapshot",
        "Signed",
    ] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
    // The reading sees a struct left unmarked.
    let left = "/// A thing.\n#[derive(Clone)]\npub struct Left {\n    pub a: u8,\n}\n";
    assert_eq!(defined(left, "struct"), [("Left".to_string(), false)]);
    let marked = "#[derive(Clone)]\n#[non_exhaustive]\npub struct Marked<T = u8> {}\n";
    assert_eq!(defined(marked, "struct"), [("Marked".to_string(), true)]);
    assert!(defined(marked, "enum").is_empty());
    // A request made by its `new` is the request of those fields.
    assert_eq!(
        Changed::new(3, PathBuf::from("/r")),
        Changed {
            watch: 3,
            root: "/r".into()
        }
    );
    assert_eq!(Version::new(0, 4), PROTOCOL);
}

#[test]
fn the_request_enum_is_non_exhaustive_so_a_call_a_minor_adds_breaks_no_build() {
    // The enums a match outside the crate covers whole, each for its reason (protocol.rs's doc): a
    // variant of `Answered` or `Member` is a line an older peer cannot read, a new major of the
    // protocol, and `Lined` is how a line reads, never on the wire.
    const EXHAUSTIVE: [&str; 3] = ["Answered", "Member", "Lined"];
    let found = defined(include_str!("protocol.rs"), "enum");
    assert!(found.contains(&("Request".to_string(), true)), "{found:?}");
    let unmarked: Vec<&str> = found
        .iter()
        .filter(|(_, marked)| !marked)
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        unmarked, EXHAUSTIVE,
        "an enum a later minor adds a variant to is `#[non_exhaustive]`; one left exhaustive is named here with its reason"
    );
    // The reading sees an enum's mark, and its name before its generics.
    let read = "/// A thing.\n#[derive(Debug)]\n#[non_exhaustive]\npub enum Marked<T> {\n    A(T),\n}\n\
                pub enum Left {\n    B,\n}\n";
    assert_eq!(
        defined(read, "enum"),
        [("Marked".to_string(), true), ("Left".to_string(), false)]
    );
}

#[test]
fn every_call_goes_on_the_wire_as_its_seam_and_call_and_reads_back_the_same() {
    let requests = every_request();
    for (request, tag) in &requests {
        let (sent, read, call) = wire(request.clone());
        assert_eq!(&call, tag);
        assert_eq!(&read, &sent, "{tag}");
    }
    let named: std::collections::BTreeSet<&str> = requests.iter().map(|(_, tag)| *tag).collect();
    let every: std::collections::BTreeSet<&str> = CALLS.iter().copied().collect();
    assert_eq!(named, every, "a request of every call");
    assert_eq!(CALLS.len(), every.len(), "each call once");
}

#[test]
fn every_calls_request_reads_back_whatever_order_its_members_come_in() {
    for (request, _) in every_request() {
        reads_back_in_any_order(&request);
    }
    // The fraction in a board field's value, which a buffer would have lost, among them.
    let reversed = ordered(
        &serde_json::to_string(&Request::from(Validate {
            field: "effort".into(),
            value: FieldValue::Number(2.5),
        }))
        .unwrap(),
        Order::Reversed,
    );
    assert_eq!(
        reversed,
        r#"{"board_fields.validate":{"value":2.5,"field":"effort"}}"#
    );
    assert_eq!(
        serde_json::from_str::<Request>(&reversed).unwrap(),
        Request::from(Validate {
            field: "effort".into(),
            value: FieldValue::Number(2.5),
        })
    );
}

#[test]
fn a_request_reads_back_only_as_its_own_call() {
    let (_, read, _) = wire(Slot {
        kind: "widget".into(),
        card: card(),
    });
    assert_eq!(Recognises::from_request(read.clone()), None);
    assert_eq!(
        Slot::from_request(read),
        Some(Slot {
            kind: "widget".into(),
            card: card(),
        })
    );
    assert_eq!(Slot::names(), ("card_kinds", "slot"));
    assert_eq!(
        ApprovalNeedsAgent::names(),
        ("card_kinds", "approval_needs_agent")
    );
    assert_eq!(Declare::names(), ("plugin", "declare"));
}

/// What `text` is refused with, read as a request: the plugin's `{"refused": …}`, as the host reads
/// and shows it.
fn refused(text: &str) -> String {
    let why = serde_json::from_str::<Request>(text)
        .expect_err(text)
        .to_string();
    let answered = serde_json::to_string(&Answered::<bool>::Refused(why)).unwrap();
    serde_json::from_str::<Answered<bool>>(&answered)
        .unwrap()
        .shown()
        .expect_err("a refusal is shown as an error")
}

#[test]
fn an_unknown_call_or_a_missing_field_is_refused_naming_it_with_an_answer_the_host_shows() {
    let card = serde_json::to_string(&card()).unwrap();
    let unknown = refused(&format!(
        r#"{{"card_kinds.colour":{{"kind":"widget","card":{card}}}}}"#
    ));
    assert!(
        unknown.contains("unknown variant `card_kinds.colour`"),
        "{unknown}"
    );
    let missing = refused(r#"{"card_kinds.slot":{"kind":"widget"}}"#);
    assert!(missing.contains("missing field `card`"), "{missing}");
    let nested = refused(&format!(
        r#"{{"card_kinds.slot":{{"kind":"widget","card":{}}}}}"#,
        card.replace(r#""slug":"pick-a-widget","#, "")
    ));
    assert!(nested.contains("missing field `slug`"), "{nested}");
    // The request is one member, its call: its fields beside it, or a second member, are refused.
    for text in [
        r#"{"kind":"widget"}"#,
        r#"{"card_kinds.slot":{"kind":"widget"},"card_kinds.gates":{"kind":"widget"}}"#,
        r#"{}"#,
        r#""card_kinds.slot""#,
    ] {
        assert!(!refused(text).is_empty(), "{text}");
    }
    let answer: Answered<bool> = serde_json::from_str(r#"{"answer":true}"#).unwrap();
    assert_eq!(answer.shown(), Ok(true));
}

#[test]
fn a_newer_peers_extra_fields_are_ignored_wherever_they_stand() {
    let mut card = serde_json::to_value(card()).unwrap();
    card["colour"] = "red".into();
    card["thread"][0]["mood"] = serde_json::json!({"at": 2.5});
    let text = serde_json::json!({
        "card_kinds.slot": {"kind": "widget", "card": card, "since": 1.5}
    })
    .to_string();
    assert_eq!(
        serde_json::from_str::<Request>(&text).unwrap(),
        Request::from(Slot {
            kind: "widget".into(),
            card: self::card(),
        })
    );
    let answer =
        r#"{"answer":{"Ok":[{"name":"certificate","state":"met","weight":0.5,"at":1.5}]}}"#;
    let read: Answered<Result<Vec<Gate>, String>> = serde_json::from_str(answer).unwrap();
    assert_eq!(
        read.shown(),
        Ok(Ok(vec![Gate {
            name: "certificate".into(),
            state: GateState::Met,
            at: Some(1.5),
            ..Gate::default()
        }]))
    );
    let declared = serde_json::to_value(Declaration::of(&Declaring)).unwrap();
    let mut newer = declared.clone();
    newer["protocol"]["minor"] = 7.into();
    newer["dialects"] = serde_json::json!(["toml"]);
    newer["card_kinds"][0]["colour"] = "red".into();
    let read: Declaration = serde_json::from_value(newer).unwrap();
    assert_eq!(read.card_kinds, Declaration::of(&Declaring).card_kinds);
    assert_eq!(read.protocol.minor, 7);
}

#[test]
fn an_answer_holding_a_block_type_the_host_does_not_know_is_refused_whole_naming_it() {
    let answer = r#"{"answer":{"Ok":[{"type":"paragraph","spans":[]},{"type":"sparkline","points":[1,2]}]}}"#;
    let why = serde_json::from_str::<Answered<Result<Vec<Block>, String>>>(answer)
        .expect_err("a block type the host does not know refuses the answer")
        .to_string();
    assert!(why.contains("unknown variant `sparkline`"), "{why}");
    let span = r#"{"answer":{"Ok":[{"type":"paragraph","spans":[{"type":"glyph"}]}]}}"#;
    let why = serde_json::from_str::<Answered<Result<Vec<Block>, String>>>(span)
        .expect_err("so does a span type")
        .to_string();
    assert!(why.contains("unknown variant `glyph`"), "{why}");
}

#[test]
fn a_host_and_a_plugin_speak_the_lower_minor_of_one_major_and_refuse_another_by_name() {
    let at = |major, minor| Version { major, minor };
    assert_eq!(at(1, 3).agreed(at(1, 5)), Ok(at(1, 3)));
    assert_eq!(at(1, 5).agreed(at(1, 3)), Ok(at(1, 3)));
    assert_eq!(PROTOCOL.agreed(PROTOCOL), Ok(PROTOCOL));
    assert_eq!(
        at(1, 3).agreed(at(2, 0)),
        Err(
            "the plugin speaks protocol 2.0 and this Symphony 1.3: install a Symphony that \
             speaks protocol 2"
                .into()
        )
    );
    assert_eq!(
        at(2, 1).agreed(at(1, 9)),
        Err(
            "the plugin speaks protocol 1.9 and this Symphony 2.1: rebuild the plugin with a \
             symphony-plugin of protocol 2"
                .into()
        )
    );
    // The host names its version in the first request, the plugin its own in its declaration.
    let (_, read, call) = wire(Declare { protocol: at(1, 3) });
    assert_eq!(call, "plugin.declare");
    assert_eq!(
        Declare::from_request(read),
        Some(Declare { protocol: at(1, 3) })
    );
    assert_eq!(Declaration::of(&Declaring).protocol, PROTOCOL);
    assert_eq!(Version::default(), PROTOCOL);
    assert_eq!(PROTOCOL.to_string(), "0.4");
}

#[test]
fn a_path_that_is_not_utf8_cannot_be_written() {
    use std::os::unix::ffi::OsStrExt as _;
    let root = PathBuf::from(std::ffi::OsStr::from_bytes(b"/r/\xff"));
    let request: Request = Serve {
        route: "widgets".into(),
        root,
        rest: String::new(),
    }
    .into();
    assert!(serde_json::to_string(&request).is_err());
}

#[test]
fn a_number_that_is_not_finite_does_not_read_back() {
    let written = serde_json::to_string(&Ok::<_, String>(FieldValue::Number(f64::NAN))).unwrap();
    assert_eq!(written, r#"{"Ok":null}"#);
    assert!(serde_json::from_str::<Result<FieldValue, String>>(&written).is_err());
    let written = serde_json::to_string(&FieldValue::Number(f64::INFINITY)).unwrap();
    assert!(serde_json::from_str::<FieldValue>(&written).is_err());
}

#[test]
fn a_board_fields_value_goes_bare_and_reads_a_fraction_back_in_any_order() {
    for value in [
        FieldValue::Number(3.5),
        FieldValue::Number(-0.25),
        FieldValue::Number(1e300),
        FieldValue::Number(3.0),
        FieldValue::Text("3.5".into()),
        FieldValue::Text(String::new()),
    ] {
        reads_back_in_any_order(&Request::from(Validate {
            field: "effort".into(),
            value: value.clone(),
        }));
        reads_back_in_any_order(&Answered::Answer(Ok::<_, String>(value)));
    }
    assert_eq!(
        serde_json::to_string(&Answered::Answer(Ok::<_, String>(FieldValue::Text(
            "x".into()
        ))))
        .unwrap(),
        r#"{"answer":{"Ok":"x"}}"#
    );
}

/// These tests link `serde_json` with the features the server's build unifies on it (the
/// dev-dependency in this package's Cargo.toml), so they read the wire as the server does: keys in
/// the order written, a number kept as its text, a float read to its nearest, and raw values.
#[test]
fn these_tests_read_json_as_the_server_links_serde_json() {
    let value: serde_json::Value = serde_json::from_str(r#"{"b":1.50,"a":2}"#).unwrap();
    assert_eq!(value.to_string(), r#"{"b":1.50,"a":2}"#);
    let float: f64 = serde_json::from_str("51.248178375505404").unwrap();
    assert_eq!(float.to_bits(), 51.248_178_375_505_404_f64.to_bits());
    let raw = serde_json::value::RawValue::from_string("[1.50]".into()).unwrap();
    assert_eq!(raw.get(), "[1.50]");
}

#[test]
fn a_fraction_read_through_serdes_buffer_is_lost_which_is_why_no_protocol_enum_buffers() {
    // The forms the protocol does not use, under the server's JSON: each reads a fraction back
    // through serde's buffered content, and fails; an integer survives.
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "call", content = "args")]
    enum Adjacent {
        #[serde(rename = "board_fields.validate")]
        Validate { at: f64 },
    }
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "type")]
    enum Internal {
        #[serde(rename = "gauge")]
        Gauge { at: f64 },
    }
    let lost = r#"{"args":{"at":2.5},"call":"board_fields.validate"}"#;
    let why = serde_json::from_str::<Adjacent>(lost)
        .unwrap_err()
        .to_string();
    assert!(why.contains("invalid type: map, expected f64"), "{why}");
    let kept = r#"{"call":"board_fields.validate","args":{"at":2.5}}"#;
    assert_eq!(
        serde_json::from_str::<Adjacent>(kept).unwrap(),
        Adjacent::Validate { at: 2.5 }
    );
    for text in [
        r#"{"type":"gauge","at":2.5}"#,
        r#"{"at":2.5,"type":"gauge"}"#,
    ] {
        assert!(serde_json::from_str::<Internal>(text).is_err(), "{text}");
    }
    assert_eq!(
        serde_json::from_str::<Internal>(r#"{"at":2,"type":"gauge"}"#).unwrap(),
        Internal::Gauge { at: 2.0 }
    );
}

#[test]
fn the_internally_tagged_blocks_spans_and_media_hold_no_fraction_and_read_back_in_any_order() {
    // `Eq` takes no float field: a block, a span or a media that held one would fail here first.
    fn holds_no_float<T: Eq>() {}
    holds_no_float::<Block>();
    holds_no_float::<Span>();
    holds_no_float::<Media>();
    let image = Image {
        src: "/x/a.png".into(),
        alt: "a".into(),
        presentation: Presentation::class("wide"),
    };
    let blocks = vec![
        Block::Heading {
            level: 3,
            anchor: "widgets".into(),
            spans: vec![
                Span::Text {
                    text: "Widgets".into(),
                },
                Span::Image {
                    image: image.clone(),
                },
            ],
        },
        Block::List {
            ordered: true,
            start: u64::MAX,
            items: vec![vec![Block::Rule]],
        },
        Block::Media {
            media: Media::Viewer {
                src: "/x/a.stl".into(),
                label: "widget".into(),
                poster: None,
                viewer: Viewer {
                    element: "stl-viewer".into(),
                    views: vec![View {
                        label: "front".into(),
                        set: vec![("orbit".into(), "0deg 75deg 2.5m".into())],
                        ..View::default()
                    }],
                    ..Viewer::default()
                },
            },
        },
        Block::Paragraph {
            spans: vec![Span::Media {
                media: Media::Gallery {
                    items: vec![GalleryItem {
                        name: "a".into(),
                        src: "/x/a.stl".into(),
                        poster: Some("/x/a.png".into()),
                    }],
                    viewer: Viewer::default(),
                },
            }],
        },
        Block::Image { image },
    ];
    reads_back_in_any_order(&Answered::<Result<Vec<Block>, String>>::Answer(Ok(blocks)));
}

#[test]
fn an_owned_card_task_file_and_page_lend_what_they_were_made_of() {
    let data = card();
    let back = data.read(CardData::of);
    assert_eq!(back, data);
    data.read(|card| {
        assert_eq!(card.thread[1].verdict, Some("approved"));
        assert!(!card.thread[1].agent);
    });
    let task = task();
    assert_eq!(TaskData::of(&task.task()), task);
    let file = FileData {
        src: "/a.stl".into(),
        label: "widget".into(),
    };
    assert_eq!(FileData::of(&file.file()), file);
    let root = Path::new("/r/ünï code");
    let named = Page::new(root, "docs/A.md").in_artifacts("out/ä");
    // A page names its render: the same for each request it makes, another for another page.
    let render = PageData::of(&named).render;
    assert_ne!(render, 0);
    assert_eq!(PageData::of(&named), PageData { render, ..page() });
    assert_ne!(PageData::of(&Page::new(root, "docs/A.md")).render, render);
    assert!(page().names(&named));
    assert!(!page().names(&Page::new(root, "docs/B.md").in_artifacts("out/ä")));
    assert!(!page().names(&Page::new(Path::new("/r"), "docs/A.md").in_artifacts("out/ä")));
    // Nor is a page of another project's artifacts folder the same page.
    assert!(!page().names(&Page::new(root, "docs/A.md")));
    let problem = Problem::new("docs/A.md", 3, "no anchor".into());
    assert_eq!(ProblemData::of(&problem).problem(), problem);
}

#[test]
fn a_page_holds_its_projects_artifacts_folder_the_default_unless_one_is_named() {
    let root = Path::new("/r");
    let page = Page::new(root, "docs/A.md");
    assert_eq!(page.artifacts, ARTIFACTS);
    assert_eq!(page.artifact("screens"), "artifacts/screens");
    let named = Page::new(root, "docs/A.md").in_artifacts("build/out");
    assert_eq!(named.artifact("screens"), "build/out/screens");
    // The same render, its folder named or not.
    let render = PageData::of(&page).render;
    let again = page.in_artifacts("x");
    assert_eq!(PageData::of(&again).render, render);
    assert_eq!(PageData::of(&again).artifacts, "x");
}

#[test]
fn a_page_names_its_artifacts_folder_since_0_2_and_a_peer_of_0_1_reads_without_it() {
    // A host of 0.1 writes no folder: the page reads as the projects' default.
    let written = serde_json::json!({"root": "/r", "doc_path": "docs/A.md", "render": 3});
    let read: PageData = serde_json::from_value(written).unwrap();
    assert_eq!(read.artifacts, ARTIFACTS);
    assert_eq!(read.render, 3);
    // A plugin of 0.1 reads a page of 0.2 as the page it knows, the folder unread.
    #[derive(Debug, PartialEq, Deserialize)]
    struct PageOf01 {
        root: PathBuf,
        doc_path: String,
        #[serde(default)]
        render: u64,
    }
    let read: PageOf01 = serde_json::from_str(&serde_json::to_string(&page()).unwrap()).unwrap();
    assert_eq!(
        read,
        PageOf01 {
            root: page().root,
            doc_path: page().doc_path,
            render: 7,
        }
    );
    // And a call's page goes on the wire with its folder, which reads back the same.
    let (asked, read, _) = wire(Render {
        component: "every".into(),
        page: page(),
        argument: "a".into(),
        option: None,
    });
    assert_eq!(read, asked);
    let Request::Render(read) = read else {
        panic!("a render reads back as a render");
    };
    assert_eq!(read.page.artifacts, "out/ä");
}

/// A plugin that declares something at every seam.
pub(crate) struct Declaring;

impl Plugin for Declaring {
    fn name(&self) -> &str {
        "declaring"
    }
    fn components(&self) -> Vec<&dyn Component> {
        vec![self]
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![self]
    }
    fn renderers(&self) -> Vec<&dyn Renderer> {
        vec![self]
    }
    fn routes(&self) -> Vec<&dyn Route> {
        vec![self]
    }
    fn nav(&self) -> Vec<NavItem> {
        vec![NavItem {
            label: "⬡ widgets".into(),
            title: "Widgets".into(),
            href: "/x/widgets".into(),
            document: "docs/WIDGETS.md".into(),
        }]
    }
    fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        vec![self]
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        vec![self]
    }
    fn board_fields(&self) -> Vec<&dyn BoardField> {
        vec![self]
    }
    fn gates(&self) -> Vec<&dyn Gates> {
        vec![self]
    }
    fn authorities(&self) -> Vec<&dyn crate::Authority> {
        vec![self]
    }
    fn watches(&self) -> Vec<&dyn Watch> {
        vec![self, self]
    }
    fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        vec![self]
    }
    fn prose_rules(&self) -> Vec<&dyn ProseRule> {
        vec![self]
    }
    fn generators(&self) -> Vec<&dyn Generator> {
        vec![self]
    }
    fn commands(&self) -> Vec<&dyn Command> {
        vec![self]
    }
    fn transports(&self) -> Vec<&dyn Transport> {
        vec![self]
    }
    fn theme(&self) -> Option<Theme> {
        Some(Theme {
            package: "lamp".into(),
            name: "Lamp Works".into(),
            icon: Some("⬡".into()),
            tokens: BTreeMap::from([("accent".into(), "#ff8800".into())]),
            stylesheets: vec!["docs/theme.css".into()],
            layouts: BTreeMap::from([("home".into(), "wide".into())]),
        })
    }
}

impl Component for Declaring {
    fn name(&self) -> &str {
        "report"
    }
    fn render(&self, _: &Page<'_>, _: &str, _: Option<&str>) -> Result<Vec<Block>, String> {
        Ok(vec![Block::Paragraph {
            spans: vec![Span::Text { text: "x".into() }],
        }])
    }
}

impl CardKind for Declaring {
    fn name(&self) -> &str {
        "widget"
    }
    fn recognises(&self, _: &Card<'_>) -> bool {
        true
    }
    fn markers(&self) -> &[&str] {
        &["Slot", "Widget"]
    }
}

impl Renderer for Declaring {
    fn name(&self) -> &str {
        "stl"
    }
    fn extensions(&self) -> &[&str] {
        &["stl", "obj"]
    }
    fn content_type(&self) -> &str {
        "model/stl"
    }
    fn draw(&self, _: &Page<'_>, _: &File<'_>) -> Result<Drawing, String> {
        Ok(Drawing::Viewer(Viewer::default()))
    }
}

impl Route for Declaring {
    fn path(&self) -> &str {
        "widgets"
    }
    fn serve(&self, _: &Path, _: &str) -> Option<Reply> {
        None
    }
}

impl HomeRegion for Declaring {
    fn id(&self) -> &str {
        "pictures"
    }
    fn region(&self, _: &Path) -> Option<Region> {
        None
    }
}

impl BoardAction for Declaring {
    fn name(&self) -> &str {
        "land"
    }
    fn offer(&self, _: &Path, _: &TaskRef<'_>) -> Result<TaskOffer, String> {
        Ok(TaskOffer::default())
    }
    fn submit(&self, _: &Path, _: &TaskRef<'_>, _: &Submission) -> Result<Submitted, String> {
        Ok(Submitted::default())
    }
}

impl BoardField for Declaring {
    fn name(&self) -> &str {
        "risk"
    }
    fn kind(&self) -> FieldKind {
        FieldKind::Choice(vec!["low".into(), "high".into()])
    }
    fn required(&self) -> bool {
        true
    }
}

impl Gates for Declaring {
    fn name(&self) -> &str {
        "stamps"
    }
    fn answer(&self, _: &Path, _: &TaskRef<'_>, _: Option<&str>) -> Result<Vec<Gate>, String> {
        Ok(vec![Gate {
            name: "certificate".into(),
            state: GateState::default(),
            ..Gate::default()
        }])
    }
}

impl crate::Authority for Declaring {
    fn adapter(&self) -> &str {
        "approvals"
    }
    fn attest(
        &self,
        _: &Path,
        operation: &Operation,
        _: std::time::Duration,
    ) -> Result<Signed, String> {
        Ok(Signed::new(operation.guard.clone(), "ab".repeat(64)))
    }
}

impl Watch for Declaring {
    fn paths(&self) -> Vec<String> {
        vec!["models".into(), "concepts".into()]
    }
    fn changed(&self, _: &Path) -> Option<String> {
        None
    }
}

impl DocCheck for Declaring {
    fn name(&self) -> &str {
        "data"
    }
    fn inputs(&self) -> Vec<String> {
        vec!["data/widgets.toml".into()]
    }
    fn check(&self, _: &Docs) -> Vec<Problem> {
        Vec::new()
    }
}

impl ProseRule for Declaring {
    fn name(&self) -> &str {
        "words"
    }
    fn check(&self, _: &str, _: &str) -> Vec<(usize, String)> {
        Vec::new()
    }
}

impl Generator for Declaring {
    fn name(&self) -> &str {
        "screens"
    }
    fn outputs(&self) -> &str {
        "docs/screens/"
    }
    fn command(&self) -> Vec<String> {
        vec!["just".into(), "screens".into()]
    }
    fn when_changed(&self) -> bool {
        true
    }
}

impl Command for Declaring {
    fn name(&self) -> &str {
        "draw-widgets"
    }
    fn summary(&self) -> &str {
        "draw the widgets' pictures"
    }
    fn run(&self, _: &[String], _: &mut dyn std::io::Write, _: &mut dyn std::io::Write) -> i32 {
        0
    }
}

impl Transport for Declaring {
    fn name(&self) -> &str {
        "uds"
    }
    fn register(&self, _: &str, reported: &str) -> Result<String, String> {
        Ok(reported.into())
    }
    fn deliver(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn alive(&self, _: &str) -> bool {
        true
    }
}

#[test]
fn a_declaration_names_every_seams_objects_in_the_plugins_order_and_reads_back_the_same() {
    let declared = Declaration::of(&Declaring);
    assert_eq!(declared.name, "declaring");
    assert_eq!(declared.components, ["report"]);
    assert_eq!(
        declared.card_kinds,
        [DeclaredKind {
            name: "widget".into(),
            markers: vec!["Slot".into(), "Widget".into()],
        }]
    );
    assert_eq!(
        declared.renderers,
        [DeclaredRenderer {
            name: "stl".into(),
            extensions: vec!["stl".into(), "obj".into()],
            content_type: "model/stl".into(),
        }]
    );
    assert_eq!(declared.routes, ["widgets"]);
    assert_eq!(declared.nav, Declaring.nav());
    assert_eq!(declared.home_regions, ["pictures"]);
    assert_eq!(declared.board_actions, ["land"]);
    assert_eq!(
        declared.board_fields,
        [DeclaredField {
            name: "risk".into(),
            kind: FieldKind::Choice(vec!["low".into(), "high".into()]),
            required: true,
        }]
    );
    assert_eq!(declared.gates, ["stamps"]);
    assert_eq!(
        declared.watches,
        [["models", "concepts"], ["models", "concepts"]]
    );
    assert_eq!(
        declared.doc_checks,
        [DeclaredCheck {
            name: "data".into(),
            inputs: vec!["data/widgets.toml".into()],
        }]
    );
    assert_eq!(declared.prose_rules, ["words"]);
    assert_eq!(
        declared.generators,
        [DeclaredGenerator {
            name: "screens".into(),
            outputs: "docs/screens/".into(),
            command: vec!["just".into(), "screens".into()],
            when_changed: true,
        }]
    );
    assert_eq!(
        declared.commands,
        [DeclaredCommand {
            name: "draw-widgets".into(),
            summary: "draw the widgets' pictures".into(),
        }]
    );
    assert_eq!(declared.transports, ["uds"]);
    assert_eq!(declared.theme, Declaring.theme());
    assert_eq!(declared.authorities, ["approvals"]);
    let text = serde_json::to_string(&declared).unwrap();
    assert_eq!(
        serde_json::from_str::<Declaration>(&text).unwrap(),
        declared
    );
}

#[test]
fn a_plugin_that_declares_nothing_declares_its_name_alone() {
    struct Bare;
    impl Plugin for Bare {
        fn name(&self) -> &str {
            "bare"
        }
    }
    assert_eq!(
        Declaration::of(&Bare),
        Declaration {
            name: "bare".into(),
            ..Declaration::default()
        }
    );
}

/// The calls [`answers`] was handed an answer of.
type Answers = std::collections::BTreeSet<&'static str>;

/// That `answer`, the call `A`'s, reads back as the wire carries it in any order of its members:
/// alone, and in the plugin's line answering a call, read raw by its id and then as `A`'s answer.
fn answers<A: Ask>(answered: &mut Answers, answer: A::Answer) {
    let answer = Answered::Answer(answer);
    reads_back_in_any_order(&answer);
    let raw = RawValue::from_string(serde_json::to_string(&answer).unwrap()).unwrap();
    let line =
        serde_json::to_string(&Envelope::<(), &RawValue>::answering(41, &raw, Some(1.5))).unwrap();
    for order in [Order::Sorted, Order::Reversed] {
        let moved = ordered(&line, order);
        let read: Raw = serde_json::from_str(&moved).unwrap();
        assert_eq!((read.id, read.us), (41, Some(1.5)), "{moved}");
        let Ok(Member::Answered(raw)) = read.member() else {
            panic!("{moved} holds no answer");
        };
        let back: Answered<A::Answer> =
            serde_json::from_str(raw.get()).unwrap_or_else(|e| panic!("{} {moved}: {e}", A::TAG));
        assert_eq!(back, answer, "{order:?} {moved}");
    }
    answered.insert(A::TAG);
}

#[test]
fn every_calls_answer_reads_back_whatever_order_its_members_come_in() {
    let mut answered = Answers::new();
    let a = &mut answered;
    let paragraph = || Block::Paragraph {
        spans: vec![Span::Text { text: "x".into() }],
    };
    answers::<Declare>(a, Declaration::of(&Declaring));
    answers::<Render>(a, Ok(vec![paragraph()]));
    answers::<Render>(a, Err("no such widget".into()));
    answers::<Resolve>(a, Ok(()));
    answers::<CacheVersion>(a, Some("v2".into()));
    answers::<Recognises>(a, true);
    answers::<Pictures>(
        a,
        vec![Picture {
            src: Some("/x/a.png".into()),
            caption: "the widget".into(),
            waiting: true,
        }],
    );
    answers::<Slot>(a, Some("North · cover".into()));
    answers::<TriageCategory>(a, None);
    answers::<AutoApprovable>(a, false);
    answers::<CardGates>(a, Some((3, "item".into())));
    answers::<ApprovalNeedsAgent>(a, true);
    answers::<Chosen>(a, Ok(Some("B".into())));
    answers::<ReadCards>(
        a,
        vec![
            vec![KindAnswers {
                kind: "widget".into(),
                recognises: true,
                pictures: vec![Picture {
                    src: None,
                    caption: "B".into(),
                    waiting: true,
                }],
                slot: Some("North · cover".into()),
                triage_category: None,
                auto_approvable: false,
                gates: Some((2, "item".into())),
                approval_needs_agent: true,
                chosen: Err("no candidate".into()),
            }],
            Vec::new(),
        ],
    );
    answers::<Draw>(a, Ok(Drawing::Blocks(vec![paragraph()])));
    answers::<Draw>(a, Ok(Drawing::Viewer(Viewer::default())));
    answers::<Serve>(a, Some(Reply::Json("{\"a\":1.5}".into())));
    answers::<Serve>(a, Some(Reply::File("/r/a.stl".into())));
    answers::<ShowRegion>(
        a,
        Some(Region {
            id: "pictures".into(),
            title: "Pictures".into(),
            note: String::new(),
            bars: vec![ProgressBar {
                key: "widgets".into(),
                name: "Widgets".into(),
                page: "docs/WIDGETS.md".into(),
                title: "3 of 7".into(),
                done: 3,
                waiting: 1,
                total: 7,
            }],
            legend: ["done".into(), "waiting".into(), "left".into()],
        }),
    );
    answers::<Offer>(
        a,
        Ok(TaskOffer {
            action: "land".into(),
            label: "Land".into(),
            branch: "sim/jump".into(),
            payload: Some("sim/jump@c0ffee".into()),
        }),
    );
    answers::<Submit>(
        a,
        Ok(Submitted {
            said: "landed".into(),
            released: vec!["sim/jump".into()],
        }),
    );
    answers::<Validate>(a, Ok(FieldValue::Number(0.1 + 0.2)));
    answers::<Validate>(a, Ok(FieldValue::Text("3".into())));
    answers::<AnswerGates>(
        a,
        Ok(vec![Gate {
            name: "certificate".into(),
            state: GateState::Met,
            by: Some("amber-heron".into()),
            at: Some(1_762_941_600.125),
            past: vec![GatePast {
                state: GateState::Missing,
                at: 1_762_941_000.5,
                ..GatePast::default()
            }],
            ..Gate::default()
        }]),
    );
    answers::<KeptWhile>(a, "c0ffee".into());
    answers::<Attest>(a, Ok(Signed::new("{}".into(), "ab".repeat(64))));
    answers::<Attest>(a, Err("the ledger is closed".into()));
    answers::<Changed>(a, Some("models".into()));
    answers::<Forget>(a, ());
    answers::<CheckDocs>(a, vec![ProblemData::of(&Problem::new("a", 0, "b".into()))]);
    answers::<DocComponents>(a, vec!["report".into()]);
    answers::<CheckProse>(a, vec![(2, "every".into())]);
    answers::<RunCommand>(a, -1);
    answers::<RegisterAgent>(a, Ok("/tmp/s.sock".into()));
    answers::<Deliver>(a, Err("gone".into()));
    answers::<Alive>(a, false);
    let every: Answers = CALLS.iter().copied().collect();
    assert_eq!(answered, every, "an answer of every call");
    // A refusal, too.
    reads_back_in_any_order(&Answered::<bool>::Refused("unknown variant".into()));
}

/// `line` read as the wire reads it: its one known member.
fn member(line: &str) -> Result<Member<Box<RawValue>, Box<RawValue>>, String> {
    serde_json::from_str::<Raw>(line)
        .map_err(|e| e.to_string())?
        .member()
}

/// The request a host's line carries, read as the plugin reads it: the envelope, then its raw
/// request as a [`Request`].
fn asked(line: &str) -> (u64, Request) {
    let envelope: Raw = serde_json::from_str(line).unwrap();
    let id = envelope.id;
    let Ok(Member::Request(raw)) = envelope.member() else {
        panic!("{line} holds no request");
    };
    (id, serde_json::from_str(raw.get()).unwrap())
}

#[test]
fn every_calls_request_goes_in_an_envelope_and_reads_back_by_its_id_in_any_order() {
    for (n, (request, tag)) in every_request().into_iter().enumerate() {
        let id = u64::try_from(n).unwrap() + 1;
        let line = serde_json::to_string(&Envelope::<&Request, ()>::asking(id, &request)).unwrap();
        assert!(
            line.starts_with(&format!(r#"{{"id":{id},"request":{{"{tag}":"#)),
            "{line}"
        );
        assert_eq!(asked(&line), (id, request.clone()), "{tag}");
        for order in [Order::Sorted, Order::Reversed] {
            let moved = ordered(&line, order);
            assert_eq!(asked(&moved), (id, request.clone()), "{order:?} {moved}");
        }
    }
    // A fraction in the request keeps its text in the raw member, whatever the order.
    let line = serde_json::to_string(&Envelope::<Request, ()>::asking(
        3,
        Validate {
            field: "effort".into(),
            value: FieldValue::Number(0.1 + 0.2),
        }
        .into(),
    ))
    .unwrap();
    let reversed = ordered(&line, Order::Reversed);
    assert_eq!(
        reversed,
        r#"{"request":{"board_fields.validate":{"value":0.30000000000000004,"field":"effort"}},"id":3}"#
    );
    assert_eq!(
        asked(&reversed).1,
        Request::from(Validate {
            field: "effort".into(),
            value: FieldValue::Number(0.1 + 0.2),
        })
    );
}

#[test]
fn a_line_holds_exactly_one_known_member_and_ignores_every_other() {
    let request = r#"{"card_kinds.slot":{"kind":"widget","card":{"slug":"a","doc":"d","topic":"t","text":"x","thread":[]}}}"#;
    // Members a later minor adds around a call are ignored, wherever they stand.
    for line in [
        format!(r#"{{"id":7,"request":{request}}}"#),
        format!(r#"{{"deadline_ms":500,"request":{request},"trace":{{"at":2.5}},"id":7}}"#),
    ] {
        let Ok(Member::Request(raw)) = member(&line) else {
            panic!("{line}");
        };
        assert_eq!(raw.get(), request);
    }
    assert!(matches!(
        member(r#"{"id":7,"cancel":{}}"#),
        Ok(Member::Cancel)
    ));
    assert!(matches!(
        member(r#"{"cancel":{"why":"late"},"id":7}"#),
        Ok(Member::Cancel)
    ));
    let Ok(Member::Answered(raw)) = member(r#"{"us":0.4,"answered":{"answer":2.5},"id":7}"#) else {
        panic!("an answer");
    };
    assert_eq!(raw.get(), r#"{"answer":2.5}"#);
    assert_eq!(
        member(r#"{"id":7}"#).err(),
        Some("the line of call 7 holds none of request, cancel and answered".into())
    );
    assert_eq!(
        member(&format!(
            r#"{{"id":7,"request":{request},"answered":{{"answer":1}}}}"#
        ))
        .err(),
        Some("the line of call 7 holds request and answered: a line holds one".into())
    );
    assert_eq!(
        member(r#"{"id":7,"cancel":{},"answered":{"answer":1},"request":{}}"#).err(),
        Some("the line of call 7 holds request and cancel and answered: a line holds one".into())
    );
    // A line names its call: without an id, or with one that is not a whole number, it is no line.
    for line in [
        r#"{"request":{}}"#,
        r#"{"id":-1,"cancel":{}}"#,
        r#"{"id":1.5,"cancel":{}}"#,
        "[]",
    ] {
        assert!(serde_json::from_str::<Raw>(line).is_err(), "{line}");
    }
    // The members written are the line's one member and the answer's time.
    assert_eq!(
        serde_json::to_string(&Envelope::<(), ()>::cancelling(9)).unwrap(),
        r#"{"id":9,"cancel":{}}"#
    );
    let answered = RawValue::from_string(r#"{"answer":null}"#.into()).unwrap();
    assert_eq!(
        serde_json::to_string(&Envelope::<(), &RawValue>::answering(
            9,
            &answered,
            Some(0.25)
        ))
        .unwrap(),
        r#"{"id":9,"answered":{"answer":null},"us":0.25}"#
    );
    assert_eq!(
        serde_json::to_string(&Envelope::<(), &RawValue>::answering(9, &answered, None)).unwrap(),
        r#"{"id":9,"answered":{"answer":null}}"#
    );
}

/// The lines `read_line` reads of `input` in chunks of `chunk` bytes, at most `max` bytes a line,
/// until the input ends or a line is too long.
fn lines_of(input: &[u8], chunk: usize, max: usize) -> Vec<Lined> {
    let mut input = std::io::BufReader::with_capacity(chunk, input);
    let mut lines = Vec::new();
    loop {
        let line = read_line(&mut input, max).unwrap();
        let last = !matches!(line, Lined::Line(_));
        lines.push(line);
        if last {
            return lines;
        }
    }
}

#[test]
fn a_line_is_read_as_bytes_whatever_they_hold_and_never_past_its_bound() {
    let line = |text: &[u8]| Lined::Line(text.to_vec());
    // Whatever the chunks the input comes in, the lines are the same.
    for chunk in [1, 2, 3, 64] {
        assert_eq!(
            lines_of(b"ab\n\xff\xfe\n\nlast", chunk, 8),
            [
                line(b"ab"),
                line(b"\xff\xfe"),
                line(b""),
                line(b"last"),
                Lined::End
            ],
            "{chunk}"
        );
        // A line of the bound is whole; one byte past it is too long, read up to the bound.
        assert_eq!(
            lines_of(b"12345678\n123456789\nnext\n", chunk, 8),
            [line(b"12345678"), Lined::TooLong(b"12345678".to_vec())],
            "{chunk}"
        );
    }
    assert_eq!(lines_of(b"", 4, 8), [Lined::End]);
    // A line too long leaves its rest unread, for a reader that goes on.
    let mut input = std::io::BufReader::with_capacity(2, &b"abcdef\ngh\n"[..]);
    assert_eq!(
        read_line(&mut input, 4).unwrap(),
        Lined::TooLong(b"abcd".to_vec())
    );
    assert_eq!(read_line(&mut input, 4).unwrap(), line(b"ef"));
    assert_eq!(read_line(&mut input, 4).unwrap(), line(b"gh"));
}

/// An input that is interrupted once, then holds `then`, then fails.
struct Interrupted {
    interrupted: bool,
    then: Vec<u8>,
}

impl std::io::Read for Interrupted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        if self.then.is_empty() {
            return Err(std::io::Error::other("the pipe broke"));
        }
        let taken = self.then.len().min(buf.len());
        buf[..taken].copy_from_slice(&self.then[..taken]);
        self.then.drain(..taken);
        Ok(taken)
    }
}

#[test]
fn an_interrupted_read_is_tried_again_and_a_failed_one_is_the_readers_error() {
    let mut input = std::io::BufReader::new(Interrupted {
        interrupted: false,
        then: b"a\n".to_vec(),
    });
    assert_eq!(
        read_line(&mut input, LINE_MAX).unwrap(),
        Lined::Line(b"a".to_vec())
    );
    assert_eq!(
        read_line(&mut input, LINE_MAX).unwrap_err().to_string(),
        "the pipe broke"
    );
}

#[test]
fn each_call_names_the_minor_it_came_in_and_commands_run_is_the_loopbacks_alone() {
    let tags: Vec<&str> = MINORS.iter().map(|(tag, _)| *tag).collect();
    assert_eq!(
        tags, CALLS,
        "a minor for every call, in the protocol's order"
    );
    for (tag, since) in MINORS {
        assert!(
            (1..=PROTOCOL.minor).contains(since),
            "{tag} came in minor {since}, which is not one of this build's 0.1 to {PROTOCOL}"
        );
    }
    assert_eq!(Slot::SINCE, 1);
    assert_eq!(Declare::SINCE, 1);
    assert_eq!(ReadCards::SINCE, 3);
    assert_eq!(LOOPBACK_ONLY, ["commands.run"]);
    // Each call's own constant says the same: the loopback's alone is not piped.
    const {
        assert!(!RunCommand::PIPED);
        assert!(Slot::PIPED && Render::PIPED && Declare::PIPED && Alive::PIPED);
    }
}
