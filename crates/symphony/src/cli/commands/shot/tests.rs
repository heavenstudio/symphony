//! The command line, keys, readiness and report of `symphony shot`, without a browser. The browser
//! cases (a real page, its pictures, its clean failures) are in apps/symphony-ui/tests/browser/shot.rs.

use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn shot(values: &[&str]) -> Shot {
    read(&args(values)).unwrap().unwrap()
}

fn refusal(values: &[&str]) -> String {
    read(&args(values)).unwrap_err()
}

#[test]
fn a_bare_shot_takes_the_desktop_viewport_and_twenty_seconds() {
    assert_eq!(
        shot(&["http://127.0.0.1:1/tasks", "--out", "a.png"]),
        Shot {
            url: "http://127.0.0.1:1/tasks".into(),
            out: "a.png".into(),
            timeout: Duration::from_secs(20),
            view: View::Size(1440, 900),
            full_page: false,
            scheme: None,
            report: false,
            steps: Vec::new(),
        }
    );
}

#[test]
fn steps_keep_their_command_line_order_across_kinds_and_repeat() {
    let shot = shot(&[
        "--click",
        "button[aria-label=\"Status\"]",
        "http://h/x",
        "--shot",
        "open.png",
        "--key=Escape",
        "--out",
        "closed.png",
        "--wait-for",
        ".kanban",
        "--type",
        "input[name=q]=a=b",
        "--hover",
        "h1",
        "--eval",
        "document.title",
        "--pause",
        "250",
        "--click",
        "h1",
        "--shot",
        "again.png",
    ]);
    assert_eq!(shot.out, PathBuf::from("closed.png"));
    assert_eq!(
        shot.steps,
        [
            Step::Click("button[aria-label=\"Status\"]".into()),
            Step::Shot("open.png".into()),
            Step::Key(key("Escape").unwrap()),
            Step::WaitFor(".kanban".into()),
            Step::Type("input[name=q]".into(), "a=b".into()),
            Step::Hover("h1".into()),
            Step::Eval("document.title".into()),
            Step::Pause(Duration::from_millis(250)),
            Step::Click("h1".into()),
            Step::Shot("again.png".into()),
        ]
    );
}

#[test]
fn the_view_options_read_as_given() {
    let shot = shot(&[
        "about:blank",
        "--out=x.png",
        "--size",
        "800x600",
        "--full-page",
        "--scheme",
        "dark",
        "--report",
        "--timeout",
        "2.5",
    ]);
    assert_eq!(shot.view, View::Size(800, 600));
    assert!(shot.full_page && shot.report);
    assert_eq!(shot.scheme, Some(Scheme::Dark));
    assert_eq!(shot.timeout, Duration::from_millis(2500));
    let phone = self::shot(&[
        "data:text/html,x",
        "--out",
        "p.png",
        "--phone",
        "--scheme",
        "light",
    ]);
    assert_eq!(phone.view, View::Phone);
    assert_eq!(phone.scheme, Some(Scheme::Light));
}

#[test]
fn help_is_no_shot() {
    assert_eq!(read(&args(&["--help"])).unwrap(), None);
    assert_eq!(read(&args(&["http://h/", "-h"])).unwrap(), None);
}

