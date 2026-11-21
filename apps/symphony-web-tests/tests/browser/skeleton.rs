//! A page's ghost: it outlines what is to come, each box as the real one with a bar for each line
//! of text in it; it stays over the page until the regions on its first screen have their
//! reads (`viewport::Settle`), at most three seconds, and not at all once a read fails, a task's
//! thread offering its read again only then and saying it loads while that read is out; its look
//! follows the theme and the reader's motion setting, and a page at rest animates none of it, the
//! placeholders out of reach of the screen included. Past it, a live update that adds or removes
//! a row leaves what the owner reads or types in where it is once they have scrolled, however
//! many frames the row takes to draw and however late the page's tasks run, shows at
//! the top what lands there while they have not, and never scrolls against the owner's own
//! scroll; the chat's form stays at the foot of its sheet.
use super::feed_deltas::{agent, agent_page};
use super::{
    LATE_TASKS, LATE_TASKS_TAKEN, NO_STORE, SHOWN, chrome, chrome::Chrome, layout_shift,
    site::Site, wait_for_feed,
};
use serde_json::{Value, json};

const TASK: &str = "/task/visual/in-progress";
const PAGE: &str = "document.body ? document.body.innerText.slice(0, 2000) : '(no body)'";

fn site() -> Site {
    layout_shift::site(
        "docs/PROBE.md",
        &layout_shift::document_sections(),
        json!([]),
    )
}

/// A browser at `width` x `height`, on `path`.
fn open(site: &Site, path: &str, width: u32, height: u32) -> Option<Chrome> {
    let mut browser = watching(width, height, &[])?;
    browser.open(&site.url(path));
    Some(browser)
}

/// A browser of its own at `width` x `height`, on no page yet, whose every page runs `scripts`
/// from its start.
fn watching(width: u32, height: u32, scripts: &[&str]) -> Option<Chrome> {
    let mut browser = chrome()?;
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": false}),
    );
    for source in scripts {
        browser.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": source }),
        );
    }
    Some(browser)
}

/// The box properties a ghost shares with the real element it stands for (`real`, a class), as
/// `[property, ghost's, real one's]` for each that differs: a probe of the real class is laid out
/// beside the ghost for the comparison and taken out again.
const SAME_BOX: &str = "((ghost, real) => { const probe = document.createElement('div'); probe.className = real; ghost.after(probe);
    const a = getComputedStyle(ghost), b = getComputedStyle(probe);
    const differ = ['paddingTop', 'paddingLeft', 'borderTopWidth', 'borderLeftWidth', 'borderTopLeftRadius', 'backgroundColor', 'fontSize', 'lineHeight', 'marginBottom']
        .filter(key => a[key] !== b[key]).map(key => [key, a[key], b[key]]);
    probe.remove(); return differ; })";

#[test]
fn the_board_before_its_first_read_draws_its_heading_filters_and_columns_of_card_boxes() {
    let site = site();
    site.hold_route("/api/task-board", true);
    let Some(mut browser) = open(&site, "/tasks", 1280, 900) else {
        site.hold_route("/api/task-board", false);
        return;
    };
    let drawn = browser.wait_for(
        "document.querySelector('.ghost-kanban.waiting-columns') !== null",
        SHOWN,
    );
    // Each ghost column's box against a real column's of its status, laid out beside it.
    let ghost = browser.eval(&format!(
        "(() => {{ const columns = [...document.querySelectorAll('.ghost-kanban.waiting-columns .ghost-column, .ghost-offline > .ghost-column')];
            const differ = column => {{ const probe = document.createElement('section'); probe.className = 'column'; probe.dataset.status = column.dataset.for; const wrapper = document.createElement('div'); column.after(wrapper); wrapper.append(probe);
                const a = getComputedStyle(column), b = getComputedStyle(probe);
                const differ = ['borderTopColor', 'backgroundColor', 'paddingTop', 'paddingLeft', 'borderTopWidth', 'borderLeftWidth', 'borderTopLeftRadius']
                    .filter(key => a[key] !== b[key]).map(key => [column.dataset.for, key, a[key], b[key]]);
                wrapper.remove(); return differ; }};
            return {{heading: document.querySelector('.page-head h1')?.textContent, fields: document.querySelectorAll('.filters .ghost-field').length,
                filled: [...document.querySelectorAll('.filters .ghost-field')].map(field => field.querySelectorAll('.bone').length),
                status: [...document.querySelectorAll('main [role=status]')].map(status => status.textContent), busy: document.querySelector('main').getAttribute('aria-busy'),
                columns: columns.map(c => c.dataset.for), column: columns.flatMap(differ),
                cards: columns.map(c => c.querySelectorAll('.ghost-card').length),
                chips: columns.map(c => c.querySelectorAll('.ghost-card .ghost-pill').length),
                card: {SAME_BOX}(columns[0].querySelector('.ghost-card'), 'taskcard waiting-card')}}; }})()"
    ));
    site.hold_route("/api/task-board", false);
    assert!(drawn, "no ghost of the board: {}", browser.eval(PAGE));
    assert_eq!(
        ghost,
        json!({"heading": "Board", "fields": 2, "filled": [0, 0], "status": ["Loading tasks…"], "busy": "true",
            "columns": ["concept", "design", "ready", "in_progress", "build", "test", "review", "done"],
            "column": [], "cards": [1, 1, 1, 1, 0, 0, 1, 1], "chips": [0, 0, 0, 0, 0, 0, 0, 0],
            "card": []}),
        "a first visit's ghost draws each column of the board in its colour with a typical card's box, its row of links and, on the cards of tasks taken up, its row of roles, and says once that the board loads"
    );
    assert!(
        browser.wait_for("document.querySelector('.ghost-kanban.waiting-columns') === null && document.querySelector('.kanban.waiting-columns') !== null", SHOWN),
        "the board never replaced its ghost: {}",
        browser.eval(PAGE)
    );
}

/// A first visit whose board read is refused (a board not made yet, say) says why in place of the
/// ghost, which would wait for ever, once and in the server's words, and draws the board once a
/// read answers: its own retry's, or the page's next.
#[test]
fn a_board_whose_first_read_fails_says_why_in_place_of_its_ghost_until_a_read_answers() {
    let site = site();
    site.fail_route("/api/task-board", true);
    let Some(mut browser) = open(&site, "/tasks", 1280, 900) else {
        site.fail_route("/api/task-board", false);
        return;
    };
    let said = browser.wait_for(
        "document.querySelector('main .unread-board') !== null",
        SHOWN,
    );
    let shown = browser.eval(
        "(() => { const block = document.querySelector('main .unread-board');
            return {heading: block?.querySelector('h2')?.textContent, why: block?.querySelector('p')?.textContent,
                retry: block?.querySelector('button')?.textContent, ghost: document.querySelector('.ghost-kanban') !== null,
                status: [...document.querySelectorAll('main [role=status]')].map(status => status.className),
                alerts: document.querySelectorAll('.page-notices [role=alert]').length,
                busy: document.querySelector('main').getAttribute('aria-busy')}; })()",
    );
    site.fail_route("/api/task-board", false);
    assert!(said, "the failed read is not said: {}", browser.eval(PAGE));
    assert_eq!(
        shown,
        json!({"heading": "The task board could not be read",
            "why": "Board snapshot unavailable (the server answered 500). Trying again…", "retry": "Retry",
            "ghost": false, "status": ["trouble card unread-board"], "alerts": 0, "busy": "false"}),
        "the block stands in the ghost's place, says the read's error once, and the page is not busy"
    );
    // The page reads again by itself too: a read of its own may have drawn the board already.
    browser.eval("document.querySelector('main .unread-board button')?.click()");
    assert!(
        browser.wait_for(
            "document.querySelector('main .unread-board') === null && document.querySelector('.kanban.waiting-columns') !== null",
            SHOWN
        ),
        "the board never replaced the block: {}",
        browser.eval(PAGE)
    );
}

