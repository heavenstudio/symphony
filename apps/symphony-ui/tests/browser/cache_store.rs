//! The client cache's store (assets/cache.js) in a real browser: a kept read comes back under its
//! format and project, another format clears it, and the approved caps, the age rule and the
//! order decide what goes: the tasks opened last, the documents visited most
//! (docs/analysis/client-cache.md, "What is kept"; the owner's additions on the board task
//! perf/client-cache/build). The page loads the store alone,
//! so no release build is needed. The page's clock is set by each step (`t.at`), and V8's precise
//! coverage of the store is reported once every case has run.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::common;
use crate::site::Site;

const STORE: &str = include_str!("../../assets/cache.js");
const DAY: u64 = 86_400_000;
const MB: u64 = 1_000_000;

/// The page's helpers: its clock, a clean store, and the store's records read beside it.
const HELPERS: &str = r#"
window.t = {
    at(ms) { Date.now = () => ms; },
    db() {
        return new Promise(resolve => {
            const request = indexedDB.open("symphony-cache");
            // No store yet: make none, so the store still makes its own.
            request.onupgradeneeded = () => request.transaction.abort();
            request.onsuccess = () => resolve(request.result);
            request.onerror = () => resolve(null);
        });
    },
    async all(store) {
        const db = await t.db();
        if (!db) return [];
        const all = await new Promise(resolve => {
            const request = db.transaction(store).objectStore(store).getAll();
            request.onsuccess = () => resolve(request.result);
        });
        db.close();
        return all;
    },
    // A key's names, each written after its length, read back and joined by spaces.
    names(key) {
        const names = [];
        for (let at = 0; at < key.length;) {
            const colon = key.indexOf(":", at), end = colon + 1 + Number(key.slice(at, colon));
            names.push(key.slice(colon + 1, end));
            at = end;
        }
        return names.join(" ");
    },
    async items() {
        return Object.fromEntries((await t.all("items")).map(item => [t.names(item.key), item]));
    },
    async parts() { return (await t.all("parts")).map(part => t.names(part.key)); },
    async kept() { return Object.keys(await t.items()).sort(); },
    reset() {
        return new Promise(resolve => {
            const request = indexedDB.deleteDatabase("symphony-cache");
            // The store closes its connection when asked, so the deletion is never blocked.
            request.onsuccess = request.onerror = () => resolve();
        });
    },
};
"#;

/// A folder holding the store and the pages that load it: `/` as the shell would, and the others
/// in a browser whose IndexedDB is missing, refused, never answers or answers past the deadline.
fn pages() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sf-browser-cache-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/cache.js"), STORE).unwrap();
    std::fs::write(
        dir.join("index.html"),
        format!(
            "<!DOCTYPE html><html><head><script>{HELPERS}</script></head><body>\
             <script type=\"module\" src=\"/assets/cache.js\"></script></body></html>"
        ),
    )
    .unwrap();
    // No IndexedDB at all, one whose opening fails (storage denied, as a private window may), one
    // whose opening never answers, and the browser's own, its opening answered only when the test
    // says (`answerLate`), past the deadline.
    for (page, indexeddb) in [
        ("nodb.html", "undefined"),
        (
            "denied.html",
            "{ open() { const request = {}; setTimeout(() => request.onerror()); return request; } }",
        ),
        ("hung.html", "{ open() { return {}; } }"),
        (
            "late.html",
            "(() => { const real = indexedDB; return { open(...names) { \
             const request = real.open(...names), late = {}; \
             Object.defineProperty(late, \"result\", { get: () => request.result }); \
             request.onupgradeneeded = () => late.onupgradeneeded(); \
             request.onerror = () => late.onerror(); \
             request.onsuccess = () => { window.answerLate = () => late.onsuccess(); }; \
             return late; } }; })()",
        ),
    ] {
        std::fs::write(
            dir.join(page),
            format!(
                "<!DOCTYPE html><html><head><script>\
                 Object.defineProperty(window, \"indexedDB\", {{ value: {indexeddb} }});</script>\
                 </head><body><script type=\"module\" src=\"/assets/cache.js\"></script></body></html>"
            ),
        )
        .unwrap();
    }
    dir
}

/// `body` run as an async function in the page; what it returns.
fn run(chrome: &mut Chrome, body: &str) -> Value {
    chrome.eval(&format!(
        "(async () => {{ const C = window.symphonyCache; {body} }})()"
    ))
}

/// The items kept, `project id` each, in order.
fn items_kept(chrome: &mut Chrome) -> Vec<String> {
    serde_json::from_value(run(chrome, "return t.kept();")).unwrap()
}

