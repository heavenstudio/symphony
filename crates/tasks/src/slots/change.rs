//! What a change from one ledger to the next does to the cap ([`super`]): the owners whose counted
//! open slots it changes, refused past the cap for a start ([`started`]) and at any count for a
//! red's close ([`changed`]), and the reds it relies on or ends ([`reds`]). The write holds each
//! owner's slot counter and each red's counter, so two writes deciding on one count meet there.

use super::{Entry, Full, Held, Ledger};
use std::collections::{BTreeMap, BTreeSet, HashMap};

impl<'a> Ledger<'a> {
    /// The open reds that make `slug` red work ([`Ledger::red`]), nearest first.
    pub(super) fn exempting(&self, slug: &'a str) -> impl Iterator<Item = &'a str> + '_ {
        // Exemption follows the actual repair subtree, not the shared feature key: a red child
        // must never exempt its ordinary parent or siblings.
        let chain = self.chain(self.reviewed(slug)).into_iter();
        chain.filter(|at| self.tasks[at].red && crate::red::open(self.tasks[at].status))
    }

    /// The ledger once `slug` is no red, its mark lifted ([`crate::Store::unmark_red`]): it and the
    /// open work under it count as any work does.
    pub fn unmarked(&self, slug: &str) -> Ledger<'a> {
        let mut after = self.clone();
        if let Some(task) = after.tasks.get_mut(slug) {
            task.red = false;
        }
        after
    }
}

/// What a change from `before` to `after` does under `cap`: the owners whose counted open slots it
/// changes, each of which the write holds the slot counter of, by handle. Refused, naming the first
/// such owner and what it holds before, when it raises an owner's open slots past `cap`: one at the
/// cap, or above it, starts nothing new. Red work and work in review count in neither, so red work
/// starts at any count, and so does a move into review.
pub fn started<'a>(
    before: &Ledger<'a>,
    after: &Ledger<'a>,
    cap: usize,
) -> Result<Vec<&'a str>, Full<'a>> {
    let (mut was, now) = (before.holdings(), after.holdings());
    let changed = differing(&was, &now);
    for &owner in &changed {
        let (held, next) = (was.get(owner), now.get(owner));
        let (held, next) = (held.map_or(0, BTreeMap::len), next.map_or(0, BTreeMap::len));
        if next > held && next > cap {
            let held = was.remove(owner).unwrap_or_default();
            let red = before.reds().remove(owner).unwrap_or_default();
            let review = before.reviews().remove(owner).unwrap_or_default();
            return Err(Full {
                owner,
                held,
                red,
                review,
            });
        }
    }
    Ok(changed)
}

/// The owners whose counted open slots a change from `before` to `after` changes, by handle, at
/// any count: what [`started`] holds, for a change the cap never refuses, a red's close. Closing an
/// open red raises its owners' counts by the open work under it, which counts from then on, so
/// the close holds their slot counters, and a start decided on a count it read before the close is
/// decided again.
pub fn changed<'a>(before: &Ledger<'a>, after: &Ledger<'a>) -> Vec<&'a str> {
    differing(&before.holdings(), &after.holdings())
}

/// The owners whose open slots differ between `was` and `now`, in order.
fn differing<'a>(
    was: &HashMap<&'a str, Held<'a>>,
    now: &HashMap<&'a str, Held<'a>>,
) -> Vec<&'a str> {
    let mut owners: Vec<&'a str> = was.keys().chain(now.keys()).copied().collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "a busy board path keeps std's in-place sort (crate::order): PULL's slot ledger"
    )]
    owners.sort_unstable();
    owners.dedup();
    owners.retain(|owner| was.get(owner) != now.get(owner));
    owners
}

/// The reds a change from `before` to `after` relies on or ends, in order: a red it ends or opens
/// (its close, or its mark lifted), and each open red that, before or after it, makes red work of
/// an open task whose record, place or red work it changes. The write holds each one's counter
/// (`counter:['red', SLUG]`), so a red's end and a start that took no slot under it meet there,
/// as two starts under one orchestrator meet at its slot counter: the one that loses reads and
/// decides again, the start then counted.
pub fn reds<'a>(before: &Ledger<'a>, after: &Ledger<'a>) -> Vec<&'a str> {
    let mut reds = BTreeSet::new();
    let open = crate::red::open;
    let exempts = |task: &Entry<'_>| task.red && open(task.status);
    for (&slug, now) in &after.tasks {
        let was = before.tasks.get(slug);
        if was.is_some_and(|was| was.red && exempts(was) != exempts(now)) {
            reds.insert(slug);
        }
        let same = was == Some(now)
            && before.place(slug) == after.place(slug)
            && before.red(slug) == after.red(slug);
        if !same && (now.open() || was.is_some_and(Entry::open)) {
            reds.extend(before.exempting(slug).chain(after.exempting(slug)));
        }
    }
    reds.into_iter().collect()
}
