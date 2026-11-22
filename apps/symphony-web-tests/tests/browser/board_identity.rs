//! Cards that differ only at the end of their title, slug, wait and reason are told apart closed,
//! a reason written on several lines stays on the closed card's one line, cards open in place to
//! the whole of each by key and by touch, and every stage of the lane scroller is named above it
//! and reached whole from the first. The listing holds a column of more than a page, so rows wait
//! as placeholders under the cards that open.
use serde_json::{Value, json};

use crate::{READY, SHOWN, chrome, components::key, site::Site};

const STAGES: [&str; 8] = [
    "concept",
    "design",
    "ready",
    "in_progress",
    "build",
    "test",
    "review",
    "done",
];
const ALIKE: [&str; 3] = ["alpha", "beta", "gamma"];

fn slug(end: &str) -> String {
    format!("tools/review-app-verdicts-saved-owner-answer-recovery-{end}")
}

fn card(end: &str) -> String {
    format!("[data-slug=\"{}\"]", slug(end))
}

fn task(slug: &str, title: &str, stage: &str, wait: Value, blocked: bool, since: f64) -> Value {
    json!({"slug": slug, "title": title, "status": if blocked {"blocked"} else {stage},
        "from": if blocked {Some(stage)} else {None}, "priority": 2, "body": "", "parent": null,
        "created_by": "owner", "created": since, "updated": since, "links": [], "depends": [],
        "roles": [], "instruction": false, "waits_on": wait})
}

/// Three blocked Ready cards alike but for their last word, 34 queued under them (a page is 30)
/// and a card in each other lifecycle stage.
fn listing() -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let mut tasks = Vec::new();
    for (at, end) in ALIKE.into_iter().enumerate() {
        tasks.push(task(
            &slug(end),
            &format!("Symphony could not commit an owner answer recorded while the review register was regrouped for release candidate {end}"),
            "ready",
            json!({"kind": "task", "who": format!("docs/research-handoff-owner-answer-register-{end}"),
                "why": format!("Saved owner answer recovery waits for the register regroup of candidate {end}"),
                "since": now - 7200.0 - at as f64}),
            true,
            now - 7200.0,
        ));
    }
    for at in 0..34 {
        tasks.push(task(
            &format!("ready/queued-{at:02}"),
            "Make the next ready change",
            "ready",
            json!({"kind": "queued", "who": "", "why": "Ready for an executor", "since": now - 3600.0 + f64::from(at)}),
            false,
            now - 3600.0,
        ));
    }
    for stage in ["concept", "design", "in_progress", "review", "done"] {
        tasks.push(task(
            &format!("{stage}/work"),
            "Draw the board at a glance",
            stage,
            json!({"kind": "working", "who": "busy-worker", "why": "executor", "since": now - 3600.0}),
            false,
            now - 3600.0,
        ));
    }
    let held = |stage: &str| {
        tasks
            .iter()
            .filter(|task| task["status"] == *stage || task["from"] == *stage)
            .count()
    };
    let counts: Vec<Value> = STAGES
        .iter()
        .map(|stage| json!([stage, held(stage)]))
        .collect();
    let columns: Vec<Value> = STAGES
        .iter()
        .map(|stage| {
            let blocked = if *stage == "ready" { 3 } else { 0 };
            json!({"status": stage, "ordinary": held(stage) - blocked, "blocked": blocked,
                "oldest": if held(stage) == 0 {None} else {Some(now - 7200.0)}})
        })
        .collect();
    json!({"tasks": tasks, "statuses": STAGES, "counts": counts, "signoff": [], "next": null,
        "waiting": {"kinds": [["task", 3], ["queued", 34], ["working", 5]], "columns": columns},
        "orchestrators": [{"handle": "calm-orchestrator", "health": "live"}], "unstarted": 0})
}

/// The board at `width` in `scheme`, its lanes at the first: opened from the blank page, so its
/// history entry is a new one and keeps no place. Opened again at the URL it shows, Chrome would
/// replace its entry and keep the place the board wrote in it (`PlaceKeeper`, boardplace.ts) if
/// that write reached the browser first, and the lanes would come back where the opening before
/// left them, the Ready lane's cards out of the window and not drawn.
fn open(page: &mut crate::Chrome, site: &Site, width: u32, scheme: &str) {
    page.open("about:blank");
    page.viewport(width, 900, width <= 760);
    page.scheme(scheme);
    page.open(&site.url("/tasks"));
    if width <= 760 {
        assert!(page.wait_for(
            "document.querySelector('.stage-tabs button') !== null",
            SHOWN
        ));
        page.eval("[...document.querySelectorAll('.stage-tabs button')].find(button => button.textContent.startsWith('Ready')).click()");
    }
    assert!(page.wait_for(
        &format!(
            "document.querySelector('{}') !== null && {READY}",
            card("gamma")
        ),
        SHOWN
    ));
}

