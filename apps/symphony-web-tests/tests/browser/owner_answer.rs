//! An owner's answer on a card shows at once on the page it was given on, and through the feed on
//! a second open page, with no reload: the card's new mark and line, and the register regrouped.
//! So on main's page and on a branch's, and whatever else the page holds open meanwhile: an excerpt
//! read beside the answer, or a draft left on another card.
use super::{SHOWN, chrome, chrome::Chrome, restored::Native};
use serde_json::{Value, json};

const REGISTER: &str = "# Decisions\n\n<!-- register:start -->\n## Needs your decision\n\n### Topic\n\n- [ ] **alpha** — First question, asked in [[NOTES#Open questions]].\n- [ ] **bravo** — Second question.\n<!-- register:end -->\n";
const NOTES: &str = "# Notes\n\n## Open questions\n\n1. Is it red?\n";
/// The branch's register: an unmerged commit of its own, so its pages are the branch's.
const BRANCH: &str = "feature/answer";

/// What the page shows of the card `slug`, as a JS expression: its mark, the register group it sits
/// under (the last group heading above it, without its count), each owner line of its thread as its
/// verdict and its words, and whether the page is still the visit [`opened`] marked.
fn card(slug: &str) -> String {
    format!(
        "(() => {{ const card = document.querySelector('[data-doc-key=\"card:{slug}\"]');
            const above = card ? [...document.querySelectorAll('.doc-body h2')].filter(h => h.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING) : [];
            return {{state: card?.dataset.state ?? null, group: above.length ? above[above.length - 1].textContent.trim().replace(/ \\(\\d+\\)$/, '') : null,
                owner: card ? [...card.querySelectorAll('.history .entry.owner')].map(e => [e.querySelector('.entry-verdict')?.textContent ?? '', e.querySelector('.entry-text')?.textContent.trim() ?? '']) : [],
                visit: window.sameVisit === true}}; }})()"
    )
}

/// `selector` within the card `slug`, as a JS string literal.
fn within(slug: &str, selector: &str) -> Value {
    json!(format!("[data-doc-key=\"card:{slug}\"] {selector}"))
}

/// Marks each lifecycle event of the page's on its timeline as `page TYPE VISIBILITY`, for
/// [`around`]: a page Chrome froze, hid or left, against one that only stopped running.
const LIFECYCLE: &str = "for (const [target, types] of [[document, ['freeze', 'resume', 'visibilitychange']], [window, ['pagehide', 'pageshow', 'blur', 'focus']]])
    for (const type of types) target.addEventListener(type, () => performance.mark(`page ${type} ${document.visibilityState}`))";

/// Open `url` in a wide window and wait for the card `slug` and a live feed; marks the visit so a
/// reload shows, and keeps the page's lifecycle ([`LIFECYCLE`]).
fn opened(page: &mut Chrome, url: &str, slug: &str) {
    page.open(url);
    page.viewport(1280, 900, false);
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector({}) !== null && document.querySelector('.document-page')?.dataset.feed === 'live'",
                within(slug, ".doc-actions button")
            ),
            SHOWN
        ),
        "{}",
        page.eval("document.body.innerText")
    );
    page.eval("window.sameVisit = true");
    page.eval(LIFECYCLE);
}

/// Open the card `slug`'s form and type `note` into its box, as the owner does.
fn type_note(page: &mut Chrome, slug: &str, note: &str) {
    page.eval(&format!(
        "document.querySelector({}).click()",
        within(slug, ".doc-actions button")
    ));
    write(page, slug, note);
}

/// Write `note` in the card `slug`'s open form, in place of what its box held.
fn write(page: &mut Chrome, slug: &str, note: &str) {
    let field = within(slug, "form.verdict textarea");
    assert!(page.wait_for(&format!("document.querySelector({field}) !== null"), SHOWN));
    page.eval(&format!(
        "(() => {{ const t = document.querySelector({field}); t.focus(); t.value = {}; t.dispatchEvent(new Event('input', {{bubbles: true}})); return true; }})()",
        json!(note)
    ));
}

/// Give the card `slug` the verdict whose button says `button`, with `note`. The page keeps when, on
/// its own clock, for [`around`].
fn answer(page: &mut Chrome, slug: &str, button: &str, note: &str) {
    type_note(page, slug, note);
    choose(page, slug, button);
}