#[test]
fn the_board_s_next_ghost_draws_the_columns_it_last_read_with_a_card_for_each_task() {
    let site = site();
    site.set_json("/api/task-board", &json!({"tasks": [task(1,2)], "statuses": ["ready"], "counts": [["ready",1]], "signoff": [], "next": null}));
    let Some(mut browser) = open(&site, "/tasks", 390, 844) else {
        return;
    };
    assert!(browser.wait_for(
        "document.querySelector('.stage-tabs button') !== null",
        SHOWN
    ));
    browser.eval("[...document.querySelectorAll('.stage-tabs button')].find(button => button.textContent.startsWith('Ready')).click()");
    assert!(
        browser.wait_for(
            "document.querySelector('.kanban .taskcard') !== null",
            SHOWN
        ),
        "the task board was never drawn: {}",
        browser.eval(PAGE)
    );
    // The next visit: its first read held, and no copy of the board on this device (its cache
    // opened late or let the board go), which would draw it at once with no ghost.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": NO_STORE }),
    );
    site.hold_route("/api/task-board", true);
    browser.open(&site.url("/tasks"));
    let drawn = browser.wait_for(
        "document.querySelector('.ghost-kanban.waiting-columns') !== null",
        SHOWN,
    );
    let ghost = browser.eval(
        "[...document.querySelectorAll('.ghost-column')].map(c => [c.dataset.for, c.querySelectorAll('.ghost-card').length, Math.round(c.getBoundingClientRect().width)])",
    );
    site.hold_route("/api/task-board", false);
    assert!(drawn, "no ghost of the board: {}", browser.eval(PAGE));
    assert!(
        browser.wait_for(
            "document.querySelector('.kanban .taskcard') !== null",
            SHOWN
        ),
        "the task board was never drawn again: {}",
        browser.eval(PAGE)
    );
    let page = browser.eval(
        "[...document.querySelectorAll('.kanban .column')].map(c => [c.dataset.status, Number(c.querySelector('h2 .count')?.textContent ?? 0), Math.round(c.getBoundingClientRect().width)])",
    );
    let counted: Vec<Value> = page
        .as_array()
        .into_iter()
        .flatten()
        .map(|column| {
            json!([
                column[0],
                column[1].as_u64().unwrap_or_default().min(16),
                column[2]
            ])
        })
        .collect();
    assert_eq!(
        ghost,
        json!(counted),
        "the ghost draws the columns the board last drew, as wide, each with a card per task it counted"
    );
}

#[test]
fn a_column_still_reading_its_tasks_draws_a_card_box_for_each_it_will_hold() {
    let site = site();
    // Fourteen tasks counted and none read yet: the column reads its first page, which never
    // brings any here, so it stays as it was drawn while it reads.
    site.set_json(
        "/api/task-board",
        &json!({"tasks": [], "statuses": ["ready"], "counts": [["ready", 14]], "signoff": [], "next": null}),
    );
    let Some(mut browser) = open(&site, "/tasks", 1280, 900) else {
        return;
    };
    let column = ".column[data-status=ready]";
    assert!(
        browser.wait_for(
            &format!("document.querySelector('{column} [data-pending=true] .ghost-card') !== null"),
            SHOWN
        ),
        "the column never read its first page: {}",
        browser.eval(PAGE)
    );
    // `pitch` is a card's box with the space under it as drawn, and `reserved` what the column
    // holds for each (`tasks::ROW`, a typical card): the same, so the column keeps its height as
    // its cards draw.
    assert_eq!(
        browser.eval(&format!(
            "(() => {{ const cards = [...document.querySelectorAll('{column} .ghost-card')];
                return {{cards: cards.length, waiting: cards.every(card => card.closest('[data-pending=true]') !== null),
                    busy: document.querySelector('{column}').getAttribute('aria-busy'), said: document.querySelectorAll('{column} [role=status]').length,
                    more: [...document.querySelectorAll('{column} button')].some(button => button.textContent.includes('Load more')),
                    box: {SAME_BOX}(cards[0], 'taskcard waiting-card'),
                    pitch: Math.round(cards[1].getBoundingClientRect().top - cards[0].getBoundingClientRect().top),
                    reserved: parseFloat(cards[0].closest('[data-pending=true]').style.minHeight) / cards.length,
                    bars: cards[0].querySelectorAll('.bone').length,
                    colour: getComputedStyle(cards[0].querySelector('.bone')).backgroundColor}}; }})()"
        )),
        json!({"cards": 14, "waiting": true, "busy": "true", "said": 0, "more": false, "box": [], "pitch": 118,
            "reserved": 118, "bars": 5, "colour": "rgba(127, 127, 127, 0.14)"}),
        "a typical card's box per task counted, no agent's yet: a bar for its slug, its title's lines and its row of links, and no Load more while the first page is read"
    );
}

/// A task waiting for sign-off remains in its lifecycle stage in the waiting board. Once each
/// stage's answer arrives, its complete count matches its cards and no ghost or spinner remains.
#[test]
fn a_stage_keeps_its_sign_off_tasks_and_stops_loading_after_its_answer() {
    let site = site();
    let task = |slug: &str, status: &str| {
        json!({"slug": slug, "title": format!("The task {slug}"), "body": "", "status": status,
            "priority": 2, "parent": null, "created_by": "owner", "created": 0.0, "updated": 0.0,
            "links": [], "depends": [], "roles": [], "instruction": false})
    };
    site.set_json(
        "/api/task-board",
        &json!({"tasks": [task("probe/asks", "review"), task("probe/half-asks", "in_progress"),
                task("probe/half-plain", "in_progress")],
            "statuses": ["in_progress", "review"], "counts": [["in_progress", 2], ["review", 1]],
            "signoff": ["probe/asks", "probe/half-asks"], "next": null}),
    );
    let Some(mut browser) = open(&site, "/tasks", 1280, 900) else {
        return;
    };
    browser.reveal("#column-review");
    assert!(browser.wait_for(
        "document.querySelector('.column[data-status=review] .taskcard') !== null",
        SHOWN
    ));
    browser.reveal("#column-in_progress");
    let reads = [column_read("in_progress"), column_read("review")];
    if let Err(stall) =
        test_time::until_true("both stages' first pages answered", super::STALL, || {
            let answers = site.answers("/api/task-board");
            reads.iter().all(|read| {
                answers
                    .iter()
                    .any(|answer| &answer.target == read && answer.status == 200)
            }) || {
                browser.pump(std::time::Duration::from_millis(25));
                false
            }
        })
    {
        panic!("{stall}: {:?}", site.targets());
    }
    let shows = "(() => { const at = status => { const c = document.querySelector(`.column[data-status=${status}]`);
            return {busy: c.getAttribute('aria-busy'), ghosts: c.querySelectorAll('.ghost-card').length,
                cards: [...c.querySelectorAll('.taskcard')].map(card => card.dataset.slug),
                count: c.querySelector('h2 .count').textContent,
                more: [...c.querySelectorAll('button')].some(button => button.textContent.includes('Load more')),
                shimmer: document.getAnimations().filter(a => a.animationName === 'shimmer' && c.contains(a.effect?.target)).length}; };
        return {in_progress: at('in_progress'), review: at('review'), lane: document.querySelector('.column[data-status=signoff]') !== null}; })()";
    let settled = format!(
        "(s => s.review.busy === 'false' && s.in_progress.busy === 'false' && s.lane === false
            && s.in_progress.cards.length === 2)({shows})"
    );
    assert!(
        browser.wait_for(&settled, SHOWN),
        "a stage kept waiting once its first page answered, its tasks in their original stages: {}",
        browser.eval(shows)
    );
    assert_eq!(
        browser.eval(shows),
        json!({
            "in_progress": {"busy": "false", "ghosts": 0, "cards": ["probe/half-asks", "probe/half-plain"], "count": "2", "more": false, "shimmer": 0},
            "review": {"busy": "false", "ghosts": 0, "cards": ["probe/asks"], "count": "1", "more": false, "shimmer": 0},
            "lane": false}),
        "each stage counts the cards it holds, its tasks in their original stages left out: no ghost, no Load more"
    );
}

