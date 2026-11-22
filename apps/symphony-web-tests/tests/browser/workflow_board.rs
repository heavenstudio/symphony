//! A custom workflow in the real client, wide and on a phone (docs/workflow/WORKFLOW.md, "The app"). The
//! board's columns are its phases by their order (Build, then two review stages, Review and
//! Verify), a phone's tabs the phases by their phone order, each held card names its place, and a
//! state chip filters the board by it. The task page offers the server's next actions, one of them
//! unavailable naming whom it needs. It sends a task back from its first review with its reason
//! and the snapshot it read, and a stale refusal prompts a refresh that reads the actions again.
//! The timeline draws the phases and states the task went through beside its stages. A task whose
//! workflow guards an edge draws the guards that let it in and the guards its action needs, takes
//! the action when the host asks its authority, and keeps the refusal's guard under it; the page
//! has nothing that approves.
//!
//! The stand-in server answers with the wire types' own serializations (`wire.cases.json`, written
//! by `crates/symphony-api/examples/typescript.rs`), extended by a task waiting in Verify. A scratch
//! board holds no custom workflow, as only a live board follows the one its default branch commits
//! (`crates/tasks/src/workflow.rs`); the routes themselves are proved on a fixture project's board
//! (`apps/symphony-server/src/live/workflow_tests.rs`). With `SYMPHONY_WORKFLOW_SHOTS` naming a
//! folder, the flow saves its pictures there.

use super::{SHOWN, chrome::Chrome, look_parity, open, samples, site::Site, texts};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use symphony_api::TASK_TIMELINE_PATH;
use symphony_api::workflow::{
    WORKFLOW_ACTIONS_PATH, WORKFLOW_APPLY_ROUTE, WORKFLOW_GUARDS_PATH, WORKFLOW_PATH,
};

/// The task sent back from its first review.
const SLUG: &str = "w/b";

/// The server's words for an edge taken from a revision the task has left.
const STALE: &str = "stale_task: the task is at revision 8, not 7";

/// The time now, in the server's seconds.
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// The held board: the sample's tasks (Build working, Review awaiting, Build blocked), each given
/// a title and an age, `w/b` at the revision its actions were read at, and a task awaiting its
/// second review in Verify.
fn listing(samples: &Value) -> Value {
    let mut listing = samples["heldListing"].clone();
    let at = now();
    let tasks = listing["tasks"].as_array_mut().unwrap();
    let mut verified = tasks[1].clone();
    verified["slug"] = json!("w/d");
    verified["workflow"] = json!({"phase": "verify", "state": "awaiting", "revision": 12});
    tasks.push(verified);
    let titles = [
        "Draws the board's phases as columns",
        "Sent back from its first review",
        "Waits on the owner in Build",
        "Waits on its second review",
    ];
    for (task, (title, hours)) in tasks
        .iter_mut()
        .zip(titles.iter().zip([3.0, 6.0, 30.0, 2.0]))
    {
        task["title"] = json!(title);
        task["created"] = json!(at - hours * 3600.0 * 2.0);
        task["updated"] = json!(at - hours * 3600.0);
    }
    tasks[1]["workflow"]["revision"] = json!(7);
    listing["statuses"] = json!(["in_progress", "review", "blocked"]);
    listing["counts"] = json!([["phase:build", 2], ["phase:review", 1], ["phase:verify", 1]]);
    listing["lifecycle_counts"] = json!([["blocked", 1], ["in_progress", 1], ["review", 2]]);
    let column = |phase: &str, ordinary: u32, blocked: u32| {
        json!({"status": format!("phase:{phase}"), "ordinary": ordinary, "blocked": blocked,
            "oldest": at - 30.0 * 3600.0})
    };
    listing["waiting"]["columns"] = json!([
        column("build", 1, 1),
        column("review", 1, 0),
        column("verify", 1, 0)
    ]);
    listing["waiting"]["states"] = json!([["awaiting", 2], ["blocked", 1], ["working", 1]]);
    listing["waiting"]["kinds"] = json!([["nobody", 4]]);
    listing
}

