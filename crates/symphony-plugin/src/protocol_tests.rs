use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::vocabulary::{GateState, Span, Viewer};
use crate::{
    BoardAction, BoardField, CardKind, Command, Component, DocCheck, Docs, Gates, Generator,
    HomeRegion, ProseRule, Renderer, Route, Transport, Watch,
};

/// `request` written as the wire carries it and read back, with the `call` it went as.
fn wire(request: impl Into<Request>) -> (Request, Request, String) {
    let request = request.into();
    let text = serde_json::to_string(&request).unwrap();
    let written: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(written["args"].is_object(), "{text}");
    let call = written["call"].as_str().unwrap().to_string();
    (request, serde_json::from_str(&text).unwrap(), call)
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

#[test]
fn every_call_goes_on_the_wire_as_its_seam_and_call_and_reads_back_the_same() {
    let root = PathBuf::from("/r");
    let requests: Vec<(Request, &str)> = vec![
        (Declare {}.into(), "plugin.declare"),
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
    ];
    for (request, tag) in requests {
        let (sent, read, call) = wire(request);
        assert_eq!(call, tag);
        assert_eq!(read, sent, "{tag}");
    }
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

#[test]
fn a_call_named_otherwise_or_missing_a_field_does_not_read() {
    for text in [
        r#"{"call":"card_kinds.colour","args":{"kind":"hull"}}"#,
        r#"{"call":"card_kinds.slot","args":{"kind":"hull"}}"#,
        r#"{"call":"card_kinds.slot","kind":"hull"}"#,
        r#"{"args":{"kind":"hull"}}"#,
    ] {
        assert!(serde_json::from_str::<Request>(text).is_err(), "{text}");
    }
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
fn a_request_holding_a_fraction_reads_back_with_its_call_first_and_not_after() {
    let request = Validate {
        field: "effort".into(),
        value: FieldData::Number(2.5),
    };
    let (sent, read, _) = wire(request);
    assert_eq!(read, sent);
    // Its args before its call are buffered until the call is known, as nothing this writes is.
    let after =
        r#"{"args":{"field":"effort","value":{"Number":2.5}},"call":"board_fields.validate"}"#;
    assert!(serde_json::from_str::<Request>(after).is_err());
    let whole =
        r#"{"args":{"field":"effort","value":{"Number":2}},"call":"board_fields.validate"}"#;
    assert!(serde_json::from_str::<Request>(whole).is_ok());
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

#[test]
fn every_answer_type_reads_back_what_was_written() {
    fn back<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
        let text = serde_json::to_string(value).unwrap();
        assert_eq!(&serde_json::from_str::<T>(&text).unwrap(), value, "{text}");
    }
    back(&Ok::<(), String>(()));
    back(&Err::<(), String>("refused".into()));
    back(&Some((3_usize, "item".to_string())));
    back(&Ok::<Option<String>, String>(None));
    back(&Ok::<_, String>(Drawing::Viewer(Viewer::default())));
    back(&Ok::<_, String>(Drawing::Blocks(vec![Block::Paragraph {
        spans: vec![Span::Text { text: "x".into() }],
    }])));
    back(&Some(Reply::File("/r/a.glb".into())));
    back(&Some(Reply::Json("{\"a\":1}".into())));
    back(&Ok::<_, String>(Submitted {
        said: "landed".into(),
        released: vec!["sim/jump".into()],
    }));
    back(&vec![(2_usize, "two".to_string())]);
    back(&vec![ProblemData::of(&Problem::new("a", 0, "b".into()))]);
    back(&-1_i32);
    back(&());
}
