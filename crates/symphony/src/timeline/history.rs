//! Bounded historical addresses. The existing timeline/gates replay keeps its own semantics; this
//! projection reads capped windows and never consults current role or session holders.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use symphony_api::timeline::{
    EndpointState, History, HistorySource, HistoryState, MessageEndpoint, RoleEndpoint, RoleSpan,
    TIMELINE_HISTORY_SOURCES, TimelineMessage,
};
use symphony_board::stages::{Line, roles};
use symphony_board::{Message, Store, Task, ThreadWindow};

#[derive(Clone)]
struct Source {
    window: ThreadWindow,
    replay: roles::Replay,
}

impl Source {
    fn new(mut window: ThreadWindow) -> Self {
        let lines: Vec<_> = window.lines.iter().map(Line::from).collect();
        let mut replay = roles::replay(&lines);
        symphony_board::order::sort_by_key(&mut window.lines, &|line| line.id);
        window.lines.dedup_by_key(|line| line.id);
        replay.uncertain |= window.uncertain;
        replay.uncertain |= window.lines.iter().any(|line| {
            line.kind == "system"
                && line.text.contains(symphony_board::alarm::UNDER)
                && parent_move(line).is_none()
        });
        // A parent change missing from the thread has no known time or historical destination.
        let mut recorded_parent: Option<Option<&str>> = None;
        for moved in window.lines.iter().filter_map(parent_move) {
            replay.uncertain |= recorded_parent.is_some_and(|previous| previous != moved.from);
            recorded_parent = Some(moved.to);
        }
        replay.uncertain |=
            recorded_parent.is_some_and(|parent| parent != window.parent.as_deref());
        // A missing close or reopening supplies no invented boundary in either direction.
        replay.uncertain |= matches!(window.status.as_str(), "done" | "cancelled") != replay.closed;
        Self { window, replay }
    }

    fn complete(&self) -> bool {
        !self.window.more && !self.replay.uncertain
    }

    fn closed_before(&self, id: i64) -> Option<bool> {
        if self.replay.uncertain {
            return None;
        }
        self.replay
            .edges
            .iter()
            .take_while(|(edge, _, _)| *edge < id)
            .last()
            .map(|(_, _, closed)| *closed)
            .or_else(|| self.complete().then_some(false))
    }

    fn parents(&self) -> Vec<String> {
        let mut parents: BTreeSet<String> = BTreeSet::new();
        parents.extend(self.window.parent.iter().cloned());
        for message in &self.window.lines {
            if let Some(moved) = parent_move(message) {
                parents.extend(moved.from.map(str::to_owned));
                parents.extend(moved.to.map(str::to_owned));
            }
        }
        parents.into_iter().collect()
    }

    fn parent_at(&self, id: i64) -> Option<&str> {
        self.window
            .lines
            .iter()
            .rev()
            .filter(|line| line.id <= id)
            .find_map(|line| parent_move(line).map(|moved| moved.to))
            .unwrap_or_else(|| {
                self.window
                    .lines
                    .iter()
                    .find_map(|line| parent_move(line).map(|moved| moved.from))
                    .unwrap_or(self.window.parent.as_deref())
            })
    }
}

fn parent_move(message: &Message) -> Option<symphony_board::alarm::Moved<'_>> {
    // Use the board's tree-line vocabulary, requiring the recorded previous parent too.
    let text = message.text.as_str();
    if message.kind != "system" {
        return None;
    }
    let (by, rest) = text.split_once(symphony_board::alarm::UNDER)?;
    if by.is_empty() || by.contains(' ') {
        return None;
    }
    // Older lines without a recorded source do not prove what the previous parent was.
    let (to, from) = rest
        .strip_suffix(')')?
        .split_once(symphony_board::alarm::WAS)?;
    let parent = |word| (word != symphony_board::alarm::NOTHING).then_some(word);
    Some(symphony_board::alarm::Moved {
        to: parent(to),
        from: parent(from),
    })
}

/// At most eight source tasks, at most one capped range read for each. A failed, missing, cyclic
/// or deeper ancestor cannot supply a holder; the original address remains available.
pub(super) fn answer(store: &Store, record: &Task) -> History {
    load(&record.slug, |slug| {
        store.thread_window(slug, None).map(Source::new)
    })
}

