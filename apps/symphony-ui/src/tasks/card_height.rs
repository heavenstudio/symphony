//! How tall a task's card draws in a board column, worked out from the task before it is drawn,
//! so the row waiting for it holds the place it will take and a column neither grows nor shrinks
//! as its rows come in (app/tasks-page-polish).
//!
//! It follows board.css's card in a column, whose text is about 240 px wide (244 px in a desk's
//! 288 px column, 236 px in a phone's 280 px): its borders, padding and margins, 40 px; a line of
//! 20.3 px (14 px at 1.45) for each line its slug, its title and, for a subtask, its parent wrap
//! to; and the rows its link chips (17.2 px tall) and its role chips (21.2 px) wrap to, 4 px
//! apart. Text is measured in Helvetica's widths, the face `--font-sans` comes to where the
//! browser has no system face of its own to give it, and the card breaks it as
//! `overflow-wrap:anywhere` does. Build and test phases come from machines, not the task, and are
//! left out: they show in the Build and Test columns, whose cards are few.

use symphony_api::Task;

/// Tenths of a pixel: a card's box, a line of text, a link chip, a role chip, the gap between
/// chips, and the width of a card's text.
const BOX: u32 = 400;
const LINE: u32 = 203;
const LINK_CHIP: u32 = 172;
const ROLE_CHIP: u32 = 212;
const GAP: u32 = 40;
const WIDTH: u32 = 2400;

/// The faces a card's text is set in.
#[derive(Clone, Copy)]
enum Face {
    /// The slug, .82 of 14 px, 11.5 px.
    Mono,
    /// The parent's line (14 px) and the chips (12 px roles, 10.5 px links).
    Regular,
    /// The title, weight 600 at 14 px.
    Bold,
}

/// What `task`'s card draws at, in whole pixels, its margin included.
pub(super) fn row_height(task: &Task) -> u32 {
    let lines = wrapped_lines(
        &format!("{} p{}", task.slug, task.priority),
        Face::Mono,
        115,
    ) + wrapped_lines(&task.title, Face::Bold, 140)
        + task.parent.as_ref().map_or(0, |parent| {
            wrapped_lines(&format!("Subtask of {parent}"), Face::Regular, 140)
        });
    // A link chip shows the link's file name at 10.5 px, 7 px padded each side within its
    // border; a role chip its role and holder at 12 px, 8 px padded with a 7 px dot 4 px before.
    let links = task.links.iter().map(|link| {
        let name = link.rsplit('/').next().unwrap_or(link);
        160_u32.saturating_add(text_width(name, Face::Regular).saturating_mul(105) / 1000)
    });
    let roles = task.roles.iter().map(|role| {
        let label = format!("{} {}", role.role, role.handle);
        290_u32.saturating_add(text_width(&label, Face::Regular).saturating_mul(120) / 1000)
    });
    BOX.saturating_add(LINE.saturating_mul(lines))
        .saturating_add(chip_rows(links, LINK_CHIP))
        .saturating_add(chip_rows(roles, ROLE_CHIP))
        .saturating_add(5)
        / 10
}

/// A character's width in thousandths of its font's size, as Helvetica draws it.
fn advance(ch: char, face: Face) -> u32 {
    let bold = matches!(face, Face::Bold);
    let pick = |regular: u32, heavy: u32| if bold { heavy } else { regular };
    match ch {
        _ if matches!(face, Face::Mono) => 600,
        'i' | 'j' | 'l' => pick(222, 278),
        '\'' => pick(191, 238),
        ' ' | ',' | '.' | '/' | 'I' | '[' | ']' | '\\' | '|' => 278,
        'f' | 't' | '!' | ':' | ';' => pick(278, 333),
        '(' | ')' | '-' | '`' => 333,
        'r' => pick(333, 389),
        'z' => 500,
        'c' | 'k' | 's' | 'v' | 'x' | 'y' | 'J' => pick(500, 556),
        'b' | 'd' | 'g' | 'h' | 'n' | 'o' | 'p' | 'q' | 'u' | '?' | 'L' => pick(556, 611),
        'F' | 'T' | 'Z' => 611,
        'A' | 'B' | 'K' | '&' => pick(667, 722),
        'E' | 'P' | 'S' | 'V' | 'X' | 'Y' => 667,
        'C' | 'D' | 'H' | 'N' | 'R' | 'U' => 722,
        'w' => pick(722, 778),
        'G' | 'O' | 'Q' => 778,
        'm' => pick(833, 889),
        'M' => 833,
        'W' => 944,
        _ => 556,
    }
}

