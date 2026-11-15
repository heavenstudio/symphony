use super::*;
use crate::testing::{asking, judged, listing, project};

const NOW: u64 = 1_800_000_000;

fn server(address: &str) -> Server {
    Server::parse(address).unwrap()
}

fn here() -> (Server, String) {
    (server("http://127.0.0.1:8765"), "This Mac".into())
}

fn studio() -> (Server, String) {
    (
        server("http://studio.tail1234.ts.net:8765"),
        "studio".into(),
    )
}

/// `follow`'s server `at` answering `list` at `now`: its notices.
fn hear(follow: &mut Follow, at: usize, list: ProjectList, now: u64) -> Vec<Notice> {
    let (notices, next) = follow.heard(at, Answer::Listed(list), now);
    assert_eq!(
        next, EVERY,
        "a server that answers is read again every {EVERY:?}"
    );
    notices
}

#[test]
fn what_waits_is_the_cards_and_the_questions_summed_over_every_project_and_server() {
    let mut follow = Follow::new([here(), studio()]);
    assert_eq!(follow.total(), 0, "nothing read yet");
    hear(
        &mut follow,
        0,
        listing(
            Some("a"),
            vec![project("symphony", 3), asking(project("lamps", 1), 2)],
        ),
        NOW,
    );
    assert_eq!(follow.total(), 6);
    hear(
        &mut follow,
        1,
        listing(Some("b"), vec![asking(project("lamps", 0), 4)]),
        NOW,
    );
    assert_eq!(
        follow.total(),
        10,
        "two servers of one project are two boards, counted apart"
    );
    // A verdict counts toward nothing; a refused project or one not read yet counts none.
    let mut refused = project("notes", 0);
    refused.refused = Some("lacks a plugin".into());
    refused.waiting = None;
    refused.asks = None;
    hear(
        &mut follow,
        1,
        listing(
            Some("b"),
            vec![
                judged(asking(project("lamps", 0), 4), 9, "lamps/x", "approve"),
                refused,
            ],
        ),
        NOW + 2,
    );
    assert_eq!(follow.total(), 10);
}

#[test]
fn one_server_reached_two_ways_is_counted_and_told_once() {
    let mut follow = Follow::new([here(), studio()]);
    let list = |cards| listing(Some("same"), vec![project("symphony", cards)]);
    hear(&mut follow, 0, list(2), NOW);
    hear(&mut follow, 1, list(2), NOW);
    assert_eq!(follow.total(), 2);
    assert_eq!(
        hear(&mut follow, 0, list(3), NOW + 2).len(),
        1,
        "the first tells"
    );
    assert_eq!(
        hear(&mut follow, 1, list(3), NOW + 2),
        [],
        "the second does not"
    );
    assert_eq!(follow.total(), 3);
    let lines = follow.lines(NOW + 2);
    assert_eq!(
        lines
            .iter()
            .map(|line| line.label.as_str())
            .collect::<Vec<_>>(),
        [
            "This Mac — 3",
            "    symphony works — 3",
            "studio — the same server as This Mac"
        ]
    );
    // Two servers whose installation is unknown are told apart.
    let mut unknown = Follow::new([here(), studio()]);
    hear(
        &mut unknown,
        0,
        listing(None, vec![project("symphony", 2)]),
        NOW,
    );
    hear(
        &mut unknown,
        1,
        listing(None, vec![project("symphony", 2)]),
        NOW,
    );
    assert_eq!(unknown.total(), 4);
}

