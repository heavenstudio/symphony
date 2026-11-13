//! The clip-path maths, against what Chrome computes and hit-tests (each expectation below that
//! names a box was read from Chrome's own `elementFromPoint` over that box).

use serde_json::json;

use super::*;

/// A box of 130 by 90 at (100, 200), a 5 px border, 10 px padding and 20 px margins, as Chrome
/// drew `width: 100px; height: 60px; border: 5px solid; padding: 10px; margin: 20px`, nothing
/// moving it, with `clip`.
fn shaped(clip: &str) -> Shaped {
    Shaped {
        name: "div#d".into(),
        own: false,
        clip: clip.into(),
        border_box: Rect {
            x: 100.0,
            y: 200.0,
            width: 130.0,
            height: 90.0,
        },
        border: [5.0; 4],
        padding: [10.0; 4],
        margin: [20.0; 4],
        scrolls: false,
        moves: vec![still(), still()],
    }
}

/// A box that does not move.
fn still() -> Motion {
    moving("none")
}

/// A box moved by `transform` alone.
fn moving(transform: &str) -> Motion {
    Motion {
        transform: transform.into(),
        translate: "none".into(),
        scale: "none".into(),
        rotate: "none".into(),
        offset_path: "none".into(),
        zoom: "1".into(),
    }
}

/// The first and the last point `clip` keeps across [`shaped`]'s middle row, inside its border box,
/// from its left edge: a quarter and three quarters into each pixel, so no point lies on an edge,
/// where Chrome's own hit test is no finer than a pixel.
fn kept_across(clip: &str) -> Option<(f64, f64)> {
    let shaped = shaped(clip);
    let kept: Vec<f64> = (0..130)
        .flat_map(|pixel| [f64::from(pixel) + 0.25, f64::from(pixel) + 0.75])
        .filter(|x| !shaped.excludes((100.0 + x, 245.0)))
        .collect();
    Some((*kept.first()?, *kept.last()?))
}

#[test]
fn a_length_reads_pixels_percentages_and_their_calc_min_max_and_clamp() {
    for (text, basis, said) in [
        ("12px", 200.0, 12.0),
        ("-10px", 200.0, -10.0),
        ("0.5px", 200.0, 0.5),
        ("1e1px", 200.0, 10.0),
        ("25%", 200.0, 50.0),
        ("1.25%", 200.0, 2.5),
        ("0", 200.0, 0.0),
        // What Chrome computes: a four-value position, an xywh() or rect() as an inset.
        ("calc(100% - 10px)", 200.0, 190.0),
        ("calc(70% - 10px)", 200.0, 130.0),
        ("calc(10% + 5px)", 200.0, 25.0),
        ("calc(50% + -5px)", 200.0, 95.0),
        // Nested, grouped, scaled by a number, either side.
        ("calc(10px + calc(10% - 2px))", 200.0, 28.0),
        ("calc((100% - 10px) / 2)", 200.0, 95.0),
        ("calc(2 * 10px + 1px)", 200.0, 21.0),
        ("calc(10% * 3)", 200.0, 60.0),
        ("min(10px, 5%)", 400.0, 10.0),
        ("max(10px, 5%)", 400.0, 20.0),
        ("min(10px, 20px, 3px)", 0.0, 3.0),
        ("clamp(10px, 50%, 30px)", 100.0, 30.0),
        ("clamp(10px, 5%, 30px)", 100.0, 10.0),
        ("clamp(10px, 20%, 30px)", 100.0, 20.0),
        ("calc(min(10px, 50%) + 1px)", 10.0, 6.0),
    ] {
        assert_eq!(length(text, basis), Some(said), "{text} of {basis}");
    }
    // Anything else is not read: another unit, a keyword, a length times a length, a number that
    // is no length, an operator without its spaces, a part left over, or an empty one.
    for text in [
        "2em",
        "10vw",
        "auto",
        "closest-side",
        "calc(10px * 10px)",
        "calc(10px / 0)",
        "calc(10px / 2px)",
        "3",
        "calc(10px+5px)",
        "calc(10px + 5)",
        "10px 5px",
        "10px)",
        "calc(10px",
        "calc(10px, 5px)",
        "clamp(10px, 20px)",
        "min(2, 3)",
        "",
        "px",
        "%",
    ] {
        assert_eq!(length(text, 100.0), None, "{text}");
    }
}

