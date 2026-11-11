//! Recipient/post-bounded answer records and their actual task ancestry.

use super::{ANSWER_SQL, Error, Store, recovery_error};
use crate::store::work::{Tree, above};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) type Answer = (i64, String);

pub(super) fn batch() -> String {
    format!(
        "RETURN $posts.map(|$p| {{ LET $handle = $p[0]; LET $post = $p[1]; RETURN {}; }});",
        read()
    )
}

/// Equality on the first two index columns leaves only this sender/kind's sequence suffix.
/// Keep the original action predicate, including deliberate releases and role acquisitions.
pub(super) fn read() -> String {
    format!(
        "array::sort(array::flatten(['msg', 'verdict', 'system'].map(|$kind| \
({}).filter(|$row| $row.eligible).map(|$row| [$row.sid, $row.slug]))))",
        select()
    )
}

pub(super) fn select() -> String {
    // Closures run through the legacy planner. Putting ANSWER_SQL's string predicates in WHERE
    // makes it abandon the index for a full table scan, even with WITH INDEX. Compute eligibility
    // on the indexed suffix instead; read() filters it before returning only row identities.
    format!(
        "SELECT sid, slug, ({ANSWER_SQL}) AS eligible FROM message WITH INDEX message_sender \
WHERE sender = $handle AND kind = $kind AND sid > $post"
    )
}

pub(super) fn rows(store: &Store, handle: &str, post: i64) -> Result<Vec<Answer>, Error> {
    store
        .read(
            &format!("RETURN {};", read()),
            json!({"handle":handle, "post":post}),
        )
        .map_err(recovery_error)
}

pub(super) fn answered(rows: &[Answer], slug: &str, tree: &Tree) -> bool {
    rows.iter()
        .any(|(_, at)| at == slug || above(tree, at).any(|parent| parent == slug))
}

/// Read only the parent chains of the recipient's eligible records, by task record id. Keep
/// each observed edge for the reservation's final transaction to recheck alongside the rows:
/// a late answer or reparenting refuses that attempt without consuming its durable reservation.
pub(super) fn parents(
    store: &Store,
    rows: &[Answer],
) -> Result<Vec<(String, Option<String>)>, Error> {
    let mut tree = Tree::new();
    let mut wanted: BTreeSet<_> = rows.iter().map(|(_, slug)| slug.clone()).collect();
    while !wanted.is_empty() {
        let edges: Vec<(String, Option<String>)> = store
            .read(
                "RETURN $slugs.map(|$slug| [$slug, type::record('task', $slug).parent ?? NULL]);",
                json!({"slugs":wanted}),
            )
            .map_err(recovery_error)?;
        tree.extend(edges.iter().cloned());
        wanted = edges
            .into_iter()
            .filter_map(|(_, parent)| parent)
            .filter(|parent| !tree.contains_key(parent))
            .collect();
    }
    let mut parents: Vec<_> = tree.into_iter().collect();
    parents.sort();
    Ok(parents)
}