#[test]
fn a_rise_in_the_cards_gives_one_notice_naming_the_first_card_and_opening_it() {
    let mut follow = Follow::new([here()]);
    assert_eq!(
        hear(
            &mut follow,
            0,
            listing(Some("a"), vec![project("symphony", 2)]),
            NOW
        ),
        [],
        "what waits when the app starts is the badge's, never a notice"
    );
    let notices = hear(
        &mut follow,
        0,
        listing(Some("a"), vec![project("symphony", 3)]),
        NOW + 2,
    );
    assert_eq!(
        notices,
        [Notice {
            kind: Kind::Card,
            title: "3 cards wait on you".into(),
            place: None,
            body: "next card of symphony".into(),
            server: 0,
            page: "http://127.0.0.1:8765/symphony/docs/symphony.md#next".into(),
        }]
    );
    assert_eq!(
        hear(
            &mut follow,
            0,
            listing(Some("a"), vec![project("symphony", 3)]),
            NOW + 4
        ),
        [],
        "the same count again tells nothing"
    );
    assert_eq!(
        hear(
            &mut follow,
            0,
            listing(Some("a"), vec![project("symphony", 1)]),
            NOW + 6
        ),
        [],
        "a fall tells nothing"
    );
    let one = hear(
        &mut follow,
        0,
        listing(Some("a"), vec![project("symphony", 2)]),
        NOW + 8,
    );
    assert_eq!(one[0].title, "2 cards wait on you");
    let mut first = project("symphony", 1);
    first.waiting = Some(1);
    let mut follow = Follow::new([here()]);
    hear(
        &mut follow,
        0,
        listing(Some("a"), vec![project("symphony", 0)]),
        NOW,
    );
    let one = hear(&mut follow, 0, listing(Some("a"), vec![first]), NOW + 2);
    assert_eq!(one[0].title, "1 card waits on you");
}

#[test]
fn a_card_with_no_first_card_named_opens_the_projects_home() {
    let mut follow = Follow::new([here()]);
    hear(
        &mut follow,
        0,
        listing(Some("a"), vec![project("symphony", 0)]),
        NOW,
    );
    let mut unnamed = project("symphony", 1);
    unnamed.next = None;
    let notices = hear(&mut follow, 0, listing(Some("a"), vec![unnamed]), NOW + 2);
    assert_eq!(
        (notices[0].body.as_str(), notices[0].page.as_str()),
        ("", "http://127.0.0.1:8765/symphony/")
    );
}

#[test]
fn a_question_on_a_task_raises_the_count_and_gives_one_notice_of_its_own() {
    let mut follow = Follow::new([here()]);
    hear(
        &mut follow,
        0,
        listing(Some("a"), vec![project("symphony", 2)]),
        NOW,
    );
    let notices = hear(
        &mut follow,
        0,
        listing(Some("a"), vec![asking(project("symphony", 2), 1)]),
        NOW + 2,
    );
    assert_eq!(
        notices,
        [Notice {
            kind: Kind::Question,
            title: "1 question waits on you".into(),
            place: None,
            body: "An agent asked you on a task".into(),
            server: 0,
            page: "http://127.0.0.1:8765/symphony/".into(),
        }]
    );
    assert_eq!(follow.total(), 3);
    let both = hear(
        &mut follow,
        0,
        listing(Some("a"), vec![asking(project("symphony", 3), 2)]),
        NOW + 4,
    );
    assert_eq!(
        both.iter()
            .map(|n| (n.kind, n.title.as_str()))
            .collect::<Vec<_>>(),
        [
            (Kind::Card, "3 cards wait on you"),
            (Kind::Question, "2 questions wait on you")
        ]
    );
}

#[test]
fn a_new_verdict_gives_one_notice_and_no_count() {
    let mut follow = Follow::new([here()]);
    hear(
        &mut follow,
        0,
        listing(Some("a"), vec![project("symphony", 1)]),
        NOW,
    );
    let approved = judged(project("symphony", 1), 40, "tools/desktop-app", "approve");
    let notices = hear(
        &mut follow,
        0,
        listing(Some("a"), vec![approved.clone()]),
        NOW + 2,
    );
    assert_eq!(
        notices,
        [Notice {
            kind: Kind::Verdict,
            title: "slate-wren approved tools/desktop-app".into(),
            place: None,
            body: "A reviewer's verdict".into(),
            server: 0,
            page: "http://127.0.0.1:8765/symphony/task/tools/desktop-app".into(),
        }]
    );
    assert_eq!(follow.total(), 1, "a verdict asks nothing of the owner");
    assert_eq!(
        hear(&mut follow, 0, listing(Some("a"), vec![approved]), NOW + 4),
        [],
        "the same verdict again"
    );
    for (id, verdict, title) in [
        (41, "changes", "slate-wren asks for changes on tools/x"),
        (42, "withdrawn", "slate-wren: withdrawn on tools/x"),
    ] {
        let notices = hear(
            &mut follow,
            0,
            listing(
                Some("a"),
                vec![judged(project("symphony", 1), id, "tools/x", verdict)],
            ),
            NOW + 6,
        );
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, title);
    }
    // An older line, as a board restored from before, tells nothing.
    let older = judged(project("symphony", 1), 7, "tools/y", "approve");
    assert_eq!(
        hear(&mut follow, 0, listing(Some("a"), vec![older]), NOW + 8),
        []
    );
}