/// A fresh store, at time `ms`.
fn fresh(chrome: &mut Chrome, ms: u64) {
    run(
        chrome,
        &format!("await t.reset(); t.at({ms}); return null;"),
    );
}

#[test]
fn the_store_keeps_reads_by_format_and_project_and_holds_the_approved_caps() {
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    let dir = pages();
    let site = Site::start(&dir, &common::fixture("board.json"));
    chrome.call("Profiler.enable", json!({}));
    chrome.call(
        "Profiler.startPreciseCoverage",
        json!({"callCount": true, "detailed": true}),
    );
    chrome.open(&site.url("/"));
    assert!(
        chrome.wait_for("window.symphonyCache && window.t", crate::SHOWN),
        "the store never loaded"
    );
    reads_come_back_under_their_format_and_project(&mut chrome);
    sizes_add_up_and_forgetting_drops_the_item_whole(&mut chrome);
    counts_are_capped_by_kind_and_project(&mut chrome);
    bytes_are_capped_by_kind_project_and_origin(&mut chrome);
    an_item_unused_for_fourteen_days_goes(&mut chrome);
    often_visited_pages_outlast_lately_visited_ones(&mut chrome);
    entrypoints_stay_while_the_project_has_room(&mut chrome);
    pages_read_ahead_go_before_visited_ones(&mut chrome);
    a_page_ranks_the_same_whether_kept_or_visited_first(&mut chrome);
    the_origin_drops_the_item_used_longest_ago(&mut chrome);
    an_item_last_used_by_a_clock_ahead_decays_and_ages_from_now(&mut chrome);
    a_part_saved_by_a_clock_ahead_reads_as_saved_now(&mut chrome);
    forgetting_an_item_leaves_items_whose_names_begin_with_its_own(&mut chrome);
    let mut ran = coverage(&mut chrome);
    for page in ["/nodb.html", "/denied.html", "/hung.html", "/late.html"] {
        chrome.open(&site.url(page));
        assert!(
            chrome.wait_for("window.symphonyCache", crate::SHOWN),
            "{page}"
        );
        if page != "/nodb.html" && page != "/denied.html" {
            an_open_that_does_not_answer_in_time_lets_the_page_go_on(&mut chrome);
        }
        if page == "/late.html" {
            a_database_that_opens_late_serves_the_calls_that_follow(&mut chrome);
        } else {
            without_indexeddb_every_read_misses_and_nothing_throws(&mut chrome);
        }
        for (at, also) in coverage(&mut chrome).into_iter().enumerate() {
            ran[at] |= also;
        }
    }
    report(&ran);
    drop(site);
    let _ = std::fs::remove_dir_all(dir);
}

fn reads_come_back_under_their_format_and_project(chrome: &mut Chrome) {
    fresh(chrome, 1_000);
    let said = run(
        chrome,
        r#"
        const kept = await C.keep(1, "alpha", "task:a", "task", "page", 'W/"aa"', {title: "A"}, undefined, false);
        return {
            kept,
            read: await C.read(1, "alpha", "task:a", "page"),
            another_project: await C.read(1, "beta", "task:a", "page"),
            another_part: await C.read(1, "alpha", "task:a", "history"),
            another_item: await C.read(1, "alpha", "task:b", "page"),
            items: await t.kept(),
        };
        "#,
    );
    assert_eq!(
        said,
        json!({
            "kept": true,
            "read": {"value": {"title": "A"}, "etag": "W/\"aa\"", "saved": 1000},
            "another_project": null, "another_part": null, "another_item": null,
            "items": ["alpha task:a"],
        })
    );
    // A client of another format clears the store as it opens it. A part another tab's client
    // writes under its own format afterwards is a miss for this one, and a read for that one.
    let said = run(
        chrome,
        r#"
        const cleared = await C.read(2, "alpha", "task:a", "page");
        const empty = await t.kept();
        await C.keep(1, "alpha", "task:b", "task", "page", 'W/"bb"', {title: "B"}, undefined, false);
        return {
            cleared, empty,
            other_format: await C.read(2, "alpha", "task:b", "page"),
            own_format: (await C.read(1, "alpha", "task:b", "page"))?.value,
        };
        "#,
    );
    assert_eq!(
        said,
        json!({"cleared": null, "empty": [], "other_format": null, "own_format": {"title": "B"}})
    );
}

