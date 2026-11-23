use super::{fields, items, parse, read, read_exact};
use crate::execution::{
    Artifact, Asked, Binding, Cancellation, Cleanup, Collection, Dispatch, Ending, Identity, Lease,
    Pending, Pin, Relation, Row, State,
};
use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize as Derived, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::fmt::Debug;
use symphony_plugin::execution::{
    AttemptId, AttemptRef, Capabilities, ContentRef, ConversationId, Input, Operation, Principal,
    PrincipalKind, Request, Resource, ResourceId, ResumeFrom, ResumeMode, Scope, StopMode,
};

fn principal(subject: &str) -> Principal {
    Principal::new(PrincipalKind::Agent, "local".into(), subject.into())
}

fn content(name: &str) -> ContentRef {
    ContentRef::new(name.into(), format!("{name}-digest"))
}

fn input() -> Input {
    Input::new("r1".into(), "d1".into(), "abc".into(), content("manifest"))
}

fn resource() -> Resource {
    let attempt = AttemptRef::new(
        Scope::new("p".into(), "b".into()),
        "t".into(),
        AttemptId::new([7; 16]),
    );
    Resource::new(
        attempt,
        "c".into(),
        ResourceId::new("res".into()),
        "g1".into(),
    )
}

fn from() -> ResumeFrom {
    let mut from = ResumeFrom::new(resource(), "fact-9".into());
    from.checkpoint = Some(content("checkpoint"));
    from.conversation = Some(ConversationId::new("conv".into()));
    from
}

/// Every request the board records as owed, one of each variant.
fn requests() -> Vec<Request> {
    vec![
        Request::Start { input: input() },
        Request::Reconcile { resource: None },
        Request::ReconcileResume {
            resource: Some(resource()),
            from: from(),
            mode: ResumeMode::ReuseWithNewGeneration,
        },
        Request::Observe {
            resource: resource(),
            cursor: Some("cursor".into()),
        },
        Request::Cancel {
            resource: resource(),
        },
        Request::Stop {
            resource: resource(),
            mode: StopMode::HardStop,
        },
        Request::Resume {
            input: input(),
            from: from(),
            mode: ResumeMode::NewResource,
        },
        Request::ExternalLink {
            resource: resource(),
            conversation: None,
        },
    ]
}

fn identity() -> Identity {
    let mut capabilities = Capabilities::default();
    capabilities.operations = vec![Operation::Start, Operation::Stop, Operation::Resume];
    capabilities.stop_modes = vec![StopMode::Pause, StopMode::Terminate];
    capabilities.resume_modes = vec![ResumeMode::NewResource];
    capabilities.idempotency_retention_ms = Some(60_000);
    capabilities.external_view = true;
    Identity {
        project: "p".into(),
        board: "b".into(),
        task: "t".into(),
        lane: "executor".into(),
        role: "executor".into(),
        handle: Some("amber-1".into()),
        predecessor: Some("0011".into()),
        relation: Some(Relation::Resume),
        decision: Some("found it".into()),
        input: input(),
        configuration: "c".into(),
        contract: symphony_plugin::execution::wire::VERSION,
        capabilities,
        initiator: principal("owner"),
        policy_revision: "1".into(),
        request_key: Some("key".into()),
        runtime_epoch: 2,
        capacity: Some("pool".into()),
    }
}

/// A state with every field set, owing `request`.
fn state(request: Request) -> State {
    let asked = |mode| Asked {
        initiator: principal("owner"),
        mode,
    };
    State {
        dispatch: Dispatch::Paused,
        ending: Some(Ending::Superseded),
        cancellation: Cancellation::Unconfirmed,
        collection: Collection::Partial,
        cleanup: Cleanup::Running,
        resource: Some(Binding {
            id: "res".into(),
            generation: "g1".into(),
        }),
        conversation: Some("conv".into()),
        cursor: Some("cursor".into()),
        checkpoint: Some(content("checkpoint")),
        completion: Some(content("completion")),
        manifest: Some(content("manifest")),
        sequence: 4,
        pending: Some(Pending {
            sequence: 4,
            kind: request.operation(),
            key: "k4".into(),
            digest: "d4".into(),
            request,
            initiator: principal("worker"),
            sent: true,
            first_sent: Some(1_000),
            cancels: true,
        }),
        unknowns: 2,
        held: Some("why".into()),
        cancel: Some(asked(None)),
        stop: Some(asked(Some(StopMode::Pause))),
        quiescent: Some(9),
        reserved: Some("pool".into()),
        cleanup_key: Some("cleanup-1".into()),
        cleanups: 1,
        successor: Some("0022".into()),
        resource_handed_to: Some("0022".into()),
    }
}