/// Press the verdict button that says `button` in the card `slug`'s open form.
fn choose(page: &mut Chrome, slug: &str, button: &str) {
    page.eval(&format!(
        "(() => {{ window.answeredAt = Math.round(performance.now()); [...document.querySelectorAll({})].find(b => b.textContent.includes({})).click(); }})()",
        within(slug, ".verdict-choices button"),
        json!(button)
    ));
}

/// An object as JSON with its keys sorted, as a JS function, so the page's and the expected one
/// compare whatever order each was written in: serde_json writes keys sorted, but in the order they
/// were given where the whole workspace's build turns on its `preserve_order`, as `just verify`'s does.
const SORTED: &str = "(o => JSON.stringify(o, Object.keys(o).sort()))";

/// Wait until `page` shows `expected` of the card `slug`; what it shows instead when it never does.
/// Each look scrolls the card to the middle of the window first, wherever a regroup moved it: its
/// thread renders only once it is near the screen, as the owner reading it has it.
fn shows(page: &mut Chrome, slug: &str, expected: &Value) -> Result<(), Value> {
    let wanted = format!(
        "(() => {{ document.querySelector({})?.scrollIntoView({{block: 'center'}}); const s = {}; return {SORTED}(s) === {SORTED}({expected}); }})()",
        json!(format!("[data-doc-key=\"card:{slug}\"]")),
        card(slug),
    );
    if page.wait_for(&wanted, SHOWN) {
        Ok(())
    } else {
        Err(page.eval(&card(slug)))
    }
}

/// What `page` shows around the card `slug`, for a failure to say why it is not what was wanted: the
/// feed's state, the card's text (a refused or unanswered save's notice among it), and on the page's
/// clock in ms: when the owner answered, each post the network finished (its path, start and end),
/// and each post's way through the page's transport (`request.rs`'s marks: where the page's own
/// callback saw the reply, where the waiting task took it up, how the send ended) among the page's
/// lifecycle events ([`LIFECYCLE`]); then each task or frame of the page's over a second, its start
/// and length, and for a frame when its rendering began, how long its tasks blocked and the scripts
/// it ran (each script's invoker, function, start, length, forced layout and pause): a page busy
/// that long blocked for most of it, and one that did not run at all for none.
fn around(page: &mut Chrome, slug: &str) -> Value {
    page.eval(&format!(
        "(async () => {{ const card = document.querySelector({});
            const long = await new Promise(resolve => {{ const seen = []; let observer = null;
                try {{ observer = new PerformanceObserver(list => seen.push(...list.getEntries()));
                    for (const type of ['longtask', 'long-animation-frame']) observer.observe({{type, buffered: true}}); }} catch (_) {{}}
                setTimeout(() => {{ observer?.disconnect(); resolve(seen); }}, 50); }});
            return {{feed: document.querySelector('.document-page')?.dataset.feed ?? null, text: card?.innerText ?? null,
            answered: window.answeredAt ?? null,
            posts: performance.getEntriesByType('resource').filter(e => /\\/api\\/(decision|message|postpone)$/.test(new URL(e.name).pathname)).map(e => [new URL(e.name).pathname, Math.round(e.startTime), Math.round(e.responseEnd)]),
            sends: performance.getEntriesByType('mark').filter(m => m.name.startsWith('send ') || m.name.startsWith('page ')).map(m => [m.name, Math.round(m.startTime)]),
            long: long.filter(e => e.duration > 1000).map(e => {{ const at = t => t ? Math.round(t - e.startTime) : null;
                return [e.entryType, Math.round(e.startTime), Math.round(e.duration), {{render: at(e.renderStart), layout: at(e.styleAndLayoutStart), blocking: e.blockingDuration == null ? null : Math.round(e.blockingDuration)}},
                    ...(e.scripts ?? []).map(s => [s.invoker, s.sourceFunctionName, at(s.startTime), Math.round(s.duration), Math.round(s.forcedStyleAndLayoutDuration), Math.round(s.pauseDuration)])]; }})}}; }})()",
        json!(format!("[data-doc-key=\"card:{slug}\"]"))
    ))
}