fn sizes_add_up_and_forgetting_drops_the_item_whole(chrome: &mut Chrome) {
    fresh(chrome, 1_000);
    let said = run(
        chrome,
        r#"
        await C.keep(1, "alpha", "task:a", "task", "page", "", {}, 100, false);
        await C.keep(1, "alpha", "task:a", "task", "history", "", {}, 50, false);
        const both = (await t.items())["alpha task:a"].size;
        await C.keep(1, "alpha", "task:a", "task", "page", "", {}, 30, false);
        const replaced = (await t.items())["alpha task:a"].size;
        await C.keep(1, "alpha", "task:ab", "task", "page", "", {x: "abc"}, undefined, false);
        const measured = (await t.items())["alpha task:ab"].size;
        const forgot = await C.forget(1, "alpha", "task:a");
        return { both, replaced, measured, forgot, items: await t.kept(), parts: await t.parts() };
        "#,
    );
    assert_eq!(
        said,
        json!({
            "both": 150, "replaced": 80, "measured": "{\"x\":\"abc\"}".len(), "forgot": true,
            "items": ["alpha task:ab"], "parts": ["alpha task:ab page"],
        })
    );
    // A value with no JSON keeps nothing and throws nothing.
    let said = run(
        chrome,
        r#"return [await C.keep(1, "alpha", "task:c", "task", "page", "", undefined, undefined, false), await t.kept()];"#,
    );
    assert_eq!(said, json!([null, ["alpha task:ab"]]));
    // A keep whose transaction aborts (a full disk; here a value IndexedDB cannot clone) keeps
    // nothing of it, not even its item, and the store goes on.
    let said = run(
        chrome,
        r#"
        const aborted = await C.keep(1, "alpha", "task:ab", "task", "history", "", () => 1, 10, false);
        const after = await t.items();
        return {
            aborted, items: Object.keys(after), size: after["alpha task:ab"].size, parts: await t.parts(),
            next: await C.keep(1, "alpha", "task:d", "task", "page", "", {}, 10, false),
        };
        "#,
    );
    assert_eq!(
        said,
        json!({
            "aborted": null, "items": ["alpha task:ab"], "size": "{\"x\":\"abc\"}".len(),
            "parts": ["alpha task:ab page"], "next": true,
        })
    );
}

/// `count` items of `kind` in `project`, each opened once and kept, a second apart from `from`.
fn opened(chrome: &mut Chrome, project: &str, kind: &str, count: u64, from: u64) -> Value {
    run(
        chrome,
        &format!(
            r#"
            const kept = [];
            for (let i = 0; i < {count}; i += 1) {{
                t.at({from} + i * 1000);
                await C.visit(1, "{project}", `{kind}:${{i}}`, "{kind}", false);
                kept.push(await C.keep(1, "{project}", `{kind}:${{i}}`, "{kind}", "page", "", {{}}, 10, false));
            }}
            return kept;
            "#
        ),
    )
}

fn counts_are_capped_by_kind_and_project(chrome: &mut Chrome) {
    fresh(chrome, 0);
    assert_eq!(
        opened(chrome, "alpha", "task", 31, 0),
        json!(vec![true; 31])
    );
    assert_eq!(
        opened(chrome, "alpha", "document", 21, 0),
        json!(vec![true; 21])
    );
    assert_eq!(
        opened(chrome, "alpha", "board", 40, 0),
        json!(vec![true; 40])
    );
    assert_eq!(opened(chrome, "beta", "task", 30, 0), json!(vec![true; 30]));
    let kept = items_kept(chrome);
    let count = |prefix: &str| kept.iter().filter(|key| key.starts_with(prefix)).count();
    assert_eq!(
        (
            count("alpha task:"),
            count("alpha document:"),
            count("alpha board:"),
            count("beta task:")
        ),
        (30, 20, 40, 30),
        "{kept:?}"
    );
    for gone in ["alpha task:0", "alpha document:0"] {
        assert!(!kept.contains(&gone.to_string()), "{gone} outlived its cap");
    }
    for stays in [
        "alpha task:1",
        "alpha document:1",
        "alpha board:0",
        "beta task:0",
    ] {
        assert!(kept.contains(&stays.to_string()), "{stays} went");
    }
    let parts: Vec<String> = serde_json::from_value(run(chrome, "return t.parts();")).unwrap();
    assert!(
        !parts.contains(&"alpha task:0 page".to_string()),
        "its parts go with it"
    );
}

/// Keep one item of `size` bytes, opened at `at`; whether it is still kept.
fn sized(chrome: &mut Chrome, project: &str, id: &str, size: u64, at: u64) -> Value {
    let kind = id.split(':').next().unwrap();
    run(
        chrome,
        &format!(
            r#"
            t.at({at});
            await C.visit(1, "{project}", "{id}", "{kind}", false);
            return C.keep(1, "{project}", "{id}", "{kind}", "page", "", {{}}, {size}, false);
            "#
        ),
    )
}