/// The width of `text` in `face`, in thousandths of its font's size.
fn text_width(text: &str, face: Face) -> u32 {
    text.chars()
        .map(|ch| advance(ch, face))
        .fold(0, u32::saturating_add)
}

/// How many lines `text` wraps to across a card's text in `face` at `size` tenths of a pixel:
/// breaking between words, after a hyphen or a slash, and inside a part wider than a line.
fn wrapped_lines(text: &str, face: Face, size: u32) -> u32 {
    let line = (WIDTH * 1000 / size.max(1)).max(1);
    let space = advance(' ', face);
    let (mut lines, mut used) = (0_u32, 0_u32);
    for word in text.split_whitespace() {
        for (index, part) in word.split_inclusive(['-', '/']).enumerate() {
            let width = text_width(part, face).max(1);
            let before = if index == 0 && used > 0 { space } else { 0 };
            if lines == 0 || used.saturating_add(before).saturating_add(width) > line {
                lines = lines.saturating_add(width.div_ceil(line));
                used = (width - 1) % line + 1;
            } else {
                used += before + width;
            }
        }
    }
    lines
}

/// The height of chips `chip` tall and as wide as `widths` (all in tenths of a pixel), wrapped in
/// rows across a card's text with a gap between them each way; no chips take none.
fn chip_rows(widths: impl Iterator<Item = u32>, chip: u32) -> u32 {
    let (mut rows, mut used) = (0_u32, 0_u32);
    for width in widths.map(|width| width.min(WIDTH)) {
        if rows == 0 || used + GAP + width > WIDTH {
            rows += 1;
            used = width;
        } else {
            used += GAP + width;
        }
    }
    rows.saturating_mul(chip)
        .saturating_add(rows.saturating_sub(1).saturating_mul(GAP))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(title: &str, parent: Option<&str>, links: &[&str], roles: &[(&str, &str)]) -> Task {
        let roles: Vec<_> = roles
            .iter()
            .map(|(role, handle)| json!({"handle": handle, "role": role, "since": 0.0, "expires": null, "health": "live"}))
            .collect();
        serde_json::from_value(json!({"slug": "bulk/ready-1", "title": title, "body": "",
            "status": "ready", "priority": 3, "parent": parent, "created_by": "owner",
            "created": 0.0, "updated": 0.0, "links": links, "depends": [], "roles": roles,
            "instruction": false}))
        .unwrap()
    }

    /// Titles wrap where the board's cards wrap them, measured in a 1280 px window: 238 px of
    /// "A task whose title is long enough to" fits a line, 256 px of "Ready task number 1 in a
    /// long column" does not, and a word longer than a line breaks inside itself.
    #[test]
    fn a_title_wraps_where_a_card_wraps_it() {
        let lines = |text: &str| wrapped_lines(text, Face::Bold, 140);
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
        // The slug breaks after its hyphens and slashes, 34 characters a line.
        let slug = |text: &str| wrapped_lines(text, Face::Mono, 115);
        assert_eq!(slug("bulk/ready-1 p3"), 1);
        assert_eq!(
            slug("app/a-very-long-task-slug-that-goes-on-and-on-to-test-wrapping-in-cards p1"),
            3
        );
    }

    /// A card with no chips draws at 101 px with a title of two lines; each row of chips adds its
    /// own height, a parent its line, and chips too wide for one row wrap to more.
    #[test]
    fn a_card_reserves_its_lines_and_its_rows_of_chips() {
        let two = "Ready task number 1 in a long column";
        assert_eq!(row_height(&task(two, None, &[], &[])), 101);
        assert_eq!(row_height(&task("Short", None, &[], &[])), 81);
        assert_eq!(
            row_height(&task(two, None, &[], &[("orchestrator", "quiet-plover")])),
            122
        );
        assert_eq!(
            row_height(&task(
                two,
                None,
                &["docs/DECISIONS.md"],
                &[("orchestrator", "quiet-plover")]
            )),
            139
        );
        assert_eq!(
            row_height(&task(two, Some("app/parent"), &[], &[])),
            121,
            "a parent adds its line"
        );
        // Two role chips of 160 px and more do not share a 240 px row.
        assert_eq!(
            row_height(&task(
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
            row_height(&task(
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
        assert_eq!(chip_rows(std::iter::empty(), ROLE_CHIP), 0);
        assert_eq!(chip_rows([1000, 1000].into_iter(), ROLE_CHIP), ROLE_CHIP);
        assert_eq!(
            chip_rows([1000, 1000, 1000].into_iter(), ROLE_CHIP),
            2 * ROLE_CHIP + GAP
        );
        assert_eq!(
            chip_rows([9000].into_iter(), LINK_CHIP),
            LINK_CHIP,
            "a chip wider than the card takes one row"
        );
    }
}