/// A column whose first page fails on a quiet board (no feed event, no refresh) is read again by
/// the page itself until the server answers: refused with a 500, then held past the limit a read
/// waits for (`symphonyAnswerMs`, a page global that shortens the 15 s for this test), it keeps
/// its ghost and says why and that it is trying again; once the server answers, its cards draw
/// with no click and the notice goes.
#[test]
fn a_column_whose_first_page_fails_is_read_again_by_itself_until_it_answers() {
    let site = site();
    let task = |slug: &str| {
        json!({"slug": slug, "title": format!("The task {slug}"), "body": "", "status": "ready",
            "priority": 2, "parent": null, "created_by": "owner", "created": 0.0, "updated": 0.0,
            "links": [], "depends": [], "roles": [], "instruction": false})
    };
    site.set_json(
        "/api/task-board",
        &json!({"tasks": [task("probe/one"), task("probe/two")], "statuses": ["ready"],
            "counts": [["ready", 2]], "signoff": [], "next": null}),
    );
    let read = column_read("ready");
    site.fail_route(&read, true);
    let Some(mut browser) = watching(1280, 900, &["window.symphonyAnswerMs = 5000;"]) else {
        return;
    };
    browser.open(&site.url("/tasks"));
    let shows = "(() => { const c = document.querySelector('.column[data-status=ready]');
        return {busy: c?.getAttribute('aria-busy') ?? null, ghosts: c?.querySelectorAll('.ghost-card').length ?? null,
            cards: [...(c?.querySelectorAll('.taskcard') ?? [])].map(card => card.dataset.slug),
            said: [...document.querySelectorAll('.page-notices:not([hidden]) > p[role=alert]')].map(p => p.textContent)}; })()";
    let says = |browser: &mut Chrome, said: &str, why: &str| {
        assert!(
            browser.wait_for(&format!("(s => s.said[0] === '{said}')({shows})"), SHOWN),
            "{why}: {} asked {:?}",
            browser.eval(shows),
            site.targets()
        );
        assert_eq!(
            browser.eval(shows),
            json!({"busy": "true", "ghosts": 2, "cards": [], "said": [said]}),
            "{why}: the column keeps its ghost, and the page says why once"
        );
    };
    says(
        &mut browser,
        "Board snapshot unavailable (the server answered 500). Trying again…",
        "a first page the server refused",
    );
    // Held before the refusal ends, so no read of the column is answered meanwhile.
    site.hold_route(&read, true);
    site.fail_route(&read, false);
    // Only a read of the page's own, asked after the refused one and given up at the limit, says
    // this: nothing else on a quiet board reads the column again.
    says(
        &mut browser,
        "The server did not answer in time. Trying again…",
        "a first page asked again by itself, and held",
    );
    site.hold_route(&read, false);
    let drawn = format!(
        "(s => s.busy === 'false' && s.cards.length === 2 && s.said.length === 0)({shows})"
    );
    assert!(
        browser.wait_for(&drawn, SHOWN),
        "the column was not drawn by itself once the server answered: {} asked {:?}",
        browser.eval(shows),
        site.targets()
    );
    assert_eq!(
        browser.eval(shows),
        json!({"busy": "false", "ghosts": 0, "cards": ["probe/one", "probe/two"], "said": []}),
        "its cards, no ghost, and nothing more said of the failure"
    );
    let asked = site.targets().iter().filter(|one| **one == read).count();
    assert!(
        asked >= 3,
        "refused, held and answered are three reads at least: {asked}"
    );
}

/// The default view's read of the first page of the column `status`, as the board asks for it.
fn column_read(status: &str) -> String {
    format!("/api/task-board?view=waiting&q=&status={status}&days=1&activity=1&work=0")
}

/// What a page animates: the shimmer animations that run (`document.getAnimations()`), and the
/// bars its placeholders hold, the parts not yet within reach of the screen.
const ANIMATED: &str = "(() => ({running: document.getAnimations().filter(a => a.animationName === 'shimmer' && a.playState === 'running').length,
    held: document.querySelectorAll('.viewport-placeholder .bone').length}))()";

/// Wait until `browser`'s page, every read on its screen in, runs no shimmer while its
/// placeholders still hold their bars: a placeholder waits for no read, so its bars are still,
/// and an animation off the screen would run every frame for as long as the page is open.
fn comes_to_rest(browser: &mut Chrome, page: &str) {
    assert!(
        browser.wait_for(
            &format!("(at => at.running === 0 && at.held > 0)({ANIMATED})"),
            SHOWN
        ),
        "{page} never came to rest with its placeholders still waiting (shimmer animations running, placeholders' bars): {}",
        browser.eval(ANIMATED)
    );
}

#[test]
fn a_board_at_rest_animates_none_of_the_rows_out_of_reach_of_the_screen() {
    let site = site();
    // Sixty tasks in one column: a screen of cards and the sentinel's reach drawn, the rest
    // placeholders, each its card's ghost.
    let Some(mut browser) = board_of(&site, 60) else {
        return;
    };
    comes_to_rest(&mut browser, "the board");
}

#[test]
fn the_agents_page_at_rest_animates_none_of_the_agents_out_of_reach_of_the_screen() {
    let site = site();
    let agents: Vec<Value> = (1..=40)
        .map(|n| agent(&format!("agent-{n:02}"), "live"))
        .collect();
    site.set_json("/api/agent-page", &agent_page(&agents, &[]));
    let Some(mut browser) = open(&site, "/agents", 1280, 900) else {
        return;
    };
    assert!(
        browser.wait_for("document.querySelector('.agent') !== null", SHOWN),
        "the agents were never drawn: {}",
        browser.eval(PAGE)
    );
    comes_to_rest(&mut browser, "the agents page");
}

