//! The wire format, pinned by the fixtures the client's tests and the server's tests share:
//! each fixture reads into the types and writes back as the same JSON.

use serde_json::Value;
use symphony_api::{Board, Change, Delta, Front};
#[path = "wire/fixture.rs"]
mod fixture;
#[path = "wire/machine_work.rs"]
mod machine_work;

fn fixture(name: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reader =
        repository_read::Reader::process(root.clone()).expect("a valid repository audit context");
    fixture::read(&reader, &root, name).expect(name)
}

/// The fixture read into `T` and written back, as JSON values, beside the fixture itself.
fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(name: &str) -> (Value, Value) {
    let text = fixture(name);
    let typed: T = serde_json::from_str(&text).expect(name);
    let written = serde_json::to_value(&typed).expect("a wire type serializes");
    (written, serde_json::from_str(&text).expect(name))
}

#[test]
fn the_board_reads_and_writes_back_as_the_same_json() {
    let (written, fixture) = round_trip::<Board>("board.json");
    assert_eq!(written, fixture);
}

#[test]
fn the_board_names_the_client_build_served_and_how_often_the_feed_pings() {
    let board: Board = serde_json::from_str(&fixture("board.json")).unwrap();
    assert_eq!(board.build, "/assets/symphony-ui-dxh0123456789abcdef.js");
    assert_eq!(board.ping_ms, 5_000);
    assert!(board.ping_ms <= symphony_api::PING_MAX_MS);
}

/// The owner reads the board from a phone over the tailnet, where a stream goes silent but open on
/// every network change: a client gives such a stream up after two and a half pings, which no
/// server may stretch past 12.5 s (ruled on tools/python-zero/ui-board).
#[test]
fn no_server_lets_a_silent_stream_hold_past_twelve_and_a_half_seconds() {
    let given_up_ms = u64::from(symphony_api::PING_MAX_MS) * 5 / 2;
    assert!(
        given_up_ms <= 12_500,
        "a silent stream is given up only after {given_up_ms} ms"
    );
}

#[test]
fn an_empty_board_without_a_task_board_reads_and_writes_back_as_the_same_json() {
    let (written, fixture) = round_trip::<Board>("board-empty.json");
    assert_eq!(written, fixture);
    let board: Board = serde_json::from_str(&self::fixture("board-empty.json")).unwrap();
    assert_eq!(board.next, Vec::<symphony_api::Card>::new());
    assert_eq!(
        board.counts.tasks, None,
        "a server with no task board sends null"
    );
}

#[test]
fn every_region_of_a_delta_reads_and_writes_back_as_the_same_json() {
    for name in ["delta-card.json", "delta-regions.json"] {
        let (written, fixture) = round_trip::<Delta>(name);
        assert_eq!(written, fixture, "{name}");
    }
    let delta: Delta = serde_json::from_str(&fixture("delta-regions.json")).unwrap();
    let regions: Vec<&str> = delta
        .changes
        .iter()
        .map(|change| match change {
            Change::Card(_) => "card",
            Change::Queue(_) => "queue",
            Change::Counts(_) => "counts",
            Change::Progress(_) => "progress",
            Change::Client { .. } => "client",
            Change::Other => "other",
        })
        .collect();
    assert_eq!(regions, ["queue", "counts", "progress", "client"]);
}

#[test]
fn a_card_links_to_its_anchor_in_its_document() {
    let board: Board = serde_json::from_str(&fixture("board.json")).unwrap();
    assert_eq!(
        board.next[3].href(),
        "/docs/game/STATIONS.md#dock-menu-order"
    );
}

#[test]
fn a_card_without_pictures_summary_or_date_leaves_them_out() {
    let board: Board = serde_json::from_str(&fixture("board.json")).unwrap();
    let card = serde_json::to_value(&board.next[2]).unwrap();
    assert!(
        card.get("summary").is_none(),
        "an empty summary is not sent: {card}"
    );
    let bare = serde_json::to_value(&board.next[1]).unwrap();
    assert!(
        bare.get("pictures").is_none(),
        "no pictures, no field: {bare}"
    );
    let lantern = &board.next[4].pictures[2];
    assert_eq!(
        (lantern.src.as_deref(), lantern.waiting),
        (None, false),
        "a lamp not drawn yet"
    );
}

#[test]
fn a_region_this_client_does_not_know_yet_reads_as_other() {
    let text = r#"{"version": 9, "changes": [
        {"region": "activity", "data": [{"kind": "comment"}]},
        {"region": "counts", "data": {"waiting": 1, "docs_waiting": 1, "spot_check": 0, "answered_today": 0}}
    ]}"#;
    let delta: Delta =
        serde_json::from_str(text).expect("an unknown region must not refuse the event");
    assert_eq!(delta.changes[0], Change::Other);
    assert!(
        matches!(&delta.changes[1], Change::Counts(counts) if counts.waiting == 1 && counts.tasks.is_none())
    );
}