#[test]
fn a_project_first_seen_or_its_board_first_read_tells_nothing() {
    let mut follow = Follow::new([here()]);
    let mut unread = project("symphony", 0);
    unread.waiting = None;
    unread.asks = None;
    hear(&mut follow, 0, listing(Some("a"), vec![unread]), NOW);
    let read = judged(asking(project("symphony", 4), 1), 3, "tools/x", "approve");
    assert_eq!(
        hear(
            &mut follow,
            0,
            listing(Some("a"), vec![read.clone(), project("lamps", 5)]),
            NOW + 2
        ),
        []
    );
    assert_eq!(follow.total(), 10);
}

#[test]
fn a_notice_names_the_server_and_the_project_only_when_there_is_more_than_one() {
    // One server, two projects: the project is named.
    let mut follow = Follow::new([here()]);
    let two = |cards| {
        listing(
            Some("a"),
            vec![project("symphony", cards), project("lamps", 0)],
        )
    };
    hear(&mut follow, 0, two(1), NOW);
    let notices = hear(&mut follow, 0, two(2), NOW + 2);
    assert_eq!(notices[0].place.as_deref(), Some("symphony works"));
    // Two servers, one project each, the same one: both named.
    let mut follow = Follow::new([here(), studio()]);
    let one = |installation, cards| listing(Some(installation), vec![project("lamps", cards)]);
    hear(&mut follow, 0, one("a", 1), NOW);
    hear(&mut follow, 1, one("b", 1), NOW);
    let notices = hear(&mut follow, 1, one("b", 2), NOW + 2);
    assert_eq!(notices[0].place.as_deref(), Some("studio · lamps works"));
    assert_eq!(notices[0].server, 1);
    assert_eq!(
        notices[0].page,
        "http://studio.tail1234.ts.net:8765/lamps/docs/lamps.md#next"
    );
    // Two servers, the second not read yet, one project: the server alone is named.
    let mut follow = Follow::new([here(), studio()]);
    hear(&mut follow, 0, one("a", 1), NOW);
    assert_eq!(
        hear(&mut follow, 0, one("a", 2), NOW + 2)[0]
            .place
            .as_deref(),
        Some("This Mac")
    );
}

#[test]
fn a_server_that_stops_answering_is_retried_after_one_second_doubling_to_thirty() {
    let mut follow = Follow::new([here()]);
    let waits: Vec<u64> = (0..8)
        .map(|n| {
            follow
                .heard(0, Answer::Lost("refused".into()), NOW + n)
                .1
                .as_secs()
        })
        .collect();
    assert_eq!(waits, [1, 2, 4, 8, 16, 30, 30, 30]);
    assert_eq!(
        hear(&mut follow, 0, listing(Some("a"), vec![]), NOW + 9),
        []
    );
    let (_, again) = follow.heard(0, Answer::Lost("refused".into()), NOW + 10);
    assert_eq!(again, FIRST_RETRY, "a list read starts the doubling again");
    assert_eq!(retry(u32::MAX), LAST_RETRY);
    assert_eq!(retry(0), FIRST_RETRY);
    let (notices, older) = follow.heard(0, Answer::Older, NOW + 11);
    assert_eq!((notices, older), (vec![], OLDER_EVERY));
    let (_, lost) = follow.heard(0, Answer::Lost("refused".into()), NOW + 12);
    assert_eq!(lost, FIRST_RETRY, "an older server's answer is an answer");
}