/// `w/b`'s timeline: claimed, submitted to its first review, sent back, worked again and submitted
/// to its second round, where it waits now; each place its edge's.
fn timeline() -> Value {
    let mut timeline: Value = serde_json::from_str(include_str!(
        "../../../../crates/symphony-api/fixtures/task-timeline.json"
    ))
    .expect("the timeline fixture");
    timeline["slug"] = json!(SLUG);
    let stages = timeline["stages"].as_array().unwrap().clone();
    let place = |stage: &Value, phase: &str, state: &str, edge: Option<&str>| {
        let label = |id: &str| {
            let mut letters = id.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(letters).collect()
            })
        };
        json!({"phase": phase, "phase_label": label(phase), "state": state,
            "state_label": if state == "awaiting" { "Awaiting review".into() } else { label(state) },
            "status": stage["status"], "from": stage["from"], "edge": edge, "by": stage["by"]})
    };
    timeline["places"] = json!([
        place(&stages[0], "build", "available", None),
        place(&stages[1], "build", "working", Some("claim")),
        place(&stages[2], "review", "awaiting", Some("submit")),
        place(&stages[3], "build", "working", Some("rework")),
        place(&stages[4], "review", "awaiting", Some("submit")),
    ]);
    timeline
}

/// The stand-in server: the held board, the active workflow, `w/b`'s page, thread and timeline,
/// and its next actions, an edge taken answered with the sample's result.
fn site() -> (Site, Value) {
    let samples = samples();
    let site = crate::site();
    let listing = listing(&samples);
    site.set_json("/api/task-board", &listing);
    site.set_json(WORKFLOW_PATH, &samples["workflow"]);
    site.set_json(WORKFLOW_ACTIONS_PATH, &samples["nextActions"]);
    site.set_json(
        WORKFLOW_APPLY_ROUTE,
        &json!({"ok": true, "result": samples["applied"]}),
    );
    let task = listing["tasks"][1].clone();
    site.set_json(
        "/api/task-page",
        &json!({"task": task, "blocks": [{"type": "paragraph", "spans": [{"type": "text", "text": "Send it back with a reason."}]}],
            "children": [], "children_count": 0}),
    );
    site.set_json(
        "/api/task-history",
        &look_parity::responses()["/api/task-history"],
    );
    site.set_json(TASK_TIMELINE_PATH, &timeline());
    (site, samples)
}

/// The page as its fonts and two frames leave it, saved as `NAME.png` in the folder
/// `SYMPHONY_WORKFLOW_SHOTS` names; nothing without one.
fn shot(page: &mut Chrome, name: &str) {
    let Some(folder) = std::env::var_os("SYMPHONY_WORKFLOW_SHOTS") else {
        return;
    };
    page.frames_with_fonts();
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("{name}.png")), page.screenshot()).unwrap();
}

/// Where the card `slug` stands: its column's key and the place its mark names.
fn card(page: &mut Chrome, slug: &str) -> Value {
    page.eval(&format!(
        "(() => {{ const card = document.querySelector('[data-slug={}]'); return card && [card.closest('.column')?.dataset.status ?? null, card.querySelector('.state-mark')?.textContent ?? null]; }})()",
        json!(slug)
    ))
}

