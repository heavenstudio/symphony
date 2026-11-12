use super::*;
use serde_json::json;

fn task(title: &str, parent: Option<&str>, links: &[&str], roles: &[(&str, &str)]) -> Task {
    card("bulk/ready-1", 3, title, parent, links, roles)
}

fn card(
    slug: &str,
    priority: i64,
    title: &str,
    parent: Option<&str>,
    links: &[&str],
    roles: &[(&str, &str)],
) -> Task {
    let roles: Vec<_> = roles
        .iter()
        .map(|(role, handle)| json!({"handle": handle, "role": role, "since": 0.0, "expires": null, "health": "live"}))
        .collect();
    serde_json::from_value(json!({"slug": slug, "title": title, "body": "",
        "status": "ready", "priority": priority, "parent": parent, "created_by": "owner",
        "created": 0.0, "updated": 0.0, "links": links, "depends": [], "roles": roles,
        "instruction": false}))
    .unwrap()
}

fn desk(task: &Task) -> u32 {
    row_height(task, &[], Layout::Desk)
}

/// Titles wrap where the board's cards wrap them, measured in a 1280 px window: 238 px of
/// "A task whose title is long enough to" fits a line, 256 px of "Ready task number 1 in a
/// long column" does not, and a word longer than a line breaks inside itself.
#[test]
fn a_title_wraps_where_a_card_wraps_it() {
    let lines = |text: &str| Lines::new(2440).set(text, Face::Bold, 140).count();
    assert_eq!(lines(""), 0);
    assert_eq!(lines("   "), 0);
    assert_eq!(lines("Polish the product"), 1);
    assert_eq!(lines("A task whose title is long enough to"), 1);
    assert_eq!(lines("Ready task number 1 in a long column"), 2);
    assert_eq!(
        lines("Merge the lamp catalogue's near-duplicates down to about ninety models"),
        3
    );
    assert_eq!(
        lines(
            "A task whose title is long enough to wrap over three lines on a card in a narrow column of the board, so the card's layout is tested"
        ),
        4
    );
    assert_eq!(
        lines(&"m".repeat(60)),
        4,
        "60 m's are 747 px, about three lines and a bit"
    );
}

/// The slug breaks after its hyphens and slashes, 35 of its characters to a line, and its
/// priority follows it after a space, in a smaller face than the slug's.
#[test]
fn a_slug_and_its_priority_share_a_line_while_they_fit() {
    let slug = |slug: &str, priority: &str| {
        Lines::new(2410)
            .set(slug, Face::Mono, 115)
            .space(Face::Regular, 140)
            .set(priority, Face::Bold, 101)
            .count()
    };
    assert_eq!(slug("bulk/ready-1", "p3"), 1);
    // 32 characters of slug are 221 px, and the priority 15 px more: 236 px in a p1's 241 px.
    assert_eq!(slug("inbox/agents-page-needs-redesign", "p1"), 1);
    assert_eq!(slug("inbox/agents-page-needs-redesign-now", "p1"), 2);
    assert_eq!(
        slug(
            "app/a-very-long-task-slug-that-goes-on-and-on-to-test-wrapping-in-cards",
            "p1"
        ),
        3
    );
}

/// A card with no chips draws at 101 px with a title of two lines; each row of chips adds its
/// own height, a parent its line, and chips too wide for one row wrap to more.
#[test]
fn a_card_reserves_its_lines_and_its_rows_of_chips() {
    let two = "Ready task number 1 in a long column";
    assert_eq!(desk(&task(two, None, &[], &[])), 101);
    assert_eq!(desk(&task("Short", None, &[], &[])), 81);
    assert_eq!(
        desk(&task(two, None, &[], &[("orchestrator", "quiet-plover")])),
        122
    );
    assert_eq!(
        desk(&task(
            two,
            None,
            &["docs/DECISIONS.md"],
            &[("orchestrator", "quiet-plover")]
        )),
        139
    );
    assert_eq!(
        desk(&task(two, Some("app/parent"), &[], &[])),
        121,
        "a parent adds its line"
    );
    // Two role chips of 160 px and more do not share a 244 px row.
    assert_eq!(
        desk(&task(
            two,
            None,
            &[],
            &[
                ("orchestrator", "quiet-plover"),
                ("executor", "misty-osprey")
            ]
        )),
        147
    );
    // Three short link chips share one row.
    assert_eq!(
        desk(&task(
            two,
            None,
            &["docs/A.md", "docs/B.md", "docs/C.md"],
            &[]
        )),
        118
    );
}

#[test]
fn chips_fill_a_row_before_they_wrap_and_none_take_no_height() {
    let role = (1000, 212);
    assert_eq!(chip_rows(std::iter::empty(), 2440), 0);
    assert_eq!(chip_rows([role, role].into_iter(), 2440), 212);
    assert_eq!(
        chip_rows([role, role, role].into_iter(), 2440),
        2 * 212 + GAP
    );
    assert_eq!(
        chip_rows([(9000, 172)].into_iter(), 2440),
        172,
        "a chip wider than the card takes one row"
    );
    assert_eq!(
        chip_rows([(1000, 172), (1000, 324), (1000, 172)].into_iter(), 2440),
        324 + GAP + 172,
        "a row is as tall as its tallest chip"
    );
}

