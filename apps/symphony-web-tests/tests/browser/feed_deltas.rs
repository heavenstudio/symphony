//! A holder's health off the feed (perf/feed-deltas): the server sends it once, as the holder's
//! `session` change, never again in each task it holds, so the role chips of the task board and of
//! a task page take it from that change: every chip of that holder, none of another's, and `lost`
//! once the session is gone from the board, as the board reads a role of no session. The tasks are
//! the look-parity fixture's, each executed by `example`. A changed task is sent as its changed
//! fields alone, so the agents page reads a task it lists no record of when it now shows it, and
//! merges that read into what it drew.
use super::{SHOWN, chrome, chrome::Chrome, common, dist, look_parity, site::Site, wait_for_feed};
use serde_json::{Value, json};

/// The board fixture at its version, with the look-parity tasks, task page and thread.
fn site() -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let content = look_parity::responses();
    for path in ["/api/task-board", "/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site
}

/// Each role chip the page shows, as its health and its title.
const CHIPS: &str = "[...document.querySelectorAll('main .chip[class*=\"health-\"]')].map(chip => [chip.className.match(/health-([a-z]+)/)[1], chip.title])";

fn chips(page: &mut Chrome) -> Vec<(String, String)> {
    serde_json::from_value(page.eval(CHIPS)).unwrap()
}

/// `handle`'s session, as the feed's `session` change carries it, at `health`.
fn session(handle: &str, health: &str) -> Value {
    json!({"handle":handle,"kind":"worker","parent":null,"worktree":"/fixture","branch":"visual/example",
        "model":"fixture","host":"fixture","affinity":[],"connected":1.0,"last_seen":0.0,
        "last_tool":null,"left_at":null,"health":health})
}

/// Push one `session` change of `handle` at `version`: its record, or its removal.
fn push(site: &Site, version: u64, handle: &str, health: Option<&str>) {
    let data = match health {
        Some(health) => json!({"key":handle,"value":session(handle, health)}),
        None => json!({"key":handle,"removed":true}),
    };
    site.push(
        version,
        &json!({"version":version,"changes":[{"region":"session","data":data}]}).to_string(),
    );
}

/// Wait until every one of the `count` role chips the page shows says `health`.
#[track_caller]
fn every_chip_says(page: &mut Chrome, count: usize, health: &str) {
    let all = format!(
        "(() => {{ const chips = {CHIPS}; return chips.length === {count} && chips.every(([h]) => h === {}); }})()",
        json!(health)
    );
    assert!(
        page.wait_for(&all, SHOWN),
        "not every chip says {health}: {:?}",
        chips(page)
    );
    for (_, title) in chips(page) {
        assert!(title.ends_with(&format!(" is {health}")), "{title}");
    }
}

/// Open `path` once its role chips are drawn and its feed is open: how many it shows.
fn opened(page: &mut Chrome, site: &Site, path: &str) -> usize {
    page.open(&site.url(path));
    assert!(
        page.wait_for(
            "document.querySelectorAll('main .chip.health-live').length > 0",
            SHOWN
        ),
        "no role chip was drawn on {path}: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    wait_for_feed(site).expect("the page never followed the feed");
    chips(page).len()
}

#[test]
fn the_boards_role_chips_take_their_holders_health_from_its_session() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    let count = opened(&mut page, &site, "/tasks");
    every_chip_says(&mut page, count, "live");

    push(&site, 4183, "someone-else", Some("stale"));
    push(&site, 4184, "example", Some("stale"));
    every_chip_says(&mut page, count, "stale");

    push(&site, 4185, "example", None);
    every_chip_says(&mut page, count, "lost");

    push(&site, 4186, "example", Some("live"));
    every_chip_says(&mut page, count, "live");
}

