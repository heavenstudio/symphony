//! Emit newly actionable PULL entries, retaining the identity of an announced unanswered post.

use crate::cli::pull::Digest;
use std::collections::BTreeSet;

pub(super) fn emit_new(
    digest: &Digest,
    known: &mut BTreeSet<(String, String)>,
    emit: &mut dyn FnMut(&str),
) {
    let entries = digest.entries();
    known.retain(|entry| entry.0 == "unanswered" || entries.contains(entry));
    if !entries.is_subset(known) {
        for line in digest.event_lines_since(known) {
            emit(&line);
        }
        known.extend(entries);
    }
}
