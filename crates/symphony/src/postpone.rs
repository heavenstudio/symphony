//! The owner's queue order, kept outside the design documents. The app currently has one
//! owner identity across all devices; the file keeps identities separate for future callers.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use symphony_api::Card;

use crate::plugins::Plugins;
use crate::{Doc, Group, ReviewError, Who};

/// The order's file, in the project's artifacts folder.
pub const FILE: &str = "review-order/postponed.json";

/// Where the order of the project at `root` is kept: [`FILE`] in its artifacts folder.
pub fn order_file(root: &Path) -> PathBuf {
    root.join(crate::project::Project::of(root).artifact(FILE))
}
pub const OWNER: &str = "owner";
pub const DAYS: u64 = 3;
const LIFETIME: u64 = DAYS * 86_400;

#[derive(Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Order {
    owners: BTreeMap<String, BTreeMap<String, Entry>>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    path: String,
    group: String,
    owner_lines: Vec<String>,
    until: u64,
    position: u64,
    remaining: BTreeSet<String>,
}

fn problem(path: &Path, error: impl std::fmt::Display) -> ReviewError {
    ReviewError(format!("{}: {error}", path.display()))
}

/// Read and atomically replace the shared state under a file lock, including across preview
/// processes. A malformed file is reported, never reset.
fn update<T>(root: &Path, change: impl FnOnce(&mut Order) -> T) -> Result<(T, bool), ReviewError> {
    let path = order_file(root);
    let dir = path.parent().expect("the state has a parent");
    fs::create_dir_all(dir).map_err(|e| problem(dir, e))?;
    let lock_path = dir.join("lock");
    let held = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&lock_path)
        .map_err(|e| problem(&lock_path, e))?;
    held.lock().map_err(|e| problem(&lock_path, e))?;
    let mut order: Order = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| problem(&path, e))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Order::default(),
        Err(e) => return Err(problem(&path, e)),
    };
    let before = order.clone();
    let result = change(&mut order);
    order.owners.retain(|_, entries| !entries.is_empty());
    let changed = order != before;
    if changed {
        let next = dir.join(format!("postponed.{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec(&order).expect("queue order serializes");
        fs::write(&next, bytes).map_err(|e| problem(&next, e))?;
        fs::rename(&next, &path).map_err(|e| problem(&path, e))?;
    }
    Ok((result, changed))
}

/// Branch proposals with the same slug are independent. Keep existing main preference keys.
pub(crate) fn key(path: &str, slug: &str) -> String {
    if path.starts_with('@') {
        format!("{path}#{slug}")
    } else {
        slug.into()
    }
}

/// Immutable decision metadata and triage order kept by the app's document watcher.
#[derive(Clone, Default)]
pub struct View {
    cards: BTreeMap<String, Entry>,
    waiting: Vec<Card>,
}

impl View {
    pub(crate) fn new(plugins: &Plugins, docs: &[Doc], waiting: &[Card]) -> Self {
        Self {
            cards: eligible(docs, plugins),
            waiting: waiting.to_vec(),
        }
    }

    /// An inherited card on a branch still uses main's active decision and preference entry.
    pub(crate) fn path_for(&self, path: &str, slug: &str) -> Option<&str> {
        self.cards
            .get(&key(path, slug))
            .filter(|entry| entry.path == path)
            .or_else(|| {
                self.cards
                    .get(slug)
                    .filter(|entry| entry.path == crate::dashboard::document_path(path))
            })
            .map(|entry| entry.path.as_str())
    }

    pub fn act(
        &self,
        root: &Path,
        owner: &str,
        path: &str,
        slug: &str,
        action: &str,
        now: u64,
    ) -> Result<bool, ReviewError> {
        act_with_cards(root, owner, path, slug, action, now, Some(&self.cards))
    }

    pub fn waiting(&self, root: &Path, owner: &str, now: u64) -> Result<Vec<Card>, ReviewError> {
        let mut waiting = self.waiting.clone();
        let positions = apply_cards(root, owner, &self.cards, &mut waiting, now)?;
        // Group-local slots are useful to callers of apply, but the mixed queue combines
        // groups. Restore the owner's global Later order before interleaving categories.
        waiting.sort_by_key(|card| positions.get(&key(&card.doc, &card.slug)).copied());
        Ok(mixed(waiting))
    }
}

/// Round-robin the document folders, oldest first within each. Postponed cards keep their
/// preference order after ordinary cards in their category; timestamp ties retain triage order.
pub(crate) fn mixed(mut waiting: Vec<Card>) -> Vec<Card> {
    waiting.sort_by(|a, b| {
        a.postponed.cmp(&b.postponed).then_with(|| {
            if a.postponed {
                std::cmp::Ordering::Equal
            } else {
                a.posted.cmp(&b.posted)
            }
        })
    });
    let mut categories: BTreeMap<String, std::collections::VecDeque<Card>> = BTreeMap::new();
    for card in waiting {
        categories
            .entry(symphony_api::queue_category(&card.doc).into())
            .or_default()
            .push_back(card);
    }
    let mut mixed = Vec::new();
    while !categories.is_empty() {
        categories.retain(|_, cards| {
            if let Some(card) = cards.pop_front() {
                mixed.push(card);
            }
            !cards.is_empty()
        });
    }
    mixed
}

fn eligible(docs: &[Doc], plugins: &Plugins) -> BTreeMap<String, Entry> {
    let entries = docs.iter().flat_map(|doc| {
        doc.decisions.iter().filter_map(|card| {
            let group = card.group(plugins);
            matches!(group, Group::Decide | Group::Confirm).then(|| {
                (
                    key(&doc.path, &card.slug),
                    Entry {
                        path: doc.path.clone(),
                        group: group.key().into(),
                        owner_lines: card
                            .entries()
                            .into_iter()
                            .filter(|e| e.who == Who::Owner)
                            .map(|e| format!("{:?}", e))
                            .collect(),
                        until: 0,
                        position: 0,
                        remaining: BTreeSet::new(),
                    },
                )
            })
        })
    });
    let mut eligible = BTreeMap::new();
    for (key, entry) in entries {
        eligible.insert(key, entry);
    }
    eligible
}

fn prune(order: &mut Order, cards: &BTreeMap<String, Entry>, now: u64) {
    for entries in order.owners.values_mut() {
        entries.retain(|slug, entry| {
            cards.get(slug).is_some_and(|card| {
                entry.path == card.path
                    && entry.group == card.group
                    && entry.owner_lines == card.owner_lines
            }) && now < entry.until
        });
        for entry in entries.values_mut() {
            entry.remaining.retain(|slug| {
                cards
                    .get(slug)
                    .is_some_and(|card| card.group == entry.group)
            });
        }
        entries.retain(|_, entry| !entry.remaining.is_empty());
    }
}

/// Apply only to slots of the same computed group. Classification and triage remain untouched.
pub fn apply(
    root: &Path,
    owner: &str,
    docs: &[Doc],
    waiting: &mut [Card],
    now: u64,
) -> Result<(), ReviewError> {
    let cards = eligible(docs, &Plugins::of(root).reading(root, docs));
    apply_cards(root, owner, &cards, waiting, now).map(|_| ())
}

fn apply_cards(
    root: &Path,
    owner: &str,
    cards: &BTreeMap<String, Entry>,
    waiting: &mut [Card],
    now: u64,
) -> Result<BTreeMap<String, u64>, ReviewError> {
    if !order_file(root)
        .try_exists()
        .map_err(|e| problem(&order_file(root), e))?
    {
        return Ok(BTreeMap::new());
    }
    let (reordered, _) = update(root, |order| {
        prune(order, cards, now);
        let entries = order.owners.get(owner);
        let mut reordered = waiting.to_vec();
        for group in ["decide", "confirm"] {
            let slots: Vec<_> = reordered
                .iter()
                .enumerate()
                .filter(|(_, card)| {
                    cards
                        .get(&key(&card.doc, &card.slug))
                        .is_some_and(|meta| meta.group == group)
                })
                .map(|(i, _)| i)
                .collect();
            let mut values: Vec<_> = slots.iter().map(|i| reordered[*i].clone()).collect();
            for card in &mut values {
                card.postponed = entries
                    .is_some_and(|entries| entries.contains_key(&key(&card.doc, &card.slug)));
            }
            values.sort_by_key(|card| {
                entries
                    .and_then(|entries| entries.get(&key(&card.doc, &card.slug)))
                    .map_or((false, 0), |entry| (true, entry.position))
            });
            for (slot, card) in slots.into_iter().zip(values) {
                reordered[slot] = card;
            }
        }
        let positions = entries
            .into_iter()
            .flat_map(|entries| entries.iter())
            .map(|(id, entry)| (id.clone(), entry.position))
            .collect();
        (reordered, positions)
    })?;
    waiting.clone_from_slice(&reordered.0);
    Ok(reordered.1)
}

/// Later, Undo, or opening a card. No register write, verdict, callback, or board task is made.
/// Returns whether the queue's visible order or postponed marks changed. Recording a visit
/// that leaves postponed cards in place needs no document scan or snapshot refresh.
pub fn act(
    root: &Path,
    owner: &str,
    path: &str,
    slug: &str,
    action: &str,
    now: u64,
) -> Result<bool, ReviewError> {
    if action == "later" {
        let mut docs = crate::board::scan(root)?;
        let plugins = Plugins::of(root);
        crate::dashboard::merge_decisions(
            &mut docs,
            &crate::branches::branch_cards(root),
            &plugins,
        );
        let cards = eligible(&docs, &plugins.reading(root, &docs));
        act_with_cards(root, owner, path, slug, action, now, Some(&cards))
    } else {
        act_with_cards(root, owner, path, slug, action, now, None)
    }
}

fn act_with_cards(
    root: &Path,
    owner: &str,
    path: &str,
    slug: &str,
    action: &str,
    now: u64,
    cards: Option<&BTreeMap<String, Entry>>,
) -> Result<bool, ReviewError> {
    if !["later", "undo", "seen"].contains(&action) {
        return Err(ReviewError("action must be later, undo, or seen".into()));
    }
    let id = key(path, slug);
    if action != "later" {
        let changed = order_file(root)
            .try_exists()
            .map_err(|e| problem(&order_file(root), e))
            .and_then(|exists| {
                if !exists {
                    return Ok(false);
                }
                update(root, |order| {
                    let Some(entries) = order.owners.get_mut(owner) else {
                        return false;
                    };
                    if action == "undo" {
                        return entries.get(&id).is_some_and(|entry| entry.path == path)
                            && entries.remove(&id).is_some();
                    }
                    let before = entries.len();
                    for entry in entries.values_mut() {
                        entry.remaining.remove(&id);
                    }
                    entries.retain(|_, entry| !entry.remaining.is_empty());
                    entries.len() != before
                })
                .map(|(changed, _)| changed)
            });
        return match changed {
            Ok(changed) => Ok(changed),
            Err(error) if action == "seen" => {
                eprintln!("· could not record card visit {slug}: {error}");
                Ok(false)
            }
            Err(error) => Err(error),
        };
    }
    let cards = cards.expect("Later has a decision snapshot");
    let card = cards
        .get(&id)
        .filter(|card| card.path == path)
        .ok_or_else(|| ReviewError(format!("no card waiting on you at {path}#{slug}")))?;
    update(root, |order| {
        prune(order, cards, now);
        let entries = order.owners.entry(owner.into()).or_default();
        for entry in entries.values_mut() {
            entry.remaining.remove(&id);
        }
        let remaining = cards
            .iter()
            .filter(|(candidate, meta)| {
                *candidate != &id && meta.group == card.group && !entries.contains_key(*candidate)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let position = entries
            .values()
            .map(|entry| entry.position)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        entries.insert(
            id.clone(),
            Entry {
                until: now.saturating_add(LIFETIME),
                position,
                remaining,
                ..card.clone()
            },
        );
        prune(order, cards, now);
    })
    .map(|(_, changed)| changed)
}

/// An owner answer clears the entry immediately, including when the marker stays pending.
/// A branch proposal's id includes its scoped document; main retains the bare slug.
pub fn answered(root: &Path, owner: &str, id: &str) -> Result<(), ReviewError> {
    if !order_file(root).exists() {
        return Ok(());
    }
    update(root, |order| {
        if let Some(entries) = order.owners.get_mut(owner) {
            entries.remove(id);
            for entry in entries.values_mut() {
                entry.remaining.remove(id);
            }
            entries.retain(|_, entry| !entry.remaining.is_empty());
        }
    })
    .map(|_| ())
}

/// Cheap timer check: an actual expiry triggers a refresh of the cached queue.
pub fn expired(root: &Path, now: u64) -> bool {
    fs::read(order_file(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Order>(&bytes).ok())
        .is_some_and(|order| {
            order
                .owners
                .values()
                .flat_map(|entries| entries.values())
                .any(|entry| now >= entry.until)
        })
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod collection_tests {
    use super::*;

    #[test]
    fn eligibility_keeps_lexical_keys_and_the_last_copy_of_a_card() {
        let doc = |text: &str| Doc {
            path: "docs/A.md".into(),
            comments: Vec::new(),
            decisions: crate::register::parse_decisions(&text.lines().collect::<Vec<_>>()),
        };
        let cards = eligible(
            &[
                doc("- [ ] **z** — Last key.\n- [ ] **a** — First copy.\n"),
                doc(
                    "- [?] **a** — Last copy.\n  [2026-10-20 11:00] owner · question — Newer.\n  [2026-10-20 12:00] agent — Answer.\n- [x] **done** — Settled.\n",
                ),
            ],
            &Plugins::default(),
        );
        let keys: Vec<_> = cards.keys().map(String::as_str).collect();
        assert_eq!(keys, ["a", "z"]);
        assert_eq!(cards["a"].owner_lines.len(), 1);
        assert!(cards["a"].owner_lines[0].contains("Newer."));
        assert!(!cards["a"].owner_lines[0].contains("Older."));
        assert!(eligible(&[], &Plugins::default()).is_empty());
    }
}