/// A chip as wide as its text keeps to one line; a link or phase chip wider than the card wraps
/// its words inside the card's width, a role chip never does.
#[test]
fn a_chip_too_wide_for_the_card_wraps_inside_itself_unless_it_is_a_role() {
    assert_eq!(chip("TASKS.md", &LINK, 2440).1, 172);
    let anchor = "reports-and-monitoring.md#What the owner opens";
    let (width, height) = chip(anchor, &LINK, 2440);
    assert_eq!((width, height), (2440, 20 + 2 * 152));
    assert_eq!(chip("executor misty-osprey", &ROLE, 2440).1, 212);
    assert_eq!(chip(&"executor ".repeat(6), &ROLE, 2440), (2440, 212));
    let observed = "Observed Build running · Stefanos-MacBook-Pro.local · partial inventory";
    assert_eq!(chip(observed, &PHASE, 2440).1, 20 + 2 * 192);
    assert_eq!(chip("Build running · alpha", &PHASE, 2440).1, 212);
}

/// A window of 760 px or less draws the phone's narrower columns, and a p0 or p1 card's text is
/// 3 px narrower than another's, behind its wider left border.
#[test]
fn the_layout_and_the_priority_set_how_wide_a_cards_text_runs() {
    assert_eq!(Layout::at(390.0), Layout::Phone);
    assert_eq!(Layout::at(760.0), Layout::Phone);
    assert_eq!(Layout::at(761.0), Layout::Desk);
    assert_eq!(Layout::at(1280.0), Layout::Desk);
    assert_eq!(Layout::now(), Layout::Desk, "with no window, a desk's");
    assert_eq!(Layout::Desk.text(2), 2440);
    assert_eq!(Layout::Desk.text(1), 2410);
    assert_eq!(Layout::Desk.text(0), 2410);
    assert_eq!(Layout::Phone.text(3), 2360);
    assert_eq!(Layout::Phone.text(0), 2330);
}

/// Cards from this repository's own board, drawn by the client in headless Chrome at 1280 and
/// 390 px, with the heights they drew at: a p1 slug that only just fits its line on a desk, a
/// link to a section whose chip wraps, two and three role chips, build phases that wrap, and
/// titles whose last line a phone's narrower card pushes over. (The machine is renamed.)
#[test]
fn a_card_reserves_what_the_live_board_drew() {
    let observed = "Observed Build running · build-host-mini.local · partial inventory";
    let cards = [
        (
            card(
                "inbox/agents-page-needs-redesign",
                1,
                "agents page needs redesign",
                None,
                &[],
                &[],
            ),
            vec![],
            (81, 101),
        ),
        (
            card(
                "tools/runs-page",
                1,
                "Each project gets a Runs page: what runs and waits now, every run with its steps and log, and trends, on a phone and wide",
                None,
                &["docs/analysis/reports-and-monitoring.md#What the owner opens"],
                &[],
            ),
            vec![],
            (174, 174),
        ),
        (
            card(
                "tools/agent-cost-estimate",
                2,
                "Agents' cost is estimated in money at API list prices, from a dated price table and the four kinds of token",
                None,
                &["docs/analysis/reports-and-monitoring.md#Agents"],
                &[],
            ),
            vec![],
            (138, 159),
        ),
        (
            card(
                "tests/signal-tests-inherit-ignored",
                0,
                "Main is red in a nightly started in the background: three board signal tests assume SIGINT and SIGQUIT are inherited at their default",
                None,
                &[],
                &[
                    ("orchestrator", "silver-sparrow"),
                    ("executor", "slate-osprey"),
                ],
            ),
            vec![],
            (208, 229),
        ),
        (
            card(
                "app/build-test-columns",
                1,
                "The board's build and test columns are always empty: nothing in this project reports a build or a test run",
                None,
                &["crates/symphony-api/src/task_stage.rs"],
                &[
                    ("orchestrator", "quiet-plover"),
                    ("executor", "sable-crane"),
                    ("reviewer", "dusky-owl"),
                ],
            ),
            vec![],
            (210, 230),
        ),
        (
            card(
                "app/timeline-clicks-reach-the-page",
                0,
                "A click in the task timeline runs only the timeline's control: Show all no longer also opens the + chat sheet",
                None,
                &[],
                &[("executor", "fleet-otter")],
            ),
            vec![observed.to_string()],
            (229, 229),
        ),
        (
            card(
                "perf/client-distribution-keeps-every-build",
                0,
                "The embedded client distribution keeps every build ever published: 266 MB in the server binary and in every consumer's build",
                None,
                &[],
                &[("executor", "glacial-ibis")],
            ),
            vec![observed.to_string(), observed.to_string()],
            (274, 274),
        ),
    ];
    for (task, phases, (on_desk, on_phone)) in cards {
        assert_eq!(
            (
                row_height(&task, &phases, Layout::Desk),
                row_height(&task, &phases, Layout::Phone)
            ),
            (on_desk, on_phone),
            "{}",
            task.slug
        );
    }
}

/// A card with build and test phases has their row above its links, 6 px below the title.
#[test]
fn a_cards_phases_take_a_row_of_their_own() {
    let short = task("Short", None, &[], &[]);
    let phases = |labels: &[&str]| {
        row_height(
            &short,
            &labels.iter().map(ToString::to_string).collect::<Vec<_>>(),
            Layout::Desk,
        )
    };
    assert_eq!(phases(&[]), 81);
    assert_eq!(phases(&["Build running · alpha"]), 81 + 6 + 21);
    assert_eq!(
        phases(&["Build running · alpha", "Test queued · alpha"]),
        81 + 6 + 21 + 4 + 21,
        "phases of 147 and 144 px do not share a 244 px row"
    );
}