#[test]
fn the_board_draws_the_workflows_phases_as_columns_and_tabs_and_filters_by_a_state() {
    let (site, _) = site();
    let Some(mut page) = super::chrome() else {
        panic!("Chrome is required for the workflow's acceptance")
    };
    open(&mut page, &site, "/tasks", 1440);
    assert!(
        page.wait_for(
            "document.querySelector('.column[data-status=\"phase:verify\"] [data-slug=\"w/d\"]') !== null",
            SHOWN
        ),
        "{:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    assert_eq!(
        page.eval("[...document.querySelectorAll('.column')].map(column => column.dataset.status).filter(status => status.startsWith('phase:'))"),
        json!(["phase:build", "phase:review", "phase:verify"]),
        "the phases that are columns, by their order"
    );
    assert_eq!(
        json!(texts(&mut page, ".column[data-status^=\"phase:\"] > h2"))
            .as_array()
            .unwrap()
            .iter()
            .map(|title| title
                .as_str()
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .to_owned())
            .collect::<Vec<_>>(),
        ["Build", "Review", "Verify"]
    );
    for status in ["ready", "in_progress", "review", "done", "blocked"] {
        assert_eq!(
            page.eval(&format!(
                "document.querySelector('.column[data-status={status}]') === null"
            )),
            json!(true),
            "no column of the board's own writers' status {status}"
        );
    }
    assert_eq!(
        card(&mut page, "w/a"),
        json!(["phase:build", "Build · Working"])
    );
    assert_eq!(
        card(&mut page, "w/b"),
        json!(["phase:review", "Review · Awaiting review"])
    );
    assert_eq!(
        card(&mut page, "w/c"),
        json!(["phase:build", "Build · Blocked"])
    );
    assert_eq!(
        card(&mut page, "w/d"),
        json!(["phase:verify", "Verify · Awaiting review"])
    );
    assert_eq!(
        json!(texts(&mut page, ".state-strip .phase-open")),
        json!(["Parked 0"]),
        "a phase that is no column opens as a list of its own"
    );
    assert_eq!(
        json!(texts(&mut page, ".state-strip .k-state")),
        json!(["Working 1", "Blocked 1", "Awaiting review 2"]),
        "a chip for each state with open tasks, in the definition's order"
    );
    shot(&mut page, "board-wide");

    page.eval("document.querySelector('.state-strip [data-state=blocked]').click()");
    assert!(page.wait_for(
        "document.querySelector('.state-strip [data-state=blocked]').getAttribute('aria-pressed') === 'true'",
        SHOWN
    ));
    assert!(
        test_time::until_true(
            "the board read again for the chosen state",
            super::STALL,
            || site
                .targets()
                .iter()
                .any(|target| target.starts_with("/api/task-board")
                    && target.contains("wait=state:blocked")),
        )
        .is_ok(),
        "{:?}",
        site.targets()
    );

    open(&mut page, &site, "/tasks", 390);
    assert!(page.wait_for(
        "document.querySelectorAll('.stage-tabs button').length > 0 && document.querySelector('[data-slug]') !== null",
        SHOWN
    ));
    let tabs: Vec<String> = json!(texts(&mut page, ".stage-tabs button"))
        .as_array()
        .unwrap()
        .iter()
        .map(|tab| {
            tab.as_str()
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        tabs[..4],
        ["Verify", "Review", "Build", "Parked"],
        "every phase a tab, by its phone order: {tabs:?}"
    );
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true),
        "no sideways scroll on a phone"
    );
    // A workflow's lanes are phases: its board draws no row of work under way.
    assert_eq!(
        page.eval("document.querySelector('.under-way')"),
        Value::Null,
        "a custom workflow's board draws no row of work under way"
    );
    shot(&mut page, "board-phone");
}

