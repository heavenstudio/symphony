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
fn the_help_says_what_a_click_accepts_and_what_it_refuses() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&args(&["-h"]), &mut out, &mut err), 0);
    // The help's lines are wrapped: read its words.
    let words = String::from_utf8(out)
        .unwrap()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for said in [
        // The --click line says what the aim does: the first line box in view that takes the press.
        "click the middle of its first line box in view that takes the press (most elements have \
         one box)",
        "its own label for a control, on top there",
        "the first whose middle is in view and takes the press, else the first in view that no box \
         around it clips there",
        "a link wrapped over two lines is aimed at its first line, and one taller than its \
         scrolling box at a line the box shows",
        "the label of the control it is (a checkbox hidden in its label or under the label's styled track)",
        "but on a link, an object or another control inside it (in a component's open shadow root \
         too, and on what is slotted into one there)",
        // What the page cannot see, it never takes as the label's; the browser's own hit confirms
        // the rest, and what it misses it says.
        "or on a custom element without an open shadow root, whose inside the page cannot see, as \
         the label's activation clicks its control",
        "The browser's own hit confirms a press through a label, so a control in a closed root on \
         a built-in element, such as a span, makes it fail as covered (one around what is slotted \
         into such a root goes unseen)",
        "stays covered (by its own ancestor too, painted over it), fails the step (naming what \
         covers it)",
        // A clip named only where the browser or the shape backs it, and no cause made up.
        "stays clipped, the browser finding it nowhere under its middle (naming the box whose \
         overflow or clip cuts it away there, or whose clip-path, an inset, circle, ellipse or \
         polygon on a box nothing transforms, leaves its middle out), stays hidden (its \
         visibility)",
        "takes no pointer at its middle, its own ancestor on top there (pointer-events: none, its \
         own clip or clip-path leaving its middle out, or a cause it does not name, as an SVG \
         ring's unpainted middle)",
        // A target offered only where a press on it is what the element lets through.
        "so the failure names it. For pointer-events: none, which lets a press through to the \
         ancestor, it adds a selector that finds the ancestor alone to target instead (or says \
         none does), unless the ancestor is the page's body or a press there clicks another \
         control through a label",
    ] {
        assert!(words.contains(said), "{said:?} not in the help: {words}");
    }
    // Nor does it say what the aim no longer does, or promise every shadow root.
    assert!(!words.contains("a wrapped inline's first line)"), "{words}");
    assert!(
        !words.contains("(in a component's shadow root too)"),
        "{words}"
    );
}