fn bytes_are_capped_by_kind_project_and_origin(chrome: &mut Chrome) {
    // Tasks: 2 MB a project.
    fresh(chrome, 0);
    for (i, id) in ["task:a", "task:b", "task:c"].into_iter().enumerate() {
        assert_eq!(
            sized(chrome, "alpha", id, 700_000, i as u64 * 1000),
            json!(true)
        );
    }
    assert_eq!(items_kept(chrome), ["alpha task:b", "alpha task:c"]);
    // Documents: 6 MB a project.
    fresh(chrome, 0);
    for (i, id) in ["document:a", "document:b", "document:c", "document:d"]
        .into_iter()
        .enumerate()
    {
        sized(chrome, "alpha", id, 1_600_000, i as u64 * 1000);
    }
    assert_eq!(
        items_kept(chrome),
        ["alpha document:b", "alpha document:c", "alpha document:d"]
    );
    // A project: 8 MB, its tasks' and documents' caps together; its board is outside them, and
    // another project's items are not its.
    fresh(chrome, 0);
    sized(chrome, "alpha", "document:a", 5_900_000, 0);
    sized(chrome, "beta", "document:a", 5_900_000, 500);
    sized(chrome, "alpha", "task:a", 1_900_000, 1000);
    sized(chrome, "alpha", "board:", 3 * MB, 2000);
    assert_eq!(
        items_kept(chrome),
        [
            "alpha board:",
            "alpha document:a",
            "alpha task:a",
            "beta document:a"
        ]
    );
    // The origin: 32 MB over every project.
    fresh(chrome, 0);
    for (i, project) in ["p1", "p2", "p3", "p4", "p5"].into_iter().enumerate() {
        sized(chrome, project, "board:", 7 * MB, i as u64 * 1000);
    }
    assert_eq!(
        items_kept(chrome),
        ["p2 board:", "p3 board:", "p4 board:", "p5 board:"]
    );
    // An item larger alone than its caps is never kept, and pushes nothing else out.
    fresh(chrome, 0);
    sized(chrome, "alpha", "document:a", 1_000, 0);
    sized(chrome, "alpha", "task:a", 1_000, 0);
    for (id, size) in [
        ("document:big", 6 * MB + 1),
        ("task:big", 2 * MB + 1),
        ("board:", 32 * MB + 1),
    ] {
        assert_eq!(sized(chrome, "alpha", id, size, 1000), json!(false), "{id}");
    }
    assert_eq!(items_kept(chrome), ["alpha document:a", "alpha task:a"]);
    // At its cap exactly, an item is kept.
    assert_eq!(sized(chrome, "alpha", "task:a", 2 * MB, 2000), json!(true));
    // Items ranked alike (opened as often, at the same moment) go by name, whichever came last.
    fresh(chrome, 0);
    for id in ["document:c", "document:b"] {
        assert_eq!(sized(chrome, "alpha", id, 2_500_000, 0), json!(true));
    }
    assert_eq!(
        sized(chrome, "alpha", "document:a", 2_500_000, 0),
        json!(false)
    );
    assert_eq!(items_kept(chrome), ["alpha document:b", "alpha document:c"]);
}

fn an_item_unused_for_fourteen_days_goes(chrome: &mut Chrome) {
    fresh(chrome, 0);
    // In clock order: an item used at a moment the clock then went back from is used again then.
    sized(chrome, "alpha", "board:", 10, 0);
    sized(chrome, "beta", "task:a", 10, 0);
    sized(chrome, "alpha", "document:a", 10, 1);
    // Exactly fourteen days unused is still kept; a moment more is not, in every project.
    sized(chrome, "alpha", "task:new", 10, 14 * DAY);
    assert_eq!(items_kept(chrome).len(), 4);
    sized(chrome, "alpha", "task:newer", 10, 14 * DAY + 1);
    assert_eq!(
        items_kept(chrome),
        ["alpha document:a", "alpha task:new", "alpha task:newer"]
    );
    // A visit renews it.
    run(
        chrome,
        &format!(
            r#"t.at({}); await C.visit(1, "alpha", "document:a", "document", false); return null;"#,
            14 * DAY
        ),
    );
    sized(chrome, "alpha", "task:later", 10, 28 * DAY);
    assert!(items_kept(chrome).contains(&"alpha document:a".to_string()));
}

