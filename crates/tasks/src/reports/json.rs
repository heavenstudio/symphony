//! The day file's text ([`symphony_api::reports::ReportDay`]), written by hand: compact JSON on one
//! line, each field in its order and each value as serde writes it, which the test holds byte for
//! byte to serde's own. Serde's writer costs the server's binary code of its own for each kind of
//! part of the file (docs/workflow/CHECKS.md, size budgets); here each field is one call to a writer of its
//! kind of value, each kept out of line, and serde_json still writes each string and number.

use serde::Serialize;
use symphony_api::reports::{
    Answer, ByStatus, DayFlow, Decisions, Flow, PlacedHours, Red, ReportDay, ReportHour, Spent,
    TokenSum, Wait, Waits,
};

/// The text of `day`.
pub fn day(day: &ReportDay) -> Vec<u8> {
    let mut out = Out(Vec::with_capacity(16 * 1024));
    out.open();
    out.count("schema", day.schema.into());
    out.text("project", &day.project);
    out.text("day", &day.day);
    out.number("start", day.start);
    out.number("read", day.read);
    out.number("until", day.until);
    out.key("closed");
    out.value(&day.closed);
    out.list("hours", &day.hours, hour);
    day_flow(&mut out, "top", &day.top);
    day_flow(&mut out, "subtasks", &day.subtasks);
    out.list("rounds", &day.rounds, |out, round| {
        out.open();
        out.count("number", round.number.into());
        out.count("changes", round.changes.into());
        out.count("approved", round.approved.into());
        out.close();
    });
    out.list("reds", &day.reds, red);
    match &day.decisions {
        Some(cards) => decisions(&mut out, cards),
        None => out.null("decisions"),
    }
    match &day.git {
        Some(git) => {
            out.object("git");
            out.text("branch", &git.branch);
            out.list("excluded", &git.excluded, |out, path| {
                out.value(path.as_str())
            });
            out.close();
        }
        None => out.null("git"),
    }
    sums(&mut out, "usage", day.usage.as_deref());
    out.list("landed", &day.landed, |out, landed| {
        out.open();
        out.text("slug", &landed.slug);
        out.number("at", landed.at);
        out.list("spent", &landed.spent, sum);
        // Each left out where there is none, as serde leaves it out, so a file of tasks with
        // neither is written as it was before they were kept.
        if let Some(kind) = &landed.kind {
            out.text("kind", kind);
        }
        if let Some(lines) = &landed.lines {
            out.object("lines");
            out.count("added", lines.added);
            out.count("removed", lines.removed);
            out.close();
        }
        out.close();
    });
    out.close();
    out.0
}

/// A list of sums of tokens, or none.
fn sums(out: &mut Out, name: &str, sums: Option<&[TokenSum]>) {
    match sums {
        Some(sums) => out.list(name, sums, sum),
        None => out.null(name),
    }
}

fn sum(out: &mut Out, sum: &TokenSum) {
    out.open();
    out.text("role", &sum.role);
    out.count("round", sum.round.into());
    out.text("model", &sum.model);
    out.text("speed", &sum.speed);
    out.count("input", sum.input);
    out.count("cache_write_5m", sum.cache_write_5m);
    out.count("cache_write_1h", sum.cache_write_1h);
    out.count("cache_read", sum.cache_read);
    out.count("output", sum.output);
    out.count("calls", sum.calls);
    out.close();
}

/// A text under way: a key or an item that follows another in its object or list gets a comma.
struct Out(Vec<u8>);

impl Out {
    fn comma(&mut self) {
        if !matches!(self.0.last(), None | Some(b'{' | b'[')) {
            self.0.push(b',');
        }
    }

    /// A key: a name of the wire types' own, which needs no escape.
    #[inline(never)]
    fn key(&mut self, name: &str) {
        self.comma();
        self.0.push(b'"');
        self.0.extend_from_slice(name.as_bytes());
        self.0.extend_from_slice(b"\":");
    }

    /// A string, a number, a flag or none, as serde_json writes it.
    #[inline(never)]
    fn value<T: Serialize + ?Sized>(&mut self, value: &T) {
        // Into memory: it cannot fail.
        let _ = value.serialize(&mut serde_json::Serializer::new(&mut self.0));
    }

    #[inline(never)]
    fn text(&mut self, name: &str, value: &str) {
        self.key(name);
        self.value(value);
    }

    #[inline(never)]
    fn number(&mut self, name: &str, value: f64) {
        self.key(name);
        self.value(&value);
    }

    #[inline(never)]
    fn count(&mut self, name: &str, value: u64) {
        self.key(name);
        self.value(&value);
    }

    #[inline(never)]
    fn maybe(&mut self, name: &str, value: Option<f64>) {
        self.key(name);
        self.value(&value);
    }

    fn null(&mut self, name: &str) {
        self.key(name);
        self.0.extend_from_slice(b"null");
    }

    /// An object's key and its opening; the object's writer closes it.
    #[inline(never)]
    fn object(&mut self, name: &str) {
        self.key(name);
        self.open();
    }

    #[inline(never)]
    fn open(&mut self) {
        self.0.push(b'{');
    }