#[test]
fn a_box_moved_only_across_the_page_is_still_and_any_other_move_is_not() {
    // As Chrome computes them: no transform, translateZ(0) and translate3d(0, 0, 0) (both read as
    // the identity), a translation, a full turn, a scale of one.
    for transform in [
        "none",
        "matrix(1, 0, 0, 1, 0, 0)",
        "matrix(1, 0, 0, 1, 10, 20)",
        "matrix(1, 0, 0, 1, 53, 5.6)",
        "matrix(1, 0, 0, 1, -3, 4)",
        "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 7, -8, 0, 1)",
    ] {
        assert!(moving(transform).still(), "{transform}");
    }
    // A mirror (scaleX(-1)), a half turn and a quarter turn, a scale, a skew, a depth of its own
    // (translateZ(5px)) and a perspective: never still.
    for transform in [
        "matrix(-1, 0, 0, 1, 0, 0)",
        "matrix(-1, 0, 0, -1, 0, 0)",
        "matrix(0, 1, -1, 0, 0, 0)",
        "matrix(2, 0, 0, 2, 0, 0)",
        "matrix(1, 0, 0.5, 1, 0, 0)",
        "matrix(1, -1.74533e-06, 1.74533e-06, 1, 0, 0)",
        "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 5, 1)",
        "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, -0.01, 0, 0, 0, 1)",
        "matrix(1, 0, 0, 1)",
        "rotate(90deg)",
        "matrix3d(1, 0, 0)",
        "",
    ] {
        assert!(!moving(transform).still(), "{transform}");
    }
    let with = |change: &dyn Fn(&mut Motion)| {
        let mut motion = still();
        change(&mut motion);
        motion.still()
    };
    // The individual transforms as Chrome computes them: a translation with no depth, the identity
    // scale and turn.
    for translate in ["10px 20px", "50%", "0px 0px 0px", "none"] {
        assert!(
            with(&|motion| motion.translate = translate.into()),
            "{translate}"
        );
    }
    assert!(!with(&|motion| motion.translate = "0px 0px 5px".into()));
    assert!(!with(&|motion| motion.translate = "1px 2px 3px 4px".into()));
    for scale in ["1", "1 1", "none"] {
        assert!(with(&|motion| motion.scale = scale.into()), "{scale}");
    }
    for scale in ["2", "1 -1", "-1"] {
        assert!(!with(&|motion| motion.scale = scale.into()), "{scale}");
    }
    for rotate in ["0deg", "x 0deg", "0turn", "none"] {
        assert!(with(&|motion| motion.rotate = rotate.into()), "{rotate}");
    }
    for rotate in [
        "180deg", "x 45deg", "0.25turn", "1rad", "100grad", "sideways",
    ] {
        assert!(!with(&|motion| motion.rotate = rotate.into()), "{rotate}");
    }
    assert!(!with(
        &|motion| motion.offset_path = "path(\"M 0 0 L 9 9\")".into()
    ));
    assert!(!with(&|motion| motion.zoom = "1.5".into()));
    assert!(!with(&|motion| motion.zoom = "normal".into()));
}

#[test]
fn a_shape_is_read_on_its_reference_box_and_nothing_else_is() {
    assert_eq!(
        split("inset(10px) content-box"),
        Some((Some(("inset", "10px")), "content-box"))
    );
    assert_eq!(
        split("circle(calc(50% - 10px) at calc(50% + 2px) 30%)"),
        Some((
            Some(("circle", "calc(50% - 10px) at calc(50% + 2px) 30%")),
            ""
        ))
    );
    assert_eq!(split("margin-box"), Some((None, "margin-box")));
    for unread in [
        "none",
        "path(\"M 0 0 L 10 10 Z\")",
        "url(\"#x\")",
        "inset(10px) sideways-box",
        "inset(10px",
        "xywh(1px 2px 3px 4px)",
        "inset(10px) content-box margin-box",
    ] {
        assert_eq!(split(unread), None, "{unread}");
    }
}

