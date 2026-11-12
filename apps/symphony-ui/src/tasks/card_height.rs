//! How tall a task's card draws in a board column, worked out from the task before it is drawn,
//! so the row waiting for it holds the place it will take and a column neither grows nor shrinks
//! as its rows come in (app/tasks-page-polish).
//!
//! It follows board.css's card in a column. The card's text is 244 px wide in a desk's 288 px
//! column and 236 px in a phone's 280 px (`max-width: 760px`), 3 px less behind the wider left
//! border of a p0 or p1. Its borders, padding and margins take 40 px; each line its slug, its
//! title and, for a subtask, its parent wrap to takes 20.3 px (14 px at 1.45); then come the rows
//! its chips wrap to, 4 px apart: its build and test phases when it has any, 6 px below the title,
//! then its link chips and its role chips. A role chip keeps to one line (21.2 px); a phase chip
//! (19.2 px a line) and a link chip (15.2 px a line) wrap inside themselves when wider than the
//! card. Text is measured in Helvetica's widths, the face `--font-sans` comes to where the browser
//! has no system face of its own to give it, and the card breaks it as `overflow-wrap:anywhere`
//! does.

use symphony_api::Task;

/// Tenths of a pixel: a card's box, a line of its text, the gap between chips, a chip's two
/// borders, and the space above its phases.
const BOX: u32 = 400;
const LINE: u32 = 203;
const GAP: u32 = 40;
const BORDER: u32 = 20;
const PHASES: u32 = 60;

/// A kind of chip: the size of its text, the room its padding, borders and dot take beside the
/// text, and how tall each line of its text is, in tenths of a pixel; and whether it wraps.
struct Chip {
    size: u32,
    beside: u32,
    line: u32,
    wraps: bool,
}

/// A link chip: its file name at 10.5 px, 7 px padded each side.
const LINK: Chip = Chip {
    size: 105,
    beside: 160,
    line: 152,
    wraps: true,
};
/// A role chip: its role and holder at 12 px, 8 px padded with a 7 px dot 4 px before.
const ROLE: Chip = Chip {
    size: 120,
    beside: 290,
    line: 192,
    wraps: false,
};
/// A phase chip: drawn as a role chip is, but wrapping its words.
const PHASE: Chip = Chip {
    wraps: true,
    ..ROLE
};

/// The board's layout, which sets how wide a card's text runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Layout {
    Desk,
    /// A window 760 px wide or less, whose columns are 280 px.
    Phone,
}

impl Layout {
    /// The layout the window draws the board in now (a desk's where there is no window).
    pub(super) fn now() -> Self {
        #[cfg(target_arch = "wasm32")]
        let width = web_sys::window()
            .and_then(|window| window.inner_width().ok())
            .and_then(|width| width.as_f64());
        #[cfg(not(target_arch = "wasm32"))]
        let width = None;
        width.map_or(Self::Desk, Self::at)
    }

    /// The layout of a window `width` px wide.
    fn at(width: f64) -> Self {
        if width <= 760.0 {
            Self::Phone
        } else {
            Self::Desk
        }
    }

    /// How wide a card's text runs, in tenths of a pixel, for a task of `priority`.
    fn text(self, priority: i64) -> u32 {
        let column = match self {
            Self::Desk => 2440,
            Self::Phone => 2360,
        };
        if matches!(priority, 0 | 1) {
            column - 30
        } else {
            column
        }
    }
}

/// The faces a card's text is set in.
#[derive(Clone, Copy)]
enum Face {
    /// The slug, .82 of 14 px, 11.5 px.
    Mono,
    /// The parent's line (14 px) and the chips (12 px roles and phases, 10.5 px links).
    Regular,
    /// The title (14 px) and the priority after the slug (.72 of 14 px), weight 600.
    Bold,
}