fn often_visited_pages_outlast_lately_visited_ones(chrome: &mut Chrome) {
    fresh(chrome, 0);
    // Opened five times on day 0, then nineteen others once each on day 1.
    run(
        chrome,
        r#"
        for (let i = 0; i < 5; i += 1) { t.at(i * 1000); await C.visit(1, "alpha", "document:often", "document", false); }
        return null;
        "#,
    );
    opened(chrome, "alpha", "document", 19, DAY);
    // A twenty-first on day 2: the least visited of the once-opened goes, not the often one.
    sized(chrome, "alpha", "document:new", 10, 2 * DAY);
    let kept = items_kept(chrome);
    assert!(
        kept.contains(&"alpha document:often".to_string()),
        "{kept:?}"
    );
    assert!(!kept.contains(&"alpha document:0".to_string()), "{kept:?}");
    // Each visit adds one to the count decayed since the last: five visits seconds apart.
    let score = run(
        chrome,
        r#"return (await t.items())["alpha document:often"].score;"#,
    )
    .as_f64()
    .unwrap();
    assert!((4.99..5.0).contains(&score), "{score}");
    // Visited twice on day 0 against once on day 4: by day 5 the recent one ranks higher.
    fresh(chrome, 0);
    run(
        chrome,
        r#"
        for (const at of [0, 1000]) { t.at(at); await C.visit(1, "alpha", "document:old", "document", false); }
        return null;
        "#,
    );
    opened(chrome, "alpha", "document", 19, 4 * DAY);
    sized(chrome, "alpha", "document:new", 10, 5 * DAY);
    let kept = items_kept(chrome);
    assert!(
        !kept.contains(&"alpha document:old".to_string()),
        "{kept:?}"
    );
    assert!(kept.contains(&"alpha document:0".to_string()), "{kept:?}");
}

fn entrypoints_stay_while_the_project_has_room(chrome: &mut Chrome) {
    fresh(chrome, 0);
    run(
        chrome,
        r#"await C.visit(1, "alpha", "document:docs/", "document", true); return null;"#,
    );
    opened(chrome, "alpha", "document", 20, DAY);
    let kept = items_kept(chrome);
    assert!(
        kept.contains(&"alpha document:docs/".to_string()),
        "{kept:?}"
    );
    assert!(!kept.contains(&"alpha document:0".to_string()), "{kept:?}");
    // Entrypoints alone over the cap: the less visited of them goes.
    fresh(chrome, 0);
    run(
        chrome,
        r#"
        for (const [id, at] of [["document:docs/", 0], ["document:docs/", 1000], ["document:guide/", 2000]]) {
            t.at(at);
            await C.visit(1, "alpha", id, "document", true);
            await C.keep(1, "alpha", id, "document", "outline", "", {}, 3500000, false);
        }
        return null;
        "#,
    );
    assert_eq!(items_kept(chrome), ["alpha document:docs/"]);
}

fn pages_read_ahead_go_before_visited_ones(chrome: &mut Chrome) {
    fresh(chrome, 0);
    opened(chrome, "alpha", "document", 19, 0);
    let said = run(
        chrome,
        r#"
        t.at(100000);
        const ahead = await C.keep(1, "alpha", "document:ahead", "document", "outline", "", {}, 10, true);
        const record = (await t.items())["alpha document:ahead"];
        return { ahead, visited: record.visited, score: record.score };
        "#,
    );
    assert_eq!(said, json!({"ahead": true, "visited": false, "score": 0}));
    // The next visited page fills the cap: the page read ahead goes, though it is the newest.
    sized(chrome, "alpha", "document:opened", 10, 200_000);
    let kept = items_kept(chrome);
    assert!(
        !kept.contains(&"alpha document:ahead".to_string()),
        "{kept:?}"
    );
    assert!(kept.contains(&"alpha document:0".to_string()), "{kept:?}");
    // Into a full cap, a page read ahead is not kept at all.
    assert_eq!(
        run(
            chrome,
            r#"t.at(300000); return C.keep(1, "alpha", "document:more", "document", "outline", "", {}, 10, true);"#,
        ),
        json!(false)
    );
    // Opened, a page read ahead counts as visited; reading a visited page ahead keeps it visited.
    fresh(chrome, 0);
    let said = run(
        chrome,
        r#"
        await C.keep(1, "alpha", "task:a", "task", "page", "", {}, 10, true);
        await C.visit(1, "alpha", "task:a", "task", false);
        await C.keep(1, "alpha", "task:a", "task", "history", "", {}, 10, true);
        const record = (await t.items())["alpha task:a"];
        return { visited: record.visited, score: record.score, size: record.size };
        "#,
    );
    assert_eq!(said, json!({"visited": true, "score": 1, "size": 20}));
}