/// A ready task's card and one under review, as the board draws them, each the typical card it
/// guesses for a row it knows nothing of (`tasks::ROW`: a title of two lines, a document linked
/// and a role chip), against the card the board's ghost guesses for each column on a first visit:
/// as tall, and holding its bars.
#[test]
fn a_ghost_card_is_as_tall_as_the_card_of_a_task_with_a_title_of_two_lines() {
    let site = site();
    let typical = |n: u32, status: &str, role: &str| {
        let mut card = task(n, 2);
        card["title"] = json!("A task title long enough to wrap onto two lines");
        card["status"] = json!(status);
        card["links"] = json!(["docs/TASKS.md"]);
        card["roles"] =
            json!([{"handle":"example","role":role,"since":1.0,"expires":null,"health":"live"}]);
        card
    };
    let ready = typical(1, "ready", "executor");
    let taken = typical(2, "review", "reviewer");
    site.set_json(
        "/api/task-board",
        &json!({"tasks": [ready, taken], "statuses": ["ready", "review"], "counts": [["ready", 1], ["review", 1]], "signoff": [], "next": null}),
    );
    // A first visit: the ghost's card in each column, its box with the space under it and whether
    // its bars overflow it; then the board's.
    site.hold_route("/api/task-board", true);
    let Some(mut browser) = open(&site, "/tasks", 1280, 900) else {
        site.hold_route("/api/task-board", false);
        return;
    };
    let pitch = |card: &str| {
        format!(
            "(css => {{ const card = document.querySelector(css), box = card.getBoundingClientRect();
                return [Math.round(box.height + parseFloat(getComputedStyle(card).marginBottom)), card.scrollHeight > card.clientHeight]; }})({card:?})"
        )
    };
    let drawn = browser.wait_for(
        "document.querySelector('.ghost-column[data-for=review] .ghost-card') !== null",
        SHOWN,
    );
    let ghost = browser.eval(&format!(
        "[{}, {}]",
        pitch(".ghost-column[data-for=ready] .ghost-card"),
        pitch(".ghost-column[data-for=review] .ghost-card")
    ));
    site.hold_route("/api/task-board", false);
    assert!(drawn, "no ghost board: {}", browser.eval(PAGE));
    browser.reveal("#column-review");
    assert!(browser.wait_for(
        "document.querySelector('.column[data-status=review] .taskcard') !== null",
        SHOWN
    ));
    browser.reveal("#column-ready");
    assert!(
        browser.wait_for(
            "document.querySelectorAll('.kanban .taskcard').length === 2",
            SHOWN
        ),
        "the task board was never drawn: {}",
        browser.eval(PAGE)
    );
    let page = browser.eval(&format!(
        "[{}, {}]",
        pitch(".column[data-status=ready] .taskcard"),
        pitch(".column[data-status=review] .taskcard")
    ));
    assert_eq!(
        (ghost, page),
        (
            json!([[118, false], [118, false]]),
            json!([[118, false], [118, false]])
        ),
        "the ghost's card for a ready task and for one under review, then the board's"
    );
}

#[test]
fn a_task_before_its_read_draws_its_head_and_brief_cards_its_thread_and_its_composer() {
    let site = site();
    // Revealing the composer also mounts Changes: answer that independent read so the only
    // loading announcement is the task's, and count its card alongside the original four.
    site.set_json(
        symphony_api::TASK_DIFF_PATH,
        &json!({"source":"branch","base":"base","head":"head","files":[],"truncated":false,"reason":null}),
    );
    // Each part is its mock-up until its own read answers: the page's and the thread's held.
    let held = ["/api/task-page", "/api/task-history"];
    for route in held {
        site.hold_route(route, true);
    }
    let Some(mut browser) = open(&site, TASK, 390, 844) else {
        for route in held {
            site.hold_route(route, false);
        }
        return;
    };
    // The thread stands below a phone's first screen, so its cover lifts at once. Its own
    // mock-up and the composer's mount only as their slots near the screen.
    assert!(
        browser.wait_for(
            "document.querySelector('main .ghost-head') !== null && document.querySelector('article.task .settle[data-settled=true]') !== null",
            SHOWN,
        ),
        "no ghost of the task: {}",
        browser.eval(PAGE)
    );
    browser.eval(
        "document.getElementById('composer-visual/in-progress').scrollIntoView({block: 'center'})",
    );
    assert!(
        browser.wait_for(
            "document.querySelector('#task-thread[data-pending=true]') !== null && document.querySelector('article.task .ghost-composer') !== null && document.querySelector('.task-diff > p:last-child')?.textContent === 'No changed files.'",
            SHOWN,
        ),
        "the pending thread and composer never mounted near the screen: {}",
        browser.eval(PAGE)
    );
    // The real head's box and title, laid out beside the ghost as the page will draw them.
    let ghost = browser.eval(&format!(
        "(() => {{ const head = document.querySelector('main .ghost-head'), page = head.parentElement;
            const real = document.createElement('div'); real.className = 'task-head card'; real.innerHTML = '<h1>A title</h1>'; page.append(real);
            const line = parseFloat(getComputedStyle(real.querySelector('h1')).lineHeight); real.remove();
            return {{cards: page.querySelectorAll('.card').length, tokens: page.querySelector('.task-tokens') !== null,
                pills: head.querySelectorAll('.ghost-pill').length,
                entries: page.querySelectorAll('.ghost-history > .ghost-entry').length, fields: page.querySelectorAll('.ghost-composer .ghost-field').length,
                head: {SAME_BOX}(head, 'task-head card'),
                title: Math.round(head.querySelector('.ghost-title').getBoundingClientRect().height / line * 10) / 10,
                said: [...document.querySelectorAll('main [role=status]')].filter(status => !status.closest('[aria-hidden=true]')).map(status => status.textContent),
                busy: page.getAttribute('aria-busy')}}; }})()"
    ));
    assert_eq!(
        ghost,
        json!({"cards": 5, "tokens": false, "pills": 2, "entries": 2, "fields": 4, "head": [], "title": 2,
            "said": ["Loading task…"], "busy": "true"}),
        "the ghost draws the task's head card as the real one, its title two lines of the title's height on a phone, then its gates, timeline, brief, Changes card, thread and composer, no tokens' card after them, and says once that the task loads"
    );
    // The thread answers first: its messages replace their outlines while the head, brief and
    // composer still wait on the page's own read.
    site.hold_route("/api/task-history", false);
    assert!(
        browser.wait_for(
            "document.querySelector('#task-thread[data-pending=false] .entry-text')?.textContent === 'Owner message'",
            SHOWN,
        ),
        "the thread waited for the task's page: {}",
        browser.eval(PAGE)
    );
    assert_eq!(
        browser.eval(
            "({head: document.querySelectorAll('article.task .ghost-head').length, brief: document.querySelectorAll('article.task .ghost-brief').length, thread: document.querySelectorAll('article.task .ghost-history').length, composer: document.querySelectorAll('article.task .ghost-composer').length, form: document.querySelectorAll('#task-send').length})"
        ),
        json!({"head": 1, "brief": 1, "thread": 0, "composer": 1, "form": 0}),
        "the thread draws independently while the page and composer keep their ghosts"
    );
    site.hold_route("/api/task-page", false);
    assert!(
        browser.wait_for("document.querySelector('main .ghost-head') === null && document.querySelector('article.task .task-head h1') !== null && document.querySelector('#task-send') !== null", SHOWN),
        "the task never replaced its ghost: {}",
        browser.eval(PAGE)
    );
    assert_eq!(
        browser.eval(
            "({ghosts: document.querySelectorAll('article.task .ghost-head, article.task .ghost-brief, article.task .ghost-history, article.task .ghost-composer').length, busy: document.querySelector('article.task').getAttribute('aria-busy')})"
        ),
        json!({"ghosts": 0, "busy": null})
    );
}