/// The card `slug`'s editor as the owner has it: whether its form is open, the words in its box, its
/// notice and its status line, and what the page keeps of the card's draft and tap.
fn editor(page: &mut Chrome, slug: &str) -> Value {
    page.eval(&format!(
        "(() => {{ const editor = document.querySelector({}); return {{open: editor?.querySelector('form')?.hidden === false, words: editor?.querySelector('textarea')?.value ?? null,
            notice: editor?.querySelector(':scope > [role=alert]')?.textContent ?? null, status: editor?.querySelector(':scope > [role=status]')?.textContent ?? null,
            kept: Object.keys(localStorage).filter(k => k.endsWith(':card:{slug}')).map(k => localStorage.getItem(k)).filter(v => v)}}; }})()",
        within(slug, ".editor")
    ))
}

/// How each of the page's posts to `/api/decision` ended, in order, by the transport's marks.
fn decisions_ended(page: &mut Chrome) -> Value {
    page.eval(
        "performance.getEntriesByType('mark').map(m => m.name).filter(n => / POST \\S*\\/api\\/decision (ok|refused|failed|no-answer)$/.test(n)).map(n => n.split(' ').pop())",
    )
}

/// The owner's verdict lines on alpha in the register `saved`.
fn verdict_lines(saved: &std::path::Path) -> Vec<String> {
    let text = std::fs::read_to_string(saved).unwrap();
    text.lines()
        .filter(|line| line.contains("owner · "))
        .map(|line| line.trim().to_string())
        .collect()
}

/// `pattern`, a pattern of DevTools' `Fetch` domain, held at `stage`.
fn holding(pattern: &str, stage: &str) -> Value {
    json!({"urlPattern": pattern, "requestStage": stage})
}

/// The server writes the owner's answer, and its reply is held past the page's deadline, so the
/// owner's own tab has no answer from the server; the feed brings the answer meanwhile, as it does
/// to every tab, and that is the save's answer there too. The card shows it and regroups, the form
/// closes, its draft and tap are done with, and no notice is left saying there was no answer.
#[test]
fn an_answer_whose_reply_comes_too_late_shows_on_its_own_tab_from_the_feed() {
    let Some(mut owner) = chrome() else { return };
    let site = Native::start_committed(
        None,
        "main",
        &[("docs/DECISIONS.md", REGISTER), ("docs/NOTES.md", NOTES)],
        None,
    );
    opened(&mut owner, &site.url("/docs/DECISIONS.md"), "alpha");
    owner.call(
        "Fetch.enable",
        json!({"patterns": [holding("*/api/decision", "Response")]}),
    );
    answer(&mut owner, "alpha", "Approve", "Keep it");
    let held = owner.wait_for_protocol_events("Fetch.requestPaused", 1, SHOWN);
    assert_eq!(held.len(), 1, "the server's reply is held");

    let answered = json!({"state":"approved","group":"Waiting on an agent","owner":[["approved","Keep it"]],"visit":true});
    let mine = shows(&mut owner, "alpha", &answered);
    assert!(mine.is_ok(), "{mine:?} {}", around(&mut owner, "alpha"));
    assert_eq!(
        decisions_ended(&mut owner),
        json!(["no-answer"]),
        "the page had no answer from the server"
    );
    assert_eq!(
        editor(&mut owner, "alpha"),
        json!({"open": false, "words": "", "notice": null, "status": "Verdict saved: approved", "kept": []}),
        "{}",
        around(&mut owner, "alpha")
    );
    assert_eq!(
        verdict_lines(&site.root().join("docs/DECISIONS.md")).len(),
        1,
        "one verdict line"
    );
    owner.call("Fetch.disable", json!({}));
}