#[test]
fn the_task_page_sends_back_with_its_snapshot_and_a_stale_refusal_asks_for_a_refresh() {
    let (site, samples) = site();
    let Some(mut page) = super::chrome() else {
        panic!("Chrome is required for the workflow's acceptance")
    };
    site.refuse_route(WORKFLOW_APPLY_ROUTE, Some(STALE));
    open(&mut page, &site, &format!("/task/{SLUG}"), 1280);
    let rework = ".flow-actions [data-edge=rework]";
    assert!(
        page.wait_for(
            &format!("document.querySelector('{rework} button') !== null"),
            SHOWN
        ),
        "{:?}",
        page.eval("document.querySelector('article.task, main')?.innerText")
    );
    assert_eq!(
        json!(texts(&mut page, ".flow-place")),
        json!(["At Review · Awaiting review"])
    );
    assert_eq!(
        json!(texts(&mut page, ".flow-manual")),
        json!(["sable-finch, its reviewer, was lost: it waits on you to move it on"]),
    );
    assert_eq!(
        page.eval("document.querySelector('.flow-actions [data-edge=pass] button').disabled"),
        json!(true)
    );
    assert_eq!(
        json!(texts(&mut page, ".flow-actions [data-edge=pass] .flow-why")),
        json!(["not the owner's to take: needs the reviewer"]),
        "an unavailable action names whom it needs"
    );
    assert_eq!(
        page.eval(&format!(
            "document.querySelector('{rework} button').disabled"
        )),
        json!(true),
        "an action with a required input waits for it"
    );
    page.eval(&format!("document.querySelector('{rework} input').focus()"));
    page.call(
        "Input.insertText",
        json!({"text": "The phone tabs skip Verify"}),
    );
    assert!(page.wait_for(
        &format!("document.querySelector('{rework} button').disabled === false"),
        SHOWN
    ));
    shot(&mut page, "task-wide");

    page.eval(&format!(
        "document.querySelector('{rework} button').click()"
    ));
    assert!(
        page.wait_for(
            "document.querySelector('.flow-moved[role=alert]') !== null",
            SHOWN
        ),
        "a stale refusal asks for a refresh: {:?}",
        page.eval("document.querySelector('.flow-task')?.innerText")
    );
    let sent = site.posts();
    let (route, body) = sent.last().expect("the edge sent");
    assert_eq!(route, WORKFLOW_APPLY_ROUTE);
    let request = &body["request"];
    assert_eq!(body["slug"], json!(SLUG));
    assert_eq!(
        (
            &request["project"],
            &request["task"],
            &request["edge"],
            &request["inputs"]
        ),
        (
            &json!("symphony"),
            &json!(SLUG),
            &json!("rework"),
            &json!({"reason": {"kind": "text", "value": "The phone tabs skip Verify"}})
        )
    );
    assert_eq!(
        request["expected"], samples["nextActions"]["snapshot"],
        "the edge names the snapshot its actions were read at"
    );
    assert!(
        request["idempotency_key"]
            .as_str()
            .is_some_and(|key| !key.is_empty())
    );
    let read = |site: &Site| {
        site.targets()
            .iter()
            .filter(|target| target.starts_with(WORKFLOW_ACTIONS_PATH))
            .count()
    };
    let before = read(&site);
    page.eval("document.querySelector('.flow-moved button').click()");
    assert!(page.wait_for("document.querySelector('.flow-moved') === null", SHOWN));
    assert!(read(&site) > before, "the refresh reads the actions again");

    site.refuse_route(WORKFLOW_APPLY_ROUTE, None);
    assert!(page.wait_for(
        &format!("document.querySelector('{rework} button')?.disabled === false"),
        SHOWN
    ));
    page.eval(&format!(
        "document.querySelector('{rework} button').click()"
    ));
    assert!(
        page.wait_for(
            "document.querySelector('.flow-task [role=status]')?.textContent === 'Send back: done'",
            SHOWN
        ),
        "{:?}",
        page.eval("document.querySelector('.flow-task')?.innerText")
    );
    let posts = site.posts();
    let keys: Vec<&Value> = posts
        .iter()
        .filter(|(route, _)| route == WORKFLOW_APPLY_ROUTE)
        .map(|(_, body)| &body["request"]["idempotency_key"])
        .collect();
    assert_eq!(keys.len(), 2);
    assert_ne!(
        keys[0], keys[1],
        "a refusal the server answered is not sent again under its key"
    );

    page.eval("document.getElementById('timeline').scrollIntoView()");
    assert!(
        page.wait_for(
            "[...document.querySelectorAll('.tl-totals')].some(line => line.textContent.startsWith('Phases: '))",
            SHOWN
        ),
        "{:?}",
        page.eval("document.getElementById('timeline')?.innerText")
    );
    let totals = json!(texts(&mut page, ".tl-totals"));
    let totals: Vec<&str> = totals
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        totals
            .iter()
            .any(|line| line.starts_with("Phases: Build ") && line.contains(" · Review "))
            && totals.iter().any(|line| line.starts_with("States: ")),
        "the phases and states apart from the stages: {totals:?}"
    );
    shot(&mut page, "timeline-wide");

    open(&mut page, &site, &format!("/task/{SLUG}"), 390);
    assert!(page.wait_for(
        &format!("document.querySelector('{rework} button') !== null"),
        SHOWN
    ));
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true),
        "no sideways scroll on a phone"
    );
    assert_eq!(
        page.eval("(() => { const r = document.querySelector('.flow-actions').getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth; })()"),
        json!(true),
        "the actions fit a phone's width"
    );
    shot(&mut page, "task-phone");
}

/// [`site`] with a workflow that guards Pass review: the definition declares the guards, the
/// preview lists Pass review as held by its guard alone, and the task's guards are the sample's
/// (a fake authority, `ledger`, which this host asks for `release` and cannot for `design`).
fn guarded_site() -> (Site, Value) {
    let (site, samples) = site();
    let mut workflow = samples["workflow"].clone();
    workflow["workflow"]["definition"]["guards"] = json!([
        {"id": "design_signoff", "adapter": "design"},
        {"id": "release_signoff", "adapter": "release"},
    ]);
    site.set_json(WORKFLOW_PATH, &workflow);
    let mut next = samples["nextActions"].clone();
    next["actions"][0]["unavailable"] = json!(["guard_unavailable"]);
    site.set_json(WORKFLOW_ACTIONS_PATH, &next);
    site.set_json(WORKFLOW_GUARDS_PATH, &samples["taskGuards"]);
    (site, samples)
}

