use super::*;

const STORE: &str = include_str!("../../assets/cache.js");
const BRIDGE: &str = include_str!("../cache.rs");

#[test]
fn an_item_is_named_by_its_kind_and_scope() {
    for (item, kind, id, pinned) in [
        (Item::board(""), Kind::Board, "board:", false),
        (Item::board("work~x"), Kind::Board, "board:@work~x/", false),
        (Item::task("", "perf/x"), Kind::Task, "task:perf/x", false),
        (
            Item::task("work~x", "perf/x"),
            Kind::Task,
            "task:@work~x/perf/x",
            false,
        ),
        (
            Item::document("docs/X.md", false),
            Kind::Document,
            "document:docs/X.md",
            false,
        ),
        (
            Item::document("@work~x/docs/", true),
            Kind::Document,
            "document:@work~x/docs/",
            true,
        ),
    ] {
        assert_eq!(
            item,
            Item {
                kind,
                id: id.into(),
                pinned
            }
        );
    }
}

#[test]
fn items_of_other_kinds_or_scopes_never_share_a_name() {
    let items = [
        Item::board(""),
        Item::board("board"),
        Item::task("", "board:"),
        Item::task("", "x"),
        Item::task("x", "x"),
        Item::document("x", false),
        Item::document("task:x", false),
    ];
    for (at, item) in items.iter().enumerate() {
        for other in &items[at + 1..] {
            assert_ne!(item.id, other.id);
        }
    }
}

/// The store's caps by kind are the approved ones, under the names this side gives its kinds.
#[test]
fn the_stores_caps_are_the_approved_ones_by_this_sides_kind_names() {
    let caps = format!(
        "const CAPS = {{ {}: [30, 2 * MB], {}: [20, 6 * MB] }};",
        Kind::Task.name(),
        Kind::Document.name()
    );
    for line in [
        caps.as_str(),
        "const PROJECT = 8 * MB, ORIGIN = 32 * MB, AGE = 14 * DAY;",
        "const MB = 1e6, DAY = 864e5;",
    ] {
        assert!(STORE.contains(line), "cache.js holds {line}");
    }
    assert!(
        !STORE.contains(&format!("{}: [", Kind::Board.name())),
        "the board's reads have no cap of their own"
    );
}

/// The parameter names of the store's function `name` in cache.js.
fn store_parameters(name: &str) -> Vec<&'static str> {
    let at = STORE
        .find(&format!("{name}: ("))
        .unwrap_or_else(|| panic!("cache.js has {name}"));
    let list = &STORE[at + name.len() + 3..];
    list[..list.find(')').unwrap()].split(", ").collect()
}

/// The parameter names of this side's binding to the store's `name`.
fn bridge_parameters(name: &str) -> Vec<&'static str> {
    let at = BRIDGE
        .find(&format!("fn store_{name}("))
        .unwrap_or_else(|| panic!("cache.rs binds {name}"));
    let list = &BRIDGE[at + name.len() + 10..];
    list[..list.find(')').unwrap()]
        .split(',')
        .filter_map(|parameter| parameter.split(':').next())
        .map(str::trim)
        .filter(|parameter| !parameter.is_empty())
        .collect()
}

/// wasm-bindgen passes arguments by position: each binding names the store's in its order.
#[test]
fn each_binding_passes_the_stores_parameters_in_its_order() {
    for name in ["read", "keep", "visit", "forget"] {
        assert_eq!(bridge_parameters(name), store_parameters(name), "{name}");
        assert!(
            BRIDGE.contains(&format!("js_name = {name}, catch")),
            "{name} is bound by its name and never throws into the page"
        );
    }
    assert_eq!(store_parameters("forget"), ["format", "project", "id"]);
}

/// Every first-visit byte of the store is in the shell, which every page kind loads: the store
/// is the shell's first part, and nothing else in the client opens IndexedDB.
#[test]
fn the_shell_carries_the_store_and_no_page_kind_does() {
    let package = include_str!("../../package.sh");
    assert!(
        package.contains("cat assets/cache.js"),
        "package.sh puts the store in the shell"
    );
    assert!(STORE.is_ascii(), "coverage offsets count the store's bytes");
    assert!(
        !BRIDGE.contains("web_sys::Idb"),
        "no IndexedDB binding in Rust"
    );
}