/// The texts of `part` on the three cards alike, as the page shows them: the end each line
/// keeps, and whether that end is drawn whole inside its card.
fn kept_ends(page: &mut crate::Chrome, part: &str) -> Value {
    page.eval(&format!(
        "{:?}.map(end => {{
            const card = document.querySelector(`[data-slug=\"tools/review-app-verdicts-saved-owner-answer-recovery-${{end}}\"]`);
            const tail = card.querySelector('{part} .cut-tail'), box = card.getBoundingClientRect(), r = tail.getBoundingClientRect();
            return [tail.textContent, r.width > 0 && r.left >= box.left && r.right <= box.right];
        }})",
        ALIKE
    ))
}

/// Every row of the Ready column stands a row's pitch under the one before it: a drawn card's
/// own height and the 8 px under it, a placeholder's 118 px. None over another and no hole.
const ROWS_ALIGNED: &str = "(() => {
    const rows = [...document.querySelectorAll('.column[data-status=ready] [id^=\"row-\"]')];
    return rows.length >= 30 && rows.every((row, at) => {
        if (at === 0) return true;
        const above = rows[at - 1], card = above.querySelector('.taskcard');
        const pitch = card === null ? 118 : card.getBoundingClientRect().height + 8;
        return Math.abs(row.getBoundingClientRect().top - above.getBoundingClientRect().top - pitch) < 0.5;
    });
})()";

#[test]
fn closed_cards_alike_but_for_their_last_word_keep_that_word_in_view() {
    let site = crate::site();
    site.set_json("/api/task-board", &listing());
    let Some(mut page) = chrome() else { return };
    for width in [1440, 390] {
        open(&mut page, &site, width, "light");
        assert_eq!(
            kept_ends(&mut page, ".waiting-slug"),
            json!([
                ["-recovery-alpha", true],
                ["-recovery-beta", true],
                ["-recovery-gamma", true]
            ]),
            "{width}: the slugs"
        );
        assert_eq!(
            kept_ends(&mut page, ".task-wait .on"),
            json!([
                ["-register-alpha", true],
                ["-register-beta", true],
                ["-register-gamma", true]
            ]),
            "{width}: the waits"
        );
        assert_eq!(
            kept_ends(&mut page, ".task-wait .why"),
            json!([
                [" candidate alpha", true],
                [" candidate beta", true],
                [" candidate gamma", true]
            ]),
            "{width}: the reasons"
        );
        assert_eq!(
            page.eval(&format!(
                "(() => {{ const card = document.querySelector('{}'), head = card.querySelector('.waiting-slug .cut-head');
                    return [card.querySelector('code').textContent, head.scrollWidth > head.clientWidth, card.getBoundingClientRect().height]; }})()",
                card("alpha")
            )),
            json!([slug("alpha"), true, 110]),
            "{width}: the slug is whole to copy, its start gives way, and the closed card keeps its box"
        );
        // A text that fits is drawn as it was: nothing cut, nothing moved apart.
        assert_eq!(
            page.eval("(() => { const code = document.querySelector('[data-slug=\"ready/queued-00\"] code'); return [code.textContent, code.scrollWidth <= code.clientWidth]; })()"),
            json!(["ready/queued-00", true]),
            "{width}"
        );
    }
}

/// A reason as a blocked task may hold it: the board trims only its ends.
const WRITTEN: &str =
    "Saved owner answer recovery waits\nfor the register   regroup of candidate alpha";

