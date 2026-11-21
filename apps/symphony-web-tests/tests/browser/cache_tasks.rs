//! A task page drawn from this device's cache on a return visit (docs/analysis/client-cache.md,
//! "A return visit, step by step"; apps/symphony-web/src/task-live.ts). With the server's reads held back, the page
//! draws the task's brief and its thread's newest messages before any answer, marked as possibly
//! out of date, its actions waiting and its draft editable. Once the board's version is read it
//! asks whether each changed: an unchanged task is a `304` with no body, and a message posted or a
//! brief edited while away is a `200`, drawn and kept. The landing offer is read on every visit and
//! never kept. A check that fails is asked again until it answers, and the page says the server is
//! not reachable while the board's read fails. A task gone since says so and is forgotten, and the
//! 31st task opened drops the one opened longest ago. The release build, on the private stand-in
//! (`site`).

use serde_json::{Value, json};

use crate::cache_pages::{LIVE, PARTS, keeps, kept, notice, shows, store_opened};
use crate::chrome::Chrome;
use crate::site::{Answered, Site};
use crate::{common, look_parity};

/// The look-parity fixture's task.
const SLUG: &str = "visual/in-progress";
/// What this device keeps of it, as [`PARTS`] names them.
const KEPT: [&str; 2] = [
    "task:visual/in-progress page",
    "task:visual/in-progress thread",
];
/// Every read the task page makes as it opens, which the stand-in holds back to show what the
/// page draws before any answer.
pub(super) const READS: [&str; 8] = [
    "/api/board",
    "/api/task-page",
    "/api/task-history",
    "/api/task-offer",
    "/api/task-gates",
    "/api/task-timeline",
    "/api/activity",
    "/api/pending",
];
/// The message box's draft, which stays editable whatever the page shows.
const DRAFT: &str = "#task-message";

fn route(slug: &str) -> String {
    format!("/task/{slug}")
}

/// The read of `path` for `slug`, its query as the page asks it.
fn target(path: &str, slug: &str) -> String {
    format!("{path}?slug={}", slug.replace('/', "%2F"))
}

/// The fixture task's page and its thread, and no landing offer.
fn site() -> Site {
    let site = Site::start(crate::dist(), &common::fixture("board.json"));
    let content = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site.set_json("/api/task-offer", &offer(None));
    site
}

fn offer(payload: Option<&str>) -> Value {
    json!({"action": "approve", "label": "Land it", "branch": SLUG, "payload": payload})
}

fn page() -> Value {
    look_parity::responses()["/api/task-page"].clone()
}

fn thread() -> Value {
    look_parity::responses()["/api/task-history"].clone()
}

/// The thread with a message `id` saying `text` after the fixture's own.
fn with_message(id: i64, text: &str) -> Value {
    let mut thread = thread();
    let mut entry = thread["entries"][0].clone();
    entry["message"]["id"] = json!(id);
    entry["message"]["text"] = json!(text);
    thread["entries"].as_array_mut().unwrap().push(entry);
    thread["after"] = json!(id);
    thread
}

/// The task page shows `slug`'s brief, `brief`, and its thread has drawn `said`. Each message
/// mounts as it nears the screen: the page is scrolled to the thread's end, its newest.
pub(super) fn draws(chrome: &mut Chrome, slug: &str, brief: &str, said: &str) {
    shows(
        chrome,
        &format!(
            "(task => (window.scrollTo(0, document.body.scrollHeight), task?.querySelector('.task-body')?.innerText.includes({}) && [...task.querySelectorAll('#task-thread .entry')].some(entry => entry.innerText.includes({}))))(document.querySelector({}))",
            json!(brief),
            json!(said),
            json!(format!("article.task[data-slug='{slug}']")),
        ),
        &format!("{slug}'s brief and thread"),
    );
}

/// A first visit to the fixture task: drawn from the server, live, and its page and thread kept.
fn visit(chrome: &mut Chrome, site: &Site) {
    store_opened(chrome, site);
    chrome.open(&site.url(&route(SLUG)));
    draws(chrome, SLUG, "Task brief", "Owner message");
    shows(chrome, LIVE, "the first visit");
    keeps(chrome, &KEPT);
}

/// When the older of the task's kept parts was kept.
fn oldest(chrome: &mut Chrome) -> f64 {
    kept(chrome)
        .iter()
        .filter(|(part, _)| KEPT.contains(&part.as_str()))
        .map(|(_, saved)| saved.as_f64().unwrap())
        .fold(f64::INFINITY, f64::min)
}