fn row(request: Request) -> Row {
    Row {
        attempt: "0011".into(),
        task: "t".into(),
        created: -5,
        identity: identity(),
        state: state(request),
        revision: 3,
        open: true,
        live: false,
        due: Some(12),
        lease: Some(Lease {
            owner: "worker".into(),
            epoch: 6,
            expires: 99,
        }),
        epoch: 6,
        facts: 11,
        workflow: Some(Pin {
            phase: "build".into(),
            state: "running".into(),
            revision: 8,
        }),
    }
}

/// `value` written as the board writes it reads back the same through [`read`], and as
/// `serde_json::from_value` reads it.
fn reads_back<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let written = serde_json::to_value(value).unwrap();
    let read: T = read(&written).unwrap();
    assert_eq!(&read, value);
    assert_eq!(read, serde_json::from_value::<T>(written).unwrap());
}

#[test]
fn every_row_the_board_writes_reads_back_the_same() {
    for request in requests() {
        reads_back(&row(request));
    }
    let mut bare = row(Request::Reconcile { resource: None });
    bare.state = State::default();
    (bare.due, bare.lease, bare.workflow) = (None, None, None);
    reads_back(&bare);
    reads_back(&Artifact {
        name: "out/report.txt".into(),
        reference: "store://1".into(),
        digest: "sha".into(),
        size: 42,
        generation: "g1".into(),
        runtime_epoch: 2,
        verified: true,
    });
}

#[test]
fn a_row_reads_the_fields_its_types_default_when_they_are_left_out_and_skips_unknown_ones() {
    let mut written = serde_json::to_value(row(Request::Start { input: input() })).unwrap();
    let object = written.as_object_mut().unwrap();
    for left_out in ["due", "lease", "workflow"] {
        object.remove(left_out);
    }
    object.insert(
        "added".into(),
        json!({"later": [1, {"field": null}], "by": "a newer board"}),
    );
    written["identity"]["capabilities"] = json!({"external_view": true});
    let read: Row = read(&written).unwrap();
    assert_eq!(read, serde_json::from_value::<Row>(written).unwrap());
    assert_eq!((read.due, read.lease, read.workflow), (None, None, None));
    assert!(read.identity.capabilities.external_view);
    assert!(read.identity.capabilities.operations.is_empty());
}

/// Both readers' answers for `value` as `T`: [`read`]'s, then `serde_json::from_value`'s.
fn both<T: DeserializeOwned + Debug>(value: Value) -> (Result<T, String>, Result<T, String>) {
    (
        read::<T>(&value).map_err(|e| e.to_string()),
        serde_json::from_value::<T>(value).map_err(|e| e.to_string()),
    )
}

#[test]
fn a_struct_in_an_array_is_refused_where_from_value_takes_it() {
    let (objects, values) = both::<Binding>(json!(["res", "g1"]));
    assert_eq!(
        objects.unwrap_err(),
        "invalid type: sequence, expected struct Binding"
    );
    assert_eq!(
        values.unwrap(),
        Binding {
            id: "res".into(),
            generation: "g1".into(),
        }
    );
    let positional = json!(["0011", "t"]);
    assert_eq!(
        Row::of(&positional).unwrap_err().to_string(),
        crate::Error::Db(
            "an execution attempt not in the board's shape: invalid type: sequence, expected \
             struct Row"
                .into()
        )
        .to_string()
    );
    assert_eq!(
        serde_json::from_value::<Row>(positional)
            .unwrap_err()
            .to_string(),
        "invalid length 2, expected struct Row with 13 elements"
    );
}

/// A map read for its first entry alone, which leaves the others unread.
#[derive(Debug)]
struct FirstEntry;

impl<'de> Deserialize<'de> for FirstEntry {
    fn deserialize<D: Deserializer<'de>>(from: D) -> Result<FirstEntry, D::Error> {
        struct First;
        impl<'de> Visitor<'de> for First {
            type Value = FirstEntry;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<FirstEntry, A::Error> {
                map.next_entry::<IgnoredAny, IgnoredAny>()?;
                Ok(FirstEntry)
            }
        }
        from.deserialize_map(First)
    }
}