#[test]
fn a_command_line_it_cannot_run_is_refused_naming_the_argument() {
    for (line, said) in [
        (
            &["--out", "a.png"][..],
            "the following arguments are required: URL",
        ),
        (
            &["http://h/"],
            "the following arguments are required: --out",
        ),
        (
            &["http://h/", "http://i/", "--out", "a"],
            "unrecognized arguments: http://i/",
        ),
        (
            &["127.0.0.1:8765/tasks", "--out", "a"],
            "is not an absolute URL",
        ),
        (
            &["http://h/", "--out", "folder/"],
            "argument --out: \"folder/\" names no file",
        ),
        (
            &["http://h/", "--out", "a", "--shot", ""],
            "argument --shot: \"\" names no file",
        ),
        (
            &["http://h/", "--out", "a", "--timeout", "0"],
            "argument --timeout: invalid SECS \"0\"",
        ),
        (
            &["http://h/", "--out", "a", "--timeout", "soon"],
            "argument --timeout: invalid SECS",
        ),
        (
            &["http://h/", "--out", "a", "--timeout", "inf"],
            "argument --timeout: invalid SECS",
        ),
        (
            &["http://h/", "--out", "a", "--size", "1440"],
            "argument --size: invalid WxH \"1440\"",
        ),
        (
            &["http://h/", "--out", "a", "--size", "0x900"],
            "argument --size: invalid WxH",
        ),
        (
            &["http://h/", "--out", "a", "--size", "1440x900x2"],
            "argument --size: invalid WxH",
        ),
        (
            &["http://h/", "--out", "a", "--size", "800x600", "--phone"],
            "argument --phone: not allowed with argument --size",
        ),
        (
            &["http://h/", "--out", "a", "--scheme", "sepia"],
            "argument --scheme: invalid choice: \"sepia\"",
        ),
        (
            &["http://h/", "--out", "a", "--pause", "1.5"],
            "argument --pause: invalid MS \"1.5\"",
        ),
        (
            &["http://h/", "--out", "a", "--key", "Hyper+a"],
            "argument --key: \"Hyper\" in \"Hyper+a\" is no modifier",
        ),
        (
            &["http://h/", "--out", "a", "--key", "F13"],
            "argument --key: \"F13\" is no key",
        ),
        (
            &["http://h/", "--out", "a", "--type", "no-equals"],
            "argument --type: \"no-equals\" is not CSS=TEXT",
        ),
        (
            &["http://h/", "--out", "a", "--type", "=text"],
            "argument --type: \"=text\" is not CSS=TEXT",
        ),
        (
            &["http://h/", "--out", "a", "--click", " "],
            "argument --click: an empty selector",
        ),
        (
            &["http://h/", "--out", "a", "--wait-for"],
            "argument --wait-for: expected one argument",
        ),
        (
            &["http://h/", "--out", "a", "--zoom", "2"],
            "unrecognized arguments: --zoom",
        ),
    ] {
        let refused = refusal(line);
        assert!(refused.contains(said), "{line:?}: {refused}");
    }
}

#[test]
fn a_refusal_prints_the_usage_and_exits_two_and_help_exits_zero() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&args(&["http://h/"]), &mut out, &mut err), 2);
    let err = String::from_utf8(err).unwrap();
    assert!(err.starts_with("usage: symphony shot"), "{err}");
    assert!(
        err.contains("symphony shot: error: the following arguments are required: --out"),
        "{err}"
    );
    assert!(out.is_empty());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&args(&["--help"]), &mut out, &mut err), 0);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("--click CSS"), "{out}");
    assert!(err.is_empty());
}

#[test]
fn typing_splits_at_the_first_equals_outside_brackets_parentheses_and_quotes() {
    for (value, split) in [
        ("#q=hello", Some(("#q", "hello"))),
        ("#q=", Some(("#q", ""))),
        ("#q=a=b", Some(("#q", "a=b"))),
        ("input[name=q]=text", Some(("input[name=q]", "text"))),
        (
            "input[name=\"a=]b\"]=x",
            Some(("input[name=\"a=]b\"]", "x")),
        ),
        (
            "li:not([data-x='1=2'])=y",
            Some(("li:not([data-x='1=2'])", "y")),
        ),
        (
            "textarea:is(.a, [b=c]) = spaced",
            Some(("textarea:is(.a, [b=c]) ", " spaced")),
        ),
        ("input[name=q]", None),
        ("plain", None),
        ("=text", None),
        (" =text", None),
    ] {
        assert_eq!(split_type(value), split, "{value}");
    }
}