/// How many answers the stand-in has sent to each of [`READS`].
pub(super) fn answered(site: &Site) -> Vec<usize> {
    READS.iter().map(|read| site.answers(read).len()).collect()
}

/// The answers to `read` since `before` ([`answered`]).
pub(super) fn since(site: &Site, before: &[usize], read: &str) -> Vec<Answered> {
    let at = READS.iter().position(|each| *each == read).unwrap();
    site.answers(read)[before[at]..].to_vec()
}

pub(super) fn hold(site: &Site, reads: &[&str], held: bool) {
    for read in reads {
        site.hold_route(read, held);
    }
}

/// Open the fixture task again with `reads` held back; the answers counted before it.
fn return_held(chrome: &mut Chrome, site: &Site, reads: &[&str]) -> Vec<usize> {
    let before = answered(site);
    hold(site, reads, true);
    chrome.open("about:blank");
    chrome.open(&site.url(&route(SLUG)));
    before
}

/// The page shows what this device kept, saying `said` of it. It is asked right after the page
/// opens, before its module (the task page's script and the chunks it imports) has drawn a
/// `main`: none yet is not yet, never a failure.
#[track_caller]
fn from_the_device(chrome: &mut Chrome, said: &str) {
    shows(
        chrome,
        &format!(
            "(main => main?.dataset.feed === 'stale' && main.dataset.cache === 'device' && document.querySelector('.page-notices .device-notice')?.textContent === {})(document.querySelector('main'))",
            json!(said)
        ),
        "the notice of a copy from this device",
    );
}

/// The task's buttons, Cancel task and Send, as `[text, disabled]`, and whether its priority is
/// disabled. The message box mounts as it nears the screen, and stays hidden under the thread's
/// ghost until the page settles (`Settle`), taking no focus meanwhile: the page is scrolled to it
/// first, and it is waited for until it shows.
fn actions(chrome: &mut Chrome) -> Value {
    chrome.eval("window.scrollTo(0, document.body.scrollHeight)");
    shows(chrome, &crate::visible(DRAFT), "the message box");
    chrome.eval(
        r#"[[...document.querySelectorAll("article.task .task-controls > button, #task-send button[type=submit]")]
            .map(button => [button.textContent.trim(), button.disabled]),
            document.getElementById("task-priority")?.hasAttribute("disabled")]"#,
    )
}

/// Every action of the task's is held, its buttons saying it is checking, and the draft takes
/// typing.
#[track_caller]
fn actions_wait(chrome: &mut Chrome) {
    let actions = actions(chrome);
    let held = actions[0].as_array().unwrap();
    assert_eq!(held.len(), 2, "Cancel task and Send: {actions}");
    for action in held {
        assert_eq!(action, &json!(["Checking…", true]), "{actions}");
    }
    assert_eq!(actions[1], json!(true), "the priority waits: {actions}");
    let typed = chrome.eval(&format!(
        "(field => {{ field.focus(); field.select(); document.execCommand('insertText', false, 'kept draft'); return [field.disabled, field.value]; }})(document.querySelector({}))",
        json!(DRAFT)
    ));
    assert_eq!(
        typed,
        json!([false, "kept draft"]),
        "the draft stays editable"
    );
}

/// Each read of `answers` named a tag, was answered `304`, and asked once.
#[track_caller]
pub(super) fn unchanged_once(answers: &[Answered], what: &str) {
    assert_eq!(answers.len(), 1, "{what}: {answers:#?}");
    let answer = &answers[0];
    assert!(
        answer
            .named
            .as_deref()
            .is_some_and(|tag| tag.starts_with("W/\"")),
        "{what}: {answer:?} names the kept copy's tag"
    );
    assert_eq!(answer.status, 304, "{what}: {answer:?}");
}

/// The board, read fresh and once: its version starts the feed.
#[track_caller]
fn board_read_fresh(site: &Site, before: &[usize]) {
    let board = since(site, before, "/api/board");
    assert_eq!(
        board
            .iter()
            .map(|answer| (answer.named.is_some(), answer.status))
            .collect::<Vec<_>>(),
        [(false, 200)],
        "{board:#?}"
    );
}

/// What the page's reads of the API took on the wire since it was opened, each by its path.
fn api_bytes(chrome: &mut Chrome) -> Vec<(String, i64, f64)> {
    chrome
        .requests()
        .into_iter()
        .filter(|request| request.url.contains("/api/") && request.finished)
        .map(|request| {
            let path = request
                .url
                .split_once("/api/")
                .map_or(String::new(), |(_, rest)| format!("/api/{rest}"));
            (path, request.status, request.bytes)
        })
        .collect()
}