#[test]
fn a_reason_on_several_lines_is_folded_onto_the_closed_cards_line_and_opened_as_written() {
    let site = crate::site();
    let mut answer = listing();
    answer["tasks"][0]["waits_on"]["why"] = json!(WRITTEN);
    site.set_json("/api/task-board", &answer);
    let Some(mut page) = chrome() else { return };
    // The reason's text, its height and whether it is drawn whole inside its card, and the card's
    // height.
    let drawn = format!(
        "(() => {{ const card = document.querySelector('{}'), why = card.querySelector('.why');
            const box = card.getBoundingClientRect(), r = why.getBoundingClientRect();
            return [why.textContent, r.height, r.top >= box.top && r.bottom <= box.bottom
                && why.scrollHeight <= why.clientHeight + 1, box.height]; }})()",
        card("alpha")
    );
    for width in [1440, 390] {
        open(&mut page, &site, width, "light");
        assert_eq!(
            page.eval(&drawn),
            json!([
                "Saved owner answer recovery waits for the register regroup of candidate alpha",
                16,
                true,
                110
            ]),
            "{width}: closed, the reason is one line in the card's box"
        );
        assert_eq!(
            kept_ends(&mut page, ".task-wait .why"),
            json!([
                [" candidate alpha", true],
                [" candidate beta", true],
                [" candidate gamma", true]
            ]),
            "{width}: its end is still kept"
        );
        page.eval(&format!(
            "document.querySelector('{} .card-more').click()",
            card("alpha")
        ));
        assert!(page.wait_for(
            &format!(
                "document.querySelector('{}').dataset.open === 'true'",
                card("alpha")
            ),
            SHOWN
        ));
        let opened = page.eval(&drawn);
        assert_eq!(
            [&opened[0], &opened[2]],
            [&json!(WRITTEN), &json!(true)],
            "{width}: opened, the reason is whole as written"
        );
        assert!(
            opened[1].as_f64().unwrap() >= 32.0,
            "{width}: its line break is drawn: {opened}"
        );
    }
}

#[test]
fn a_card_opens_in_place_by_key_and_by_touch_and_the_rows_under_it_stay_aligned() {
    let site = crate::site();
    site.set_json("/api/task-board", &listing());
    let Some(mut page) = chrome() else { return };
    open(&mut page, &site, 1440, "light");
    crate::wait_for_feed(&site).expect("the board never followed its feed");
    assert_eq!(
        page.eval(ROWS_ALIGNED),
        json!(true),
        "before any card opens"
    );
    let whole = |end: &str| {
        format!(
            "(() => {{ const card = document.querySelector('{}'); const box = card.getBoundingClientRect();
                return card.dataset.open === 'true' && card.querySelector('.card-more').getAttribute('aria-expanded') === 'true'
                    && box.height > 110
                    && [...card.querySelectorAll('.taskcard-title, code, .on, .why')].every(part => {{
                        const r = part.getBoundingClientRect();
                        return part.scrollWidth <= part.clientWidth + 1 && part.scrollHeight <= part.clientHeight + 1
                            && r.left >= box.left && r.right <= box.right && r.top >= box.top && r.bottom <= box.bottom;
                    }}); }})()",
            card(end)
        )
    };
    let closed = |end: &str| {
        format!(
            "(() => {{ const card = document.querySelector('{}'); return card.dataset.open === undefined && card.getBoundingClientRect().height === 110; }})()",
            card(end)
        )
    };
    let place =
        "[document.querySelector('.waiting-columns').scrollLeft, scrollY, location.pathname]";
    let before = page.eval(place);

    // The keyboard: the button is the stop after the card's title, and Enter opens and closes it.
    page.eval(&format!(
        "document.querySelector('{} .taskcard-title').focus()",
        card("alpha")
    ));
    key(&mut page, "Tab", "Tab", 9);
    assert_eq!(
        page.eval(
            "[document.activeElement.className, document.activeElement.getAttribute('aria-label')]"
        ),
        json!([
            "card-more",
            "Show all of Symphony could not commit an owner answer recorded while the review register was regrouped for release candidate alpha"
        ])
    );
    key(&mut page, "Enter", "Enter", 13);
    assert!(
        page.wait_for(&whole("alpha"), SHOWN),
        "Enter opens the card whole"
    );
    assert_eq!(page.eval(ROWS_ALIGNED), json!(true), "under an opened card");
    assert_eq!(
        page.eval(place),
        before,
        "opening moves no scroller and leaves the board"
    );

    // Live news of the open card, and a row that goes from over it, leave it open and in line.
    let board: Value = serde_json::from_str(&crate::common::fixture("board.json")).unwrap();
    let version = board["version"].as_u64().unwrap();
    site.push(version + 1, &json!({"version": version + 1, "changes": [{"region": "task-fields", "data": {"key": slug("alpha"), "fields": {"review_rounds": 2}}}]}).to_string());
    assert!(page.wait_for(
        &format!(
            "document.querySelector('{} .review-mark')?.textContent === '2 reviews'",
            card("alpha")
        ),
        SHOWN
    ));
    assert_eq!(
        page.eval(&whole("alpha")),
        json!(true),
        "its own news keeps it open"
    );
    assert_eq!(page.eval(ROWS_ALIGNED), json!(true), "after its own news");

    // Touch: a tap on the next card's button opens it, and follows no link.
    page.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled": true, "maxTouchPoints": 1}),
    );
    page.tap(&format!("{} .card-more", card("beta")));
    assert!(
        page.wait_for(&whole("beta"), SHOWN),
        "a tap opens the card whole"
    );
    assert_eq!(
        page.eval(&whole("alpha")),
        json!(true),
        "each card opens alone"
    );
    assert_eq!(
        page.eval(ROWS_ALIGNED),
        json!(true),
        "under two opened cards"
    );
    assert_eq!(page.eval(place), before);

    // The rows far below, placeholders until now, draw in line under the two opened cards.
    page.eval(
        "document.querySelector('[id=\"row-ready/queued-26\"]').scrollIntoView({block: 'center'})",
    );
    assert!(page.wait_for(
        "document.querySelector('[data-slug=\"ready/queued-26\"]') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval(ROWS_ALIGNED),
        json!(true),
        "once the rows below have drawn"
    );
    assert_eq!(
        page.eval(&whole("alpha")),
        json!(true),
        "off the screen and back, still open"
    );

    // Closed again, each card is its fixed box.
    page.eval(&format!(
        "document.querySelector('{} .card-more').focus()",
        card("alpha")
    ));
    key(&mut page, "Enter", "Enter", 13);
    assert!(page.wait_for(&closed("alpha"), SHOWN));
    page.tap(&format!("{} .card-more", card("beta")));
    assert!(page.wait_for(&closed("beta"), SHOWN));
    assert_eq!(page.eval(ROWS_ALIGNED), json!(true), "closed again");
    assert!(
        !site
            .targets()
            .iter()
            .any(|url| url.contains("/api/task-page") || url.contains("/api/task-history")),
        "a card opens from what the board holds, reading no task"
    );
}