#[test]
fn every_other_refusal_says_what_from_value_says() {
    fn same<T: DeserializeOwned + Debug>(value: Value, says: &str) {
        let (objects, values) = both::<T>(value.clone());
        assert_eq!(objects.unwrap_err(), says, "{value}");
        assert_eq!(values.unwrap_err(), says, "{value}");
    }
    same::<Binding>(json!({"id": "res"}), "missing field `generation`");
    same::<Binding>(
        json!({"id": 1, "generation": "g1"}),
        "invalid type: number, expected a string",
    );
    same::<Binding>(
        json!("res"),
        "invalid type: string \"res\", expected struct Binding",
    );
    same::<Binding>(json!(null), "invalid type: null, expected struct Binding");
    same::<Option<Binding>>(
        json!(true),
        "invalid type: boolean `true`, expected struct Binding",
    );
    same::<Lease>(
        json!({"owner": "w", "epoch": -1, "expires": 1}),
        "invalid number",
    );
    same::<Dispatch>(
        json!("flying"),
        "unknown variant `flying`, expected one of `pending`, `starting`, `unknown`, `running`, \
         `paused`, `held`, `terminal`",
    );
    for number in [json!(3), json!(-3), json!(1.5)] {
        same::<Dispatch>(number, "invalid type: number, expected string or map");
    }
    same::<Dispatch>(json!({"held": 1}), "invalid type: number, expected unit");
    same::<Dispatch>(
        json!({"held": null, "paused": null}),
        "invalid value: map, expected map with a single key",
    );
    same::<Dispatch>(
        json!({}),
        "invalid value: map, expected map with a single key",
    );
    same::<Request>(
        json!("Cancel"),
        "invalid type: unit variant, expected struct variant",
    );
    same::<Request>(
        json!({"Cancel": "res"}),
        "invalid type: string \"res\", expected struct variant",
    );
    let stop = serde_json::to_value(Request::Stop {
        resource: resource(),
        mode: StopMode::Pause,
    })
    .unwrap();
    let fields = json!([stop["Stop"]["resource"], stop["Stop"]["mode"]]);
    same::<Request>(
        json!({ "Stop": fields }),
        "invalid type: sequence, expected struct variant",
    );
    same::<Vec<Operation>>(
        json!("Start"),
        "invalid type: string \"Start\", expected a sequence",
    );
    same::<AttemptId>(
        Value::from(vec![0; 17]),
        "invalid length 17, expected fewer elements in array",
    );
    same::<AttemptId>(
        Value::from(vec![0; 15]),
        "invalid length 15, expected an array of length 16",
    );
    same::<FirstEntry>(
        json!({"a": 1, "b": 2}),
        "invalid length 2, expected fewer elements in map",
    );
    same::<FirstEntry>(json!([1]), "invalid type: sequence, expected a map");
}

#[test]
fn any_value_reads_whole_and_the_rest_of_each_form_as_from_value_reads_it() {
    let value = json!({"a": [1, -2, 1.5, "s", null, true, {"b": []}], "c": {}});
    assert_eq!(read::<Value>(&value).unwrap(), value);
    let (objects, values) = both::<Dispatch>(json!({"held": null}));
    assert_eq!(objects.unwrap(), Dispatch::Held);
    assert_eq!(values.unwrap(), Dispatch::Held);
    let (objects, values) = both::<(u8, String)>(json!([1, "s"]));
    assert_eq!(objects.unwrap(), values.unwrap());
}

#[derive(Debug, PartialEq, Derived)]
enum Shape {
    Unit,
    Newtype(u8),
    Tuple(u8, String),
    Struct { a: u8 },
}

#[derive(Debug, PartialEq, Derived)]
struct Newtype(u8);

#[derive(Debug, PartialEq, Derived)]
struct Pair(u8, String);

#[derive(Debug, PartialEq, Derived)]
struct Nothing;

/// `value` read as `T` by both readers, each the same: the same value or the same refusal.
fn agree<T: DeserializeOwned + Debug>(value: Value) {
    let (objects, values) = both::<T>(value.clone());
    assert_eq!(format!("{objects:?}"), format!("{values:?}"), "{value}");
}