/// The gates' outline under a task's head (`head`): its rows, how far under the head it starts and
/// its height.
const GATES_UNDER: &str = "(head => { const gates = head?.parentElement.querySelector('.task-gates');
    if (!gates) return null;
    const above = head.getBoundingClientRect(), box = gates.getBoundingClientRect();
    return {rows: gates.querySelectorAll('li.gate-shadow').length, gap: Math.round(box.top - above.bottom), height: Math.round(box.height)}; })";

/// The task's gates outline on its ghost, then on its page while the gates are read, and once they
/// are drawn the height of their section.
fn gates_while_read(site: &Site, browser: &mut Chrome) -> (Value, Value, Value) {
    for route in ["/api/task-page", "/api/task-gates"] {
        site.hold_route(route, true);
    }
    browser.open(&site.url(TASK));
    let drawn = browser.wait_for("document.querySelector('main .ghost-head') !== null", SHOWN);
    let ghost = browser.eval(&format!(
        "{GATES_UNDER}(document.querySelector('main .ghost-head'))"
    ));
    site.hold_route("/api/task-page", false);
    // The page is in and its gates are still read: the section's own outline.
    let page = browser.wait_for(
        "document.querySelector('article.task .task-head h1') !== null && document.querySelector('article.task .task-gates[data-gates=reading]') !== null",
        SHOWN,
    );
    let outline = browser.eval(&format!(
        "{GATES_UNDER}(document.querySelector('article.task .task-head'))"
    ));
    site.hold_route("/api/task-gates", false);
    assert!(drawn, "no ghost of the task: {}", browser.eval(PAGE));
    assert!(
        page,
        "the task never drew its gates' outline: {}",
        browser.eval(PAGE)
    );
    assert!(
        browser.wait_for(
            "document.querySelector('.task-gates[data-gates=read]') !== null",
            SHOWN
        ),
        "the gates were never drawn: {}",
        browser.eval(PAGE)
    );
    frames(browser);
    let read = browser.eval(
        "Math.round(document.querySelector('.task-gates[data-gates=read]').getBoundingClientRect().height)",
    );
    (ghost, outline, read)
}

#[test]
fn a_task_before_its_read_holds_its_gates_outline_where_the_page_draws_it() {
    let site = site();
    // A task the server has answered no gates for yet: its page says nothing of them.
    site.set_json(
        "/api/task-page",
        &super::look_parity::responses()["/api/task-page"],
    );
    let Some(mut browser) = open(&site, "/", 390, 844) else {
        return;
    };
    // No copy of the task on this device, which would draw its next visit at once with no ghost.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": NO_STORE }),
    );
    let (ghost, outline, read) = gates_while_read(&site, &mut browser);
    assert_eq!(
        ghost["rows"],
        json!(4),
        "a browser that has seen none of the task's gates outlines four: {ghost}"
    );
    assert_eq!(
        ghost, outline,
        "the ghost's gates outline is the one the page holds until their answer, as far under the head"
    );
    // The next visit: this browser kept the two gates it drew, at their height.
    let (ghost, outline, again) = gates_while_read(&site, &mut browser);
    assert_eq!(
        (&ghost["rows"], &ghost["height"]),
        (&json!(2), &read),
        "a browser that drew the task's gates outlines as many, as tall: {ghost}"
    );
    assert_eq!(ghost, outline);
    assert_eq!(again, read);
}

#[test]
fn a_part_of_a_document_not_yet_near_the_screen_is_lines_of_text_not_a_slab() {
    let site = site();
    let Some(mut browser) = open(&site, "/docs/PROBE.md", 390, 844) else {
        return;
    };
    assert!(
        browser.wait_for(
            "document.querySelector('.settle[data-settled=true] .doc-body .viewport-placeholder .bone') !== null",
            SHOWN
        ),
        "no part of the document waits below the screen: {}",
        browser.eval(PAGE)
    );
    assert_eq!(
        browser.eval(
            "(() => { const box = document.querySelector('.doc-body .viewport-placeholder'), bars = [...box.querySelectorAll('.bone')];
                return {slab: getComputedStyle(box).backgroundColor, bars: bars.length, height: Math.round(bars[0].getBoundingClientRect().height),
                    colour: getComputedStyle(bars[0]).backgroundColor}; })()"
        ),
        json!({"slab": "rgba(0, 0, 0, 0)", "bars": 6, "height": 13, "colour": "rgba(127, 127, 127, 0.14)"}),
        "a section's 160 px stand for six lines of text, each a bar of the line's own height"
    );
}

#[test]
fn a_document_before_its_read_draws_its_head_its_outline_and_its_body_where_they_will_be() {
    let site = site();
    site.hold_route("/api/document", true);
    let Some(mut browser) = open(&site, "/docs/PROBE.md", 1280, 900) else {
        site.hold_route("/api/document", false);
        return;
    };
    let drawn = browser.wait_for("document.querySelector('.ghost-doc-body') !== null", SHOWN);
    let ghost = browser.eval(
        "(() => { const body = document.querySelector('.ghost-doc-body');
            return {status: document.querySelector('.document-page [role=status]')?.textContent,
                head: document.querySelectorAll('.ghost-doc-head .bone').length, outline: document.querySelector('.ghost-toc')?.checkVisibility() === true,
                headings: body.querySelectorAll('.ghost-heading').length, bars: body.querySelectorAll('.bone').length,
                left: Math.round(body.getBoundingClientRect().left)}; })()",
    );
    site.hold_route("/api/document", false);
    assert!(drawn, "no ghost of the document: {}", browser.eval(PAGE));
    assert!(
        browser.wait_for("document.querySelector('.ghost-doc') === null && document.querySelector('.settle[data-settled=true] .doc-body') !== null", SHOWN),
        "the document never replaced its ghost: {}",
        browser.eval(PAGE)
    );
    let left = browser
        .eval("Math.round(document.querySelector('.doc-body').getBoundingClientRect().left)");
    assert_eq!(
        ghost,
        json!({"status": "Loading document…", "head": 5, "outline": true, "headings": 2, "bars": 13, "left": left}),
        "the ghost draws the path, title and links, the outline beside the body on a wide screen, and the body's card of headings and paragraphs where the body will be"
    );
}

#[test]
fn the_queue_before_its_read_draws_its_group_cards_with_the_rows_they_hold() {
    let site = site();
    site.hold_route("/api/queue", true);
    let Some(mut browser) = open(&site, "/queue", 390, 844) else {
        site.hold_route("/api/queue", false);
        return;
    };
    let drawn = browser.wait_for(
        "document.querySelector('.queue-page .ghost-group') !== null",
        SHOWN,
    );
    let ghost = browser.eval(&format!(
        "(() => {{ const groups = [...document.querySelectorAll('.queue-page .ghost-group')], first = groups[0];
            return {{status: document.querySelector('.queue-page [role=status]')?.textContent,
                rows: groups.map(group => group.querySelectorAll(':scope > .ghost-row').length),
                chips: first.querySelectorAll('.ghost-chips > .ghost-pill').length,
                ranked: groups.map(group => group.querySelectorAll('.ghost-rank').length),
                later: groups.map(group => group.querySelectorAll('.ghost-later').length),
                group: {SAME_BOX}(first, 'card queue-group'), row: {SAME_BOX}(first.querySelector('.ghost-row'), 'queue-row'),
                rank: {SAME_BOX}(first.querySelector('.ghost-rank'), 'queue-rank')}}; }})()"
    ));
    site.hold_route("/api/queue", false);
    assert!(drawn, "no ghost of the queue: {}", browser.eval(PAGE));
    assert_eq!(
        ghost,
        json!({"status": "Loading the queue…", "rows": [4, 2, 2], "chips": 2, "ranked": [4, 0, 0],
            "later": [4, 0, 0], "group": [], "row": [], "rank": []}),
        "the queue's ghost draws each group's card as the real one; what waits on the owner opens on its topic chips and ranks its rows, each with its Later button"
    );
    assert!(
        browser.wait_for("document.querySelector('.ghost-group') === null && document.querySelector('[data-ready=queue] #you') !== null", SHOWN),
        "the queue never replaced its ghost: {}",
        browser.eval(PAGE)
    );
}