#[test]
fn a_change_names_its_region_before_its_data_and_a_known_region_carries_data() {
    let data_first = r#"{"version": 9, "changes": [{"data": {"build": "x"}, "region": "client"}]}"#;
    let err = serde_json::from_str::<Delta>(data_first)
        .unwrap_err()
        .to_string();
    assert!(err.contains("names its region before its data"), "{err}");
    let no_data = r#"{"version": 9, "changes": [{"region": "counts"}]}"#;
    let err = serde_json::from_str::<Delta>(no_data)
        .unwrap_err()
        .to_string();
    assert!(err.contains("the counts region has no data"), "{err}");
    let unknown_bare = r#"{"version": 9, "changes": [{"region": "activity"}]}"#;
    assert_eq!(
        serde_json::from_str::<Delta>(unknown_bare).unwrap().changes,
        [Change::Other]
    );
}

#[test]
fn a_front_that_does_not_say_whether_it_has_counted_reads_as_counted() {
    let without = r#"{"pid": 7, "port": 8765, "root": "/r", "ready": true, "app": null,
        "tailnet": null, "orphans": 2}"#;
    let front: Front = serde_json::from_str(without).expect("a server without the field answers");
    assert!(!front.uncounted, "a start would wait on a count never sent");
    assert!(!front.census_pending);
    assert_eq!(front.orphans, 2);
    let with: Front = serde_json::from_str(
        &without.replace('}', r#", "uncounted": true, "census_pending": true}"#),
    )
    .unwrap();
    assert!(with.uncounted);
    assert!(with.census_pending);
}

#[test]
fn the_project_the_page_inlines_reads_and_writes_back_as_the_same_json() {
    let (written, fixture) = round_trip::<symphony_api::ProjectView>("project.json");
    assert_eq!(written, fixture);
    let plain: symphony_api::ProjectView =
        serde_json::from_str(r#"{"name":"Orbit","register":"notes/register.md"}"#).unwrap();
    assert!(
        plain.nav.is_empty(),
        "a project without plugins has no pages of theirs"
    );
    assert_eq!(plain.entrypoints(), ["docs"], "none sent is the convention");
}

#[test]
fn a_route_is_a_document_page_under_an_entrypoint_unless_the_symphony_answers_it() {
    let project: symphony_api::ProjectView =
        serde_json::from_str(&fixture("project.json")).unwrap();
    for (path, page) in [
        ("docs", true),
        ("docs/", true),
        ("docs/game", true),
        ("docs/game/SPEC.md", true),
        ("docs/pictures/a.png", false),
        ("docs/v1.0", false),
        ("docs/v1.0/", true),
        ("docs/pictures/a.png/", true),
        ("docsx/A.md", false),
        ("", false),
        ("tasks", false),
        ("tasks/", false),
        ("lamps", false),
        ("README.md", false),
    ] {
        assert_eq!(project.is_document_route(path), page, "{path}");
    }
    let dotted = symphony_api::ProjectView {
        entrypoints: vec!["v1.0".into()],
        ..project.clone()
    };
    for (path, page) in [
        ("v1.0", true),
        ("v1.0/", true),
        ("v1.0/A.md", true),
        ("v1.0/a.png", false),
        ("v1.0x", false),
    ] {
        assert_eq!(dotted.is_document_route(path), page, "{path}");
    }
    // a page keeps its kind: a folder whatever its name, a document only as Markdown with no slash
    let markdown_named = symphony_api::ProjectView {
        entrypoints: vec!["guide.md".into(), "docs".into()],
        ..project.clone()
    };
    for (path, folder) in [
        ("guide.md", true),
        ("guide.md/", true),
        ("guide.md/A.md", false),
        ("guide.md/old.md/", true),
        ("docs", true),
        ("docs/game", true),
        ("docs/game/SPEC.md", false),
        ("docs/a.png", false),
        ("tasks/", false),
    ] {
        assert_eq!(markdown_named.is_folder_route(path), folder, "{path}");
    }
    assert_eq!(project.documents_href(), "/docs/");
    assert_eq!(project.readme_route(), "docs/README.md");
    let spec = symphony_api::ProjectView {
        entrypoints: vec![String::new(), "notes".into()],
        ..project
    };
    for (path, page) in [
        ("", true),
        ("index.md", true),
        ("guide", true),
        ("guide/START.md", true),
        ("notes/A.md", true),
        ("artifacts/A.md", true),
        ("pictures/a.png", false),
        ("tasks", false),
        ("task/a/b", false),
        ("agents", false),
        ("queue", false),
        ("api/docs", false),
        ("artifacts/uploads/a.md", false),
        ("favicon.ico", false),
        ("lamps", false),
        ("lampsx", true),
    ] {
        assert_eq!(spec.is_document_route(path), page, "{path}");
    }
    assert_eq!(spec.documents_href(), "/");
    assert_eq!(spec.readme_route(), "README.md");
}