#[test]
fn the_shapes_no_row_holds_read_as_from_value_reads_them() {
    for value in [
        json!("Unit"),
        json!({"Unit": null}),
        json!({"Unit": 1}),
        json!({"Newtype": 7}),
        json!("Newtype"),
        json!({"Tuple": [1, "s"]}),
        json!({"Tuple": []}),
        json!({"Tuple": 1}),
        json!({"Tuple": [1, "s", 2]}),
        json!("Tuple"),
        json!({"Struct": {"a": 1}}),
        json!({"Struct": [1]}),
        json!({"Other": null}),
        json!(["Unit"]),
    ] {
        agree::<Shape>(value);
    }
    for value in [json!(5), json!("5"), json!([5])] {
        agree::<Newtype>(value);
    }
    for value in [
        json!([1, "s"]),
        json!([1]),
        json!([1, "s", 2]),
        json!({"0": 1}),
    ] {
        agree::<Pair>(value);
    }
    for value in [json!(null), json!(1), json!([])] {
        agree::<Nothing>(value);
    }
    for value in [json!(null), json!(1), json!({"a": 1})] {
        agree::<Option<Option<u8>>>(value);
    }
    for value in [json!({"a": 1}), json!({"a": "s"}), json!(null)] {
        agree::<std::collections::HashMap<String, u8>>(value);
    }
}

#[test]
fn an_open_tables_row_reads_past_a_field_it_does_not_know_and_reads_alone_refuses_it() {
    use crate::rows::{Dump, RoleRow, TaskRow, WorkflowRow};
    let role = json!({"slug": "a", "handle": "h", "role": "executor", "since": 1.0});
    let mut ahead = role.clone();
    ahead["fixture_note"] = json!({"kept": [1, null]});
    // Through the store's reader, at the top, in a list and inside another struct, as it is read
    // without the field.
    let (objects, values) = both::<RoleRow>(ahead.clone());
    assert_eq!(objects.unwrap(), read::<RoleRow>(&role).unwrap());
    assert_eq!(
        values.unwrap_err(),
        "unknown field `fixture_note`, expected one of `slug`, `handle`, `role`, `since`, `expires`"
    );
    let rows: Vec<RoleRow> = read(&json!([role, ahead])).unwrap();
    assert_eq!(rows[0], rows[1]);
    let held: (Vec<RoleRow>, i64) = read(&json!([[ahead], 3])).unwrap();
    assert_eq!(held.0[0], rows[0]);
    // Read exactly, as a copy reads the board, it is refused at every depth: in a list, a tuple,
    // an option, a map's value and a variant's, and the dump's own tables.
    #[derive(Debug, Derived)]
    enum Holding {
        Role(#[allow(dead_code)] RoleRow),
    }
    let refused = "unknown field `fixture_note`, expected one of `slug`, `handle`, `role`, `since`, `expires`";
    assert_eq!(read_exact::<RoleRow>(&role).unwrap(), rows[0]);
    assert_eq!(
        read_exact::<RoleRow>(&ahead).unwrap_err().to_string(),
        refused
    );
    let deep = [
        json!([role, ahead]),
        json!([[ahead], 3]),
        json!({"role": ahead}),
        json!({"Role": ahead}),
        json!({"roles": [role, ahead]}),
    ];
    let exactly = [
        read_exact::<Vec<RoleRow>>(&deep[0]).map(drop),
        read_exact::<(Vec<Option<RoleRow>>, i64)>(&deep[1]).map(drop),
        read_exact::<std::collections::BTreeMap<String, RoleRow>>(&deep[2]).map(drop),
        read_exact::<Holding>(&deep[3]).map(drop),
        read_exact::<Dump>(&deep[4]).map(drop),
    ];
    for (value, exactly) in deep.iter().zip(exactly) {
        assert_eq!(exactly.unwrap_err().to_string(), refused, "{value}");
    }
    read::<(Vec<Option<RoleRow>>, i64)>(&deep[1]).unwrap();
    read::<std::collections::BTreeMap<String, RoleRow>>(&deep[2]).unwrap();
    read::<Holding>(&deep[3]).unwrap();
    assert_eq!(
        read::<Dump>(&deep[4]).unwrap().roles,
        [rows[0].clone(), rows[0].clone()]
    );
    // An alias the struct names is handed to it: SurrealDB's `sid` for a task's `id`.
    let task = json!({"sid": 7, "slug": "sim/jump", "title": "Jump", "status": "ready",
        "created_by": "owner", "created": 1.0, "updated": 2.0, "fixture_note": "x"});
    let read_task: TaskRow = read(&task).unwrap();
    assert_eq!((read_task.id, read_task.slug.as_str()), (7, "sim/jump"));
    assert!(
        read_exact::<TaskRow>(&task)
            .unwrap_err()
            .to_string()
            .starts_with("unknown field `fixture_note`")
    );
    // The rest refuse it through either reader: a closed table's row and the dump itself.
    let record = json!({"key": ["active"], "kind": "active", "body": {}, "at": 1.0, "extra": 1});
    let (objects, values) = both::<WorkflowRow>(record);
    assert!(objects.unwrap_err().starts_with("unknown field `extra`"));
    assert!(values.unwrap_err().starts_with("unknown field `extra`"));
    let dump = json!({"roles": [], "fixture_table": []});
    let (objects, values) = both::<Dump>(dump);
    assert!(
        objects
            .unwrap_err()
            .starts_with("unknown field `fixture_table`")
    );
    assert!(
        values
            .unwrap_err()
            .starts_with("unknown field `fixture_table`")
    );
}

/// A map's key as a derived unit enum reads it.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Derived)]
#[serde(rename_all = "snake_case")]
enum Key {
    First,
    Second,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Derived)]