/// Let the page draw three more frames: what lands is drawn in one, and held in the next.
fn frames(browser: &mut Chrome) {
    browser.eval("window.__framed = false; requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(() => { window.__framed = true; })))");
    assert!(browser.wait_for("window.__framed === true", SHOWN));
}

/// A page expression: whether a region waiting for its read (`[data-pending=true]`) lies within its
/// sentinel's reach, 600 px above and below what its own scroller shows of the window, as the
/// shell's hold (`reviewHoldPlace`) and `viewport::waits_near` decide: one past it never draws.
const WAITS_NEAR: &str = "[...document.querySelectorAll('[data-pending=true]')].some(box => {
    const scroller = node => { while ((node = node.parentElement) && !(node.scrollHeight > node.clientHeight && /auto|scroll/.test(getComputedStyle(node).overflowY))); return node; };
    const reaches = (r, clip) => r.bottom > r.top && r.right > r.left
        && r.bottom > Math.max(0, clip?.top ?? 0) - 600 && r.top < Math.min(innerHeight, clip?.bottom ?? innerHeight) + 600
        && r.right > Math.max(0, clip?.left ?? 0) && r.left < Math.min(innerWidth, clip?.right ?? innerWidth);
    const r = box.getBoundingClientRect();
    return reaches(r) && reaches(r, scroller(box)?.getBoundingClientRect());
})";

/// Wait until the page is still: the top of `selector`'s box has stayed within 0.5 px for five
/// frames in a row, and no region waiting for its read lies within its sentinel's reach
/// ([`WAITS_NEAR`]), so nothing near it is about to draw.
fn still(browser: &mut Chrome, selector: &str) {
    browser.eval(&format!(
        "window.__still = false;
        (function frame(last, calm) {{
            const node = document.querySelector({}), top = node ? node.getBoundingClientRect().top : NaN;
            const near = {WAITS_NEAR};
            calm = node && !near && Math.abs(top - last) <= 0.5 ? calm + 1 : 0;
            if (calm >= 5) window.__still = true; else requestAnimationFrame(() => frame(top, calm));
        }})(NaN, 0)",
        json!(selector)
    ));
    assert!(
        browser.wait_for("window.__still === true", SHOWN),
        "the page never stood still around {selector}: {}",
        browser.eval(PAGE)
    );
}

/// The top of `selector`'s box, and the page's scroll offset.
fn place(browser: &mut Chrome, selector: &str) -> (f64, f64) {
    let at = browser.eval(&format!(
        "({{top: document.querySelector({}).getBoundingClientRect().top, scroll: scrollY}})",
        json!(selector)
    ));
    (
        at["top"].as_f64().unwrap_or(f64::NAN),
        at["scroll"].as_f64().unwrap_or(f64::NAN),
    )
}

/// The page's sizes, `[scrollWidth, clientWidth, innerWidth, innerHeight, clientHeight, scrollY]`
/// of its document and window, at the first frame and at every frame that changes one, until
/// [`SIZES`] is read: a scroll width past the client's is a horizontal overflow, which puts a
/// scrollbar along the window's foot, and a client width or height short of the window's inner one
/// is the room a classic scrollbar takes, which an overlay one does not.
const WATCH_SIZES: &str = "window.__sizes = []; window.__sized = true;
(function frame() {
    const page = document.documentElement, now = [page.scrollWidth, page.clientWidth, innerWidth, innerHeight, page.clientHeight, scrollY];
    const last = window.__sizes[window.__sizes.length - 1];
    if (!last || now.some((size, at) => size !== last[at])) window.__sizes.push(now);
    if (window.__sized && window.__sizes.length < 100) requestAnimationFrame(frame);
})()";

/// The sizes [`WATCH_SIZES`] recorded, with the page's at this moment last; it stops recording.
const SIZES: &str = "(() => { window.__sized = false; const page = document.documentElement;
    return [...window.__sizes, [page.scrollWidth, page.clientWidth, innerWidth, innerHeight, page.clientHeight, scrollY]]; })()";

/// A page script under which, once the test sets `window.__late` to a count of frames, every
/// `IntersectionObserver` callback comes that many frames late, as on a slow phone: a region the
/// page reserves draws that many frames after it comes near the screen.
const LATE_DRAWS: &str = r#"window.__late = 0;
const Observer = window.IntersectionObserver;
window.IntersectionObserver = class extends Observer {
    constructor(callback, options) {
        super((entries, observer) => {
            let left = window.__late;
            (function wait() { if (left-- > 0) requestAnimationFrame(wait); else callback(entries, observer); })();
        }, options);
    }
};"#;

/// Counting frames from now (`window.__drawn`): the first in which the message `#message-7` holds
/// the box it reserves, and the first in which its own box has taken that box's place.
const WATCH_DRAWN: &str = "window.__drawn = {reserved: null, drawn: null};
(function frame(at) {
    const slot = document.getElementById('message-7'), waiting = slot?.querySelector('[data-pending=true]');
    if (waiting && __drawn.reserved === null) __drawn.reserved = at;
    if (slot && !waiting) __drawn.drawn = at; else requestAnimationFrame(() => frame(at + 1));
})(0)";

#[test]
fn a_message_that_lands_above_the_composer_leaves_the_owner_typing_in_place() {
    // A busy machine's frames: the message's box gives way to it frames after it is drawn.
    message_lands_above_the_composer(
        6,
        0,
        0,
        &[
            "An answer that lands while the owner types, long enough to wrap over a few lines of a phone's thread.",
        ],
    );
}

#[test]
fn a_message_that_lands_while_the_pages_tasks_run_late_leaves_the_owner_typing_in_place() {
    // A loaded machine's renderer: every task React's scheduler posts runs twelve frames late
    // ([`LATE_TASKS`]), the message's draw among them. The page draws it within the event that
    // brings it, so the shell's hold, which ends once four frames pass with nothing moved, starts
    // from the message in the page and lasts while its box waits to draw, never ending before it.
    message_lands_above_the_composer(
        1,
        0,
        12,
        &[
            "An answer that lands while the owner types, drawn by a renderer that runs its frames first.",
            "Its second paragraph, long enough to wrap over a few lines of a phone's thread like the first.",
        ],
    );
}