    #[inline(never)]
    fn close(&mut self) {
        self.0.push(b'}');
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
    out.number("start", hour.start);
    flow(out, "top", &hour.top);
    flow(out, "subtasks", &hour.subtasks);
    out.count("landed", hour.landed.into());
    out.count("reds_filed", hour.reds_filed.into());
    out.count("reds_closed", hour.reds_closed.into());
    waits(out, &hour.waits);
    match hour.decisions {
        Some(counts) => {
            out.object("decisions");
            out.count("open", counts.open.into());
            out.count("answered", counts.answered.into());
            out.count("auto_approved", counts.auto_approved.into());
            out.count("withdrawn", counts.withdrawn.into());
            out.close();
        }
        None => out.null("decisions"),
    }
    match hour.git {
        Some(git) => {
            out.object("git");
            out.count("commits", git.commits.into());
            out.count("merges", git.merges.into());
            out.count("added", git.added);
            out.count("removed", git.removed);
            out.count("reverts", git.reverts.into());
            out.close();
        }
        None => out.null("git"),
    }
    sums(out, "tokens", hour.tokens.as_deref());
    // Left out where there is none, as serde leaves it out: a board with no workflow of its own
    // writes the file it wrote before.
    if let Some(placed) = &hour.workflow {
        out.object("workflow");
        placed_hours(out, "top", &placed.top);
        placed_hours(out, "subtasks", &placed.subtasks);
        out.close();
    }
    out.close();
}

fn placed_hours(out: &mut Out, name: &str, hours: &PlacedHours) {
    out.object(name);
    out.list("phases", &hours.phases, spent);
    out.list("states", &hours.states, spent);
    out.close();
}

fn spent(out: &mut Out, spent: &Spent) {
    out.open();
    out.text("id", &spent.id);
    out.text("label", &spent.label);
    out.number("hours", spent.hours);
    out.close();
}

fn flow(out: &mut Out, name: &str, flow: &Flow) {
    out.object(name);
    counts(out, "held", &flow.held);
    counts(out, "entered", &flow.entered);
    match flow.done {
        Some(done) => out.count("done", done.into()),
        None => out.null("done"),
    }
    out.object("task_hours");
    for (status, hours) in flow.task_hours.each() {
        // What [`ByStatus`] leaves out: a zero (a negative one too, as serde compares it).
        if *hours != 0.0 {
            out.number(status, *hours);
        }
    }
    out.close();
    out.number("unclaimed_hours", flow.unclaimed_hours);
    out.count("bounces", flow.bounces.into());
    out.close();
}

/// The statuses whose count is not zero, as [`ByStatus`] writes them.
fn counts(out: &mut Out, name: &str, counts: &ByStatus<u32>) {
    out.object(name);
    for (status, count) in counts.each() {
        if *count != 0 {
            out.count(status, (*count).into());
        }
    }
    out.close();
}

fn waits(out: &mut Out, waits: &Waits) {
    out.object("waits");
    out.list("approved_to_landed", &waits.approved_to_landed, wait);
    out.list("review_to_reviewer", &waits.review_to_reviewer, wait);
    out.list("changes_to_resumed", &waits.changes_to_resumed, wait);
    out.list("changes_to_review", &waits.changes_to_review, wait);
    out.close();
}

fn wait(out: &mut Out, wait: &Wait) {
    out.open();
    out.text("slug", &wait.slug);
    out.number("seconds", wait.seconds);
    out.close();
}

fn day_flow(out: &mut Out, name: &str, flow: &DayFlow) {
    out.object(name);
    match flow.lead {
        Some(lead) => {
            out.object("lead");
            out.count("n", lead.n.into());
            out.number("median", lead.median);
            out.number("p90", lead.p90);
            out.close();
        }
        None => out.null("lead"),
    }
    out.list("longest", &flow.longest, |out, sitting| {
        out.open();
        out.text("slug", &sitting.slug);
        out.text("status", &sitting.status);
        out.number("since", sitting.since);
        out.close();
    });
    out.close();
}

fn red(out: &mut Out, red: &Red) {
    out.open();
    out.text("slug", &red.slug);
    out.text("series", &red.series);
    out.number("filed", red.filed);
    out.maybe("closed", red.closed);
    out.text("status", &red.status);
    match &red.between {
        Some(range) => out.text("between", range),
        None => out.null("between"),
    }
    match &red.landed {
        Some(slugs) => out.list("landed", slugs, |out, slug| out.value(slug.as_str())),
        None => out.null("landed"),
    }
    out.close();
}

fn decisions(out: &mut Out, cards: &Decisions) {
    out.object("decisions");
    out.list("open", &cards.open, |out, card| {
        out.open();
        out.text("doc", &card.doc);
        out.text("slug", &card.slug);
        out.maybe("since", card.since);
        out.close();
    });
    out.list("answered", &cards.answered, answer);
    out.list("auto_approved", &cards.auto_approved, answer);
    out.list("withdrawn", &cards.withdrawn, answer);
    out.close();
}

fn answer(out: &mut Out, answer: &Answer) {
    out.open();
    out.text("doc", &answer.doc);
    out.text("slug", &answer.slug);
    out.number("at", answer.at);
    out.text("how", &answer.how);
    out.close();
}
