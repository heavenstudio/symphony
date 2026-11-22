//! A slice sorted through the one sort the crate compiles ([`sort_by`], [`sort_by_key`]).
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
//! This allocates one position per element, and compares before moving any element. It serves
//! inspection and reporting sorts, where sharing the code is worth that extra allocation. Busy
//! board paths keep their in-place sorts. A comparison that panics leaves the elements untouched.

use std::cmp::Ordering;

/// `items` sorted by `compare`, stably, as `slice::sort_by` sorts them.
///
/// Uses one temporary position per element to share the sorting code across element types.
/// Suitable for inspection and presentation reads; latency-sensitive paths can keep their
/// in-place sort. If `compare` panics, `items` remains unchanged.
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