#[test]
fn a_message_drawn_late_above_the_composer_leaves_the_owner_typing_in_place() {
    // A slow phone: the message's own box, far taller than the one it reserves, comes ninety frames
    // after that box: the hold lasts until it is drawn, however many frames that takes.
    message_lands_above_the_composer(
        1,
        90,
        0,
        &[
            "An answer that lands while the owner types, drawn long after its place in the thread is kept for it.",
            "Its second paragraph, long enough to wrap over a few lines of a phone's thread like the first.",
            "A third, so the message stands far taller than the box the thread kept for it.",
            "And a fourth, the last of them.",
        ],
    );
}

/// The task's page on a phone, scrolled down to its composer with a reply typed in it, as a message
/// of `paragraphs` lands above it from the feed on a machine `throttle` times slower, the message's
/// own box drawn `late` frames after the box it reserves ([`LATE_DRAWS`]) and every task of React's
/// scheduler run `late_tasks` frames late from the landing ([`LATE_TASKS`]): the composer keeps its
/// place, the focus and the reply.
fn message_lands_above_the_composer(
    throttle: u32,
    late: u32,
    late_tasks: u32,
    paragraphs: &[&str],
) {
    let site = site();
    // A thread long enough to put the composer below the first screen.
    let entries: Vec<Value> = (1..=6)
        .map(|id| json!({"message": {"id": id, "slug": "visual/in-progress", "sender": "owner", "recipient": null,
            "kind": "message", "text": "An earlier message of the thread, long enough to wrap onto a second line.", "ts": 1.0},
            "stamp": "2026-11-10", "blocks": []}))
        .collect();
    site.set_json(
        "/api/task-history",
        &json!({"entries": entries, "before": null, "after": 6}),
    );
    let Some(mut browser) = watching(390, 844, &[LATE_DRAWS, LATE_TASKS]) else {
        return;
    };
    browser.open(&site.url(TASK));
    assert!(
        browser.wait_for(
            "document.querySelector('.settle[data-settled=true] .history') !== null && document.querySelector('[id^=composer-]') !== null",
            SHOWN
        ),
        "the task and its thread were never drawn: {}",
        browser.eval(PAGE)
    );
    // The owner goes down to the composer, under the timeline's box, the brief and the thread.
    browser.eval("document.querySelector('[id^=composer-]').scrollIntoView({block: 'end'})");
    assert!(
        browser.wait_for(
            "document.querySelector('[id^=composer-] textarea') !== null",
            SHOWN
        ),
        "the composer was never drawn: {}",
        browser.eval(PAGE)
    );
    wait_for_feed(&site).expect("the page never followed the feed");
    let composer = "[id^=composer-] textarea";
    browser.eval(&format!(
        "(() => {{ const field = document.querySelector({}); field.scrollIntoView({{block: 'end'}}); field.focus(); }})()",
        json!(composer)
    ));
    browser.call(
        "Input.insertText",
        json!({"text": "A reply the owner is typing"}),
    );
    // The way down brought the thread's last messages near the screen, each drawn in place of the
    // box it reserved, and the thread's region, once none waits, laid out again: the composer's
    // place is read once those have drawn, as no live update is there to hold it while they do.
    still(&mut browser, composer);
    let (before, scrolled) = place(&mut browser, composer);
    browser.eval(WATCH_SIZES);
    assert!(
        scrolled > 0.0,
        "the composer must sit below the first screen for the page to have room to hold it"
    );
    if throttle > 1 {
        browser.call(
            "Emulation.setCPUThrottlingRate",
            json!({ "rate": throttle }),
        );
    }
    if late_tasks > 0 {
        assert_eq!(
            browser.eval(LATE_TASKS_TAKEN),
            json!(true),
            "React's scheduler takes no task through the page script's channel, so none would run late"
        );
    }
    browser.eval(&format!("window.__late = {late}"));
    browser.eval(&format!("window.__lateTasks = {late_tasks}"));
    browser.eval(WATCH_DRAWN);
    let blocks: Vec<Value> = paragraphs
        .iter()
        .map(|text| json!({"type": "paragraph", "spans": [{"type": "text", "text": text}]}))
        .collect();
    let message = json!({"id": 7, "slug": "visual/in-progress", "sender": "agent", "recipient": null,
        "kind": "message", "text": "", "ts": 2.0, "stamp": "2026-11-11", "blocks": blocks});
    site.push(
        4183,
        &json!({"version": 4183, "changes": [{"region": "message", "data": message}]}).to_string(),
    );
    assert!(
        browser.wait_for(
            "document.querySelectorAll('#task-thread .entry').length === 7",
            SHOWN
        ),
        "the message never landed: {}",
        browser.eval(PAGE)
    );
    // The shell holds the composer in place frame by frame until four pass with nothing moved and
    // no box near the screen waiting to draw (`reviewHoldPlace`): a loaded machine's frames can
    // still be in that hold three frames after the message lands, so the composer is read once the
    // page stands still around it.
    still(&mut browser, composer);
    let (after, _) = place(&mut browser, composer);
    browser.eval("window.__lateTasks = 0");
    let sizes = browser.eval(SIZES);
    let drawn = browser.eval("window.__drawn");
    eprintln!(
        "skeleton: the composer at {before} then {after}; the message's box reserved and drawn at frames {drawn}; [scrollWidth, clientWidth, innerWidth, innerHeight, clientHeight, scrollY] as each changed: {sizes}"
    );
    if late > 0 {
        let frames = drawn["drawn"].as_u64().zip(drawn["reserved"].as_u64());
        assert!(
            frames.is_some_and(|(drawn, reserved)| drawn >= reserved + u64::from(late)),
            "the message's own box must come {late} frames after the box it reserves, at frames {drawn}"
        );
    }
    assert_eq!(
        browser.eval(&format!(
            "(() => {{ const field = document.querySelector({}); return {{focused: document.activeElement === field, text: field.value}}; }})()",
            json!(composer)
        )),
        json!({"focused": true, "text": "A reply the owner is typing"})
    );
    assert!(
        (after - before).abs() < 1.0,
        "the composer moved from {before} to {after}; the message's box reserved and drawn at frames {drawn}; [scrollWidth, clientWidth, innerWidth, innerHeight, clientHeight, scrollY] as each changed: {sizes}"
    );
}

#[test]
fn the_chat_keeps_its_form_at_the_foot_of_its_sheet_with_new_conversation_just_above() {
    let site = site();
    let chat = layout_shift::CHAT;
    let Some(mut browser) = chrome() else {
        return;
    };
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 390, "height": 844, "deviceScaleFactor": 1, "mobile": true}),
    );
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": chat.held }),
    );
    browser.open(&site.url("/"));
    assert!(
        browser.wait_for(
            &format!("document.querySelector({}) !== null", json!(chat.control)),
            SHOWN
        ),
        "the chat's control was never drawn: {}",
        browser.eval(PAGE)
    );
    browser.eval(&format!(
        "document.querySelector({}).click()",
        json!(chat.control)
    ));
    assert!(
        browser.wait_for(chat.shown, SHOWN),
        "the conversation never landed: {}",
        browser.eval(PAGE)
    );
    let at = browser.eval(
        "(() => { const sheet = document.getElementById('quick-sheet'), box = s => sheet.querySelector(s).getBoundingClientRect();
            const thread = box('.quick-thread'), restart = box('.quick-restart'), form = box('form');
            return {thread: [thread.bottom], restart: [restart.top, restart.bottom], form: [form.top, form.bottom], sheet: [sheet.getBoundingClientRect().bottom]}; })()",
    );
    let edge = |key: &str, index: usize| at[key][index].as_f64().unwrap_or(f64::NAN);
    // In this order, top to bottom, none over the next, the form inside the sheet.
    assert!(
        edge("thread", 0) <= edge("restart", 0) + 0.5
            && edge("restart", 1) <= edge("form", 0) + 0.5
            && edge("form", 1) <= edge("sheet", 0) + 0.5,
        "the thread, New conversation and the form overlap or leave the sheet: {at}"
    );
}