/// A page script that hears each commit of the page's React tree as React DevTools does (its
/// global hook), and keeps in `window.__commits` what each drew again: every component that ran,
/// named by the first element it draws (a card by its slug, a column by its status, else the
/// element's id or tag and class). A component that bailed out, and what is under it, is not named.
const RENDERS: &str = r#"(() => {
  const commits = [];
  window.__commits = commits;
  const first = (fiber) => {
    for (let node = fiber.child; node !== null; node = node.sibling) {
      if (node.tag === 5) return node.stateNode;
      const found = first(node);
      if (found !== null) return found;
    }
    return null;
  };
  const name = (element) => {
    if (element === null) return "";
    if (element.dataset.slug) return `card ${element.dataset.slug}`;
    if (element.dataset.status) return `column ${element.dataset.status}`;
    if (element.id) return `#${element.id}`;
    return `${element.localName}.${element.className}`;
  };
  const drawn = (fiber, out) => {
    for (let node = fiber.child; node !== null; node = node.sibling) {
      const before = node.alternate;
      // A function component's or a memo's that ran (PerformedWork), or one mounted.
      if ([0, 11, 15].includes(node.tag) && (before === null || (node.flags & 1) !== 0)) {
        out.push(name(first(node)));
      }
      // Children React left as they were were not drawn again.
      if (before === null || node.child !== before.child) drawn(node, out);
    }
  };
  let renderers = 0;
  window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
    supportsFiber: true,
    isDisabled: false,
    renderers: new Map(),
    inject(renderer) { renderers += 1; this.renderers.set(renderers, renderer); return renderers; },
    checkDCE() {},
    onScheduleFiberRoot() {},
    onCommitFiberUnmount() {},
    onPostCommitFiberRoot() {},
    setStrictMode() {},
    onCommitFiberRoot(_id, root) {
      const out = [];
      drawn(root.current, out);
      commits.push(out);
    },
  };
})()"#;

/// A live update to one card of the board draws that card again and its column (its count), and
/// nothing else of the board: the other cards, their rows and the other columns stay as they were.
#[test]
fn a_live_update_to_one_card_draws_only_that_card_and_its_column() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": RENDERS }),
    );
    let count = opened(&mut page, &site, "/tasks");
    assert!(count > 1, "{:?}", chips(&mut page));
    assert!(
        page.wait_for("window.__commits.length > 0", SHOWN),
        "the page's React tree was never heard"
    );
    let from = page.eval("window.__commits.length");
    push_fields(
        &site,
        4183,
        "visual/ready",
        &json!({"title": "Example ready renamed"}),
    );
    let renamed = "document.querySelector('[data-slug=\"visual/ready\"] .taskcard-title')?.textContent === 'Example ready renamed'";
    assert!(page.wait_for(renamed, SHOWN), "the update was never drawn");
    // The commit that drew the new title, by what it drew of the board.
    let drawn = page.eval(&format!(
        "[...new Set(window.__commits.slice({from}).find(names => names.includes('card visual/ready')).filter(name => /^(card |column |#row-)/.test(name)))].sort()"
    ));
    assert_eq!(
        drawn,
        json!(["#row-visual/ready", "card visual/ready", "column ready"]),
        "{:?}",
        page.eval(&format!("window.__commits.slice({from})"))
    );
}

#[test]
fn a_task_pages_role_chips_take_their_holders_health_from_its_session() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    let count = opened(&mut page, &site, "/task/visual/in-progress");
    assert_eq!(count, 1, "{:?}", chips(&mut page));

    push(&site, 4183, "example", Some("stale"));
    every_chip_says(&mut page, count, "stale");

    push(&site, 4184, "example", None);
    every_chip_says(&mut page, count, "lost");
}

/// An agent the agents page lists, `handle` at `health`.
pub(super) fn agent(handle: &str, health: &str) -> Value {
    json!({"handle":handle,"kind":"worker","model":"","parent":null,"task":null,"role":null,"tasks":[],
        "worktree":"","branch":"","agent":null,"session_id":null,"pane":null,"callback":null,"chat":false,
        "state":"working","state_detail":null,"health":health,"last_tool_call":null,"last_message":null})
}

/// A task of the agents page's at `status`, held in `roles` (each a handle and its role).
pub(super) fn held(slug: &str, title: &str, status: &str, roles: &[(&str, &str)]) -> Value {
    json!({"slug":slug,"title":title,"body":"","status":status,"priority":2,"parent":null,
        "created_by":"owner","created":1.0,"updated":2.0,"links":[],"depends":[],"instruction":false,
        "roles":roles.iter().map(|(handle, role)| json!({"handle":handle,"role":role,"since":1.0,
            "expires":null,"health":"live"})).collect::<Vec<_>>()})
}

/// An answer of `/api/agent-page`: its whole list, `agents`, beside `tasks`.
pub(super) fn agent_page(agents: &[Value], tasks: &[Value]) -> Value {
    json!({"view":{"generated":1.0,"probes":{"panes":"ok","slots":"ok"},"agents":agents},
        "tasks":tasks,"total":agents.len(),"next":null})
}

