//! One recipient's answer records after one post, and their actual task ancestry: what recovery
//! reads again, and its reservation checks once more, before it resumes a conversation.

use super::{Error, Store, pg, recovery_error};
use crate::store::statement::Statement;
use crate::store::work::{Tree, above};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) type Answer = (i64, String);

pub(super) fn rows(store: &Store, handle: &str, post: i64) -> Result<Vec<Answer>, Error> {
    store
        .bounded_read(
            Statement::from(pg::answer_rows()),
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
    let mut wanted: BTreeSet<_> = BTreeSet::new();
    wanted.extend(rows.iter().map(|(_, slug)| slug.clone()));
    while !wanted.is_empty() {
        let edges: Vec<(String, Option<String>)> = store
            .bounded_read(Statement::from(pg::parents()), json!({"slugs":wanted}))
            .map_err(recovery_error)?;
        tree.extend(edges.iter().cloned());
        wanted.clear();
        wanted.extend(
            (edges.into_iter())
                .filter_map(|(_, parent)| parent)
                .filter(|parent| !tree.contains_key(parent)),
        );
    }
    let mut parents: Vec<_> = tree.into_iter().collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "a busy board path keeps std's in-place sort (crate::order): PULL's unanswered posts"
    )]
    parents.sort_unstable();
    Ok(parents)
}