#[test]
fn a_warm_task_page_draws_its_brief_and_newest_messages_before_any_answer_and_an_unchanged_task_is_a_304_with_no_body()
 {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    let first = api_bytes(&mut chrome);
    let saved = oldest(&mut chrome);
    let before = return_held(&mut chrome, &site, &READS);
    draws(&mut chrome, SLUG, "Task brief", "Owner message");
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    actions_wait(&mut chrome);
    assert_eq!(
        answered(&site),
        before,
        "drawn before any read was answered"
    );
    hold(&site, &READS, false);
    shows(&mut chrome, LIVE, "the warm visit, once checked");
    let current = actions(&mut chrome);
    assert_eq!(
        current,
        json!([[["Cancel task", false], ["Send", false]], false]),
        "the actions, once current"
    );
    assert_eq!(
        chrome.eval(&format!("document.querySelector({}).value", json!(DRAFT))),
        json!("kept draft"),
        "the draft typed meanwhile"
    );
    board_read_fresh(&site, &before);
    unchanged_once(&since(&site, &before, "/api/task-page"), "the task page");
    unchanged_once(&since(&site, &before, "/api/task-history"), "the thread");
    // A 304 carries no body: its head alone crossed the wire.
    let warm = api_bytes(&mut chrome);
    for path in ["/api/task-page", "/api/task-history"] {
        let read = warm
            .iter()
            .find(|(read, _, _)| read.starts_with(path))
            .unwrap_or_else(|| panic!("{path} read: {warm:#?}"));
        assert_eq!(read.1, 304, "{read:?}");
        assert!(read.2 < 300.0, "{path} sent a body: {read:?}");
    }
    let total = |reads: &[(String, i64, f64)]| reads.iter().map(|(_, _, bytes)| bytes).sum::<f64>();
    eprintln!(
        "a task page's reads, first visit: {} bytes {first:#?}\nwarm, unchanged: {} bytes {warm:#?}",
        total(&first),
        total(&warm)
    );
}

#[test]
fn a_message_posted_or_a_brief_edited_while_away_is_a_200_drawn_and_kept() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    // A message posted while away: the thread's newest page answers 200, the page 304.
    site.set_json("/api/task-history", &with_message(2, "Posted while away"));
    let saved = oldest(&mut chrome);
    let before = return_held(&mut chrome, &site, &READS);
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    // The notice can show before a loaded machine has drawn the kept thread under it: the thread is
    // read once its kept message is in.
    draws(&mut chrome, SLUG, "Task brief", "Owner message");
    assert_eq!(
        chrome
            .eval("document.querySelector('#task-thread').innerText.includes('Posted while away')"),
        json!(false),
        "the copy drawn is the one kept"
    );
    hold(&site, &READS, false);
    draws(&mut chrome, SLUG, "Task brief", "Posted while away");
    shows(&mut chrome, LIVE, "the thread, once read");
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('#task-thread .entry')].map(e => e.innerText.includes('Owner message') ? 1 : e.innerText.includes('Posted while away') ? 2 : 0)"),
        json!([1, 2]),
        "each message once, in order"
    );
    let history = since(&site, &before, "/api/task-history");
    assert!(
        history.len() == 1 && history[0].named.is_some() && history[0].status == 200,
        "{history:#?}"
    );
    unchanged_once(&since(&site, &before, "/api/task-page"), "the task page");
    shows(
        &mut chrome,
        &format!("{PARTS}.then(kept => kept[{}] > {saved})", json!(KEPT[1])),
        "the new thread kept",
    );

    // Its copy is the one drawn next; then the brief is edited while away.
    let mut edited = page();
    edited["blocks"] =
        json!([{"type": "paragraph", "spans": [{"type": "text", "text": "Task brief, edited"}]}]);
    edited["task"]["updated"] = json!(2.0);
    site.set_json("/api/task-page", &edited);
    let saved = oldest(&mut chrome);
    let before = return_held(&mut chrome, &site, &READS);
    draws(&mut chrome, SLUG, "Task brief", "Posted while away");
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    hold(&site, &READS, false);
    draws(&mut chrome, SLUG, "Task brief, edited", "Posted while away");
    shows(&mut chrome, LIVE, "the page, once read");
    let page_read = since(&site, &before, "/api/task-page");
    assert!(
        page_read.len() == 1 && page_read[0].named.is_some() && page_read[0].status == 200,
        "{page_read:#?}"
    );
    unchanged_once(&since(&site, &before, "/api/task-history"), "the thread");
    shows(
        &mut chrome,
        &format!("{PARTS}.then(kept => kept[{}] > {saved})", json!(KEPT[0])),
        "the edited page kept",
    );
    return_held(&mut chrome, &site, &READS);
    draws(&mut chrome, SLUG, "Task brief, edited", "Posted while away");
    hold(&site, &READS, false);
    shows(&mut chrome, LIVE, "the third visit");
}

