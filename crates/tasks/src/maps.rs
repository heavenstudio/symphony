//! Ordered maps built by insertion, sharing the tree's insertion code instead of compiling a
//! stable sort for every key/value type. Inserting the pairs from the back keeps the last pair
//! for each equal key, including its key's representation ([`Collect::collect_map`], [`of`]).

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

/// `pairs` as a map, a key's last value kept.
pub(crate) fn of<K: Ord, V>(pairs: Vec<(K, V)>) -> BTreeMap<K, V> {
    let mut map = BTreeMap::new();
    for (key, value) in pairs.into_iter().rev() {
        map.entry(key).or_insert(value);
    }
    map
}

#[cfg(test)]
mod tests;
