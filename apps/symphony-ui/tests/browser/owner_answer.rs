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
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 1280, "height": 900, "deviceScaleFactor": 1, "mobile": false}),
    );
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

/// The server writes the owner's answer but its reply is held past the page's deadline, and the tab
/// has no feed to learn it from: its stream closed as the page froze, and the read that would open
/// it again is held. The owner changes the words and sends again. The retry goes under the first
/// answer's tap, so the server writes no second verdict line. The form closes as after any answered
/// save, and the page says the change was not sent, keeping it as the card's draft to send as an
/// answer of its own. Once the page reads again, the card shows the answer that was saved, regrouped.
#[test]
fn a_retry_after_no_answer_with_other_words_writes_no_second_verdict_line() {
    let Some(mut owner) = chrome() else { return };
    let site = Native::start_committed(
        None,
        "main",
        &[("docs/DECISIONS.md", REGISTER), ("docs/NOTES.md", NOTES)],
        None,
    );
    let saved = site.root().join("docs/DECISIONS.md");
    opened(&mut owner, &site.url("/docs/DECISIONS.md"), "alpha");
    owner.call(
        "Fetch.enable",
        json!({"patterns": [holding("*/api/decision", "Response"), holding("*/api/document?*", "Request")]}),
    );
    owner.eval(
        "document.dispatchEvent(new Event('freeze')); document.dispatchEvent(new Event('resume'))",
    );
    assert!(owner.wait_for(
        "document.querySelector('.document-page')?.dataset.feed === 'stale'",
        SHOWN
    ));

    answer(&mut owner, "alpha", "Approve", "Keep it");
    assert!(
        owner.wait_for(
            &format!(
                "document.querySelector({})?.textContent.includes('no answer from the server') === true",
                within("alpha", ".editor > [role=alert]")
            ),
            SHOWN
        ),
        "{}",
        around(&mut owner, "alpha")
    );
    assert_eq!(
        verdict_lines(&saved).len(),
        1,
        "the first answer is written"
    );

    // The retry goes through; the page's reads stay held.
    owner.call(
        "Fetch.enable",
        json!({"patterns": [holding("*/api/document?*", "Request")]}),
    );
    write(&mut owner, "alpha", "Keep it, and say why");
    choose(&mut owner, "alpha", "Approve");
    assert!(
        owner.wait_for(
            &format!(
                "document.querySelector({})?.textContent.includes('your change was not sent') === true",
                within("alpha", ".editor > [role=status]")
            ),
            SHOWN
        ),
        "{} {}",
        editor(&mut owner, "alpha"),
        around(&mut owner, "alpha")
    );
    assert_eq!(decisions_ended(&mut owner), json!(["no-answer", "ok"]));
    let lines = verdict_lines(&saved);
    assert!(
        lines.len() == 1 && lines[0].ends_with("owner · approved — Keep it"),
        "no second verdict line: {lines:?}"
    );
    assert_eq!(
        editor(&mut owner, "alpha"),
        json!({"open": false, "words": "Keep it, and say why", "notice": null,
            "status": "Verdict saved: approved; your change was not sent: send it again to add it",
            "kept": ["Keep it, and say why"]}),
        "the change stays to send"
    );

    owner.call("Fetch.disable", json!({}));
    let answered = json!({"state":"approved","group":"Waiting on an agent","owner":[["approved","Keep it"]],"visit":true});
    let mine = shows(&mut owner, "alpha", &answered);
    assert!(mine.is_ok(), "{mine:?} {}", around(&mut owner, "alpha"));
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
fn an_owner_verdict_shows_and_regroups_on_its_page_and_a_second_tab_without_a_reload() {
    answered_on_both_tabs("/docs/DECISIONS.md", |_| {});
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
        // The excerpt fills once its box is on screen: the owner reads it there.
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
