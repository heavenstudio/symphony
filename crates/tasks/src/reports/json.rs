//! The day file's text ([`symphony_api::reports::ReportDay`]), written by hand: compact JSON on one
//! line, each field in its order and each value as serde writes it, which the test holds byte for
//! byte to serde's own. Serde's writer costs the server's binary code of its own for each kind of
//! part of the file (docs/CHECKS.md, size budgets); this is a function a part, each string and
//! number still written by serde_json.

use serde::Serialize;
use symphony_api::reports::{
    Answer, ByStatus, DayFlow, Decisions, Flow, Red, ReportDay, ReportHour, Wait, Waits,
};

/// The text of `day`.
pub fn day(day: &ReportDay) -> Vec<u8> {
    let mut out = Out(Vec::with_capacity(16 * 1024));
    out.open();
    out.field("schema", &day.schema);
    out.field("project", day.project.as_str());
    out.field("day", day.day.as_str());
    out.field("start", &day.start);
    out.field("read", &day.read);
    out.field("until", &day.until);
    out.field("closed", &day.closed);
    out.list("hours", &day.hours, hour);
    day_flow(&mut out, "top", &day.top);
    day_flow(&mut out, "subtasks", &day.subtasks);
    out.list("rounds", &day.rounds, |out, round| {
        out.open();
        out.field("number", &round.number);
        out.field("changes", &round.changes);
        out.field("approved", &round.approved);
        out.close();
    });
    out.list("reds", &day.reds, red);
    out.key("decisions");
    match &day.decisions {
        Some(cards) => decisions(&mut out, cards),
        None => out.null(),
    }
    out.key("git");
    match &day.git {
        Some(git) => {
            out.open();
            out.field("branch", git.branch.as_str());
            out.list("excluded", &git.excluded, |out, path| out.value(path.as_str()));
            out.close();
        }
        None => out.null(),
    }
    out.close();
    out.0
}

/// A text under way: a key or an item that follows another in its object or list gets a comma.
struct Out(Vec<u8>);

impl Out {
    fn open(&mut self) {
        self.0.push(b'{');
    }

    fn close(&mut self) {
        self.0.push(b'}');
    }

    fn null(&mut self) {
        self.0.extend_from_slice(b"null");
    }

    fn comma(&mut self) {
        if !matches!(self.0.last(), None | Some(b'{' | b'[')) {
            self.0.push(b',');
        }
    }

    /// A key, a name of the wire types' own that needs no escape.
    fn key(&mut self, name: &str) {
        self.comma();
        self.0.push(b'"');
        self.0.extend_from_slice(name.as_bytes());
        self.0.extend_from_slice(b"\":");
    }

    /// A string, a number, a flag or none, as serde_json writes it.
    fn value<T: Serialize + ?Sized>(&mut self, value: &T) {
        // Into memory: it cannot fail.
        let _ = value.serialize(&mut serde_json::Serializer::new(&mut self.0));
    }

    fn field<T: Serialize + ?Sized>(&mut self, name: &str, value: &T) {
        self.key(name);
        self.value(value);
    }

    fn list<T>(&mut self, name: &str, items: &[T], item: fn(&mut Out, &T)) {
        self.key(name);
        self.0.push(b'[');
        for each in items {
            self.comma();
            item(self, each);
        }
        self.0.push(b']');
    }
}

fn hour(out: &mut Out, hour: &ReportHour) {
    out.open();
    out.field("start", &hour.start);
    flow(out, "top", &hour.top);
    flow(out, "subtasks", &hour.subtasks);
    out.field("landed", &hour.landed);
    out.field("reds_filed", &hour.reds_filed);
    out.field("reds_closed", &hour.reds_closed);
    waits(out, &hour.waits);
    out.key("decisions");
    match hour.decisions {
        Some(counts) => {
            out.open();
            out.field("open", &counts.open);
            out.field("answered", &counts.answered);
            out.field("auto_approved", &counts.auto_approved);
            out.field("withdrawn", &counts.withdrawn);
            out.close();
        }
        None => out.null(),
    }
    out.key("git");
    match hour.git {
        Some(git) => {
            out.open();
            out.field("commits", &git.commits);
            out.field("merges", &git.merges);
            out.field("added", &git.added);
            out.field("removed", &git.removed);
            out.field("reverts", &git.reverts);
            out.close();
        }
        None => out.null(),
    }
    out.close();
}

fn flow(out: &mut Out, name: &str, flow: &Flow) {
    out.key(name);
    out.open();
    by_status(out, "held", &flow.held);
    by_status(out, "entered", &flow.entered);
    by_status(out, "task_hours", &flow.task_hours);
    out.field("unclaimed_hours", &flow.unclaimed_hours);
    out.field("bounces", &flow.bounces);
    out.close();
}

/// The statuses whose figure is not zero, as [`ByStatus`] writes them.
fn by_status<T: Serialize + Default + PartialEq>(out: &mut Out, name: &str, figures: &ByStatus<T>) {
    out.key(name);
    out.open();
    for (status, figure) in figures.each() {
        if *figure != T::default() {
            out.field(status, figure);
        }
    }
    out.close();
}

fn waits(out: &mut Out, waits: &Waits) {
    out.key("waits");
    out.open();
    out.list("approved_to_landed", &waits.approved_to_landed, wait);
    out.list("review_to_reviewer", &waits.review_to_reviewer, wait);
    out.list("changes_to_resumed", &waits.changes_to_resumed, wait);
    out.list("changes_to_review", &waits.changes_to_review, wait);
    out.close();
}

fn wait(out: &mut Out, wait: &Wait) {
    out.open();
    out.field("slug", wait.slug.as_str());
    out.field("seconds", &wait.seconds);
    out.close();
}

fn day_flow(out: &mut Out, name: &str, flow: &DayFlow) {
    out.key(name);
    out.open();
    out.key("lead");
    match flow.lead {
        Some(lead) => {
            out.open();
            out.field("n", &lead.n);
            out.field("median", &lead.median);
            out.field("p90", &lead.p90);
            out.close();
        }
        None => out.null(),
    }
    out.list("longest", &flow.longest, |out, sitting| {
        out.open();
        out.field("slug", sitting.slug.as_str());
        out.field("status", sitting.status.as_str());
        out.field("since", &sitting.since);
        out.close();
    });
    out.close();
}

fn red(out: &mut Out, red: &Red) {
    out.open();
    out.field("slug", red.slug.as_str());
    out.field("series", red.series.as_str());
    out.field("filed", &red.filed);
    out.field("closed", &red.closed);
    out.field("status", red.status.as_str());
    out.close();
}

fn decisions(out: &mut Out, cards: &Decisions) {
    out.open();
    out.list("open", &cards.open, |out, card| {
        out.open();
        out.field("doc", card.doc.as_str());
        out.field("slug", card.slug.as_str());
        out.field("since", &card.since);
        out.close();
    });
    out.list("answered", &cards.answered, answer);
    out.list("auto_approved", &cards.auto_approved, answer);
    out.list("withdrawn", &cards.withdrawn, answer);
    out.close();
}

fn answer(out: &mut Out, answer: &Answer) {
    out.open();
    out.field("doc", answer.doc.as_str());
    out.field("slug", answer.slug.as_str());
    out.field("at", &answer.at);
    out.field("how", answer.how.as_str());
    out.close();
}