fn load(
    slug: &str,
    mut fetch: impl FnMut(&str) -> Result<Source, symphony_board::Error>,
) -> History {
    let mut sources = BTreeMap::new();
    let mut metadata = Vec::new();
    let mut pending = VecDeque::from([slug.to_string()]);
    let mut seen = BTreeSet::new();
    let mut sources_complete = true;
    while let Some(reading) = pending.pop_front() {
        if !seen.insert(reading.clone()) {
            continue;
        }
        if metadata.len() >= TIMELINE_HISTORY_SOURCES {
            sources_complete = false;
            break;
        }
        match fetch(&reading) {
            Ok(source) => {
                pending.extend(
                    source
                        .parents()
                        .into_iter()
                        .filter(|slug| !seen.contains(slug)),
                );
                metadata.push(HistorySource {
                    slug: reading.clone(),
                    state: if source.window.more || source.replay.uncertain {
                        HistoryState::Partial
                    } else {
                        HistoryState::Complete
                    },
                    first_id: source.window.lines.first().map(|line| line.id),
                    last_id: source.window.lines.last().map(|line| line.id),
                    before: source
                        .window
                        .lines
                        .first()
                        .filter(|_| source.window.more)
                        .map(|line| line.id),
                });
                sources.insert(reading, source);
            }
            Err(_) => {
                sources_complete = false;
                metadata.push(HistorySource {
                    slug: reading,
                    ..HistorySource::default()
                });
            }
        }
    }
    History {
        sources_complete,
        ..project(slug, metadata, &sources)
    }
}

fn active(span: &roles::Span, message: &Message, at_send: bool) -> bool {
    span.from_id <= message.id
        && span
            .to_id
            .is_none_or(|end| message.id < end || (at_send && message.id == end))
}

/// Candidates for a role, with whether the set is proved exhaustive. An incomplete nearer
/// source stops inheritance: it may be hiding a local assignment.
fn holders(
    sources: &BTreeMap<String, Source>,
    slug: &str,
    role: &str,
    message: &Message,
    at_send: bool,
    seen: &mut BTreeSet<String>,
) -> (Vec<RoleEndpoint>, bool) {
    if !seen.insert(slug.into()) {
        return (Vec::new(), false);
    }
    let Some(source) = sources.get(slug) else {
        return (Vec::new(), false);
    };
    let in_range = source
        .window
        .lines
        .first()
        .is_none_or(|first| message.id >= first.id)
        || !source.window.more;
    let spans: Vec<_> = source
        .replay
        .spans
        .iter()
        .filter(|span| span.role == role && active(span, message, at_send))
        .collect();
    let reliable = source.complete()
        && in_range
        && spans.iter().all(|span| {
            !span.uncertain && span.from <= message.ts && span.to.is_none_or(|to| message.ts <= to)
        });
    let found: Vec<_> = spans
        .into_iter()
        .map(|span| RoleEndpoint {
            role: role.into(),
            by: span.by.clone(),
            source: slug.into(),
        })
        .collect();
    if !found.is_empty()
        || !reliable
        || role != "orchestrator"
        || source.replay.closed_at(if at_send {
            message.id.saturating_sub(1)
        } else {
            message.id
        })
    {
        return (found, reliable);
    }
    source
        .parent_at(message.id)
        .map_or((Vec::new(), true), |parent| {
            holders(sources, parent, role, message, at_send, seen)
        })
}

fn endpoint(
    sources: &BTreeMap<String, Source>,
    slug: &str,
    message: &Message,
    address: Option<&str>,
) -> MessageEndpoint {
    let Some(address) = address else {
        return MessageEndpoint {
            state: EndpointState::Everyone,
            ..MessageEndpoint::default()
        };
    };
    if matches!(address, "owner" | "board") {
        return MessageEndpoint {
            state: EndpointState::Outside,
            ..MessageEndpoint::default()
        };
    }
    let named_role = address
        .strip_prefix('@')
        .filter(|role| matches!(*role, "orchestrator" | "executor" | "reviewer"));
    let mut candidates = Vec::new();
    let mut reliable = true;
    for role in ["orchestrator", "executor", "reviewer"] {
        if named_role.is_some_and(|named| named != role) {
            continue;
        }
        let (found, complete) = holders(sources, slug, role, message, true, &mut BTreeSet::new());
        reliable &= complete;
        candidates.extend(
            found
                .into_iter()
                .filter(|found| named_role.is_some() || found.by == address),
        );
    }
    let state = if candidates.len() > 1 {
        EndpointState::Ambiguous
    } else if candidates.len() == 1 && reliable {
        EndpointState::Known
    } else {
        EndpointState::Unknown
    };
    MessageEndpoint { state, candidates }
}