struct Named(String);

#[test]
fn a_maps_key_reads_as_the_string_it_is() {
    use std::collections::BTreeMap;
    agree::<BTreeMap<String, u8>>(json!({"1": 2, "true": 3}));
    agree::<BTreeMap<Key, u8>>(json!({"first": 1, "second": 2}));
    agree::<BTreeMap<Key, u8>>(json!({"third": 1}));
    agree::<BTreeMap<std::path::PathBuf, u8>>(json!({"a/b": 1}));
    // The keys `serde_json` parses from their strings, which no type the board reads has.
    let (objects, values) = both::<BTreeMap<u32, u8>>(json!({"1": 2}));
    assert_eq!(
        objects.unwrap_err(),
        "invalid type: string \"1\", expected u32"
    );
    assert_eq!(values.unwrap(), BTreeMap::from([(1, 2)]));
    let (objects, values) = both::<BTreeMap<bool, u8>>(json!({"true": 2}));
    assert_eq!(
        objects.unwrap_err(),
        "invalid type: string \"true\", expected a boolean"
    );
    assert_eq!(values.unwrap(), BTreeMap::from([(true, 2)]));
    let (objects, values) = both::<BTreeMap<Named, u8>>(json!({"a": 2}));
    assert_eq!(
        objects.unwrap_err(),
        "invalid type: string \"a\", expected tuple struct Named"
    );
    assert_eq!(values.unwrap(), BTreeMap::from([(Named("a".into()), 2)]));
}

/// `text` as [`parse`] reads it, then as `serde_json::from_str` does, its refusal dropped.
fn texts<T: DeserializeOwned>(text: &str) -> (Option<T>, Option<T>) {
    (parse(text), serde_json::from_str(text).ok())
}

#[test]
fn text_reads_as_serde_json_reads_it_and_is_dropped_where_it_does_not() {
    let binding = Binding {
        id: "res".into(),
        generation: "g1".into(),
    };
    let written = r#" {"id": "res", "generation": "g1"} "#;
    assert_eq!(
        texts::<Binding>(written),
        (Some(binding.clone()), Some(binding.clone()))
    );
    assert_eq!(parse::<Binding>(written.as_bytes()), Some(binding.clone()));
    for row in requests().into_iter().map(row) {
        let text = serde_json::to_string(&row).unwrap();
        let (parsed, direct) = texts::<Row>(&text);
        assert_eq!(parsed, direct, "{text}");
        assert_eq!(parsed, Some(row));
    }
    assert_eq!(
        texts::<Vec<(u8, Option<f64>)>>("[[1, 1.50], [2, null]]"),
        (
            Some(vec![(1, Some(1.5)), (2, None)]),
            Some(vec![(1, Some(1.5)), (2, None)])
        )
    );
    for unread in [
        "",
        "{",
        "null",
        "7",
        r#"{"id": "res"}"#,
        r#"{"id": 1, "generation": "g1"}"#,
        r#"{"id": "res", "generation": "g1"} {"id": "res", "generation": "g1"}"#,
        r#"{"id": "res", "generation": "g1"} trailing"#,
        "\u{feff}{\"id\": \"res\", \"generation\": \"g1\"}",
    ] {
        assert_eq!(texts::<Binding>(unread), (None, None), "{unread}");
    }
    assert_eq!(parse::<Binding>([0xff, 0xfe]), None);
}