/// The row the owner reads on the task board.
const READ: &str = "[data-slug=\"tools/t-08\"]";

/// Each frame for about a second: the read row's top, the board scroller's offset, the page's and
/// the tops of the reserved boxes still waiting; and each scroller offset the page sets.
const TRACE: &str = "window.__trace = [];
const offset = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollTop');
Object.defineProperty(Element.prototype, 'scrollTop', {configurable: true, get() { return offset.get.call(this); }, set(value) { window.__trace.push(['set', Math.round(performance.now()), value]); offset.set.call(this, value); }});
(function frame(left) { const row = document.querySelector('[data-slug=\"tools/t-08\"]'), board = document.querySelector('.waiting-columns'); window.__trace.push([Math.round(performance.now()), row ? row.getBoundingClientRect().top : null, scrollY, scrollY, [...document.querySelectorAll('[data-pending=true]')].map(box => Math.round(box.getBoundingClientRect().top))]); if (left > 0) requestAnimationFrame(() => frame(left - 1)); })(60)";

fn task(n: u32, priority: u32) -> Value {
    json!({"slug": format!("tools/t-{n:02}"), "title": format!("Task {n}"), "body": "",
        "status": "ready", "priority": priority, "parent": null, "created_by": "owner",
        "created": 0.0, "updated": 0.0, "links": [], "depends": [], "roles": [],
        "instruction": false})
}

/// Push `row` onto the board's feed as `tools/t-00` at `version`; `None` removes it.
fn land(site: &Site, version: u64, row: Option<Value>) {
    let data = match row {
        Some(row) => json!({"key": "tools/t-00", "value": row}),
        None => json!({"key": "tools/t-00", "removed": true}),
    };
    site.push(
        version,
        &json!({"version": version, "changes": [{"region": "task", "data": data}]}).to_string(),
    );
}

/// The task board at 1280 x 900 with fourteen ready tasks at p2, following the feed.
fn board_of_fourteen(site: &Site) -> Option<Chrome> {
    board_of(site, 14)
}

/// The task board at 1280 x 900 with `count` ready tasks at p2, following the feed.
fn board_of(site: &Site, count: u32) -> Option<Chrome> {
    let tasks: Vec<Value> = (1..=count).map(|n| task(n, 2)).collect();
    site.set_json(
        "/api/task-board",
        &json!({"tasks": tasks, "statuses": ["ready"], "counts": [["ready", count]], "signoff": [], "next": null}),
    );
    let mut browser = open(site, "/tasks", 1280, 900)?;
    assert!(
        browser.wait_for(
            "document.querySelector('[data-slug=\"tools/t-01\"]') !== null",
            SHOWN
        ),
        "the task board was never drawn: {}",
        browser.eval(PAGE)
    );
    wait_for_feed(site).expect("the page never followed the feed");
    Some(browser)
}

/// [`board_of_fourteen`], the board scrolled down its column to `READ`'s row; and where that row
/// then is.
fn board_scrolled_to_a_row(site: &Site) -> Option<(Chrome, f64)> {
    let mut browser = board_of_fourteen(site)?;
    browser.eval("(() => { const row = document.querySelector('[id=\"row-tools/t-08\"]'); scrollBy(0, row.getBoundingClientRect().top - 120); })()");
    assert!(
        browser.wait_for(
            &format!("document.querySelector({}) !== null", json!(READ)),
            SHOWN
        ),
        "the row read never drew"
    );
    // The rows above it may still be drawing, each from its box's guess to its card.
    still(&mut browser, READ);
    let (top, _) = place(&mut browser, READ);
    assert!(
        browser.eval("scrollY") != json!(0),
        "the row must sit below the board's first screen"
    );
    browser.call("Emulation.setCPUThrottlingRate", json!({"rate": 6}));
    Some((browser, top))
}

#[test]
fn a_task_added_or_removed_above_the_row_the_owner_reads_leaves_it_in_place() {
    let site = site();
    let Some((mut browser, before)) = board_scrolled_to_a_row(&site) else {
        return;
    };
    browser.eval(TRACE);
    land(&site, 4183, Some(task(0, 0)));
    // Its reserved box gives way to the row once it is near the screen.
    assert!(
        browser.wait_for(
            "document.querySelector('[id=\"row-tools/t-00\"]') !== null",
            SHOWN
        ),
        "the added task never landed: {}",
        browser.eval(PAGE)
    );
    still(&mut browser, READ);
    let (added, _) = place(&mut browser, READ);
    let trace = browser.eval("window.__trace");
    land(&site, 4184, None);
    assert!(
        browser.wait_for(
            "document.querySelector('[id=\"row-tools/t-00\"]') === null",
            SHOWN
        ),
        "the removed task never left"
    );
    still(&mut browser, READ);
    let (removed, _) = place(&mut browser, READ);
    assert!(
        (added - before).abs() < 1.0 && (removed - before).abs() < 1.0,
        "the row read moved from {before} to {added} when a task was added above it, and to {removed} when it was removed; [ms, row top, board scroll, page scroll] each frame from the add: {trace}"
    );
}

/// In the frame after the added task is drawn, the owner's own wheel scrolls the board 300 px
/// further down; the board's offset then, and a second later.
const SCROLL_DURING_THE_HOLD: &str = r#"window.__scrolled = null;
new MutationObserver((records, observer) => {
    if (!document.getElementById('row-tools/t-00')) return;
    observer.disconnect();
    requestAnimationFrame(() => {
        const board = document.querySelector('.waiting-columns');
        board.dispatchEvent(new WheelEvent('wheel', {deltaY: 300, bubbles: true}));
        scrollBy(0, 300);
        const then = scrollY;
        setTimeout(() => { window.__scrolled = {then, later: scrollY}; }, 1000);
    });
}).observe(document.body, {subtree: true, childList: true});"#;

#[test]
fn the_owner_s_own_scroll_while_a_task_lands_is_never_scrolled_back() {
    let site = site();
    let Some((mut browser, _)) = board_scrolled_to_a_row(&site) else {
        return;
    };
    browser.eval(SCROLL_DURING_THE_HOLD);
    land(&site, 4183, Some(task(0, 0)));
    assert!(
        browser.wait_for("window.__scrolled !== null", SHOWN),
        "the added task never landed: {}",
        browser.eval(PAGE)
    );
    let scrolled = browser.eval("window.__scrolled");
    let (then, later) = (
        scrolled["then"].as_f64().unwrap_or(f64::NAN),
        scrolled["later"].as_f64().unwrap_or(f64::NAN),
    );
    // The owner's scroll took (the board had room for it), and the hold let it be: the new row
    // drawn in place of its 116 px reserved box may move the board after it, never 300 px back.
    assert!(then >= 300.0, "the owner's scroll never took: {scrolled}");
    assert!(
        (later - then).abs() < 116.0,
        "the board was scrolled back from {then} to {later} against the owner's own scroll"
    );
}