#[test]
fn an_unreachable_server_shows_its_last_count_greyed_counts_toward_no_badge_and_tells_a_rise_once_back()
 {
    let mut follow = Follow::new([here(), studio()]);
    hear(
        &mut follow,
        0,
        listing(Some("a"), vec![project("symphony", 1)]),
        NOW,
    );
    hear(
        &mut follow,
        1,
        listing(Some("b"), vec![asking(project("lamps", 2), 1)]),
        NOW,
    );
    assert_eq!(follow.total(), 4);
    follow.heard(1, Answer::Lost("refused".into()), NOW + 10);
    assert_eq!(follow.total(), 1, "the badge sums what answers now");
    let lines = follow.lines(NOW + 250);
    assert_eq!(
        lines
            .iter()
            .map(|line| (line.label.as_str(), line.greyed))
            .collect::<Vec<_>>(),
        [
            ("This Mac — 1", false),
            ("    symphony works — 1", false),
            ("studio — 3, not answering, last reached 4 min ago", true),
            ("    lamps works — 3", true),
        ]
    );
    assert_eq!(
        (
            lines[3].server,
            lines[3].page.as_deref(),
            lines[2].page.as_deref()
        ),
        (1, Some("http://studio.tail1234.ts.net:8765/lamps/"), None)
    );
    let back = hear(
        &mut follow,
        1,
        listing(Some("b"), vec![asking(project("lamps", 3), 1)]),
        NOW + 300,
    );
    assert_eq!(
        back.len(),
        1,
        "a rise past the last count read tells once it answers again"
    );
    assert_eq!(follow.total(), 5);
}

#[test]
fn the_menu_says_what_each_server_is_while_unread_older_or_never_reached() {
    let mut follow = Follow::new([
        here(),
        studio(),
        (server("http://100.64.0.9:1"), "spare".into()),
    ]);
    follow.heard(1, Answer::Older, NOW);
    follow.heard(2, Answer::Lost("refused".into()), NOW);
    let lines = follow.lines(NOW);
    assert_eq!(
        lines
            .iter()
            .map(|line| (
                line.label.as_str(),
                line.greyed,
                line.server,
                line.page.as_deref()
            ))
            .collect::<Vec<_>>(),
        [
            ("This Mac", false, 0, None),
            (
                "studio — needs a newer Symphony for its count",
                false,
                1,
                None
            ),
            ("spare — not answering", true, 2, None),
        ]
    );
    let mut refused = project("notes", 0);
    refused.refused = Some("lacks a plugin".into());
    let mut unread = project("orbit", 0);
    unread.waiting = None;
    hear(
        &mut follow,
        0,
        listing(Some("a"), vec![refused, unread]),
        NOW,
    );
    let lines = follow.lines(NOW);
    assert_eq!(lines[1].label, "    notes works — refused by this server");
    assert_eq!(lines[2].label, "    orbit works");
    assert_eq!(
        follow
            .servers()
            .map(|(server, name)| (server.to_string(), name.to_owned()))
            .collect::<Vec<_>>(),
        [
            ("http://127.0.0.1:8765".to_owned(), "This Mac".to_owned()),
            (
                "http://studio.tail1234.ts.net:8765".to_owned(),
                "studio".to_owned()
            ),
            ("http://100.64.0.9:1".to_owned(), "spare".to_owned()),
        ]
    );
}

#[test]
fn a_read_of_a_server_the_app_does_not_follow_changes_nothing() {
    let mut follow = Follow::new([here()]);
    let (notices, next) = follow.heard(5, Answer::Older, NOW);
    assert_eq!((notices, next), (vec![], EVERY));
    assert_eq!(follow.lines(NOW).len(), 1);
}

#[test]
fn how_long_ago_reads_in_words() {
    for (seconds, said) in [
        (0, "less than a minute ago"),
        (59, "less than a minute ago"),
        (60, "1 min ago"),
        (3599, "59 min ago"),
        (3600, "1 h ago"),
        (86_399, "23 h ago"),
        (86_400, "1 day ago"),
        (3 * 86_400 + 5, "3 days ago"),
    ] {
        assert_eq!(ago(seconds), said, "{seconds}");
    }
}