#[test]
fn keys_carry_their_codes_text_and_held_modifiers() {
    let tab = key("Tab").unwrap();
    assert_eq!(
        tab,
        Key {
            key: "Tab".into(),
            code: "Tab".into(),
            key_code: 9,
            text: None,
            modifiers: 0
        }
    );
    let enter = key("Enter").unwrap();
    assert_eq!((enter.key_code, enter.text.as_deref()), (13, Some("\r")));
    let back = key("Shift+Tab").unwrap();
    assert_eq!((back.key.as_str(), back.modifiers), ("Tab", SHIFT));
    let capital = key("Shift+a").unwrap();
    assert_eq!(
        (
            capital.key.as_str(),
            capital.code.as_str(),
            capital.key_code,
            capital.text.as_deref()
        ),
        ("A", "KeyA", 65, Some("A"))
    );
    let select_all = key("Control+a").unwrap();
    assert_eq!((select_all.text, select_all.modifiers), (None, CONTROL));
    let shortcut = key("Meta+Shift+k").unwrap();
    assert_eq!((shortcut.modifiers, shortcut.text), (META | SHIFT, None));
    let digit = key("7").unwrap();
    assert_eq!((digit.code.as_str(), digit.key_code), ("Digit7", 55));
    let plus = key("+").unwrap();
    assert_eq!(
        (plus.key.as_str(), plus.code.as_str(), plus.text.as_deref()),
        ("+", "", Some("+"))
    );
    let shifted_plus = key("Shift++").unwrap();
    assert_eq!(
        (shifted_plus.key.as_str(), shifted_plus.modifiers),
        ("+", SHIFT)
    );
    let space = key("Space").unwrap();
    assert_eq!(
        (
            space.key.as_str(),
            space.code.as_str(),
            space.text.as_deref()
        ),
        (" ", "Space", Some(" "))
    );
    assert_eq!(key("Esc").unwrap().key, "Escape");
    assert_eq!(key("Alt+ArrowDown").unwrap().modifiers, ALT);
    assert!(key("").is_err());
    assert!(key("ab").is_err());
}

fn request(url: &str, kind: &str) -> Request {
    Request {
        url: url.into(),
        kind: kind.into(),
        ..Request::default()
    }
}

#[test]
fn a_page_is_drawn_once_loaded_navigated_with_no_region_loading_and_only_streams_open() {
    let drawn = json!({"ready": "complete", "navigation": "ready", "loading": 0});
    let stream = request("http://h/api/feed", "EventSource");
    let done = Request {
        finished: true,
        ..request("http://h/api/task-board", "Fetch")
    };
    let failed = Request {
        failed: Some("net::ERR_ABORTED".into()),
        canceled: true,
        ..request("http://h/api/old", "Fetch")
    };
    assert!(waiting(&drawn, &[stream.clone(), done.clone(), failed.clone()]).is_empty());
    // A page that is not the Symphony's has no navigation marker of its own.
    assert!(
        waiting(
            &json!({"ready": "complete", "navigation": null, "loading": 0}),
            &[]
        )
        .is_empty()
    );
    let pending: Vec<Request> = (0..5)
        .map(|n| request(&format!("http://h/{n}"), "Fetch"))
        .collect();
    let said = waiting(
        &json!({"ready": "interactive", "navigation": "loading", "loading": 2}),
        &[vec![stream, done, failed], pending].concat(),
    );
    assert_eq!(
        said,
        [
            "the document to load (it is interactive)",
            "the page's navigation (html[data-navigation=loading])",
            "2 region(s) still loading ([data-ready=loading])",
            "5 request(s) in flight: http://h/0, http://h/1, http://h/2 and 2 more",
        ]
    );
}