#[test]
fn a_reference_box_is_the_border_box_or_one_inside_or_around_it() {
    // What an inset(0), or a circle of 10 px at the box's left edge, keeps across the middle row
    // on each box, as Chrome kept it (its 5 px border, 10 px padding): the stroke and view boxes
    // are the border box, the fill box the content box, as for any box with a CSS layout.
    for (clip, kept) in [
        ("inset(0px)", (0.25, 129.75)),
        ("inset(0px) border-box", (0.25, 129.75)),
        ("inset(0px) stroke-box", (0.25, 129.75)),
        ("inset(0px) view-box", (0.25, 129.75)),
        ("inset(0px) padding-box", (5.25, 124.75)),
        ("inset(0px) content-box", (15.25, 114.75)),
        ("inset(0px) fill-box", (15.25, 114.75)),
        ("inset(0px) margin-box", (0.25, 129.75)),
        ("border-box", (0.25, 129.75)),
        ("padding-box", (5.25, 124.75)),
        ("content-box", (15.25, 114.75)),
        ("circle(10px at 0% 50%)", (0.25, 9.75)),
        ("circle(10px at 0% 50%) padding-box", (0.25, 14.75)),
        ("circle(10px at 0% 50%) content-box", (5.25, 24.75)),
    ] {
        assert_eq!(kept_across(clip), Some(kept), "{clip}");
    }
    // Its margin box's left edge 20 px outside it: a circle there reaches none of the box.
    assert_eq!(kept_across("circle(10px at 0% 50%) margin-box"), None);
    // The margin box alone keeps the margins around the border box.
    assert!(!shaped("margin-box").excludes((81.0, 245.0)));
    assert!(shaped("margin-box").excludes((79.0, 245.0)));
    // A box that may scroll may have a scrollbar between its border and its padding: its padding
    // and content boxes are not read, and claim nothing; its border and margin boxes still are.
    let scrolling = |clip: &str| Shaped {
        scrolls: true,
        ..shaped(clip)
    };
    for clip in [
        "inset(0px) content-box",
        "inset(0px) padding-box",
        "fill-box",
    ] {
        assert!(!scrolling(clip).excludes((101.0, 245.0)), "{clip}");
    }
    assert!(scrolling("inset(0px) margin-box").excludes((79.0, 245.0)));
    assert!(scrolling("inset(0px 0px 0px 10px)").excludes((101.0, 245.0)));
}

#[test]
fn an_inset_keeps_its_rectangle_less_its_rounded_corners() {
    let out = |clip: &str, (x, y): (f64, f64)| shaped(clip).excludes((100.0 + x, 200.0 + y));
    // One to four sides, as a margin's.
    for (clip, inside, outside) in [
        ("inset(10px)", (10.0, 10.0), (9.5, 45.0)),
        ("inset(10px 20%)", (26.0, 10.0), (25.5, 45.0)),
        ("inset(1px 2px 3px 4px)", (127.5, 86.5), (128.0, 45.0)),
        ("inset(0px 0px 60%)", (65.0, 35.5), (65.0, 36.0)),
        // An xywh() as Chrome computes it: 10 px from the left, 20 px down, 30 by 40.
        (
            "inset(20px calc(100% - 40px) calc(100% - 60px) 10px)",
            (39.5, 59.5),
            (40.0, 45.0),
        ),
    ] {
        assert!(!out(clip, inside), "{clip} keeps {inside:?}");
        assert!(out(clip, outside), "{clip} leaves out {outside:?}");
    }
    // Sides that cross keep nothing.
    assert!(out("inset(50% 0px 60%)", (65.0, 45.0)));
    assert!(out("inset(0px 70% 0px 40%)", (50.0, 45.0)));
    // Rounded corners, as Chrome hit-tests them on this 130x90 box: a corner of 20 px leaves out
    // (2, 2) and keeps (7, 7); a corner of half the box an ellipse's, (10, 10) out and (35, 6) in.
    assert!(out("inset(0px round 20px)", (2.0, 2.0)));
    assert!(!out("inset(0px round 20px)", (7.0, 7.0)));
    assert!(out("inset(0px round 50%)", (10.0, 10.0)));
    assert!(!out("inset(0px round 50%)", (35.0, 6.0)));
    // Each corner its own, by the four values and a slash between widths and heights.
    let corners = "inset(0px round 0px 30px 0px 0px / 0px 30px 0px 0px)";
    assert!(!out(corners, (0.0, 0.0)));
    assert!(out(corners, (128.0, 2.0)));
    assert!(!out(corners, (128.0, 88.0)));
    let wide = "inset(0px round 40px / 10px)";
    assert!(out(wide, (5.0, 1.0)));
    assert!(!out(wide, (5.0, 9.0)));
    // A rounded inset's sides still cut.
    assert!(out("inset(10px round 5px)", (5.0, 45.0)));
    // Corners too large for the box are made smaller alike: 100 px on a 130 px wide box, two per
    // side, are scaled to 45 each (by the height's 90 / 200), so (20, 45) on the left edge's
    // middle is kept and (2, 10) is not.
    assert!(!out("inset(0px round 100px)", (20.0, 45.0)));
    assert!(out("inset(0px round 100px)", (2.0, 10.0)));
    // Unread: a fifth side, a fifth radius, a slash with nothing after it.
    for clip in [
        "inset(1px 2px 3px 4px 5px)",
        "inset(0px round 1px 2px 3px 4px 5px)",
        "inset(0px round 10px /)",
        "inset(0px round)",
        "inset(2em)",
    ] {
        assert!(!out(clip, (0.0, 0.0)), "{clip}");
    }
}

