//! Recorded role holds for historical addresses. Unlike `holds`, these include orchestration,
//! end at closure and retain exact boundary ids and clocks instead of normalising timestamps.
//! No session or current role row is evidence here.

use super::{CLOSED, EVERY_ROLE, Event, JOINED, Line, RELEASED, event};
use std::collections::{BTreeMap, BTreeSet};

/// One recorded role stretch. Regressing clocks keep their recorded values and mark the stretch
/// uncertain, so a consumer cannot turn them into a definite duration or historical endpoint.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub by: String,
    pub role: String,
    pub from: f64,
    pub from_id: i64,
    pub to: Option<f64>,
    pub to_id: Option<i64>,
    pub uncertain: bool,
}

/// What the recorded lines prove, independent of today's task and session rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Replay {
    pub spans: Vec<Span>,
    pub closed: bool,
    /// Conflicting duplicates or non-finite clocks make the source unsuitable for resolution.
    pub uncertain: bool,
    /// Recorded closure/reopening boundaries in id order: `(id, exact time, closed)`.
    pub edges: Vec<(i64, f64, bool)>,
}

fn role(word: &str) -> bool {
    matches!(word, "orchestrator" | "executor" | "reviewer")
}

fn handle(word: &str) -> bool {
    !word.is_empty() && !word.chars().any(char::is_whitespace)
}

impl Replay {
    /// Whether the recorded task was closed at this message id.
    pub fn closed_at(&self, id: i64) -> bool {
        self.edges
            .iter()
            .take_while(|(at, _, _)| *at <= id)
            .last()
            .is_some_and(|(_, _, closed)| *closed)
    }
    fn end(&mut self, by: Option<&str>, role: Option<&str>, line: &Line) {
        for span in self.spans.iter_mut().filter(|span| {
            span.to_id.is_none()
                && by.is_none_or(|by| span.by == by)
                && role.is_none_or(|role| span.role == role)
        }) {
            span.to = Some(line.ts);
            span.to_id = Some(line.id);
            span.uncertain |= line.ts < span.from;
        }
    }

    fn start(&mut self, by: &str, role: &str, line: &Line, uncertain: bool) {
        if !self.closed
            && !self
                .spans
                .iter()
                .any(|span| span.to_id.is_none() && span.by == by && span.role == role)
        {
            self.spans.push(Span {
                by: by.into(),
                role: role.into(),
                from: line.ts,
                from_id: line.id,
                to: None,
                to_id: None,
                uncertain,
            });
        }
    }
}

/// Replay unique ids in write order. Duplicate joins do not split holds. A move only ends the
/// holders it explicitly names; several known holders remain several. Closure suspends held roles
/// until a recorded reopening starts another stretch. A missing assignment supplies no holder.
pub fn replay(lines: &[Line]) -> Replay {
    let mut result = Replay::default();
    let mut ordered = BTreeMap::new();
    for line in lines {
        if let Some(previous) = ordered.insert(line.id, line) {
            result.uncertain |= previous != line;
        }
    }
    let mut held: BTreeSet<(String, String)> = BTreeSet::new();
    let mut latest = f64::NEG_INFINITY;
    for line in ordered.values().copied() {
        if !line.ts.is_finite() {
            result.uncertain = true;
            continue;
        }
        let backwards = line.ts < latest;
        if backwards {
            for span in result.spans.iter_mut().filter(|span| span.to_id.is_none()) {
                span.uncertain = true;
            }
        }
        latest = latest.max(line.ts);
        let read = event(line);
        if let Some(Event::Move { from, to }) = read.as_ref()
            && CLOSED.contains(from)
            && !CLOSED.contains(to)
        {
            result.closed = false;
            result.edges.push((line.id, line.ts, false));
            for (by, role) in &held {
                result.start(by, role, line, backwards);
            }
        }
        if matches!(read, Some(Event::Close { .. }))
            || matches!(read, Some(Event::Move { to, .. }) if CLOSED.contains(&to))
        {
            result.end(None, None, line);
            result.closed = true;
            result.edges.push((line.id, line.ts, true));
        }
        let mut starts = Vec::new();
        let mut ends = Vec::new();
        if let Some(Event::Claim { by, handed }) = read {
            ends.extend(handed.into_iter().map(|by| (by, Some("executor"))));
            starts.push((by, "executor"));
        }
        if line.kind == "system" {
            if let Some((by, word)) = line
                .text
                .split_once(JOINED)
                .filter(|(by, word)| handle(by) && role(word))
            {
                starts.push((by, word));
            }
            if let Some((by, rest)) = line.text.split_once(RELEASED).filter(|(by, _)| handle(by)) {
                let word = rest.split_once(" (").map_or(rest, |(word, _)| word);
                if word == EVERY_ROLE {
                    ends.push((by, None));
                } else if role(word) {
                    ends.push((by, Some(word)));
                }
            }
            if let Some((from, to)) = crate::orchestration::moved(&line.text)
                .filter(|(from, to)| handle(to) && from.iter().all(|by| handle(by)))
            {
                ends.extend(from.into_iter().map(|by| (by, Some("orchestrator"))));
                starts.push((to, "orchestrator"));
            }
        }
        for (by, role) in ends {
            result.end(Some(by), role, line);
            held.retain(|(holder, word)| holder != by || role.is_some_and(|role| word != role));
        }
        for (by, role) in starts {
            if !handle(by) {
                continue;
            }
            held.insert((by.into(), role.into()));
            result.start(by, role, line, backwards);
        }
    }
    if result.uncertain {
        for span in &mut result.spans {
            span.uncertain = true;
        }
    }
    result
}

#[cfg(test)]
mod tests;