#[test]
fn a_hidden_page_is_not_drawn_until_it_is_back_in_front() {
    let page = |visible: &str| json!({"ready": "complete", "navigation": "ready", "loading": 0, "visible": visible});
    assert_eq!(
        waiting(&page("hidden"), &[]),
        ["the page to be in front (it is hidden)"]
    );
    assert!(waiting(&page("visible"), &[]).is_empty());
}

fn wait(seconds: f64) -> Wait {
    Wait {
        until: Instant::now(),
        timeout: Duration::from_secs_f64(seconds),
    }
}

#[test]
fn a_wait_is_over_at_its_end_and_says_its_length() {
    let ended = wait(2.5);
    assert!(ended.over());
    assert_eq!(ended.said(), "2.5s");
    let running = Wait {
        until: Instant::now() + Duration::from_secs(60),
        ..wait(20.0)
    };
    assert!(!running.over());
    assert_eq!(running.said(), "20s");
}

#[test]
fn a_probe_reads_the_element_where_it_is_after_scrolling_it_or_in_the_next_frame() {
    let css = "summary[data-x=\"a\"]";
    let quoted = r#"document.querySelector("summary[data-x=\"a\"]")"#;
    let now = probe(css, Look::Now);
    assert!(now.starts_with("(() => {"), "{now}");
    assert!(
        now.contains(quoted),
        "the selector goes in as a JSON string: {now}"
    );
    assert!(!now.contains("scrollIntoView") && !now.contains("requestAnimationFrame"));
    let scrolled = probe(css, Look::Scrolled);
    let (scroll, read) = (
        scrolled.find("scrollIntoView").unwrap(),
        scrolled.find("getBoundingClientRect").unwrap(),
    );
    assert!(
        scroll < read,
        "the box is read after the scroll: {scrolled}"
    );
    assert!(!scrolled.contains("requestAnimationFrame"));
    let frame = probe(css, Look::NextFrame);
    assert_eq!(
        frame,
        format!("new Promise(resolve => requestAnimationFrame(() => resolve({now})))")
    );
}

#[test]
fn a_probe_tests_what_is_on_top_at_the_middle_of_the_box_it_read() {
    for look in [Look::Now, Look::Scrolled, Look::NextFrame] {
        let probe = probe("#a", look);
        let (read, hit, answer) = (
            probe.find("getBoundingClientRect").unwrap(),
            probe
                .find("document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)")
                .unwrap_or_else(|| panic!("no hit test at the box's middle: {probe}")),
            probe.find("return {x: box.x").unwrap(),
        );
        assert!(read < hit && hit < answer, "{look:?}: {probe}");
        // The element, or something inside it, is the target; anything else is named.
        assert!(probe.contains("element.contains(at) ? true"), "{probe}");
        assert!(
            probe.contains("view: [innerWidth, innerHeight], hit}"),
            "{probe}"
        );
    }
}

/// A box at `x`, `y`, `width` by `height` in a 1440x900 viewport, on top at its middle.
fn place(x: f64, y: f64, width: f64, height: f64) -> Place {
    Place {
        x,
        y,
        width,
        height,
        view: (1440.0, 900.0),
        hit: Hit::Target,
    }
}

/// [`place`], with `cover` on top at its middle.
fn covered(place: &Place, cover: &str) -> Place {
    Place {
        hit: Hit::Covered(cover.into()),
        ..place.clone()
    }
}