/// A save the board's people refuse: the server's answer to a person who lacks the power the
/// route needs is a 403 whose `error` says which (`apps/symphony-server/src/powers.rs`), and that
/// answer reaches no route, so nothing is written. The page shows those words as the card's
/// notice, keeps the form open on the person's words and their draft, and the card as it was.
#[test]
fn a_save_the_boards_people_refuse_shows_its_words_and_keeps_the_draft() {
    let Some(mut owner) = chrome() else { return };
    let site = Native::start_committed(
        None,
        "main",
        &[("docs/DECISIONS.md", REGISTER), ("docs/NOTES.md", NOTES)],
        None,
    );
    opened(&mut owner, &site.url("/docs/DECISIONS.md"), "alpha");
    owner.call(
        "Fetch.enable",
        json!({"patterns": [holding("*/api/decision", "Request")]}),
    );
    answer(&mut owner, "alpha", "Approve", "Keep it");
    let held = owner.wait_for_protocol_events("Fetch.requestPaused", 1, SHOWN);
    assert_eq!(
        held.len(),
        1,
        "the save is held before it reaches the server"
    );
    let refusal = json!({"error": "Forbidden: ana may not decide on this board"}).to_string();
    let body = owner.eval(&format!("btoa({})", json!(refusal)));
    owner.call(
        "Fetch.fulfillRequest",
        json!({"requestId": held[0]["requestId"], "responseCode": 403,
               "responseHeaders": [{"name": "Content-Type", "value": "application/json"}],
               "body": body}),
    );
    assert!(
        decisions_end(&mut owner, &json!(["refused"])),
        "{}",
        around(&mut owner, "alpha")
    );
    let notice = within("alpha", ".editor > [role=alert]");
    assert!(
        owner.wait_for(&format!("document.querySelector({notice}) !== null"), SHOWN),
        "{}",
        around(&mut owner, "alpha")
    );
    let shown = editor(&mut owner, "alpha");
    assert_eq!(
        (
            &shown["open"],
            &shown["words"],
            &shown["notice"],
            &shown["status"]
        ),
        (
            &json!(true),
            &json!("Keep it"),
            &json!("Forbidden: ana may not decide on this board"),
            &Value::Null
        ),
        "{shown}"
    );
    let kept = shown["kept"].as_array().cloned().unwrap_or_default();
    assert!(
        kept.contains(&json!("Keep it")),
        "the draft is kept: {shown}"
    );
    let pending = json!({"state":"pending","group":"Needs your decision","owner":[],"visit":true});
    let unchanged = shows(&mut owner, "alpha", &pending);
    assert!(unchanged.is_ok(), "{unchanged:?}");
    assert_eq!(
        std::fs::read_to_string(site.root().join("docs/DECISIONS.md")).unwrap(),
        REGISTER,
        "nothing is written"
    );
    owner.call("Fetch.disable", json!({}));
}

/// The second sending's words, which the server never writes: the first sending's tap holds alpha's
/// answer, and every retry goes under it.
const CHANGED: &str = "Keep it, and say why";

/// Waits until the page's posts to `/api/decision` have ended as `ended`, by the transport's marks.
fn decisions_end(page: &mut Chrome, ended: &Value) -> bool {
    page.wait_for(
        &format!(
            "(() => {{ const ended = performance.getEntriesByType('mark').map(m => m.name).filter(n => / POST \\S*\\/api\\/decision (ok|refused|failed|no-answer)$/.test(n)).map(n => n.split(' ').pop()); return JSON.stringify(ended) === {}; }})()",
            json!(ended.to_string())
        ),
        SHOWN,
    )
}

/// Waits until the page has sent `count` posts to `/api/decision`, by the transport's marks.
fn decisions_sent(page: &mut Chrome, count: usize) -> bool {
    page.wait_for(
        &format!(
            "performance.getEntriesByType('mark').filter(m => / POST \\S*\\/api\\/decision start$/.test(m.name)).length === {count}"
        ),
        SHOWN,
    )
}

/// The server's lock on its document writes, held as a save of the lander's holds it: every save
/// waits behind it, its answer with it, while the feed and the reads go on. Dropped, the saves go
/// on in the order they came.
fn writes_held(site: &Native) -> std::fs::File {
    let home = site.root().join(".git/symphony/server");
    std::fs::create_dir_all(&home).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("verdict-writes.lock"))
        .unwrap();
    lock.lock().unwrap();
    lock
}

/// The answer deadline of a post the held writes leave unanswered, in ms: the page's own
/// (`ANSWER_MS`, 15 s) named shorter (`symphonyAnswerMs`), as `polish` and `skeleton` name it for
/// a read held past it. The server cannot answer while its writes are held, whatever the length.
const HELD_ANSWER_MS: u32 = 5_000;

