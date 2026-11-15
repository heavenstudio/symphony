//! The register a page draws when a card it shows in place of its file's copy belongs in another
//! group than that copy: a branch's page drawing main's card the owner answered there, its answer
//! saved on main ([`crate::branches::mains_card`]), or a page whose one card holds another copy's
//! lines. The page draws the card in the group its answer puts it in, as the page it was saved on
//! does, and every section keeps the file's lines as its place, so a save from the page lands where
//! it did.

use std::collections::{HashMap, VecDeque};

use gix::diff::blob::{Algorithm, Diff, InternedInput};

use crate::Decision;
use crate::plugins::Plugins;
use crate::register::{parse_decisions, sort_register};
use crate::text::splitlines;

/// What a page draws of the file `text`, whose cards are `parsed`, when it shows the cards `cards`
/// in place of the file's copies: the text, each such card written as that card and the register
/// regrouped as a save regroups it ([`sort_register`]), and for each of its lines (by index) the
/// file's line (1-based) it stands for. A card's lines are its own card's in the file (a line it has
/// past the file's copy, the file copy's last); a line the regroup left as it was is itself; a group
/// or topic heading the file lacks stands for none.
///
/// None when every card shown belongs in the group its file copy does, or the register cannot be
/// regrouped (a register a save would refuse): the page then draws the file's register as it is.
pub(super) fn drawn(
    text: &str,
    parsed: &[Decision],
    cards: &[Decision],
    plugins: &Plugins,
) -> Option<(String, Vec<Option<usize>>)> {
    let shown = |slug: &str| cards.iter().find(|one| one.slug == slug);
    if !parsed
        .iter()
        .any(|card| shown(&card.slug).is_some_and(|one| one.group(plugins) != card.group(plugins)))
    {
        return None;
    }
    let lines = splitlines(text);
    let mut written: Vec<String> = Vec::with_capacity(lines.len());
    let mut origin: Vec<Option<usize>> = Vec::with_capacity(lines.len());
    // The index of the file's next line to copy as it is.
    let mut next = 0;
    for card in parsed {
        let Some(one) = shown(&card.slug) else {
            continue;
        };
        for (at, line) in lines.iter().enumerate().take(card.line - 1).skip(next) {
            written.push((*line).to_string());
            origin.push(Some(at + 1));
        }
        for (at, line) in splitlines(&crate::branches::markdown(one))
            .iter()
            .enumerate()
        {
            written.push((*line).to_string());
            origin.push(Some(card.line + at.min(card.raw.len())));
        }
        next = card.line + card.raw.len();
    }
    for (at, line) in lines.iter().enumerate().skip(next) {
        written.push((*line).to_string());
        origin.push(Some(at + 1));
    }
    let sorted = sort_register(&(written.join("\n") + "\n"), plugins).ok()?;
    let sorted_lines = splitlines(&sorted);
    let mut places = kept(&written, &sorted_lines, &origin);
    // A card's lines, wherever the regroup put it: its own (a slug's nth copy, the nth).
    let mut blocks: HashMap<String, VecDeque<usize>> = HashMap::new();
    for card in parse_decisions(&written) {
        blocks
            .entry(card.slug)
            .or_default()
            .push_back(card.line - 1);
    }
    for card in parse_decisions(&sorted_lines) {
        let Some(from) = blocks.get_mut(&card.slug).and_then(VecDeque::pop_front) else {
            continue;
        };
        for at in 0..=card.raw.len() {
            if let (Some(place), Some(&own)) =
                (places.get_mut(card.line - 1 + at), origin.get(from + at))
            {
                *place = own;
            }
        }
    }
    Some((sorted, places))
}

/// For each of the lines `after`, the place (`origin`) of the line of `before` it is where a diff of
/// the two keeps it, and none where it does not.
fn kept(before: &[String], after: &[&str], origin: &[Option<usize>]) -> Vec<Option<usize>> {
    let mut input = InternedInput::default();
    input.update_before(before.iter().map(String::as_str));
    input.update_after(after.iter().copied());
    let mut places = vec![None; after.len()];
    let (mut old, mut new) = (0, 0);
    for hunk in Diff::compute(Algorithm::Myers, &input).hunks() {
        while new < hunk.after.start as usize {
            places[new] = origin[old];
            (old, new) = (old + 1, new + 1);
        }
        (old, new) = (hunk.before.end as usize, hunk.after.end as usize);
    }
    while new < after.len() {
        places[new] = origin[old];
        (old, new) = (old + 1, new + 1);
    }
    places
}

/// The file's lines a section drawn over the lines `start..=end` (1-based) of the drawn text stands
/// for, by `places` ([`drawn`]): from the first of them the file holds to the last; `(0, 0)`, no
/// place in the file, for a heading only the regroup wrote.
pub(super) fn place(places: &[Option<usize>], start: usize, end: usize) -> (usize, usize) {
    let mut held = places
        .iter()
        .take(end)
        .skip(start.saturating_sub(1))
        .flatten();
    let Some(&first) = held.next() else {
        return (0, 0);
    };
    (first, held.last().copied().unwrap_or(first))
}

#[cfg(test)]
#[path = "regroup_tests.rs"]
mod tests;