#[test]
fn a_probe_answers_a_place_nothing_yet_or_no_selector() {
    let found = json!({"x": 10.5, "y": 20.0, "width": 100.0, "height": 22.0, "view": [390, 844], "hit": true});
    assert_eq!(
        Place::read("#a", &found),
        Ok(Some(Place {
            view: (390.0, 844.0),
            ..place(10.5, 20.0, 100.0, 22.0)
        }))
    );
    // What is on top at its middle: another element by its name, or nothing at all.
    let mut on_top = found.clone();
    on_top["hit"] = json!("header#top.sticky");
    assert_eq!(
        Place::read("#a", &on_top).unwrap().unwrap().hit,
        Hit::Covered("header#top.sticky".into())
    );
    for nothing in [json!(null), json!(false)] {
        let mut missed = found.clone();
        missed["hit"] = nothing;
        assert_eq!(
            Place::read("#a", &missed).unwrap().unwrap().hit,
            Hit::Nothing
        );
    }
    let mut unsaid = found.clone();
    unsaid.as_object_mut().unwrap().remove("hit");
    assert_eq!(
        Place::read("#a", &unsaid).unwrap().unwrap().hit,
        Hit::Nothing
    );
    assert_eq!(Place::read("#a", &Value::Null), Ok(None));
    // An answer short of a number is no place.
    assert_eq!(
        Place::read("#a", &json!({"x": 1, "y": 2, "width": 3, "height": 4})),
        Ok(None)
    );
    assert_eq!(
        Place::read("a[", &json!({"invalid": "'a[' is not a valid selector."})),
        Err("a[: not a selector: 'a[' is not a valid selector.".to_string())
    );
}

#[test]
fn a_place_is_drawn_with_a_box_and_in_view_by_its_middle() {
    let summary = place(100.0, 449.0, 200.0, 22.0);
    assert!(summary.drawn() && summary.in_view());
    assert_eq!(summary.middle(), (200.0, 460.0));
    assert!(!place(100.0, 449.0, 0.0, 22.0).drawn());
    assert!(!place(100.0, 449.0, 200.0, 0.0).drawn());
    // Half of it out of view is still in reach; its middle past an edge is not.
    assert!(place(-90.0, -10.0, 200.0, 22.0).in_view());
    assert!(!place(-110.0, 449.0, 200.0, 22.0).in_view());
    assert!(!place(100.0, -12.0, 200.0, 22.0).in_view());
    assert!(!place(100.0, 889.0, 200.0, 22.0).in_view(), "middle at 900");
    assert!(
        !place(1340.0, 449.0, 200.0, 22.0).in_view(),
        "middle at 1440"
    );
    assert!(place(100.0, 888.0, 200.0, 22.0).in_view(), "middle at 899");
}

#[test]
fn the_aim_is_taken_once_two_reads_in_a_row_find_the_box_in_one_place() {
    let summary = place(100.0, 449.0, 200.0, 22.0);
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(summary.clone())), Next::Again);
    assert_eq!(aim.read(Some(summary)), Next::At((200.0, 460.0)));
}

#[test]
fn a_box_that_moved_is_aimed_at_where_it_then_held() {
    // The summary of the brief: read at 449, then pushed down to 544 by rows mounting above it.
    let (before, after) = (
        place(100.0, 449.0, 200.0, 22.0),
        place(100.0, 544.0, 200.0, 22.0),
    );
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(before.clone())), Next::Again);
    assert_eq!(aim.read(Some(after.clone())), Next::Again);
    assert_eq!(aim.latest, Latest::Moving);
    assert_eq!(aim.read(Some(after)), Next::At((200.0, 555.0)));
    // A box that grew or shrank where it stands moved too.
    let mut aim = Aim::default();
    aim.read(Some(before));
    assert_eq!(
        aim.read(Some(place(100.0, 449.0, 200.0, 38.0))),
        Next::Again
    );
}

#[test]
fn a_box_held_still_under_something_else_is_scrolled_again_not_aimed_at() {
    // The two cases of the brief: rows mounting in a scrolling container push the target past the
    // container's edge, where the page below it is on top; or push it up under a sticky header.
    // Either holds still in the viewport, so only the hit test tells it is not the one there.
    let target = place(100.0, 600.0, 80.0, 20.0);
    for cover in ["div#below", "header#header"] {
        let under = covered(&target, cover);
        let mut aim = Aim::default();
        for read in 0..3 {
            assert_eq!(
                aim.read(Some(under.clone())),
                Next::Scroll,
                "{cover} {read}"
            );
            assert_eq!(aim.latest, Latest::Covered(cover.into()));
        }
        // Scrolled again, clear of the cover: two reads in a row on top in one place.
        let clear = place(100.0, 440.0, 80.0, 20.0);
        assert_eq!(aim.read(Some(clear.clone())), Next::Again, "{cover}");
        assert_eq!(aim.read(Some(clear)), Next::At((140.0, 450.0)), "{cover}");
    }
}

