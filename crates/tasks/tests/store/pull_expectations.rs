//! Exact initial PULL envelopes shared by callback scenarios; numeric oracles live in low_volume.

use super::super::callbacks::Listener;
use super::super::common::Fixture;

pub(super) fn callback_state(f: &Fixture, handle: &str) -> serde_json::Value {
    f.store.raw().row("callback_state", handle).get().unwrap()
}

pub(super) fn listen_orchestrator(f: &Fixture, handle: &str, initial_pull: &str) -> Listener {
    let listener = super::listen(f, handle);
    let count = f
        .store
        .slot_holdings()
        .unwrap()
        .get(handle)
        .map_or(0, std::collections::BTreeSet::len);
    assert_eq!(listener.text(), with_volume(handle, count, initial_pull));
    listener
}

pub(in super::super) fn with_volume(handle: &str, count: usize, pull: &str) -> String {
    if count >= 4 {
        return pull.to_owned();
    }
    let (header, rows) = pull.split_once('\n').unwrap_or((pull, ""));
    let low = format!(
        "low_volume {handle} open={count} threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
    );
    [header, low.as_str(), rows]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