/// `send`, the page's `count`th post to `/api/decision`, sent under [`HELD_ANSWER_MS`]: the page
/// names the shorter deadline for that post alone, which takes its deadline as it starts, and its
/// own again once the post is on its way, so no read and no later post is cut short by it.
fn unanswered(owner: &mut Chrome, count: usize, send: impl FnOnce(&mut Chrome)) {
    owner.eval(&format!("window.symphonyAnswerMs = {HELD_ANSWER_MS}"));
    send(owner);
    assert!(decisions_sent(owner, count), "{}", around(owner, "alpha"));
    owner.eval("delete window.symphonyAnswerMs");
}

/// The owner answers alpha "Keep it" while the server's writes are held ([`writes_held`]), so the
/// save waits there past the page's deadline: no answer, and nothing for the feed, which stays live,
/// to bring. The owner changes the words to [`CHANGED`] and sends again under the first sending's
/// tap, which waits too. The page is left with both sendings unanswered and the form open on the
/// changed words, and the server with both still to write: the first writes alpha's answer once
/// the writes go on, and the second, under the same tap, nothing. The register, for what it holds,
/// and the held writes.
fn unanswered_twice(owner: &mut Chrome, site: &Native) -> (std::path::PathBuf, std::fs::File) {
    let saved = site.root().join("docs/DECISIONS.md");
    opened(owner, &site.url("/docs/DECISIONS.md"), "alpha");
    let writes = writes_held(site);

    unanswered(owner, 1, |owner| {
        answer(owner, "alpha", "Approve", "Keep it")
    });
    assert!(
        decisions_end(owner, &json!(["no-answer"])),
        "{}",
        around(owner, "alpha")
    );
    write(owner, "alpha", CHANGED);
    unanswered(owner, 2, |owner| choose(owner, "alpha", "Approve"));
    assert!(
        decisions_end(owner, &json!(["no-answer", "no-answer"])),
        "{}",
        around(owner, "alpha")
    );
    assert_eq!(
        editor(owner, "alpha")["open"],
        json!(true),
        "an unanswered save keeps its form"
    );
    assert!(verdict_lines(&saved).is_empty(), "the writes are held");
    assert_eq!(
        owner.eval("document.querySelector('.document-page')?.dataset.feed"),
        json!("live"),
        "the feed stays live"
    );
    (saved, writes)
}

/// The page says the server holds alpha's first answer, and that the change was not sent: the form
/// closed as after any answered save, the changed words kept as the card's draft to send as an answer
/// of its own, and the register holds one verdict line, the first sending's.
fn kept_the_change(owner: &mut Chrome, saved: &std::path::Path) {
    assert!(
        owner.wait_for(
            &format!(
                "document.querySelector({})?.textContent.includes('your change was not sent') === true",
                within("alpha", ".editor > [role=status]")
            ),
            SHOWN
        ),
        "{} {}",
        editor(owner, "alpha"),
        around(owner, "alpha")
    );
    let lines = verdict_lines(saved);
    assert!(
        lines.len() == 1 && lines[0].ends_with("owner · approved — Keep it"),
        "no second verdict line: {lines:?}"
    );
    assert_eq!(
        editor(owner, "alpha"),
        json!({"open": false, "words": CHANGED, "notice": null,
            "status": "Verdict saved: approved; your change was not sent: send it again to add it",
            "kept": [CHANGED]}),
        "the change stays to send"
    );
}