#[test]
fn a_covered_read_between_two_agreeing_ones_starts_the_reads_over() {
    let target = place(100.0, 449.0, 200.0, 22.0);
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    // A toast passing over it for one frame: the read before it no longer counts.
    assert_eq!(aim.read(Some(covered(&target, "div.toast"))), Next::Scroll);
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    assert_eq!(aim.read(Some(target)), Next::At((200.0, 460.0)));
}

#[test]
fn a_middle_on_nothing_in_the_page_is_scrolled_again_and_out_of_view_wins_over_the_hit() {
    let target = place(1420.0, 449.0, 30.0, 22.0);
    let mut aim = Aim::default();
    // Its middle in the viewport but under the scrollbar: no element there.
    let missed = Place {
        hit: Hit::Nothing,
        ..target.clone()
    };
    assert_eq!(aim.read(Some(missed)), Next::Scroll);
    assert_eq!(aim.latest, Latest::Unreached);
    // Out of view, nothing is at its middle either: it is out of view that the failure says.
    let below = Place {
        hit: Hit::Nothing,
        ..place(100.0, 1200.0, 200.0, 22.0)
    };
    assert_eq!(aim.read(Some(below)), Next::Scroll);
    assert_eq!(aim.latest, Latest::OutOfView);
    // Nor does a box with none look for what covers it.
    let boxless = covered(&place(100.0, 449.0, 0.0, 0.0), "main");
    assert_eq!(aim.read(Some(boxless)), Next::Again);
    assert_eq!(aim.latest, Latest::Boxless);
}

#[test]
fn a_box_that_moves_on_every_read_is_never_aimed_at() {
    let mut aim = Aim::default();
    for frame in 0..100 {
        let next = aim.read(Some(place(100.0, 300.0 + f64::from(frame), 200.0, 40.0)));
        assert_eq!(next, Next::Again, "{frame}");
    }
    assert_eq!(aim.latest, Latest::Moving);
}

#[test]
fn a_box_gone_without_one_or_out_of_view_starts_the_reads_over() {
    let summary = place(100.0, 449.0, 200.0, 22.0);
    for (between, next, latest) in [
        (None, Next::Again, Latest::Gone),
        (
            Some(place(100.0, 449.0, 0.0, 0.0)),
            Next::Again,
            Latest::Boxless,
        ),
        (
            Some(place(100.0, 1200.0, 200.0, 22.0)),
            Next::Scroll,
            Latest::OutOfView,
        ),
    ] {
        let mut aim = Aim::default();
        assert_eq!(aim.read(Some(summary.clone())), Next::Again);
        assert_eq!(aim.read(between.clone()), next, "{between:?}");
        assert_eq!(aim.latest, latest);
        // The read before it is not one the box held still since.
        assert_eq!(aim.read(Some(summary.clone())), Next::Again, "{between:?}");
        assert_eq!(aim.read(Some(summary.clone())), Next::At((200.0, 460.0)));
    }
}