#[test]
fn the_landing_offer_is_read_on_every_visit_and_never_kept() {
    let site = site();
    site.set_json("/api/task-offer", &offer(Some("patch")));
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    let land = "[...document.querySelectorAll('#task-send button')].find(b => b.textContent.includes('Land it') || b.textContent === 'Checking…' && b.type === 'button')";
    visit(&mut chrome, &site);
    chrome.eval("window.scrollTo(0, document.body.scrollHeight)");
    shows(
        &mut chrome,
        &format!("(b => b?.textContent === '✓ Land it' && !b.disabled)({land})"),
        "the landing button",
    );
    let before = answered(&site);
    // The offer is answered at once; the task's own reads wait.
    let saved = oldest(&mut chrome);
    let held = ["/api/board", "/api/task-page", "/api/task-history"];
    return_held(&mut chrome, &site, &held);
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    chrome.eval("window.scrollTo(0, document.body.scrollHeight)");
    shows(
        &mut chrome,
        &format!("(b => b?.textContent === 'Checking…' && b.disabled)({land})"),
        "the landing button waits while the page is drawn from the device",
    );
    let offers = since(&site, &before, "/api/task-offer");
    assert!(
        !offers.is_empty()
            && offers
                .iter()
                .all(|offer| offer.named.is_none() && offer.status == 200),
        "read again, fresh: {offers:#?}"
    );
    hold(&site, &held, false);
    shows(&mut chrome, LIVE, "the return visit, once checked");
    shows(
        &mut chrome,
        &format!("(b => b?.textContent === '✓ Land it' && !b.disabled)({land})"),
        "the landing button, once current",
    );
    // The task's head links the task it depends on, which the shell may read ahead and keep once
    // the page is idle, however soon that comes (`ahead`, apps/symphony-web/shell/shell.ts): a
    // task's page and its thread, as a visit keeps them. Whichever tasks are kept, none keeps its
    // offer.
    let parts = kept(&mut chrome);
    let shown = format!("task:{SLUG} ");
    let task: Vec<&String> = parts
        .keys()
        .filter(|part| part.starts_with(&shown))
        .collect();
    assert_eq!(
        task,
        KEPT.iter().collect::<Vec<_>>(),
        "the offer is never kept"
    );
    let other: Vec<&String> = parts
        .keys()
        .filter(|part| {
            part.starts_with("task:") && !part.ends_with(" page") && !part.ends_with(" thread")
        })
        .collect();
    assert!(
        other.is_empty(),
        "a task keeps its page and its thread alone, never its offer: {parts:?}"
    );
}

#[test]
fn a_check_that_fails_is_asked_again_until_it_answers_and_a_board_that_fails_is_not_reachable() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    let saved = oldest(&mut chrome);
    // Neither the board nor the page's check answers: the copy stays drawn, saying so. (The
    // page's check waits for no board, so one that answered would leave nothing to check.)
    let target = target("/api/task-page", SLUG);
    let before = answered(&site);
    site.fail_route(&target, true);
    site.fail_route("/api/board", true);
    return_held(&mut chrome, &site, &[]);
    let unreachable = notice(&mut chrome, saved, true);
    from_the_device(&mut chrome, &unreachable);
    actions_wait(&mut chrome);
    // The board answers and the feed opens, but the page's check still fails: the page is
    // checking, and asks again, waiting longer each time.
    site.fail_route("/api/board", false);
    let checking = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &checking);
    let failed = |site: &Site| {
        since(site, &before, "/api/task-page")
            .iter()
            .filter(|answer| answer.status == 500)
            .count()
    };
    if let Err(stall) =
        test_time::until_true("the check asked again", crate::STALL, || failed(&site) >= 2)
    {
        panic!("{stall}: {:#?}", since(&site, &before, "/api/task-page"));
    }
    from_the_device(&mut chrome, &checking);
    actions_wait(&mut chrome);
    site.fail_route(&target, false);
    shows(&mut chrome, LIVE, "the check, once answered");
    let asked = since(&site, &before, "/api/task-page");
    assert_eq!(
        asked
            .last()
            .map(|answer| (answer.named.is_some(), answer.status)),
        Some((true, 304)),
        "{asked:#?}"
    );
}