/// Two sendings under one tap go unanswered, the first with "Keep it", the second with other
/// words. The owner sends the changed words a third time, and the server's writes go on while it is
/// in flight: the first sending writes alpha's answer, and the third's reply comes, that the tap was
/// written already, with the words it holds, the first sending's. The page says so, never that the
/// changed words were saved, and keeps them; the card shows the answer that was saved, regrouped.
#[test]
fn a_retry_after_two_unanswered_sendings_keeps_the_words_the_server_did_not_write() {
    let Some(mut owner) = chrome() else { return };
    let site = Native::start_committed(
        None,
        "main",
        &[("docs/DECISIONS.md", REGISTER), ("docs/NOTES.md", NOTES)],
        None,
    );
    let (saved, writes) = unanswered_twice(&mut owner, &site);

    choose(&mut owner, "alpha", "Approve");
    assert!(
        decisions_sent(&mut owner, 3),
        "{}",
        around(&mut owner, "alpha")
    );
    drop(writes);
    assert!(
        decisions_end(&mut owner, &json!(["no-answer", "no-answer", "ok"])),
        "{}",
        around(&mut owner, "alpha")
    );
    kept_the_change(&mut owner, &saved);

    let answered = json!({"state":"approved","group":"Waiting on an agent","owner":[["approved","Keep it"]],"visit":true});
    let mine = shows(&mut owner, "alpha", &answered);
    assert!(mine.is_ok(), "{mine:?} {}", around(&mut owner, "alpha"));
}

/// Two sendings under one tap go unanswered, as above, and the owner reloads the page: the card's
/// stored tap carries the changed words' signature, under the first sending's tap. The server's
/// writes go on, and the reloaded card shows the answer the server holds; the owner resumes the
/// draft and sends it, under that tap, and the server answers with the words it holds. The page
/// keeps the changed words and says they were not sent, as on the page that sent them.
#[test]
fn a_draft_resumed_after_a_reload_keeps_the_words_its_tap_does_not_hold() {
    let Some(mut owner) = chrome() else { return };
    let site = Native::start_committed(
        None,
        "main",
        &[("docs/DECISIONS.md", REGISTER), ("docs/NOTES.md", NOTES)],
        None,
    );
    let (saved, writes) = unanswered_twice(&mut owner, &site);
    let tap = owner.eval(
        "Object.keys(localStorage).filter(k => k.startsWith('review-tap:') && k.endsWith(':card:alpha')).map(k => localStorage.getItem(k).split('\\n').slice(1).join('\\n'))",
    );
    assert_eq!(
        tap,
        json!([format!("approved\n{CHANGED}")]),
        "the stored tap signs the changed words"
    );

    opened(&mut owner, &site.url("/docs/DECISIONS.md"), "alpha");
    drop(writes);
    let answered = json!({"state":"approved","group":"Waiting on an agent","owner":[["approved","Keep it"]],"visit":true});
    let reloaded = shows(&mut owner, "alpha", &answered);
    assert!(
        reloaded.is_ok(),
        "{reloaded:?} {}",
        around(&mut owner, "alpha")
    );
    let resume = within("alpha", ".editor button");
    assert!(
        owner.wait_for(
            &format!(
                "(() => {{ const resume = [...document.querySelectorAll({resume})].find(b => b.textContent === 'Resume draft'); resume?.click(); return resume !== undefined; }})()"
            ),
            SHOWN
        ),
        "{} {}",
        editor(&mut owner, "alpha"),
        around(&mut owner, "alpha")
    );
    assert_eq!(
        editor(&mut owner, "alpha")["words"],
        json!(CHANGED),
        "the draft is resumed"
    );
    choose(&mut owner, "alpha", "Approve");
    assert!(
        decisions_end(&mut owner, &json!(["ok"])),
        "{}",
        around(&mut owner, "alpha")
    );
    kept_the_change(&mut owner, &saved);
}