/// The states of the guards' rows drawn in the box `of` names (an edge, or `entry`), by guard.
fn guard_states(page: &mut Chrome, of: &str) -> Value {
    page.eval(&format!(
        "[...document.querySelectorAll('[data-guards={of}] [data-guard]')].map(row => [row.dataset.guard, row.dataset.state])"
    ))
}

#[test]
fn the_task_page_names_each_guard_its_authority_and_the_last_refusal_and_approves_nothing() {
    let (site, samples) = guarded_site();
    let Some(mut page) = super::chrome() else {
        panic!("Chrome is required for the workflow's acceptance")
    };
    let error = "guard_unavailable: edge pass is guarded by release_signoff, which ledger refuses";
    site.refuse_with(
        WORKFLOW_APPLY_ROUTE,
        &json!({"error": error, "guard": samples["guardRefusal"]}),
    );
    let pass = ".flow-actions [data-edge=pass]";
    for width in [1280, 390] {
        open(&mut page, &site, &format!("/task/{SLUG}"), width);
        assert!(
            page.wait_for(
                "document.querySelector('[data-guards=pass] [data-guard]') !== null",
                SHOWN
            ),
            "{:?}",
            page.eval("document.querySelector('.flow-task')?.innerText")
        );
        // What let it in, over the actions: the guard, that it no longer stands and why, its
        // authority and the generation and epoch the board accepts.
        assert_eq!(
            guard_states(&mut page, "entry"),
            json!([["design_signoff", "decided again"]])
        );
        assert!(
            texts(&mut page, "[data-guards=entry] .guard-head")
                .first()
                .is_some_and(|head| head.starts_with("Let in by Submit · revision 7 · ")),
        );
        let entry = texts(&mut page, "[data-guards=entry] .guard")
            .first()
            .cloned()
            .unwrap_or_default();
        for said in [
            "design signoff",
            "Met at generation 11",
            "ledger has decided again since (generation 12 now)",
            "accepted gen 12 · 2026",
        ] {
            assert!(entry.contains(said), "{said}: {entry}");
        }
        // What Pass review needs, under it: no word yet, from an authority this host asks, so the
        // action is the owner's to take.
        assert_eq!(
            guard_states(&mut page, "pass"),
            json!([["release_signoff", "no word yet"]])
        );
        assert_eq!(
            json!(texts(&mut page, "[data-guards=pass] .guard-by")),
            json!(["ledger · releaseaccepted gen 41 · 2026"])
        );
        assert_eq!(
            page.eval(&format!(
                "document.querySelector('{pass} > button').disabled"
            )),
            json!(false)
        );
        assert_eq!(
            page.eval("document.querySelector('[data-guards=rework]')"),
            json!(null),
            "an action no guard holds draws no box"
        );
        shot(&mut page, &format!("guards-before-{width}"));

        // The take is refused by the authority: the guard's row says so, with its reasons as
        // links, and the name opens what the board read.
        page.eval(&format!(
            "document.querySelector('{pass} > button').click()"
        ));
        assert!(
            page.wait_for(
                "document.querySelector('[data-guards=pass] [data-state=refused]') !== null",
                SHOWN
            ),
            "{:?}",
            page.eval("document.querySelector('.flow-task')?.innerText")
        );
        assert_eq!(
            json!(texts(&mut page, ".flow-task [role=status]")),
            json!(["Pass review was not taken: release signoff holds it"])
        );
        assert_eq!(
            page.eval("[...document.querySelectorAll('[data-guards=pass] .guard-why a')].map(link => [link.textContent, link.getAttribute('href'), link.target])"),
            json!([
                ["task w/a", "/task/w/a", ""],
                ["the release check failed", "https://ci.example/runs/7", "_blank"],
            ])
        );
        let open_record = "document.querySelector('[data-guards=pass] .guard-open')";
        assert_eq!(
            page.eval(&format!("{open_record}.getAttribute('aria-expanded')")),
            json!("false")
        );
        page.eval(&format!("{open_record}.click()"));
        assert!(page.wait_for(
            "document.querySelector('[data-guards=pass] .guard-record') !== null",
            SHOWN
        ));
        assert_eq!(
            json!(texts(&mut page, "[data-guards=pass] .guard-record dt")),
            json!([
                "ledger's word",
                "Its reasons",
                "Its time",
                "Covers",
                "Signature",
                "From ledger",
                "Admitted on it",
                "Asked from here"
            ])
        );
        assert_eq!(
            page.eval(&format!(
                "[{open_record}.getAttribute('aria-expanded'), document.getElementById({open_record}.getAttribute('aria-controls')) !== null]"
            )),
            json!(["true", true])
        );
        // The authority's name is the link to its ledger's page for the guard.
        assert_eq!(
            page.eval(
                "document.querySelector('[data-guards=pass] .guard-by a').getAttribute('href')"
            ),
            json!("/docs/LEDGER.md#w/a:release_signoff")
        );
        // Nothing in the panel approves, grants or takes a file: its only buttons open records.
        assert_eq!(
            page.eval("[...document.querySelectorAll('.guard-box button, .guard-box input, .guard-box textarea, .guard-box form')].map(one => one.className)"),
            json!(["guard-open", "guard-open"])
        );
        assert_eq!(
            page.eval("document.documentElement.scrollWidth <= innerWidth"),
            json!(true),
            "no sideways scroll at {width}"
        );
        assert_eq!(
            page.eval("[...document.querySelectorAll('.guard-box')].every(box => { const r = box.getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth; })"),
            json!(true),
            "the guards fit the page at {width}"
        );
        // A phone's picture is of the action and its guards, which the page's head would hide.
        if width <= 760 {
            page.eval("document.querySelector('.flow-actions').scrollIntoView()");
        }
        shot(&mut page, &format!("guards-after-{width}"));
    }
    let posts = site.posts();
    let (route, body) = posts.last().expect("the edge sent");
    assert_eq!(route, WORKFLOW_APPLY_ROUTE);
    assert_eq!(body["request"]["edge"], json!("pass"));
    assert!(
        body["request"].get("attestations").is_none() && body.get("attestations").is_none(),
        "the page sends no attestation: the host asks the authority"
    );

    // An authority this host cannot ask: the action waits, and its row says what would let it go
    // from here, with no command, as the command line takes no edge as the owner.
    let mut apart = samples["taskGuards"].clone();
    apart["authorities"][1]["asked_here"] = json!(false);
    site.set_json(WORKFLOW_GUARDS_PATH, &apart);
    open(&mut page, &site, &format!("/task/{SLUG}"), 1280);
    assert!(page.wait_for(
        "document.querySelector('[data-guards=pass] [data-guard]') !== null",
        SHOWN
    ));
    assert_eq!(
        guard_states(&mut page, "pass"),
        json!([["release_signoff", "not from here"]]),
        "a reload forgets the refusal, and the row is the board's records again"
    );
    assert_eq!(
        page.eval(&format!(
            "document.querySelector('{pass} > button').disabled"
        )),
        json!(true)
    );
    assert_eq!(
        json!(texts(&mut page, "[data-guards=pass] .guard-why")),
        json!([
            "This app cannot ask ledger: no plugin of this project answers for its adapter, release. Pass review goes from here once one does."
        ])
    );
    assert_eq!(
        page.eval("document.querySelector('[data-guards=pass] code')"),
        json!(null)
    );
    shot(&mut page, "guards-not-from-here-1280");
}

#[test]
fn a_task_of_a_workflow_with_no_guards_reads_and_draws_none() {
    let (site, _) = site();
    let Some(mut page) = super::chrome() else {
        panic!("Chrome is required for the workflow's acceptance")
    };
    open(&mut page, &site, &format!("/task/{SLUG}"), 1280);
    assert!(page.wait_for(
        "document.querySelector('.flow-actions [data-edge=rework] button') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.querySelector('.guard-box')"),
        json!(null)
    );
    assert!(
        !site
            .targets()
            .iter()
            .any(|target| target.starts_with(WORKFLOW_GUARDS_PATH)),
        "a workflow that declares no guard reads none: {:?}",
        site.targets()
    );
}
