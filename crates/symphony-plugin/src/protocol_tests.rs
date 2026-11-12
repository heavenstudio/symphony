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
enum Order {
    Sorted,
    Reversed,
}

/// `text`'s JSON written again with every object's members in `order`, at every depth: as a writer
/// that sorts its keys writes it, or the other way round. Numbers keep their text as written.
fn ordered(text: &str, order: Order) -> String {
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
fn reads_back_in_any_order<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let text = serde_json::to_string(value).unwrap();
    for order in [Order::Sorted, Order::Reversed] {
        let moved = ordered(&text, order);
        let read = serde_json::from_str::<T>(&moved)
            .unwrap_or_else(|e| panic!("{order:?} {moved} does not read back: {e}"));
        assert_eq!(&read, value, "{order:?} {moved}");
    }
}

fn card() -> CardData {
    CardData {
        slug: "pick-a-hull".into(),
        doc: "docs/SHIPS.md".into(),
        topic: "Hulls \"quoted\"".into(),
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

fn page() -> PageData {
    PageData {
        root: "/r/ünï code".into(),
        doc_path: "docs/A.md".into(),
    }
}

/// A request of every call, with its call: two of a board field's, its value a fraction and a text.
fn every_request() -> Vec<(Request, &'static str)> {
    let root = PathBuf::from("/r");
    vec![
        (Declare { protocol: PROTOCOL }.into(), "plugin.declare"),
        (
            Render {
                component: "ship".into(),
                page: page(),
                argument: "lantern".into(),
                option: Some("wide".into()),
            }
            .into(),
            "components.render",
        ),
        (
            Resolve {
                component: "ship".into(),
                page: page(),
                argument: String::new(),
                option: None,
            }
            .into(),
            "components.resolve",
        ),
        (
            CacheVersion {
                component: "ship".into(),
                page: page(),
            }
            .into(),
            "components.cache_version",
        ),
        (
            Recognises {
                kind: "hull".into(),
                card: card(),
            }
            .into(),
            "card_kinds.recognises",
        ),
        (
            Pictures {
                kind: "hull".into(),
                root: root.clone(),
                card: card(),
            }
            .into(),
            "card_kinds.pictures",
        ),
        (
            Slot {
                kind: "hull".into(),
                card: card(),
            }
            .into(),
            "card_kinds.slot",
        ),
        (
            TriageCategory {
                kind: "hull".into(),
                card: card(),
            }
            .into(),
            "card_kinds.triage_category",
        ),
        (
            AutoApprovable {
                kind: "hull".into(),
                card: card(),
            }
            .into(),
            "card_kinds.auto_approvable",
        ),
        (
            CardGates {
                kind: "hull".into(),
                card: card(),
            }
            .into(),
            "card_kinds.gates",
        ),
        (
            ApprovalNeedsAgent {
                kind: "hull".into(),
                card: card(),
            }
            .into(),
            "card_kinds.approval_needs_agent",
        ),
        (
            Chosen {
                kind: "hull".into(),
                card: card(),
            }
            .into(),
            "card_kinds.chosen",
        ),
        (
            Draw {
                renderer: "glb".into(),
                page: page(),
                file: FileData {
                    src: "/x/hulls/a.glb?v=1#top".into(),
                    label: String::new(),
                },
            }
            .into(),
            "renderers.draw",
        ),
        (
            Serve {
                route: "hulls".into(),
                root: root.clone(),
                rest: "a.glb".into(),
            }
            .into(),
            "routes.serve",
        ),
        (
            ShowRegion {
                region: "art".into(),
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
                field: "effort".into(),
                value: FieldData::Number(0.1 + 0.2),
            }
            .into(),
            "board_fields.validate",
        ),
        (
            Validate {
                field: "effort".into(),
                value: FieldData::Text("3".into()),
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
            Changed {
                watch: 2,
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
                command: "ship-art".into(),
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
            value: FieldData::Number(2.5),
        }))
        .unwrap(),
        Order::Reversed,
    );
    assert_eq!(
        reversed,
        r#"{"board_fields.validate":{"value":{"Number":2.5},"field":"effort"}}"#
    );
    assert_eq!(
        serde_json::from_str::<Request>(&reversed).unwrap(),
        Request::from(Validate {
            field: "effort".into(),
            value: FieldData::Number(2.5),
        })
    );
}

#[test]
fn a_request_reads_back_only_as_its_own_call() {
    let (_, read, _) = wire(Slot {
        kind: "hull".into(),
        card: card(),
    });
    assert_eq!(Recognises::from_request(read.clone()), None);
    assert_eq!(
        Slot::from_request(read),
        Some(Slot {
            kind: "hull".into(),
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
        r#"{{"card_kinds.colour":{{"kind":"hull","card":{card}}}}}"#
    ));
    assert!(
        unknown.contains("unknown variant `card_kinds.colour`"),
        "{unknown}"
    );
    let missing = refused(r#"{"card_kinds.slot":{"kind":"hull"}}"#);
    assert!(missing.contains("missing field `card`"), "{missing}");
    let nested = refused(&format!(
        r#"{{"card_kinds.slot":{{"kind":"hull","card":{}}}}}"#,
        card.replace(r#""slug":"pick-a-hull","#, "")
    ));
    assert!(nested.contains("missing field `slug`"), "{nested}");
    // The request is one member, its call: its fields beside it, or a second member, are refused.
    for text in [
        r#"{"kind":"hull"}"#,
        r#"{"card_kinds.slot":{"kind":"hull"},"card_kinds.gates":{"kind":"hull"}}"#,
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
        "card_kinds.slot": {"kind": "hull", "card": card, "since": 1.5}
    })
    .to_string();
    assert_eq!(
        serde_json::from_str::<Request>(&text).unwrap(),
        Request::from(Slot {
            kind: "hull".into(),
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
    assert_eq!(PROTOCOL.to_string(), "0.1");
}

#[test]
fn a_path_that_is_not_utf8_cannot_be_written() {
    use std::os::unix::ffi::OsStrExt as _;
    let root = PathBuf::from(std::ffi::OsStr::from_bytes(b"/r/\xff"));
    let request: Request = Serve {
        route: "hulls".into(),
        root,
        rest: String::new(),
    }
    .into();
    assert!(serde_json::to_string(&request).is_err());
}

#[test]
fn a_number_that_is_not_finite_does_not_read_back() {
    let written = serde_json::to_string(&Ok::<_, String>(FieldData::Number(f64::NAN))).unwrap();
    assert_eq!(written, r#"{"Ok":{"Number":null}}"#);
    assert!(serde_json::from_str::<Result<FieldData, String>>(&written).is_err());
    let written = serde_json::to_string(&FieldData::Number(f64::INFINITY)).unwrap();
    assert!(serde_json::from_str::<FieldData>(&written).is_err());
}

#[test]
fn a_fractional_field_value_reads_back_tagged_where_the_boards_untagged_one_does_not() {
    // The server's JSON keeps numbers exact, and so cannot read one back through a buffer.
    assert!(serde_json::from_str::<FieldValue>("3.5").is_err());
    assert_eq!(
        serde_json::from_str::<FieldValue>("3").unwrap(),
        FieldValue::Number(3.0)
    );
    for value in [
        FieldValue::Number(3.5),
        FieldValue::Number(-0.25),
        FieldValue::Number(1e300),
        FieldValue::Text("3.5".into()),
        FieldValue::Text(String::new()),
    ] {
        let data = FieldData::from(value.clone());
        let text = serde_json::to_string(&data).unwrap();
        let read: FieldData = serde_json::from_str(&text).unwrap();
        assert_eq!(FieldValue::from(read), value, "{text}");
    }
    assert_eq!(
        serde_json::to_string(&FieldData::Text("x".into())).unwrap(),
        r#"{"Text":"x"}"#
    );
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
            anchor: "hulls".into(),
            spans: vec![
                Span::Text {
                    text: "Hulls".into(),
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
                src: "/x/a.glb".into(),
                label: "hull".into(),
                poster: None,
                viewer: Viewer {
                    element: "model-viewer".into(),
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
                        src: "/x/a.glb".into(),
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
        src: "/a.glb".into(),
        label: "hull".into(),
    };
    assert_eq!(FileData::of(&file.file()), file);
    let root = Path::new("/r/ünï code");
    let named = Page::new(root, "docs/A.md");
    assert_eq!(PageData::of(&named), page());
    assert!(page().names(&named));
    assert!(!page().names(&Page::new(root, "docs/B.md")));
    assert!(!page().names(&Page::new(Path::new("/r"), "docs/A.md")));
    let problem = Problem::new("docs/A.md", 3, "no anchor".into());
    assert_eq!(ProblemData::of(&problem).problem(), problem);
}

/// A plugin that declares something at every seam.
struct Declaring;

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
            label: "⬡ hulls".into(),
            title: "Hulls".into(),
            href: "/x/hulls".into(),
            document: "docs/HULLS.md".into(),
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
            package: "foundry".into(),
            name: "Star Foundry".into(),
            icon: Some("⬡".into()),
            tokens: BTreeMap::from([("accent".into(), "#ff8800".into())]),
            stylesheets: vec!["docs/theme.css".into()],
            layouts: BTreeMap::from([("home".into(), "wide".into())]),
        })
    }
}

impl Component for Declaring {
    fn name(&self) -> &str {
        "ship"
    }
    fn render(&self, _: &Page<'_>, _: &str, _: Option<&str>) -> Result<Vec<Block>, String> {
        Ok(vec![Block::Paragraph {
            spans: vec![Span::Text { text: "x".into() }],
        }])
    }
}

impl CardKind for Declaring {
    fn name(&self) -> &str {
        "hull"
    }
    fn recognises(&self, _: &Card<'_>) -> bool {
        true
    }
    fn markers(&self) -> &[&str] {
        &["Slot", "Hull"]
    }
}

impl Renderer for Declaring {
    fn name(&self) -> &str {
        "glb"
    }
    fn extensions(&self) -> &[&str] {
        &["glb", "gltf"]
    }
    fn content_type(&self) -> &str {
        "model/gltf-binary"
    }
    fn draw(&self, _: &Page<'_>, _: &File<'_>) -> Result<Drawing, String> {
        Ok(Drawing::Viewer(Viewer::default()))
    }
}

impl Route for Declaring {
    fn path(&self) -> &str {
        "hulls"
    }
    fn serve(&self, _: &Path, _: &str) -> Option<Reply> {
        None
    }
}

impl HomeRegion for Declaring {
    fn id(&self) -> &str {
        "art"
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
        vec!["data/ships.toml".into()]
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
        "ship-art"
    }
    fn summary(&self) -> &str {
        "draw the ships' art"
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
    assert_eq!(declared.components, ["ship"]);
    assert_eq!(
        declared.card_kinds,
        [DeclaredKind {
            name: "hull".into(),
            markers: vec!["Slot".into(), "Hull".into()],
        }]
    );
    assert_eq!(
        declared.renderers,
        [DeclaredRenderer {
            name: "glb".into(),
            extensions: vec!["glb".into(), "gltf".into()],
            content_type: "model/gltf-binary".into(),
        }]
    );
    assert_eq!(declared.routes, ["hulls"]);
    assert_eq!(declared.nav, Declaring.nav());
    assert_eq!(declared.home_regions, ["art"]);
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
            inputs: vec!["data/ships.toml".into()],
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
            name: "ship-art".into(),
            summary: "draw the ships' art".into(),
        }]
    );
    assert_eq!(declared.transports, ["uds"]);
    assert_eq!(declared.theme, Declaring.theme());
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

/// That `answer`, the call `A`'s, reads back as the wire carries it in any order of its members.
fn answers<A: Ask>(answered: &mut Answers, answer: A::Answer) {
    reads_back_in_any_order(&Answered::Answer(answer));
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
    answers::<Render>(a, Err("no such hull".into()));
    answers::<Resolve>(a, Ok(()));
    answers::<CacheVersion>(a, Some("v2".into()));
    answers::<Recognises>(a, true);
    answers::<Pictures>(
        a,
        vec![Picture {
            src: Some("/x/a.png".into()),
            caption: "the hull".into(),
            waiting: true,
        }],
    );
    answers::<Slot>(a, Some("North · cover".into()));
    answers::<TriageCategory>(a, None);
    answers::<AutoApprovable>(a, false);
    answers::<CardGates>(a, Some((3, "item".into())));
    answers::<ApprovalNeedsAgent>(a, true);
    answers::<Chosen>(a, Ok(Some("B".into())));
    answers::<Draw>(a, Ok(Drawing::Blocks(vec![paragraph()])));
    answers::<Draw>(a, Ok(Drawing::Viewer(Viewer::default())));
    answers::<Serve>(a, Some(Reply::Json("{\"a\":1.5}".into())));
    answers::<Serve>(a, Some(Reply::File("/r/a.glb".into())));
    answers::<ShowRegion>(
        a,
        Some(Region {
            id: "art".into(),
            title: "Art".into(),
            note: String::new(),
            bars: vec![ProgressBar {
                key: "hulls".into(),
                name: "Hulls".into(),
                page: "docs/HULLS.md".into(),
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
    answers::<Validate>(a, Ok(FieldData::Number(0.1 + 0.2)));
    answers::<Validate>(a, Ok(FieldData::Text("3".into())));
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
    answers::<Changed>(a, Some("models".into()));
    answers::<Forget>(a, ());
    answers::<CheckDocs>(a, vec![ProblemData::of(&Problem::new("a", 0, "b".into()))]);
    answers::<DocComponents>(a, vec!["ship".into()]);
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
