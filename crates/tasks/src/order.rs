//! A slice sorted through the one sort the crate compiles ([`sort_by`], [`sort_by_key`]): every sort
//! of the board's, `symphony`'s and the server's code (RULES.md `code/shared-sorter`).
//!
//! The standard library's sort is generic over the element and the comparison, and every closure
//! is a type of its own: each `sort_by` with a closure, and each `sort` of another element type,
//! compiles the whole stable sort again, about 4 KB of machine code (its quicksort, its merges and
//! their sorts of a few elements). These sort the slice's positions instead, a `[usize]` compared
//! through `&dyn Fn`, so that sort is compiled once for the crate, then move each element to its
//! place: the order `slice::sort_by` gives, equal elements in the order they stood. What each
//! element type and comparison still compiles is the move and a call (docs/workflow/CHECKS.md, "Size
//! budgets").
//!
//! This allocates one position per element, and compares before moving any element; a comparison
//! that panics leaves the elements untouched. Each of those crates' clippy.toml refuses std's seven
//! slice sorts, and three kinds of site keep std's, each under an
//! `#[expect(clippy::disallowed_methods, reason = "...")]` naming it:
//!
//! - A busy board path, which runs on every pull or sweep, where a position per element and a call
//!   through `dyn` per comparison would cost: PULL's digest and what it reads (its alarms, waits,
//!   unanswered posts and slot ledger), the staff pass, the idle reap, the sweep and its
//!   `Store::descendants`, and a filing's `give_filed_saying`.
//! - This module's one sort; the server's build script, which links no board; and symphony-chrome's
//!   harness file, which the board's and the server's tests read by path.
//! - In a test that sorts one copy of an input with std's sort and another with this one to hold
//!   the two equal, the std side. A test's other sorts are this module's.
//!
//! A collect into a `BTreeMap` or `BTreeSet` sorts too: its `FromIterator` gathers the items in a
//! `Vec` and sorts them with std's stable sort, compiled for each element type. Those crates build
//! one by `extend` on an empty one instead, which inserts each item through the `insert` its type
//! compiles anyway and keeps a repeated key's last value, as `collect` does. No lint can name that
//! collect (it is `Iterator::collect`), so review holds it.

use std::cmp::Ordering;

/// `items` sorted by `compare`, stably, as `slice::sort_by` sorts them.
///
/// Uses one temporary position per element to share the sorting code across element types.
/// Every sort but a busy board path's (the module's doc). If `compare` panics, `items` remains
/// unchanged.
pub fn sort_by<T>(items: &mut [T], compare: &impl Fn(&T, &T) -> Ordering) {
    if items.len() < 2 {
        return;
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    positions(&mut order, &|a, b| compare(&items[a], &items[b]));
    place(items, &mut order);
}

/// `items` sorted by `key`, stably, as `slice::sort_by_key` sorts them.
///
/// Has the allocation and panic behavior of [`sort_by`]. The key is evaluated for comparisons,
/// rather than cached, just as with `slice::sort_by_key`.
pub fn sort_by_key<T, K: Ord>(items: &mut [T], key: &impl Fn(&T) -> K) {
    sort_by(items, &|a, b| key(a).cmp(&key(b)));
}

/// `order`, positions of a slice, sorted by `compare` of the elements there: the one sort.
fn positions(order: &mut [usize], compare: &dyn Fn(usize, usize) -> Ordering) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one sort the shared sorter compiles"
    )]
    order.sort_by(|&a, &b| compare(a, b));
}

/// `items` with the element at `order[i]` moved to `i`, by following each cycle of `order` once;
/// `order` is left as its own positions.
fn place<T>(items: &mut [T], order: &mut [usize]) {
    for start in 0..order.len() {
        let mut at = start;
        loop {
            let from = order[at];
            order[at] = at;
            if from == start {
                break;
            }
            items.swap(at, from);
            at = from;
        }
    }
}

#[cfg(test)]
mod tests;