/// Two tabs on `page` of a server whose main and branch both hold the register; `meanwhile` is what
/// the owner's tab holds open before the owner approves alpha there. Both tabs show the answer and
/// the regroup, and bravo is left as it was. The owner's tab, for what else it should still show.
fn answered_on_both_tabs(page: &str, meanwhile: impl FnOnce(&mut Chrome)) -> Option<Chrome> {
    let mut owner = chrome()?;
    let mut other = chrome()?;
    let branch_register = REGISTER.replace("Second question.", "Second question, on the branch.");
    let site = Native::start_committed(
        None,
        "main",
        &[("docs/DECISIONS.md", REGISTER), ("docs/NOTES.md", NOTES)],
        Some((BRANCH, &[("docs/DECISIONS.md", &branch_register)])),
    );
    let url = site.url(page);
    opened(&mut owner, &url, "alpha");
    opened(&mut other, &url, "alpha");
    let pending = json!({"state":"pending","group":"Needs your decision","owner":[],"visit":true});
    assert_eq!(shows(&mut owner, "alpha", &pending), Ok(()));
    meanwhile(&mut owner);

    let answered_at = std::time::Instant::now();
    answer(&mut owner, "alpha", "Approve", "Keep it");
    let answered = json!({"state":"approved","group":"Waiting on an agent","owner":[["approved","Keep it"]],"visit":true});
    let mine = shows(&mut owner, "alpha", &answered);
    let waited = answered_at.elapsed();
    let theirs = shows(&mut other, "alpha", &answered);
    let saved = site.root().join("docs/DECISIONS.md");
    let saved = if page.starts_with("/@") {
        site.root().with_extension("tree").join("docs/DECISIONS.md")
    } else {
        saved
    };
    assert!(
        mine.is_ok() && theirs.is_ok(),
        "{page}: {waited:?} after the answer the owner's tab shows {mine:?} ({}), the second tab {theirs:?} ({}); the file:\n{}\nthe server said:\n{}",
        around(&mut owner, "alpha"),
        around(&mut other, "alpha"),
        std::fs::read_to_string(saved).unwrap(),
        std::fs::read_to_string(site.root().join("artifacts/server.log")).unwrap_or_default()
    );
    let unanswered =
        json!({"state":"pending","group":"Needs your decision","owner":[],"visit":true});
    assert_eq!(shows(&mut owner, "bravo", &unanswered), Ok(()), "{page}");
    assert_eq!(shows(&mut other, "bravo", &unanswered), Ok(()), "{page}");
    Some(owner)
}

#[test]
fn an_owner_verdict_on_a_branch_page_shows_and_regroups_on_both_tabs() {
    let Some(mut owner) = answered_on_both_tabs(
        &format!("/@{}/docs/DECISIONS.md", BRANCH.replace('/', "~")),
        |_| {},
    ) else {
        return;
    };
    assert_eq!(
        owner.eval(&format!(
            "document.querySelector({}).textContent.includes('Second question, on the branch.')",
            within("bravo", ".section-prose")
        )),
        json!(true),
        "the page is the branch's"
    );
}

/// The owner read the card's linked notes in its excerpt while writing: the excerpt stays open, and
/// the answer still shows and regroups.
#[test]
fn an_owner_verdict_given_beside_an_open_excerpt_shows_and_regroups() {
    let Some(mut owner) = answered_on_both_tabs("/docs/DECISIONS.md", |owner| {
        owner.eval(&format!(
            "document.querySelector({}).click()",
            within("alpha", ".show-link")
        ));
        // The excerpt fills, and the owner scrolls to it and reads it there.
        assert!(owner.wait_for(
            &format!(
                "(() => {{ const b = document.querySelector({}); b?.scrollIntoView({{block: 'center'}}); return b?.textContent.includes('Is it red?') === true; }})()",
                within("alpha", ".excerpt-box")
            ),
            SHOWN
        ));
    }) else {
        return;
    };
    // The excerpt opens in the card's line, beside its button, which says it is open.
    assert_eq!(
        owner.eval(&format!(
            "[document.querySelector({}) !== null, document.querySelector({})?.getAttribute('aria-expanded')]",
            within("alpha", ".excerpt-box"),
            within("alpha", ".show-link")
        )),
        json!([true, "true"]),
        "the excerpt the owner was reading stays open"
    );
}

/// A draft the owner left on another card holds that card's words, not the register's order.
#[test]
fn a_draft_left_on_another_card_does_not_stop_the_answer_regrouping() {
    let Some(mut owner) = answered_on_both_tabs("/docs/DECISIONS.md", |owner| {
        type_note(owner, "bravo", "half a thought");
        owner.eval(&format!(
            "document.querySelector({}).click()",
            within("bravo", ".doc-actions button")
        ));
        owner.eval("document.activeElement?.blur()");
        assert!(owner.wait_for(
            &format!(
                "[...document.querySelectorAll({})].some(b => b.textContent === 'Resume draft')",
                within("bravo", ".editor button")
            ),
            SHOWN
        ));
    }) else {
        return;
    };
    assert_eq!(
        owner.eval(&format!(
            "document.querySelector({}).value",
            within("bravo", "form.verdict textarea")
        )),
        json!("half a thought"),
        "the draft is kept"
    );
}