/// The reads of the agents page's first page (not of its lost agents) answered so far.
fn first_page_reads(site: &Site) -> usize {
    site.answers("/api/agent-page")
        .iter()
        .filter(|answer| !answer.target.contains("health=lost"))
        .count()
}

/// Push one `task-fields` change of `slug` at `version`.
fn push_fields(site: &Site, version: u64, slug: &str, fields: &Value) {
    site.push(
        version,
        &json!({"version":version,"changes":[{"region":"task-fields","data":{"key":slug,"fields":fields}}]})
            .to_string(),
    );
}

const LOST_SHOWN: &str =
    "[...document.querySelectorAll('details')].some(d => d.innerText.includes('ghost-agent'))";

/// The agents page lists no record of a task the feed changed: it reads its first page again only
/// when it now shows the task (a shown agent's role other than orchestrator, or its end), and merges
/// that read, so the Lost agents list opened keeps its rows. Reads are counted once a later event
/// is drawn, as the feed waits on each read it starts.
#[test]
fn the_agents_page_merges_what_the_feed_brings_in_and_keeps_its_lost_list() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let agents = [agent("orch-agent", "live"), agent("work-agent", "live")];
    site.set_json("/api/agent-page", &agent_page(&agents, &[]));
    site.set_json(
        "/api/agent-page?health=lost",
        &agent_page(&[agent("ghost-agent", "lost")], &[]),
    );
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/agents"));
    assert!(page.wait_for("document.querySelectorAll('.agent').length === 2", SHOWN));
    wait_for_feed(&site).expect("the page never followed the feed");
    page.eval("[...document.querySelectorAll('summary')].find(s => s.textContent.trim() === 'Lost agents').click()");
    assert!(
        page.wait_for(LOST_SHOWN, SHOWN),
        "the lost list never showed: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    let reads = first_page_reads(&site);

    // An open task its orchestrator alone is shown holding: in no list the page reads. The agent
    // change after it is drawn once it was taken, and any read it started answered.
    push_fields(
        &site,
        4183,
        "work/o",
        &json!({"roles": [{"handle":"orch-agent","role":"orchestrator","since":1.0,"expires":null,"health":"live"}]}),
    );
    site.push(
        4184,
        &json!({"version":4184,"changes":[{"region":"agent","data":{"key":"work-agent","value":agent("work-agent","stale")}}]})
            .to_string(),
    );
    assert!(
        page.wait_for(
            "document.querySelectorAll('.agents-card .chip.health-stale').length === 1",
            SHOWN
        ),
        "the agent change was never drawn: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    assert_eq!(
        first_page_reads(&site),
        reads,
        "an orchestrator's role read the page again"
    );

    let work = held(
        "work/x",
        "Work in hand",
        "in_progress",
        &[("orch-agent", "orchestrator"), ("work-agent", "executor")],
    );
    site.set_json(
        "/api/agent-page",
        &agent_page(&agents, std::slice::from_ref(&work)),
    );
    push_fields(&site, 4185, "work/x", &json!({"roles": work["roles"]}));
    assert!(
        page.wait_for(
            "document.querySelector('.agents-now')?.innerText.includes('Work in hand')",
            SHOWN
        ),
        "the task a shown agent came to work on never showed in Happening now: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    assert_eq!(first_page_reads(&site), reads + 1);
    assert_eq!(
        page.eval(LOST_SHOWN),
        json!(true),
        "the opened Lost agents list lost its rows"
    );

    // An open task only its orchestrator is shown holding finishes: its Finished line takes it.
    let finished = held(
        "work/y",
        "Finished work",
        "done",
        &[("orch-agent", "orchestrator")],
    );
    site.set_json("/api/agent-page", &agent_page(&agents, &[work, finished]));
    push_fields(&site, 4186, "work/y", &json!({"status": "done"}));
    assert!(
        page.wait_for(
            "[...document.querySelectorAll('.agent')].some(a => a.innerText.includes('orch-agent') && a.innerText.includes('Finished: Finished work'))",
            SHOWN
        ),
        "the orchestrator's Finished line never took its task: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    assert_eq!(first_page_reads(&site), reads + 2);
    assert_eq!(page.eval(LOST_SHOWN), json!(true));
    assert_eq!(
        page.eval("document.querySelectorAll('.agents-card .agent').length"),
        json!(2)
    );
}