/// Open the new page `KIND:new` at `at`: its visit and its first keep, in the order `steps` names.
/// What each answered, and its record and parts when it is kept.
fn open_new_page(chrome: &mut Chrome, kind: &str, steps: [&str; 2], at: [u64; 2]) -> Value {
    let calls: Vec<String> = steps
        .iter()
        .zip(at)
        .map(|(step, at)| {
            let call = match *step {
                "keep" => {
                    format!(
                        r#"C.keep(1, "alpha", "{kind}:new", "{kind}", "page", "", {{}}, 10, false)"#
                    )
                }
                _ => format!(r#"C.visit(1, "alpha", "{kind}:new", "{kind}", false)"#),
            };
            format!("t.at({at}); said.push(await {call});")
        })
        .collect();
    run(
        chrome,
        &format!(
            r#"
            const said = [];
            {}
            const record = (await t.items())["alpha {kind}:new"];
            const parts = (await t.parts()).filter(part => part.startsWith("alpha {kind}:new"));
            return {{ said, score: record?.score ?? null, size: record?.size ?? null, parts }};
            "#,
            calls.join("\n")
        ),
    )
}

/// `count` items of `kind`, item `i` opened three times from `i` seconds on.
fn opened_often(chrome: &mut Chrome, kind: &str, count: u64) {
    run(
        chrome,
        &format!(
            r#"
            for (let i = 0; i < {count}; i += 1) for (const at of [0, 100, 200]) {{
                t.at(i * 1000 + at);
                await C.visit(1, "alpha", `{kind}:${{i}}`, "{kind}", false);
            }}
            return null;
            "#
        ),
    );
}

/// A page's visit and its first keep run apart, in either order: its item counts the visit from
/// the first of them, so it ranks the same and the caps decide the same after both. A new task
/// always enters, as the one opened longest ago goes; a new document enters only past the documents
/// visited less.
fn a_page_ranks_the_same_whether_kept_or_visited_first(chrome: &mut Chrome) {
    let mut after = Vec::new();
    for steps in [["keep", "visit"], ["visit", "keep"]] {
        // Into a full cap of documents opened once: the new one stays, the least visited goes.
        fresh(chrome, 0);
        opened(chrome, "alpha", "document", 20, 0);
        assert_eq!(
            open_new_page(chrome, "document", steps, [100_000, 100_000]),
            json!({"said": [true, true], "score": 1, "size": 10, "parts": ["alpha document:new page"]}),
            "{steps:?}"
        );
        let kept = items_kept(chrome);
        assert_eq!(kept.len(), 20, "{steps:?}: {kept:?}");
        assert!(
            !kept.contains(&"alpha document:0".to_string()),
            "{steps:?}: {kept:?}"
        );
        after.push(kept);
        // Into a full cap of documents opened often: the new one ranks lowest and is not kept,
        // and pushes nothing out, its visit no more than its keep.
        fresh(chrome, 0);
        opened_often(chrome, "document", 20);
        assert_eq!(
            open_new_page(chrome, "document", steps, [100_000, 100_000]),
            json!({"said": [false, false], "score": null, "size": null, "parts": []}),
            "{steps:?}"
        );
        assert_eq!(items_kept(chrome).len(), 20, "{steps:?}");
        // Into a full cap of tasks opened often: the new one stays, and the one opened longest
        // ago goes, however often it was.
        fresh(chrome, 0);
        opened_often(chrome, "task", 30);
        assert_eq!(
            open_new_page(chrome, "task", steps, [100_000, 100_000]),
            json!({"said": [true, true], "score": 1, "size": 10, "parts": ["alpha task:new page"]}),
            "{steps:?}"
        );
        let kept = items_kept(chrome);
        assert_eq!(kept.len(), 30, "{steps:?}: {kept:?}");
        assert!(
            !kept.contains(&"alpha task:0".to_string()),
            "{steps:?}: {kept:?}"
        );
        assert!(
            kept.contains(&"alpha task:1".to_string()),
            "{steps:?}: {kept:?}"
        );
    }
    assert_eq!(after[0], after[1], "either order keeps the same items");
    // A keep's visit is counted only for the visit that follows it at once: a later one counts.
    fresh(chrome, 0);
    let said = open_new_page(chrome, "task", ["keep", "visit"], [0, 2 * 60_000]);
    let score = said["score"].as_f64().unwrap();
    assert!((1.99..2.0).contains(&score), "{said}");
}

/// Over the origin's cap the item used longest ago goes, however often it was visited: the visits
/// weigh only a project's documents against each other.
fn the_origin_drops_the_item_used_longest_ago(chrome: &mut Chrome) {
    fresh(chrome, 0);
    run(
        chrome,
        r#"
        for (const at of [0, 100, 200]) { t.at(at); await C.visit(1, "p0", "document:often", "document", false); }
        return C.keep(1, "p0", "document:often", "document", "page", "", {}, 5900000, false);
        "#,
    );
    for (i, project) in ["p1", "p2", "p3", "p4", "p5"].into_iter().enumerate() {
        sized(
            chrome,
            project,
            "document:a",
            5_900_000,
            (i as u64 + 1) * 1000,
        );
    }
    assert_eq!(
        items_kept(chrome),
        [
            "p1 document:a",
            "p2 document:a",
            "p3 document:a",
            "p4 document:a",
            "p5 document:a"
        ]
    );
}

/// An item last used by a clock ahead of the page's (a clock set back since) is taken as used now,
/// so it decays and ages from now instead of staying first until the clock catches up.
fn an_item_last_used_by_a_clock_ahead_decays_and_ages_from_now(chrome: &mut Chrome) {
    fresh(chrome, 0);
    sized(chrome, "alpha", "task:ahead", 10, 10 * DAY);
    sized(chrome, "alpha", "task:now", 10, 1000);
    let used = run(
        chrome,
        r#"const items = await t.items(); return [items["alpha task:ahead"].used, items["alpha task:now"].used];"#,
    );
    assert_eq!(used, json!([1000, 1000]));
    sized(chrome, "alpha", "task:later", 10, 14 * DAY + 1001);
    assert_eq!(
        items_kept(chrome),
        ["alpha task:later"],
        "both unused for fourteen days since"
    );
}

/// A part saved by a clock ahead of the page's reads as saved now, never in the future, so a page
/// that ages what it drew by it never says it was saved later than now; once the clock passes
/// that moment, the part reads as saved then.
fn a_part_saved_by_a_clock_ahead_reads_as_saved_now(chrome: &mut Chrome) {
    fresh(chrome, 0);
    let said = run(
        chrome,
        &format!(
            r#"
            t.at({ahead});
            await C.keep(1, "alpha", "task:a", "task", "page", "", {{}}, 10, false);
            const saved = [];
            for (const at of [1000, {ahead}, {after}]) {{
                t.at(at);
                saved.push((await C.read(1, "alpha", "task:a", "page")).saved);
            }}
            return saved;
            "#,
            ahead = 10 * DAY,
            after = 11 * DAY
        ),
    );
    assert_eq!(said, json!([1000, 10 * DAY, 10 * DAY]));
}

/// Names are written whole in a key: forgetting an item forgets no item whose name begins with
/// its own, whatever the names hold.
fn forgetting_an_item_leaves_items_whose_names_begin_with_its_own(chrome: &mut Chrome) {
    fresh(chrome, 0);
    let said = run(
        chrome,
        r#"
        const ids = ["document:a", "document:a\nb", "document:a1", "document:a:2"];
        for (const id of ids) await C.keep(1, "alpha", id, "document", "outline", "", {id}, 10, false);
        await C.keep(1, "alpha\ndocument:a", "x", "document", "outline", "", {}, 10, false);
        await C.forget(1, "alpha", "document:a");
        const read = [];
        for (const id of ids) read.push((await C.read(1, "alpha", id, "outline"))?.value.id ?? null);
        return { read, parts: (await t.parts()).length, items: await t.kept() };
        "#,
    );
    assert_eq!(
        said,
        json!({
            "read": [null, "document:a\nb", "document:a1", "document:a:2"],
            "parts": 4,
            "items": ["alpha\ndocument:a x", "alpha document:a\nb", "alpha document:a1", "alpha document:a:2"],
        })
    );
}

/// An open that does not answer within its deadline: the page's first call waits for it, no
/// longer, and gets nothing.
fn an_open_that_does_not_answer_in_time_lets_the_page_go_on(chrome: &mut Chrome) {
    let said = run(
        chrome,
        r#"
        const started = performance.now();
        const read = await C.read(1, "alpha", "task:a", "page");
        return { read, waited: performance.now() - started };
        "#,
    );
    assert_eq!(said["read"], json!(null));
    let waited = said["waited"].as_f64().unwrap();
    assert!((490.0..5_000.0).contains(&waited), "waited {waited} ms");
}

/// A database that opens past the deadline: the calls before it went on without the cache, and
/// the calls after it, with no reload between (the shell changes pages without one), keep and read
/// through it.
fn a_database_that_opens_late_serves_the_calls_that_follow(chrome: &mut Chrome) {
    assert!(
        chrome.wait_for("typeof window.answerLate === \"function\"", crate::SHOWN),
        "the browser's database never opened"
    );
    let said = run(
        chrome,
        r#"
        const before = await C.keep(1, "late", "task:a", "task", "page", 'W/"l"', {title: "late"}, undefined, false);
        window.answerLate();
        const after = await C.keep(1, "late", "task:a", "task", "page", 'W/"l"', {title: "late"}, undefined, false);
        return { before, after, read: (await C.read(1, "late", "task:a", "page"))?.value ?? null };
        "#,
    );
    assert_eq!(
        said,
        json!({"before": null, "after": true, "read": {"title": "late"}})
    );
}

fn without_indexeddb_every_read_misses_and_nothing_throws(chrome: &mut Chrome) {
    let said = run(
        chrome,
        r#"
        return [
            await C.keep(1, "alpha", "task:a", "task", "page", "", {}, 10, false),
            await C.visit(1, "alpha", "task:a", "task", false),
            await C.read(1, "alpha", "task:a", "page"),
            await C.forget(1, "alpha", "task:a"),
        ];
        "#,
    );
    assert_eq!(said, json!([null, null, null, null]));
}

/// Which of the store's bytes ran, by V8's precise block coverage since it was last taken: of
/// every copy of the store the page's isolate still holds (a page left behind may keep its own).
fn coverage(chrome: &mut Chrome) -> Vec<bool> {
    let taken = chrome.call("Profiler.takePreciseCoverage", json!({}));
    let scripts: Vec<&Value> = taken["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|script| {
            script["url"]
                .as_str()
                .unwrap_or("")
                .ends_with("/assets/cache.js")
        })
        .collect();
    assert!(!scripts.is_empty(), "no coverage of the store: {taken}");
    let mut ran = vec![false; STORE.len()];
    for script in scripts {
        for (at, counted) in counts(script).into_iter().enumerate() {
            ran[at] |= counted > 0;
        }
    }
    ran
}

