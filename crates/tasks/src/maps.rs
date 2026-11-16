//! A map collected from its pairs through a `Vec` ([`Collect::collect_map`], [`of`]), so the sort
//! it takes is compiled once for each key and value type, not once for each iterator.
//!
//! `BTreeMap`'s `collect` gathers its pairs in a `Vec` and sorts them stably by key, and the sort's
//! comparison is a closure of that `collect`: it is compiled again, about 4 KB of machine code, for
//! each type of iterator a map is collected from, and the board collects one map type from a dozen.
//! Gathered in a `Vec` first, every map of one key and value type is the one `collect` of a `Vec`
//! ([`of`]), which reuses that `Vec`'s allocation: the same pairs in the same order, a key's last
//! value kept, as `collect` keeps it (docs/CHECKS.md, "Size budgets"). A map type collected once in
//! the crate gains nothing by it, and is collected as it is.

use std::collections::BTreeMap;

/// An iterator of pairs collected as a map through [`of`].
pub(crate) trait Collect<K, V>: Iterator<Item = (K, V)> + Sized {
    /// The pairs as a map, a key's last value kept, as `collect` makes it.
    fn collect_map(self) -> BTreeMap<K, V>
    where
        K: Ord,
    {
        of(self.collect())
    }
}

impl<K, V, I: Iterator<Item = (K, V)>> Collect<K, V> for I {}

/// `pairs` as a map, a key's last value kept: the one `collect` of a map of `K` and `V`.
pub(crate) fn of<K: Ord, V>(pairs: Vec<(K, V)>) -> BTreeMap<K, V> {
    pairs.into_iter().collect()
}

#[cfg(test)]
mod tests;