#[test]
fn a_circle_or_an_ellipse_is_read_by_its_radii_and_its_centre_however_given() {
    let out = |clip: &str, (x, y): (f64, f64)| shaped(clip).excludes((100.0 + x, 200.0 + y));
    // The default circle: its centre the box's, its radius the closest side's (45). On its edge
    // exactly, a point is not claimed: only what lies outside is.
    assert!(!out("circle()", (65.0, 1.0)));
    assert!(!out("circle()", (20.5, 45.0)));
    assert!(!out("circle()", (20.0, 45.0)));
    assert!(out("circle()", (19.5, 45.0)));
    // A percentage of the box's diagonal over the square root of two: 50% of 111.8 is 55.9.
    assert!(!out("circle(50%)", (10.0, 45.0)));
    assert!(out("circle(50%)", (9.0, 45.0)));
    // The farthest side from (10, 20) is the right one, 120 away.
    assert!(out("circle(farthest-side at 10px 20px)", (129.5, 89.5)));
    assert!(!out("circle(farthest-side at 10px 20px)", (125.0, 20.0)));
    // A centre as Chrome computes it from keywords and offsets: at left top is 0% 0%, at right 10px
    // bottom 20px is calc(100% - 10px) calc(100% - 20px).
    assert!(!out("circle(30px at 0% 0%)", (20.0, 20.0)));
    assert!(out("circle(30px at 0% 0%)", (30.0, 30.0)));
    let corner = "circle(15px at calc(100% - 10px) calc(100% - 20px))";
    assert!(!out(corner, (120.0, 70.0)));
    assert!(out(corner, (100.0, 70.0)));
    // The same centres by their keywords, as an older serialization might say them.
    for clip in [
        "circle(30px at left top)",
        "circle(30px at top left)",
        "circle(30px at left 0px top 0%)",
        "circle(30px at top 0px left 0px)",
    ] {
        assert!(!out(clip, (20.0, 20.0)), "{clip}");
        assert!(out(clip, (30.0, 30.0)), "{clip}");
    }
    for (clip, kept, left_out) in [
        (
            "circle(15px at right 10px bottom 20px)",
            (120.0, 70.0),
            (100.0, 70.0),
        ),
        ("circle(10px at center)", (65.0, 45.0), (65.0, 30.0)),
        ("circle(10px at top)", (65.0, 5.0), (65.0, 15.0)),
        ("circle(10px at right)", (125.0, 45.0), (115.0, 45.0)),
        ("circle(10px at 10px)", (10.0, 50.0), (10.0, 60.0)),
        (
            "circle(10px at bottom 10px right 0px)",
            (125.0, 80.0),
            (115.0, 75.0),
        ),
    ] {
        assert!(!out(clip, kept), "{clip} keeps {kept:?}");
        assert!(out(clip, left_out), "{clip} leaves out {left_out:?}");
    }
    // An ellipse: its default radii the closest sides on each axis (65, 45); given, a percentage
    // of the box's width across and its height down.
    assert!(!out("ellipse()", (1.0, 45.0)));
    assert!(out("ellipse()", (10.0, 10.0)));
    assert!(!out("ellipse(20px 10px)", (84.0, 45.0)));
    assert!(out("ellipse(20px 10px)", (65.0, 56.0)));
    assert!(!out("ellipse(50% 50%)", (1.0, 45.0)));
    // From (39, 0): the closest side across 39 away, the farthest down 90.
    let sided = "ellipse(closest-side farthest-side at 30% 0%)";
    assert!(!out(sided, (39.0, 89.0)));
    assert!(out(sided, (1.0, 45.0)));
    // A radius of nothing keeps nothing off its centre's line.
    assert!(out(
        "ellipse(closest-side farthest-side at 0% 0%)",
        (1.0, 45.0)
    ));
    // Unread: a radius of an unread unit, an ellipse of one radius, a position of three words or
    // with a keyword where its offset goes.
    for clip in [
        "circle(2em)",
        "circle(10px 20px)",
        "circle(closest-corner)",
        "ellipse(20px)",
        "ellipse(1px 2px 3px)",
        "circle(1px at left 10px top)",
        "circle(1px at left top left top)",
        "circle(1px at center 10px top 0px)",
        "circle(1px at 1px 2px 3px 4px 5px)",
    ] {
        assert!(!out(clip, (129.0, 89.0)), "{clip}");
    }
}