/// What `task`'s card draws at in `layout`, in whole pixels, its margin included, with the
/// build and test phases its card shows (`machine_work::phase_labels`).
pub(super) fn row_height(task: &Task, phases: &[String], layout: Layout) -> u32 {
    let width = layout.text(task.priority);
    let slug = Lines::new(width)
        .set(&task.slug, Face::Mono, 115)
        .space(Face::Regular, 140)
        .set(&format!("p{}", task.priority), Face::Bold, 101)
        .count();
    let title = Lines::new(width).set(&task.title, Face::Bold, 140).count();
    let parent = task.parent.as_ref().map_or(0, |parent| {
        Lines::new(width)
            .set(&format!("Subtask of {parent}"), Face::Regular, 140)
            .count()
    });
    let phases = if phases.is_empty() {
        0
    } else {
        PHASES + chip_rows(phases.iter().map(|label| chip(label, &PHASE, width)), width)
    };
    let links = task.links.iter().map(|link| {
        let name = link.rsplit('/').next().unwrap_or(link);
        chip(name, &LINK, width)
    });
    let roles = task
        .roles
        .iter()
        .map(|role| chip(&format!("{} {}", role.role, role.handle), &ROLE, width));
    BOX.saturating_add(LINE.saturating_mul(slug + title + parent))
        .saturating_add(phases)
        .saturating_add(chip_rows(links, width))
        .saturating_add(chip_rows(roles, width))
        .saturating_add(5)
        / 10
}

/// How wide and tall a chip of `kind` reading `text` draws in a card whose text is `width`
/// wide: as wide as its text, or as the card, where a chip that wraps takes as many lines as
/// its text needs.
fn chip(text: &str, kind: &Chip, width: u32) -> (u32, u32) {
    let natural = kind
        .beside
        .saturating_add(text_width(text, Face::Regular).saturating_mul(kind.size) / 1000);
    let lines = if kind.wraps && natural > width {
        Lines::new(width.saturating_sub(kind.beside))
            .set(text, Face::Regular, kind.size)
            .count()
            .max(1)
    } else {
        1
    };
    (
        natural.min(width),
        BORDER.saturating_add(kind.line.saturating_mul(lines)),
    )
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

/// Text set line after line across a card, as the card breaks it: between words, after a hyphen
/// or a slash, and inside a part wider than a line. Widths are kept in ten-thousandths of a pixel,
/// a face's thousandths of its size times the size in tenths.
struct Lines {
    width: u32,
    lines: u32,
    used: u32,
    /// The space before the next word.
    gap: u32,
}

impl Lines {
    /// Nothing set yet across `width` tenths of a pixel.
    fn new(width: u32) -> Self {
        Self {
            width: width.saturating_mul(1000).max(1),
            lines: 0,
            used: 0,
            gap: 0,
        }
    }

    /// A space in `face` at `size` tenths of a pixel before what is set next.
    fn space(mut self, face: Face, size: u32) -> Self {
        self.gap = advance(' ', face).saturating_mul(size);
        self
    }

    /// `text` in `face` at `size` tenths of a pixel, set after what the lines hold.
    fn set(mut self, text: &str, face: Face, size: u32) -> Self {
        let space = advance(' ', face).saturating_mul(size);
        for word in text.split_whitespace() {
            for (index, part) in word.split_inclusive(['-', '/']).enumerate() {
                let before = if index == 0 {
                    std::mem::replace(&mut self.gap, space)
                } else {
                    0
                };
                self.place(text_width(part, face).saturating_mul(size), before);
            }
        }
        self.gap = 0;
        self
    }

    /// One unbreakable part `width` wide, `before` after the last on its line or on a line of its
    /// own, broken across lines where it is wider than one.
    fn place(&mut self, width: u32, before: u32) {
        let width = width.max(1);
        if self.lines == 0 || self.used.saturating_add(before).saturating_add(width) > self.width {
            self.lines = self.lines.saturating_add(width.div_ceil(self.width));
            self.used = (width - 1) % self.width + 1;
        } else {
            self.used += before + width;
        }
    }

    /// How many lines the text set takes.
    fn count(&self) -> u32 {
        self.lines
    }
}

/// The height of `chips` (each its width and height, in tenths of a pixel) wrapped in rows
/// across a card's text `width` wide, a gap between them each way, each row as tall as its
/// tallest chip; no chips take none.
fn chip_rows(chips: impl Iterator<Item = (u32, u32)>, width: u32) -> u32 {
    let (mut total, mut row, mut used) = (0_u32, None::<u32>, 0_u32);
    for (chip, height) in chips {
        let chip = chip.min(width);
        match row {
            Some(tall) if used.saturating_add(GAP).saturating_add(chip) <= width => {
                used += GAP + chip;
                row = Some(tall.max(height));
            }
            _ => {
                if let Some(tall) = row {
                    total = total.saturating_add(tall).saturating_add(GAP);
                }
                row = Some(height);
                used = chip;
            }
        }
    }
    total.saturating_add(row.unwrap_or(0))
}

#[cfg(test)]
mod tests;