#[test]
fn a_task_gone_while_away_says_so_and_this_device_forgets_it() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    let gone = format!("no task '{SLUG}'");
    site.miss_route("/api/task-page", Some(&gone));
    site.miss_route("/api/task-history", Some(&gone));
    let saved = oldest(&mut chrome);
    return_held(&mut chrome, &site, &READS);
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    hold(&site, &READS, false);
    shows(
        &mut chrome,
        &format!(
            "document.querySelector('main').innerText.includes({})",
            json!(format!("No task {SLUG}"))
        ),
        "the task gone",
    );
    shows(&mut chrome, LIVE, "the page, once answered");
    shows(
        &mut chrome,
        &format!("{PARTS}.then(kept => !Object.keys(kept).some(part => part.startsWith('task:')))"),
        "the task forgotten",
    );
}

/// The `n`th of the tasks opened in turn.
fn numbered(n: usize) -> String {
    format!("perf/t{n:02}")
}

/// Open `slug` from the page shown, by a link the shell follows in place.
fn follow(chrome: &mut Chrome, slug: &str) {
    chrome.eval(&format!(
        "(link => {{ link.href = {}; document.body.append(link); link.click(); link.remove(); }})(document.createElement('a'))",
        json!(route(slug))
    ));
}

#[test]
fn the_31st_task_opened_drops_the_one_opened_longest_ago_and_a_kept_one_opened_in_place_draws_at_once()
 {
    let site = site();
    for n in 1..=31 {
        let slug = numbered(n);
        let mut page = page();
        page["task"]["slug"] = json!(slug);
        page["task"]["title"] = json!(format!("Task {n}"));
        page["blocks"] = json!([{"type": "paragraph", "spans": [{"type": "text", "text": format!("Brief {n}")}]}]);
        site.set_json(&target("/api/task-page", &slug), &page);
    }
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    store_opened(&mut chrome, &site);
    chrome.open(&site.url(&route(&numbered(1))));
    // Task 1 again after the 30th: the second is then the one opened longest ago.
    let opened: Vec<usize> = (1..=30).chain([1, 31]).collect();
    for (at, n) in opened.iter().enumerate() {
        let slug = numbered(*n);
        if at > 0 {
            follow(&mut chrome, &slug);
        }
        shows(
            &mut chrome,
            &format!(
                "document.querySelector(\"article.task[data-slug='{slug}'] .task-body\")?.innerText.includes('Brief {n}')"
            ),
            &slug,
        );
        keeps(&mut chrome, &[&format!("task:{slug} page")]);
    }
    let tasks = |chrome: &mut Chrome| {
        let mut tasks: Vec<String> = kept(chrome)
            .keys()
            .filter_map(|part| part.strip_prefix("task:"))
            .filter_map(|part| part.split(' ').next())
            .map(str::to_string)
            .collect();
        tasks.dedup();
        tasks
    };
    let expected: Vec<String> = (1..=31).filter(|n| *n != 2).map(numbered).collect();
    shows(
        &mut chrome,
        &format!(
            "{PARTS}.then(kept => !Object.keys(kept).some(part => part.startsWith({})))",
            json!(format!("task:{} ", numbered(2)))
        ),
        "the second task dropped",
    );
    assert_eq!(
        tasks(&mut chrome),
        expected,
        "30 tasks kept, the second gone"
    );

    // A kept task opened in place, its reads held, is drawn from the device at once.
    let fifth = numbered(5);
    let reads = [
        target("/api/task-page", &fifth),
        target("/api/task-history", &fifth),
    ];
    for read in &reads {
        site.hold_route(read, true);
    }
    follow(&mut chrome, &fifth);
    shows(
        &mut chrome,
        "(main => main.dataset.cache === 'device' && document.querySelector(\"article.task[data-slug='perf/t05'] .task-body\")?.innerText.includes('Brief 5'))(document.querySelector('main'))",
        "the fifth task from the device",
    );
    for read in &reads {
        site.hold_route(read, false);
    }
    shows(&mut chrome, LIVE, "the fifth task, once checked");
    // The second, opened again, is read whole: nothing of it is kept.
    let second = numbered(2);
    follow(&mut chrome, &second);
    shows(
        &mut chrome,
        "document.querySelector(\"article.task[data-slug='perf/t02'] .task-body\")?.innerText.includes('Brief 2')",
        "the second task, read again",
    );
    let asked = site.answers("/api/task-page");
    let last = asked
        .iter()
        .rev()
        .find(|answer| answer.target == target("/api/task-page", &second))
        .unwrap();
    assert_eq!(
        (last.named.is_some(), last.status),
        (false, 200),
        "{last:?}"
    );
}