/// One script's count at each byte: the innermost range holding it sets it.
fn counts(script: &Value) -> Vec<u64> {
    let mut ranges: Vec<(usize, usize, u64)> = script["functions"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|function| function["ranges"].as_array().unwrap().iter())
        .map(|range| {
            (
                range["startOffset"].as_u64().unwrap() as usize,
                range["endOffset"].as_u64().unwrap() as usize,
                range["count"].as_u64().unwrap(),
            )
        })
        .collect();
    // Outer ranges first, so an inner block's count overrides its function's. The store is ASCII
    // (a host test holds it so), so V8's UTF-16 offsets are its byte offsets.
    ranges.sort_by_key(|&(start, end, _)| (start, std::cmp::Reverse(end)));
    let mut counts = vec![0_u64; STORE.len()];
    for (start, end, count) in ranges {
        counts[start.min(STORE.len())..end.min(STORE.len())].fill(count);
    }
    counts
}

/// Report the store's code lines that ran whole, in part (with the code that never ran) and never;
/// a store entry point or its eviction that never ran fails.
fn report(ran: &[bool]) {
    let mut whole = 0;
    let mut part = Vec::new();
    let mut never = Vec::new();
    let mut entered = Vec::new();
    let mut offset = 0;
    for (number, line) in STORE.split_inclusive('\n').enumerate() {
        let code = line.trim();
        let start = offset;
        offset += line.len();
        if code.is_empty() || code.starts_with("//") || matches!(code, "}" | "};") {
            continue;
        }
        let cells: Vec<(char, bool)> = line
            .char_indices()
            .filter(|(_, c)| !c.is_whitespace())
            .map(|(at, c)| (c, ran[start + at]))
            .collect();
        match cells.iter().filter(|(_, ran)| *ran).count() {
            0 => never.push((number + 1, code)),
            all if all == cells.len() => whole += 1,
            _ => part.push((number + 1, not_run(line, &ran[start..offset]))),
        }
        if cells.iter().any(|(_, ran)| *ran) {
            entered.push(code);
        }
    }
    eprintln!(
        "cache.js coverage: {} code lines, {whole} ran whole, {} in part, {} never; in part, what never ran: {part:?}; never: {never:?}",
        whole + part.len() + never.len(),
        part.len(),
        never.len()
    );
    for name in [
        "read:",
        "keep:",
        "visit:",
        "forget:",
        "function plan(",
        "function evict(",
        "function drop(",
    ] {
        assert!(
            entered.iter().any(|code| code.starts_with(name)),
            "{name} never ran"
        );
    }
}

/// The spans of `line` whose bytes never ran, trimmed.
fn not_run(line: &str, ran: &[bool]) -> Vec<String> {
    let mut spans = Vec::new();
    let mut span = String::new();
    for (at, c) in line.char_indices() {
        if ran[at] {
            if !span.trim().is_empty() {
                spans.push(span.trim().to_string());
            }
            span.clear();
        } else {
            span.push(c);
        }
    }
    if !span.trim().is_empty() {
        spans.push(span.trim().to_string());
    }
    spans
}