#[test]
fn text_reads_differently_only_for_an_array_as_a_struct_and_a_field_said_twice() {
    // Its fields in an array's order: dropped here, read by `serde_json`.
    let (parsed, direct) = texts::<Binding>(r#"["res", "g1"]"#);
    assert_eq!(parsed, None);
    assert_eq!(direct.unwrap().id, "res");
    let (parsed, direct) = texts::<Vec<Binding>>(r#"[{"id": "a", "generation": "g"}, ["b", "g"]]"#);
    assert_eq!(parsed, None);
    assert_eq!(direct.unwrap().len(), 2);
    // A field said twice: the later one here, refused by `serde_json`.
    let twice = r#"{"id": "first", "generation": "g1", "id": "second"}"#;
    assert_eq!(
        serde_json::from_str::<Binding>(twice)
            .unwrap_err()
            .to_string(),
        "duplicate field `id` at line 1 column 40"
    );
    assert_eq!(parse::<Binding>(twice).unwrap().id, "second");
}

/// A recipient's row as a derived reader reads it: a field it needs, an option of a struct, a
/// number and a field with a default, beside the same row read by field ([`by_field`]).
#[derive(Debug, PartialEq, Derived)]
struct Recipient {
    handle: String,
    state: Option<Binding>,
    checked: f64,
    #[serde(default)]
    pending: bool,
}

/// `value` read as a list of [`Recipient`], item by item and field by field, as the dispatch's
/// reads of many recipients read their answers.
fn by_field(value: &Value) -> Result<Vec<Recipient>, String> {
    let row = |item: &Value| {
        let row = fields(item, "struct Recipient")?;
        Ok(Recipient {
            handle: row.field("handle")?,
            state: row.optional("state")?,
            checked: row.field("checked")?,
            pending: row.defaulted("pending")?,
        })
    };
    let rows: Result<Vec<Recipient>, serde_json::Error> =
        items(value).and_then(|items| items.iter().map(row).collect());
    rows.map_err(|e| e.to_string())
}

#[test]
fn a_list_read_by_field_reads_and_refuses_as_its_derived_reader() {
    let binding = json!({"id": "res", "generation": "g1"});
    for value in [
        json!([]),
        json!([{"handle": "a", "state": binding, "checked": 5, "pending": true}]),
        json!([{"handle": "a", "checked": 0.5}, {"handle": "b", "checked": 1.0e3}]),
        json!([{"handle": "a", "state": null, "checked": 1, "pending": false, "more": [1]}]),
        json!({"handle": "a", "checked": 1}),
        json!(null),
        json!("a"),
        json!(["a"]),
        json!([["a", null, 1, true]]),
        json!([{"handle": "a", "checked": 1}, 7]),
        json!([{"checked": 1}]),
        json!([{"handle": "a"}]),
        json!([{"handle": 1, "checked": 1}]),
        json!([{"handle": "a", "checked": "soon"}]),
        json!([{"handle": "a", "checked": null}]),
        json!([{"handle": "a", "checked": 1, "pending": null}]),
        json!([{"handle": "a", "checked": 1, "pending": "yes"}]),
        json!([{"handle": "a", "checked": 1, "state": {"id": "res"}}]),
        json!([{"handle": "a", "checked": 1, "state": ["res", "g1"]}]),
        json!([{"handle": "a", "checked": 1, "state": 3}]),
    ] {
        let derived = read::<Vec<Recipient>>(&value).map_err(|e| e.to_string());
        assert_eq!(by_field(&value), derived, "{value}");
    }
    assert_eq!(
        by_field(&json!([{"handle": "a", "checked": 2}])).unwrap(),
        [Recipient {
            handle: "a".into(),
            state: None,
            checked: 2.0,
            pending: false,
        }]
    );
    assert_eq!(
        by_field(&json!({"handle": "a"})).unwrap_err(),
        "invalid type: map, expected a sequence"
    );
    assert_eq!(
        by_field(&json!(["a"])).unwrap_err(),
        "invalid type: string \"a\", expected struct Recipient"
    );
}

#[test]
fn a_struct_read_by_field_refuses_the_first_wrong_field_it_asks_for() {
    // Two fields wrong: by field, the first one asked for; a derived reader, the first in the
    // object's order, which the board's build keeps as written (`preserve_order`).
    let two_wrong = json!([{"checked": "soon", "handle": 1}]);
    assert_eq!(
        by_field(&two_wrong).unwrap_err(),
        "invalid type: number, expected a string"
    );
    assert_eq!(
        read::<Vec<Recipient>>(&two_wrong).unwrap_err().to_string(),
        "invalid type: string \"soon\", expected f64"
    );
}