#[test]
fn every_line_of_the_help_fits_a_hundred_columns() {
    // The usage before it is one line, as a synopsis is; the help's own lines are wrapped.
    for line in HELP.lines() {
        assert!(
            line.chars().count() <= 100,
            "{} columns: {line:?}",
            line.chars().count()
        );
    }
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

/// Where each of `parts` is in `script`, in their order there, or a panic naming the one missing.
fn positions(script: &str, parts: &[&str]) -> Vec<usize> {
    parts
        .iter()
        .map(|part| {
            script
                .find(part)
                .unwrap_or_else(|| panic!("no {part:?} in {script}"))
        })
        .collect()
}

fn in_order(script: &str, parts: &[&str]) {
    let found = positions(script, parts);
    assert!(
        found.windows(2).all(|pair| pair[0] < pair[1]),
        "{parts:?} out of order at {found:?}: {script}"
    );
}

#[test]
fn a_probe_tests_what_is_on_top_at_the_middle_of_the_box_it_read() {
    for look in [Look::Now, Look::Scrolled, Look::NextFrame] {
        let probe = probe("#a", look);
        // The box is chosen, then its middle reached, then the answer given.
        in_order(
            &probe,
            &[
                "element.getClientRects()",
                "element.getBoundingClientRect();\nconst [x, y] = middle(box);",
                "const p = reach(x, y);",
                "return {x: box.x",
            ],
        );
        assert!(
            probe.contains("view: [innerWidth, innerHeight], scroll: [scrollX, scrollY], hit}"),
            "{probe}"
        );
    }
}

#[test]
fn a_probe_aims_at_the_first_line_box_in_view_that_takes_the_press_then_one_nothing_clips() {
    let probe = probe("#a", Look::Now);
    // Only line boxes with an area count: a wrapped inline's empty fragment at a line's end does not.
    assert!(
        probe.contains("[...element.getClientRects()].filter(r => r.width > 0 && r.height > 0)"),
        "{probe}"
    );
    // In view as the aim reads it: the middle within the viewport, its far edges excluded.
    assert!(
        probe.contains(
            "const seen = ([x, y]) => x >= 0 && x < innerWidth && y >= 0 && y < innerHeight;"
        ),
        "{probe}"
    );
    // The first in view that takes the press, else the first no box around it clips there, else
    // the first in view, its first, its whole box.
    assert!(
        probe.contains(
            "const box = viewed.find(r => takes(reach(...middle(r)))) ?? viewed.find(r => !clips.some(c => away(c, ...middle(r)))) ?? viewed[0] ?? rects[0] ?? element.getBoundingClientRect();"
        ),
        "{probe}"
    );
}

#[test]
fn a_probe_reads_what_clips_the_element_through_its_containing_blocks() {
    let probe = probe("#a", Look::Now);
    // Its own clip first, then each box around it in the flat tree, short of the body.
    in_order(
        &probe,
        &[
            "const self = cuts(element, style);",
            "for (let a = flat(element); a && a !== document.body && a !== document.documentElement; a = flat(a))",
        ],
    );
    // An absolute one escapes static boxes that make no containing block, a fixed one every box
    // but those a transform, a filter or containment makes one.
    assert!(
        probe.contains(
            "if ((position === 'fixed' && !confines(t)) || (position === 'absolute' && t.position === 'static' && !confines(t))) continue;"
        ),
        "{probe}"
    );
    // A box clips by its own clip, by an overflow other than visible on an axis or by paint
    // containment, to its padding edge; a display: contents one, an inline one or an SVG element
    // inside an svg makes no box to clip.
    for part in [
        "const cut = cuts(a, t);",
        "const x = paint || t.overflowX !== 'visible', y = paint || t.overflowY !== 'visible';",
        "if ((x || y) && t.display !== 'inline')",
        "[left, top, left + a.clientWidth, top + a.clientHeight]",
        "if (t.display === 'contents' || (a instanceof SVGElement && a.ownerSVGElement)) continue;",
        "const away = (c, x, y) => (c.x && (x < c.edges[0] || x >= c.edges[2])) || (c.y && (y < c.edges[1] || y >= c.edges[3]));",
    ] {
        assert!(probe.contains(part), "no {part:?} in {probe}");
    }
}

#[test]
fn a_probe_reaches_through_shadow_roots_and_takes_the_targets_own_label_as_its_activation_does() {
    let probe = probe("#a", Look::Now);
    // Through every open shadow root to the element really there, then up the flat tree, as the
    // press travels, to the first interactive element, which, when a label, clicks its control; a
    // custom element whose inside the page cannot see ends the walk with no label.
    in_order(
        &probe,
        &[
            "const inner = deep.shadowRoot.elementFromPoint(x, y);",
            "while (first && !first.matches(INTERACTIVE) && !unseen(first)) first = flat(first);",
            "via: first && first.localName === 'label' ? first.control : null",
        ],
    );
    for part in [
        "const up = e => e.parentElement || (e.parentNode instanceof ShadowRoot ? e.parentNode.host : null);",
        // A slotted node's slot comes before its parent, as in the flat tree.
        "const flat = e => e.assignedSlot || up(e);",
        "const unseen = e => e instanceof HTMLElement && e.localName.includes('-') && !e.shadowRoot;",
    ] {
        assert!(probe.contains(part), "no {part:?} in {probe}");
    }
    assert!(
        probe.contains(
            "const takes = p => !!p.at && (element.contains(p.at) || p.via === element);"
        ),
        "{probe}"
    );
    // HTML's interactive content, and an object, go in as a JSON string.
    assert!(
        probe.contains(&format!("const INTERACTIVE = {};", json!(INTERACTIVE))),
        "{probe}"
    );
    for interactive in [
        "a[href]", "button", "input", "label", "select", "textarea", "iframe", "embed", "object",
    ] {
        assert!(
            INTERACTIVE.split(", ").any(|one| one == interactive),
            "{interactive}"
        );
    }
}

#[test]
fn a_probe_says_why_the_element_misses_the_press_the_first_reason_that_holds() {
    let probe = probe("#a", Look::Now);
    in_order(
        &probe,
        &[
            "const hit = !p.at ? null",
            // Taken: inside it, or through its label, for the browser's own hit to confirm.
            ": takes(p) ? element.contains(p.at) || {label: true}",
            ": style.visibility !== 'visible' ? {hidden: style.visibility}",
            ": clipper ? {clipped: named(clipper.by)}",
            // A cover in a shadow root is named in its host's; an ancestor painted over it covers it.
            ": !held || listed ? inside(p.deep)",
            ": shaped && shaped !== element ? {clipped: inside(shaped)}",
            ": {holder: named(p.at), why: shaped || clips.some(c => !c.by && away(c, x, y)) ? 'clip' : style.pointerEvents === 'none' ? 'none' : 'unnamed', target: page ? null : alone(p.at), page, labels: p.via && (alone(p.via) ?? inside(p.via))};",
        ],
    );
    // What is named in a shadow root is named in its outermost host's.
    assert!(
        probe.contains(
            r#"const inside = e => { let host = e; while (host.getRootNode() instanceof ShadowRoot) host = host.getRootNode().host; return host === e ? named(e) : named(e) + ' in ' + named(host) + "'s shadow root"; };"#
        ),
        "{probe}"
    );
    // The browser lists what a press at the point could reach: the element among it, nothing clips
    // it away there.
    assert!(
        probe.contains(
            "const listed = document.elementsFromPoint(x, y).some(e => element.contains(e));"
        ),
        "{probe}"
    );
    // A clip-path is named only where its shape leaves the point out, and never for an element
    // that takes no pointer, which says why already.
    assert!(
        probe.contains(
            "for (let e = element; held && !listed && style.pointerEvents !== 'none' && e && e !== p.deep && !shaped; e = flat(e)) if (excludes(e, x, y)) shaped = e;"
        ),
        "{probe}"
    );
    // The page's body and root are no target.
    assert!(
        probe.contains("const page = p.at === document.body || p.at === document.documentElement;"),
        "{probe}"
    );
    // What is there holds the element when it is the element's ancestor in the flat tree.
    assert!(
        probe.contains("for (let e = element; e && !held; e = flat(e)) held = e === p.deep;"),
        "{probe}"
    );
    // A box around it clips it there, when the browser lists it nowhere there; its own clip makes a
    // holder's reason, not a clipping box.
    assert!(
        probe.contains("const clipper = listed ? null : clips.find(c => c.by && away(c, x, y));"),
        "{probe}"
    );
}

#[test]
fn a_probe_claims_a_clip_path_only_for_a_basic_shape_on_a_box_nothing_around_it_transforms() {
    let probe = probe("#a", Look::Now);
    for part in [
        // An HTML element's box, not inline, that neither it nor any box around it up the flat tree
        // moves: a transform, any of the individual transforms, a motion path or a zoom, so a box
        // mirrored or turned with its ancestor, its size kept, is never tested at the mirrored point.
        "const moved = t => t.transform !== 'none' || ['scale', 'rotate', 'translate', 'offsetPath'].some(p => (t[p] || 'none') !== 'none') || (t.zoom || '1') !== '1';",
        "const level = e => { for (let a = e; a; a = flat(a)) if (moved(getComputedStyle(a))) return false; return true; };",
        // Its layout size its drawn size too, and an inset, circle, ellipse or polygon on its
        // border box; anything else is never claimed.
        "const t = e instanceof HTMLElement ? getComputedStyle(e) : null;",
        "t.display !== 'inline' && level(e) ? /^(inset|circle|ellipse|polygon)\\((.*)\\)( border-box)?$/.exec(t.clipPath) : null;",
        "if (!m || Math.abs(b.width - e.offsetWidth) > 1 || Math.abs(b.height - e.offsetHeight) > 1) return false;",
        // Lengths in pixels or percentages only: a calc() or a keyword claims nothing.
        "const size = (v, basis) => /^-?\\d*\\.?\\d+px$/.test(v) ? parseFloat(v) : /^-?\\d*\\.?\\d+%$/.test(v) ? parseFloat(v) / 100 * basis : NaN;",
        // An inset's rectangle, its rounded corners left out (they only cut more), empty when its
        // sides cross.
        "return r <= l || b0 <= t0 || px < l || px >= r || py < t0 || py >= b0;",
        // A circle's radius as a percentage of the box's diagonal over the square root of two.
        "return Math.hypot(px - cx, py - cy) > radius(words[0], Math.hypot(w, h) / Math.SQRT2, [cx, w - cx, cy, h - cy]);",
        "((px - cx) / radius(words[0], w, [cx, w - cx])) ** 2 + ((py - cy) / radius(words[1], h, [cy, h - cy])) ** 2 > 1;",
        // A polygon by its fill rule.
        "return rule === 'evenodd' ? crossings % 2 === 0 : winding === 0;",
    ] {
        assert!(probe.contains(part), "no {part:?} in {probe}");
    }
    // Not the box's own transform alone, which a mirrored or turned ancestor slips past.
    assert!(
        !probe.contains("t.transform === 'none' && (t.scale"),
        "{probe}"
    );
}

#[test]
fn a_press_through_a_label_is_confirmed_by_the_node_the_browser_finds_there() {
    // From the node itself, inside a closed shadow root too, up the flat tree: the element, or its
    // own label, before anything else interactive, else the node there by its name.
    in_order(
        CONFIRM,
        &[
            "const element = document.querySelector(css);",
            "for (let e = this; e; e = flat(e)) {",
            "if (e === element) return true;",
            "if (e.nodeType === 1 && e.matches(interactive)) return e.localName === 'label' && e.control === element ? true : inside(found);",
        ],
    );
    // A text node there is named by the element it lies in.
    assert!(
        CONFIRM.contains("const found = this.nodeType === 1 ? this : flat(this);"),
        "{CONFIRM}"
    );
    // The names and the walk are the probe's own.
    for part in ["const flat = ", "const inside = "] {
        assert!(NAMES.contains(part), "{part}");
        assert!(!CONFIRM.contains(part), "{part}");
        assert!(!AIM.contains(part), "{part}");
        assert!(probe("#a", Look::Now).contains(part), "{part}");
    }
    // Its answer: taken, or covered by what the browser found there.
    assert_eq!(Hit::confirmed(&json!(true)), Hit::Target);
    assert_eq!(
        Hit::confirmed(&json!("button in span#host's shadow root")),
        Hit::Missed(Miss::Covered("button in span#host's shadow root".into()))
    );
    for unsaid in [json!(null), json!(false), json!(3)] {
        assert_eq!(
            Hit::confirmed(&unsaid),
            Hit::Missed(Miss::Covered("what the browser found there".into()))
        );
    }
    // The protocol takes a point by the whole CSS pixel it lies in, in the document's coordinates:
    // the viewport's middle moved by the page's scroll, read in the same frame.
    assert_eq!(
        [pixel(12.5), pixel(8.0), pixel(0.99), pixel(-0.5)],
        [12, 8, 0, -1]
    );
    let scrolled = Place {
        scroll: (30.0, 1200.0),
        ..place(10.0, 20.0, 100.0, 40.0)
    };
    assert_eq!(scrolled.middle(), (60.0, 40.0));
    assert_eq!(scrolled.in_document(), (90.0, 1240.0));
    assert_eq!(place(10.0, 20.0, 100.0, 40.0).in_document(), (60.0, 40.0));
}

#[test]
fn a_probe_names_an_element_by_a_selector_that_finds_it_alone() {
    let probe = probe("#a", Look::Now);
    // Its name when that finds it alone, else a path of names up from it, each step pinned by
    // :nth-of-type among its kin, until the path finds it alone; none in a shadow root. Its own
    // pin is never offered without its parent's name before it.
    in_order(
        &probe,
        &[
            "if (e.getRootNode() !== document) return null;",
            "if (finds(name(e), e)) return name(e);",
            "css = name(n) + (kin.length > 1 ? ':nth-of-type(' + (kin.indexOf(n) + 1) + ')' : '') + (css && ' > ' + css);",
            "if ((n !== e || kin.length === 1) && finds(css, e)) return css;",
        ],
    );
    assert!(
        probe.contains("return all.length === 1 && all[0] === e;"),
        "{probe}"
    );
}

/// A box at `x`, `y`, `width` by `height` in a 1440x900 viewport, on top at its middle.
fn place(x: f64, y: f64, width: f64, height: f64) -> Place {
    Place {
        x,
        y,
        width,
        height,
        view: (1440.0, 900.0),
        scroll: (0.0, 0.0),
        hit: Hit::Target,
    }
}

/// [`place`], missing the press at its middle for `miss`.
fn missing(place: &Place, miss: Miss) -> Place {
    Place {
        hit: Hit::Missed(miss),
        ..place.clone()
    }
}

/// [`place`], with `cover` on top at its middle.
fn covered(place: &Place, cover: &str) -> Place {
    missing(place, Miss::Covered(cover.into()))
}

/// A holder by `name`, missing the press for `why`, found alone by `name` itself.
fn holder(name: &str, why: Why) -> Holder {
    Holder {
        name: name.into(),
        why,
        instead: Instead::Alone(name.into()),
        labels: None,
    }
}

#[test]
fn a_probe_answers_a_place_nothing_yet_or_no_selector() {
    let found = json!({"x": 10.5, "y": 20.0, "width": 100.0, "height": 22.0, "view": [390, 844], "scroll": [0, 640.5], "hit": true});
    assert_eq!(
        Place::read("#a", &found),
        Ok(Some(Place {
            view: (390.0, 844.0),
            scroll: (0.0, 640.5),
            ..place(10.5, 20.0, 100.0, 22.0)
        }))
    );
    let hit = |said: Value| {
        let mut read = found.clone();
        read["hit"] = said;
        Place::read("#a", &read).unwrap().unwrap().hit
    };
    // Taken through its label, for the browser's own hit to confirm; anything else under `label`
    // is not.
    assert_eq!(hit(json!({"label": true})), Hit::Label);
    assert_eq!(hit(json!({"label": "yes"})), Hit::Missed(Miss::Nothing));
    // Why it misses the press: another element by its name, its visibility, the box that clips it.
    assert_eq!(
        hit(json!("header#top.sticky")),
        Hit::Missed(Miss::Covered("header#top.sticky".into()))
    );
    assert_eq!(
        hit(json!({"hidden": "hidden"})),
        Hit::Missed(Miss::Hidden("hidden".into()))
    );
    assert_eq!(
        hit(json!({"clipped": "div#panel"})),
        Hit::Missed(Miss::Clipped("div#panel".into()))
    );
    // Its own ancestor, by its name, why, a selector that finds it alone and the control a press
    // there clicks through a label, the last two when there are such.
    for (why, read) in [
        ("none", Why::PointerEventsNone),
        ("clip", Why::OwnClip),
        ("unnamed", Why::Unnamed),
        ("anything else", Why::Unnamed),
    ] {
        assert_eq!(
            hit(
                json!({"holder": "nav.tools > button", "why": why, "target": "nav.tools > button:nth-of-type(2)", "page": false, "labels": null})
            ),
            Hit::Missed(Miss::Holder(Holder {
                name: "nav.tools > button".into(),
                why: read,
                instead: Instead::Alone("nav.tools > button:nth-of-type(2)".into()),
                labels: None,
            }))
        );
    }
    assert_eq!(
        hit(
            json!({"holder": "label#l", "why": "none", "target": null, "page": false, "labels": "input#other"})
        ),
        Hit::Missed(Miss::Holder(Holder {
            name: "label#l".into(),
            why: Why::PointerEventsNone,
            instead: Instead::Unfound,
            labels: Some("input#other".into()),
        }))
    );
    // The page's body is no target, whatever selector finds it.
    for target in [json!(null), json!("body")] {
        assert_eq!(
            hit(
                json!({"holder": "html > body", "why": "none", "target": target, "page": true, "labels": null})
            ),
            Hit::Missed(Miss::Holder(Holder {
                name: "html > body".into(),
                why: Why::PointerEventsNone,
                instead: Instead::Page,
                labels: None,
            }))
        );
    }
    // Anything else is nothing at its middle, an ancestor without its name too.
    for nothing in [json!(null), json!(false), json!({"why": "none"}), json!(3)] {
        assert_eq!(hit(nothing), Hit::Missed(Miss::Nothing));
    }
    let mut unsaid = found.clone();
    unsaid.as_object_mut().unwrap().remove("hit");
    assert_eq!(
        Place::read("#a", &unsaid).unwrap().unwrap().hit,
        Hit::Missed(Miss::Nothing)
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
            assert_eq!(aim.latest, Latest::Missed(Miss::Covered(cover.into())));
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

/// [`place`], with its own ancestor `name` on top at its middle, missing the press for `why`.
fn held(place: &Place, name: &str, why: Why) -> Place {
    missing(place, Miss::Holder(holder(name, why)))
}

#[test]
fn a_target_whose_ancestor_takes_the_pointer_at_its_middle_is_scrolled_again_never_aimed_at() {
    // An icon with pointer-events: none in its button, an SVG ring around an empty middle, or a
    // checkbox its own clip hides in its label: held still in view, but a press at its middle would
    // reach what holds it.
    let icon = place(100.0, 449.0, 24.0, 24.0);
    for (name, why) in [
        ("button#go", Why::PointerEventsNone),
        ("svg#rings", Why::Unnamed),
        ("label#toggle", Why::OwnClip),
    ] {
        let mut aim = Aim::default();
        for read in 0..3 {
            assert_eq!(
                aim.read(Some(held(&icon, name, why))),
                Next::Scroll,
                "{name} {read}"
            );
            assert_eq!(aim.latest, Latest::Missed(Miss::Holder(holder(name, why))));
        }
        // Scrolled again, it is on top and aimed at.
        assert_eq!(aim.read(Some(icon.clone())), Next::Again, "{name}");
        assert_eq!(aim.read(Some(icon.clone())), Next::At((112.0, 461.0)));
    }
    // Between two agreeing reads, an ancestor's read starts them over, as a cover's does.
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(icon.clone())), Next::Again);
    assert_eq!(
        aim.read(Some(held(&icon, "button#go", Why::PointerEventsNone))),
        Next::Scroll
    );
    assert_eq!(aim.read(Some(icon.clone())), Next::Again);
    // And out of view wins over it.
    let below = held(
        &place(100.0, 1200.0, 24.0, 24.0),
        "button#go",
        Why::PointerEventsNone,
    );
    assert_eq!(aim.read(Some(below)), Next::Scroll);
    assert_eq!(aim.latest, Latest::OutOfView);
}

#[test]
fn a_target_a_box_around_it_clips_away_is_scrolled_again_and_one_its_visibility_hides_is_not() {
    let target = place(100.0, 449.0, 80.0, 20.0);
    // Clipped out of its scrolling box's view: a scroll of the box may bring it back.
    let clipped = missing(&target, Miss::Clipped("div#list".into()));
    let mut aim = Aim::default();
    for read in 0..3 {
        assert_eq!(aim.read(Some(clipped.clone())), Next::Scroll, "{read}");
        assert_eq!(aim.latest, Latest::Missed(Miss::Clipped("div#list".into())));
    }
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    assert_eq!(aim.read(Some(target.clone())), Next::At((140.0, 459.0)));
    // Hidden by its visibility: no scroll shows it, so it is read again until the page shows it.
    let hidden = missing(&target, Miss::Hidden("hidden".into()));
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    for read in 0..3 {
        assert_eq!(aim.read(Some(hidden.clone())), Next::Again, "{read}");
        assert_eq!(aim.latest, Latest::Missed(Miss::Hidden("hidden".into())));
    }
    // The read before it does not count: two shown reads in a row are.
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    assert_eq!(aim.read(Some(target)), Next::At((140.0, 459.0)));
}

#[test]
fn a_middle_on_nothing_in_the_page_is_scrolled_again_and_out_of_view_wins_over_the_hit() {
    let target = place(1420.0, 449.0, 30.0, 22.0);
    let mut aim = Aim::default();
    // Its middle in the viewport but under the scrollbar: no element there.
    let missed = missing(&target, Miss::Nothing);
    assert_eq!(aim.read(Some(missed)), Next::Scroll);
    assert_eq!(aim.latest, Latest::Missed(Miss::Nothing));
    // Out of view, nothing is at its middle either: it is out of view that the failure says.
    let below = missing(&place(100.0, 1200.0, 200.0, 22.0), Miss::Nothing);
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
            Latest::Missed(Miss::Covered("main > div".into())),
            "#x stayed covered: main > div was on top at its middle after every scroll within 5s",
        ),
        // A real cause, never "takes no pointer", and no hint to target anything else.
        (
            Latest::Missed(Miss::Hidden("hidden".into())),
            "#x stayed hidden within 5s: its visibility is hidden, so no press reaches it",
        ),
        (
            Latest::Missed(Miss::Clipped("div#panel".into())),
            "#x stayed clipped: div#panel clips it away at its middle after every scroll within 5s",
        ),
        // Never "covered" for what holds it: why it takes no pointer, and what to target instead.
        (
            Latest::Missed(Miss::Holder(holder(
                "button#go.icon",
                Why::PointerEventsNone,
            ))),
            "#x takes no pointer at its middle (pointer-events: none): button#go.icon, which holds \
             it, was on top there after every scroll within 5s; target button#go.icon instead",
        ),
        // No cause named: what holds it is named, but a press on it does nothing for the target, so
        // it is not offered, whether a selector finds it alone, none does or it is the page.
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Alone("section > svg:nth-of-type(2)".into()),
                ..holder("section > svg", Why::Unnamed)
            })),
            "#x takes no pointer at its middle: section > svg, which holds it, was on top there \
             after every scroll within 5s",
        ),
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Unfound,
                ..holder("x-card > div", Why::Unnamed)
            })),
            "#x takes no pointer at its middle: x-card > div, which holds it, was on top there \
             after every scroll within 5s",
        ),
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Page,
                ..holder("html > body", Why::Unnamed)
            })),
            "#x takes no pointer at its middle: html > body, which holds it, was on top there \
             after every scroll within 5s",
        ),
        // No selector finds it alone: said plainly.
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Unfound,
                ..holder("x-card > div", Why::PointerEventsNone)
            })),
            "#x takes no pointer at its middle (pointer-events: none): x-card > div, which holds \
             it, was on top there after every scroll within 5s; no selector finds x-card > div \
             alone, to target instead",
        ),
        // The page's body holds it: no target, said as such.
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Page,
                ..holder("html > body", Why::PointerEventsNone)
            })),
            "#x takes no pointer at its middle (pointer-events: none): html > body, which holds \
             it, was on top there after every scroll within 5s; html > body is the page itself, \
             nothing to target instead",
        ),
        // A press on a label for another control clicks that control: never offered instead, the
        // control named in its component's shadow root when it lies in one.
        (
            Latest::Missed(Miss::Holder(Holder {
                labels: Some("input#other".into()),
                ..holder("label#l", Why::PointerEventsNone)
            })),
            "#x takes no pointer at its middle (pointer-events: none): label#l, which holds it, \
             was on top there after every scroll within 5s; a press there clicks input#other (by \
             its label), not it",
        ),
        (
            Latest::Missed(Miss::Holder(Holder {
                labels: Some("input#check in x-check#host's shadow root".into()),
                instead: Instead::Unfound,
                ..holder("x-check#host", Why::PointerEventsNone)
            })),
            "#x takes no pointer at its middle (pointer-events: none): x-check#host, which holds \
             it, was on top there after every scroll within 5s; a press there clicks input#check \
             in x-check#host's shadow root (by its label), not it",
        ),
        // Its own clip hides it, the page's body holding it: nothing offered either.
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Page,
                ..holder("html > body", Why::OwnClip)
            })),
            "#x stayed clipped by its own clip: html > body, which holds it, was on top at its \
             middle after every scroll within 5s",
        ),
        // Its own clip hides it: what holds it is no substitute, whether a selector finds it or not.
        (
            Latest::Missed(Miss::Holder(holder("label#toggle", Why::OwnClip))),
            "#x stayed clipped by its own clip: label#toggle, which holds it, was on top at its \
             middle after every scroll within 5s",
        ),
        (
            Latest::Missed(Miss::Holder(Holder {
                labels: Some("input#first".into()),
                ..holder("label#toggle", Why::OwnClip)
            })),
            "#x stayed clipped by its own clip: label#toggle, which holds it, was on top at its \
             middle after every scroll within 5s; a press there clicks input#first (by its label), \
             not it",
        ),
        (
            Latest::Missed(Miss::Nothing),
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
    // SYMPHONY_CHROME is read by this process alone; the variable is set only in a child.
    if std::env::var_os("SYMPHONY_SHOT_MISSING_CHROME").is_some() {
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
        .env("SYMPHONY_SHOT_MISSING_CHROME", "1")
        .env(symphony_env::CHROME.name, "/no/such/chrome")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&child.stderr);
    assert!(
        child.status.success(),
        "{}{stderr}",
        String::from_utf8_lossy(&child.stdout)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}