#[test]
fn every_stage_is_named_over_the_lanes_and_the_last_is_reached_whole_from_the_first() {
    let site = crate::site();
    site.set_json("/api/task-board", &listing());
    let Some(mut page) = chrome() else { return };
    open(&mut page, &site, 1440, "light");
    let sides = "[...document.querySelectorAll('.stage-jump button')].map(button => [button.textContent.replace(/[‹› ]/g, '').toLowerCase(), button.dataset.side])";
    let whole = |stage: &str| {
        format!(
            "(() => {{ const frame = document.querySelector('.waiting-columns').getBoundingClientRect(), lane = document.querySelector('.column[data-status={stage}]').getBoundingClientRect();
                return lane.left >= frame.left - 1 && lane.right <= frame.right + 1 && lane.right <= innerWidth && lane.width >= 280; }})()"
        )
    };
    // 8 lanes of 280 px and their gaps do not fit 1440 px: the row says which are cut.
    assert!(
        page.wait_for(
            &format!(
                "JSON.stringify({sides}) === {:?}",
                json!([
                    ["concept", "shown"],
                    ["design", "shown"],
                    ["ready", "shown"],
                    ["inprogress", "shown"],
                    ["build", "after"],
                    ["test", "after"],
                    ["review", "after"],
                    ["done", "after"]
                ])
                .to_string()
            ),
            SHOWN
        ),
        "the row marks the lanes cut: {}",
        page.eval(sides)
    );
    assert_eq!(page.eval(&whole("done")), json!(false));
    assert_eq!(
        page.eval("(() => { const row = document.querySelector('.stage-jump').getBoundingClientRect(), lanes = document.querySelector('.waiting-columns').getBoundingClientRect(), last = document.querySelector('.stage-jump button:last-child');
            const r = last.getBoundingClientRect(); return [row.bottom <= lanes.top, r.right <= innerWidth && r.left >= 0, last.textContent]; })()"),
        json!([true, true, "Done ›"]),
        "the last stage's button is whole on the screen above the lanes"
    );

    // By the keyboard, from the first lane: the last one whole, its heading focused, so the next
    // Tab is that lane's first control, not the first lane's.
    page.eval("document.querySelector('.stage-jump button:last-child').focus()");
    key(&mut page, "Enter", "Enter", 13);
    assert!(
        page.wait_for(&whole("done"), SHOWN),
        "the last lane is shown whole"
    );
    assert_eq!(
        page.eval("[document.activeElement.tagName, document.activeElement.closest('.column').dataset.status]"),
        json!(["H2", "done"])
    );
    key(&mut page, "Tab", "Tab", 9);
    assert_eq!(
        page.eval("[document.activeElement.tagName, document.activeElement.closest('.column')?.dataset.status]"),
        json!(["BUTTON", "done"])
    );
    assert!(
        page.wait_for(
            &format!(
                "JSON.stringify({sides}) === {:?}",
                json!([
                    ["concept", "before"],
                    ["design", "before"],
                    ["ready", "before"],
                    ["inprogress", "before"],
                    ["build", "shown"],
                    ["test", "shown"],
                    ["review", "shown"],
                    ["done", "shown"]
                ])
                .to_string()
            ),
            SHOWN
        ),
        "the row follows the scroller: {}",
        page.eval(sides)
    );
    assert_eq!(
        page.eval("document.querySelector('.stage-jump button').textContent"),
        json!("‹ Concept")
    );

    // The place is kept across a list opened in the board's place, as before.
    let left = page.eval("document.querySelector('.waiting-columns').scrollLeft");
    page.eval("document.querySelector('.board-backlog').click()");
    assert!(page.wait_for("document.querySelector('.stage-jump') === null && document.querySelector('.back-to-stages') !== null", SHOWN));
    page.eval("document.querySelector('.back-to-stages').click()");
    assert!(page.wait_for("document.querySelector('.stage-jump') !== null", SHOWN));
    assert_eq!(
        page.eval("document.querySelector('.waiting-columns').scrollLeft"),
        left
    );
    assert_eq!(page.eval(&whole("done")), json!(true));

    // A click, and back to the first; a lane already whole moves nothing.
    page.eval("document.querySelector('.stage-jump button').click()");
    assert!(page.wait_for(&whole("concept"), SHOWN));
    assert_eq!(
        page.eval("document.querySelector('.waiting-columns').scrollLeft"),
        json!(0)
    );
    page.eval("document.querySelector('.stage-jump button:nth-child(3)').click()");
    assert_eq!(
        page.eval("document.querySelector('.waiting-columns').scrollLeft"),
        json!(0)
    );

    // A window that holds every lane marks none; a phone has its own stage selector.
    page.viewport(2600, 900, false);
    assert!(page.wait_for(
        "[...document.querySelectorAll('.stage-jump button')].every(button => button.dataset.side === 'shown' && !button.textContent.includes('›'))",
        SHOWN
    ));
    open(&mut page, &site, 390, "light");
    assert_eq!(
        page.eval("document.querySelector('.stage-jump') === null && document.querySelector('.ghost-jump') === null"),
        json!(true)
    );
}

#[test]
fn the_ghost_keeps_the_stage_rows_room_so_the_lanes_do_not_move_when_they_load() {
    let site = crate::site();
    site.set_json("/api/task-board", &listing());
    site.hold_route("/api/task-board", true);
    let Some(mut page) = chrome() else { return };
    page.viewport(1440, 900, false);
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": crate::NO_STORE }),
    );
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('.waiting-ghost .ghost-jump') !== null",
        SHOWN
    ));
    // The row's height and how far under the wait strip the lanes start.
    let ghost = page.eval("[document.querySelector('.ghost-jump').getBoundingClientRect().height, document.querySelector('.ghost-kanban').getBoundingClientRect().top - document.querySelector('.wait-strip').getBoundingClientRect().bottom]");
    site.hold_route("/api/task-board", false);
    assert!(page.wait_for(
        "document.querySelector('.waiting-board .taskcard') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval("[document.querySelector('.stage-jump').getBoundingClientRect().height, document.querySelector('.waiting-columns').getBoundingClientRect().top - document.querySelector('.wait-strip').getBoundingClientRect().bottom]"),
        ghost
    );
}