#[test]
fn an_aim_not_taken_says_what_its_latest_read_found() {
    let mut aim = Aim::default();
    assert_eq!(
        aim.late("#x", wait(5.0)),
        "#x was never read in an animation frame within 5s"
    );
    for (latest, said) in [
        (Latest::Gone, "#x left the page within 5s"),
        (
            Latest::Boxless,
            "#x is in the page but has no box within 5s",
        ),
        (
            Latest::OutOfView,
            "#x stayed out of view: its middle was outside the viewport after every scroll within 5s",
        ),
        (
            Latest::Covered("main > div".into()),
            "#x stayed covered: main > div was on top at its middle after every scroll within 5s",
        ),
        (
            Latest::Unreached,
            "#x stayed out of reach: nothing in the page was at its middle after every scroll \
             within 5s",
        ),
        (
            Latest::Moving,
            "#x kept moving: no two reads in a row, each in an animation frame, found its box in \
             one place within 5s",
        ),
    ] {
        aim.latest = latest;
        assert_eq!(aim.late("#x", wait(5.0)), said);
    }
}

#[test]
fn a_page_that_did_not_load_is_named_once_with_why_or_its_wait() {
    let url = "http://127.0.0.1:9/tasks";
    assert_eq!(
        not_loaded(
            url,
            wait(5.0),
            false,
            &format!("{url}: net::ERR_CONNECTION_REFUSED")
        ),
        "http://127.0.0.1:9/tasks did not load: net::ERR_CONNECTION_REFUSED"
    );
    // A reason that does not start with the URL is kept whole.
    assert_eq!(
        not_loaded(url, wait(5.0), false, "Page.navigate: the target closed"),
        "http://127.0.0.1:9/tasks did not load: Page.navigate: the target closed"
    );
    // Past the wait, the driver's own words (which call ran out) give way to the wait's length.
    assert_eq!(
        not_loaded(
            url,
            wait(5.0),
            true,
            "Page.navigate: no reply before the deadline"
        ),
        "http://127.0.0.1:9/tasks did not load within 5s"
    );
}

#[test]
fn a_page_not_drawn_says_what_it_still_waited_for_and_what_failed_meanwhile() {
    let still = [
        "the page's navigation (html[data-navigation=loading])".to_string(),
        "1 request(s) in flight: http://h/api/board".to_string(),
    ];
    assert_eq!(
        not_drawn("--key Escape", wait(10.0), &still, &[]),
        "--key Escape: the page was not drawn within 10s: still waiting for the page's navigation \
         (html[data-navigation=loading]); 1 request(s) in flight: http://h/api/board"
    );
    let failed = [
        json!({"url": "http://h/api/tasks", "status": 503}),
        json!({"url": "http://h/x.js", "error": "net::ERR_FAILED"}),
    ];
    assert_eq!(
        not_drawn(
            "http://h/tasks",
            wait(60.0),
            &["the page to paint: its fonts, then two frames".to_string()],
            &failed
        ),
        "http://h/tasks: the page was not drawn within 60s: still waiting for the page to paint: \
         its fonts, then two frames; meanwhile these failed: \
         {\"url\":\"http://h/api/tasks\",\"status\":503}, \
         {\"url\":\"http://h/x.js\",\"error\":\"net::ERR_FAILED\"}"
    );
}

#[test]
fn traffic_is_quiet_since_its_last_request_began_finished_or_failed_but_not_before_the_wait() {
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let event = |method: &str| json!({"method": method, "params": {"requestId": "1"}});
    let (sent, data, finished, failed, page) = (
        event("Network.requestWillBeSent"),
        event("Network.dataReceived"),
        event("Network.loadingFinished"),
        event("Network.loadingFailed"),
        event("Page.loadEventFired"),
    );
    // Nothing yet, or only what came before the wait: the wait's own start.
    assert_eq!(quiet_since(std::iter::empty(), at(40)), at(40));
    assert_eq!(
        quiet_since([(at(10), &sent), (at(20), &finished)].into_iter(), at(40)),
        at(40)
    );
    // The last request to begin, finish or fail, however the events are ordered.
    for (events, last) in [
        (vec![(at(50), &sent)], 50),
        (vec![(at(50), &sent), (at(90), &finished)], 90),
        (vec![(at(70), &failed), (at(60), &sent)], 70),
    ] {
        assert_eq!(quiet_since(events.into_iter(), at(40)), at(last));
    }
    // A stream's data and the page's own events are no traffic.
    assert_eq!(
        quiet_since(
            [(at(50), &sent), (at(80), &data), (at(95), &page)].into_iter(),
            at(40)
        ),
        at(50)
    );
}