fn project(
    slug: &str,
    metadata: Vec<HistorySource>,
    sources: &BTreeMap<String, Source>,
) -> History {
    let mut history = History {
        sources: metadata,
        ..History::default()
    };
    if let Some(source) = sources.get(slug) {
        history.roles.extend(
            source
                .replay
                .spans
                .iter()
                .filter(|span| span.role != "orchestrator")
                .map(|span| RoleSpan {
                    role: span.role.clone(),
                    by: span.by.clone(),
                    source: slug.into(),
                    from: span.from,
                    from_id: span.from_id,
                    to: span.to,
                    to_id: span.to_id,
                    uncertain: span.uncertain || source.replay.uncertain,
                }),
        );
        history.roles.extend(orchestrators(slug, sources));
        let mut seen = BTreeSet::new();
        for message in source
            .window
            .lines
            .iter()
            .filter(|line| line.kind != "system" && seen.insert(line.id))
        {
            history.messages.push(TimelineMessage {
                id: message.id,
                at: message.ts,
                sender: message.sender.clone(),
                recipient: message.recipient.clone(),
                kind: message.kind.clone(),
                // A person a board named, by the board's mark of the line, holds no lane of the
                // task's, as the owner does not.
                from: if source.window.by_person.contains(&message.id) {
                    endpoint(sources, slug, message, Some("owner"))
                } else {
                    endpoint(sources, slug, message, Some(&message.sender))
                },
                to: endpoint(sources, slug, message, message.recipient.as_deref()),
                after_close: source.closed_before(message.id),
            });
        }
        symphony_board::order::sort_by(&mut history.messages, &|a, b| {
            a.at.total_cmp(&b.at).then(a.id.cmp(&b.id))
        });
    }
    history
}

/// Effective orchestration on this task, changing only at recorded boundaries. A parent's span
/// begins on the child's recorded creation or reparenting, never at an invented assignment time.
fn orchestrators(slug: &str, sources: &BTreeMap<String, Source>) -> Vec<RoleSpan> {
    let Some(local) = sources.get(slug) else {
        return Vec::new();
    };
    let first = local.window.lines.first().map_or(i64::MAX, |line| line.id);
    let mut events = BTreeMap::new();
    let mut boundaries = BTreeSet::new();
    for (source_slug, source) in sources {
        for span in source
            .replay
            .spans
            .iter()
            .filter(|span| span.role == "orchestrator")
        {
            boundaries.insert(span.from_id);
            boundaries.extend(span.to_id);
        }
        boundaries.extend(source.replay.edges.iter().map(|(id, _, _)| *id));
        for line in &source.window.lines {
            if parent_move(line).is_some()
                || (source_slug == slug
                    && line.kind == "system"
                    && line.text.starts_with(symphony_board::stages::CREATED))
            {
                boundaries.insert(line.id);
            }
            if line.id >= first {
                events.insert(line.id, line);
            }
        }
    }
    let mut spans: Vec<RoleSpan> = Vec::new();
    for event in events.values() {
        let (found, reliable) = holders(
            sources,
            slug,
            "orchestrator",
            event,
            false,
            &mut BTreeSet::new(),
        );
        for span in spans.iter_mut().filter(|span| span.to_id.is_none()) {
            if !found
                .iter()
                .any(|found| found.by == span.by && found.source == span.source)
            {
                span.to = Some(event.ts);
                span.to_id = Some(event.id);
            }
            span.uncertain |= event.ts < span.from;
            span.uncertain |= !reliable;
        }
        for found in found {
            if !spans.iter().any(|span| {
                span.to_id.is_none() && span.by == found.by && span.source == found.source
            }) {
                spans.push(RoleSpan {
                    role: found.role,
                    by: found.by,
                    source: found.source,
                    from: event.ts,
                    from_id: event.id,
                    to: None,
                    to_id: None,
                    // An ordinary first observation cannot prove an assignment's start.
                    uncertain: !reliable || !boundaries.contains(&event.id),
                });
            }
        }
    }
    spans
}

#[cfg(test)]
mod tests;