#[test]
fn a_polygon_is_read_by_its_fill_rule() {
    let out = |clip: &str, (x, y): (f64, f64)| shaped(clip).excludes((100.0 + x, 200.0 + y));
    let triangle = "polygon(0px 0px, 100% 0px, 50% 100%)";
    assert!(!out(triangle, (65.0, 10.0)));
    assert!(out(triangle, (5.0, 80.0)));
    assert!(!out(
        "polygon(calc(10% + 5px) 0px, 100% 0px, 50% 100%)",
        (100.0, 10.0)
    ));
    // A five-pointed star drawn in one stroke: its middle is inside twice over, so the even-odd
    // rule leaves it out and the non-zero rule keeps it.
    let star = "50% 0%, 79% 100%, 2% 35%, 98% 35%, 21% 100%";
    assert!(!out(&format!("polygon({star})"), (65.0, 50.0)));
    assert!(!out(&format!("polygon(nonzero, {star})"), (65.0, 50.0)));
    assert!(out(&format!("polygon(evenodd, {star})"), (65.0, 50.0)));
    // Unread: two points, a point of one or three values.
    for clip in [
        "polygon(0px 0px, 10px 10px)",
        "polygon(0px 0px, 10px, 10px 10px)",
        "polygon(0px 0px 0px, 10px 0px, 10px 10px)",
        "polygon(evenodd)",
    ] {
        assert!(!out(clip, (129.0, 89.0)), "{clip}");
    }
}

#[test]
fn a_box_or_one_around_it_that_moves_otherwise_than_across_the_page_claims_nothing() {
    let cut = |moves: Vec<Motion>| {
        Shaped {
            moves,
            ..shaped("inset(0px 0px 60%)")
        }
        .excludes((165.0, 280.0))
    };
    // translateZ(0) on the page's root, a translation on the box: still read.
    assert!(cut(vec![still(), moving("matrix(1, 0, 0, 1, 0, 0)")]));
    assert!(cut(vec![moving("matrix(1, 0, 0, 1, 30, 10)"), still()]));
    // Mirrored, turned half or a quarter, scaled, anywhere up the tree: never claimed.
    for transform in [
        "matrix(-1, 0, 0, 1, 0, 0)",
        "matrix(-1, 0, 0, -1, 0, 0)",
        "matrix(0, 1, -1, 0, 0, 0)",
        "matrix(0.5, 0, 0, 0.5, 0, 0)",
    ] {
        assert!(!cut(vec![still(), moving(transform)]), "{transform}");
        assert!(!cut(vec![moving(transform), still()]), "{transform}");
    }
    // Nothing moving it: the point the inset leaves out is claimed, the one it keeps is not.
    assert!(cut(vec![still()]));
    assert!(!shaped("inset(0px 0px 60%)").excludes((165.0, 210.0)));
}

#[test]
fn a_probes_shape_is_read_whole_or_not_at_all() {
    let read = json!({
        "name": "div#cut",
        "own": false,
        "clip": "inset(0px 0px 60%)",
        "box": [100, 200, 130, 90],
        "border": [5, 5, 5, 5],
        "padding": [10, 10, 10, 10],
        "margin": [20, 20, 20, 20],
        "scrolls": false,
        "moves": [
            ["none", "none", "none", "none", "none", "1"],
            ["matrix(1, 0, 0, 1, 0, 0)", "none", "none", "none", "none", "1"]
        ]
    });
    assert_eq!(
        Shaped::read(&read),
        Some(Shaped {
            name: "div#cut".into(),
            clip: "inset(0px 0px 60%)".into(),
            moves: vec![still(), moving("matrix(1, 0, 0, 1, 0, 0)")],
            ..shaped("")
        })
    );
    for key in [
        "name", "own", "clip", "box", "border", "padding", "margin", "scrolls", "moves",
    ] {
        let mut short = read.clone();
        short.as_object_mut().unwrap().remove(key);
        assert_eq!(Shaped::read(&short), None, "{key}");
    }
    for (key, wrong) in [
        ("box", json!([1, 2, 3])),
        ("border", json!([1, 2, 3, "4"])),
        ("moves", json!([["none"]])),
        ("own", json!("yes")),
    ] {
        let mut wrong_read = read.clone();
        wrong_read[key] = wrong;
        assert_eq!(Shaped::read(&wrong_read), None, "{key}");
    }
}