#[test]
fn the_report_keeps_errors_failures_and_overflow_and_leaves_out_the_rest() {
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let events = [
        event(
            "Runtime.consoleAPICalled",
            json!({"type": "error", "args": [{"type": "string", "value": "boom"}, {"type": "number", "value": 3}, {"type": "object", "description": "Error: deep"}]}),
        ),
        event(
            "Runtime.consoleAPICalled",
            json!({"type": "log", "args": [{"type": "string", "value": "fine"}]}),
        ),
        event(
            "Runtime.exceptionThrown",
            json!({"exceptionDetails": {"text": "Uncaught", "exception": {"description": "TypeError: x is undefined"}}}),
        ),
        event(
            "Runtime.exceptionThrown",
            json!({"exceptionDetails": {"text": "Uncaught (in promise)"}}),
        ),
        event(
            "Log.entryAdded",
            json!({"entry": {"level": "error", "source": "network", "text": "Failed to load resource", "url": "http://h/missing"}}),
        ),
        event(
            "Log.entryAdded",
            json!({"entry": {"level": "warning", "source": "other", "text": "meh"}}),
        ),
        event("Network.requestWillBeSent", json!({"requestId": "1"})),
    ];
    let requests = [
        Request {
            status: 404,
            finished: true,
            ..request("http://h/missing", "Fetch")
        },
        Request {
            status: 200,
            finished: true,
            ..request("http://h/fine", "Fetch")
        },
        Request {
            failed: Some("net::ERR_CONNECTION_REFUSED".into()),
            ..request("http://h/refused", "Fetch")
        },
        Request {
            failed: Some("net::ERR_ABORTED".into()),
            canceled: true,
            ..request("http://h/given-up", "Fetch")
        },
    ];
    let said = report(
        &events,
        &requests,
        &json!({"scroll_width": 1500, "viewport_width": 1440}),
    );
    assert_eq!(
        said,
        json!({
            "console_errors": [
                {"source": "console", "text": "boom 3 Error: deep"},
                {"source": "exception", "text": "TypeError: x is undefined"},
                {"source": "exception", "text": "Uncaught (in promise)"},
                {"source": "network", "text": "Failed to load resource", "url": "http://h/missing"},
            ],
            "failed_requests": [
                {"url": "http://h/missing", "status": 404},
                {"url": "http://h/refused", "error": "net::ERR_CONNECTION_REFUSED"},
            ],
            "scroll_width": 1500,
            "viewport_width": 1440,
            "horizontal_overflow": true,
        })
    );
    let fits = report(
        &[],
        &[],
        &json!({"scroll_width": 390, "viewport_width": 390}),
    );
    assert_eq!(fits["horizontal_overflow"], json!(false));
    assert_eq!(fits["console_errors"], json!([]));
}

#[test]
fn a_machine_without_the_named_chrome_fails_before_any_browser_starts() {
    // SF_CHROME is read by this process alone; the variable is set only in a child.
    if std::env::var_os("SF_SHOT_MISSING_CHROME").is_some() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(
            &args(&["http://127.0.0.1:1/", "--out", "never.png"]),
            &mut out,
            &mut err,
        );
        let err = String::from_utf8(err).unwrap();
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("/no/such/chrome is not a file"), "{err}");
        assert!(!Path::new("never.png").exists());
        return;
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cli::commands::shot::tests::a_machine_without_the_named_chrome_fails_before_any_browser_starts",
            "--nocapture",
        ])
        .env("SF_SHOT_MISSING_CHROME", "1")
        .env("SF_CHROME", "/no/such/chrome")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}
