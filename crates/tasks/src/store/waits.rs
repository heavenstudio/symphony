//! What a task's landing waits on, as the tool that lands it records it ([`crate::alarm::Waits`]):
//! the board's own field `fields._board.waits`, with the assessment it states beside it
//! (`fields._board.assessed`), written here and not through `set_field`, which is a plugin's; and
//! where the rework a record of changes asked for ended, as the alarm first learnt it
//! (`fields._board.fixed`).

use serde_json::json;

use super::postgres::waits as pg;
use super::statement::Statement;
use super::work::OWNER;
use super::{Error, REFUSED, Store, Task, refused, repr};
use crate::alarm::{ASSESSED, FIXED, Tip, WAITS, WAITS_SET, Waits, Written};
use crate::red::FIELDS;

/// The state on which one assessment was decided, read together and checked by its write.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentState {
    pub slug: String,
    pub status: String,
    pub version: i64,
}

/// The original committed occurrence, including whether it still stands in the write's snapshot.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssessmentReceipt {
    pub slug: String,
    pub id: String,
    pub record: String,
    pub sid: i64,
    pub at: f64,
    pub current: bool,
}

type SourceCheck<'a> = dyn FnMut() -> Result<bool, Error> + 'a;

struct SourceObservation {
    current: bool,
    unavailable: Option<String>,
}

impl Store {
    /// One snapshot of the task's assessment fence. It writes and migrates nothing.
    pub fn assessment_state(&self, slug: &str) -> Result<AssessmentState, Error> {
        let states: Vec<AssessmentState> = self.retrying(|| {
            let read = Statement::from(pg::assessment_state());
            self.read(read, json!({"slug": slug}))
        })?;
        states
            .into_iter()
            .next()
            .ok_or_else(|| super::work::no_task(slug))
    }

    /// A fresh occurrence commits only against `expected`. A known committed pair returns its
    /// original receipt even after that state or the standing assessment changed.
    pub fn record_waits_checked(
        &self,
        slug: &str,
        record: &str,
        assessment: &str,
        handle: &str,
        expected: &AssessmentState,
    ) -> Result<AssessmentReceipt, Error> {
        expected_state(slug, expected)?;
        let task = self.write_waits(slug, record, assessment, handle, Some(expected), None)?;
        receipt(&task, record, assessment)
    }

    /// Observe source freshness on every real transaction attempt. The observation is bound to
    /// that query, but does not serialize a later external reference update. A known committed
    /// pair returns its original receipt even if this observation changed or is unreadable;
    /// unknown evidence can never authorize a fresh mutation. The callback must be read-only.
    pub fn record_waits_guarded(
        &self,
        slug: &str,
        record: &str,
        assessment: &str,
        handle: &str,
        expected: &AssessmentState,
        source: &mut SourceCheck<'_>,
    ) -> Result<AssessmentReceipt, Error> {
        expected_state(slug, expected)?;
        let task = self.write_waits(
            slug,
            record,
            assessment,
            handle,
            Some(expected),
            Some(source),
        )?;
        receipt(&task, record, assessment)
    }

    /// Record what the landing of task `slug` waits on as `handle`: `record` is the tip the landing
    /// judged, `: `, then each name ([`Waits`]), and `assessment` names the assessment it states
    /// ([`crate::alarm::Assessed`]: a fresh one, such as a new verdict, names another; a retry of the same write
    /// the same). The line it writes on the task's thread is `HANDLE set _board.waits to RECORD`,
    /// which the pipeline alarm replays, each line a record of its own; beside the record it keeps
    /// the assessment and that line's time. The record standing, of the assessment standing, is
    /// written again by nothing: the task comes back as it is. The same text of a fresh assessment
    /// is written and said again. The transaction retains every committed assessment/text pair in
    /// `fields._board.assessments`, so an older retry cannot overwrite a newer assessment either.
    pub fn record_waits(
        &self,
        slug: &str,
        record: &str,
        assessment: &str,
        handle: &str,
    ) -> Result<Task, Error> {
        self.write_waits(slug, record, assessment, handle, None, None)
    }

    fn write_waits(
        &self,
        slug: &str,
        record: &str,
        assessment: &str,
        handle: &str,
        expected: Option<&AssessmentState>,
        mut source: Option<&mut SourceCheck<'_>>,
    ) -> Result<Task, Error> {
        if Waits::parse(record).is_none() {
            return Err(refused(format!(
                "a landing's record is its tip, `: `, then what it waits on, `, ` between: not {}",
                repr(record)
            )));
        }
        if assessment.trim().is_empty() {
            return Err(refused(
                "a landing's assessment must have an identity".into(),
            ));
        }
        let acting = handle != OWNER;
        self.retrying(|| {
            let observed = match source.as_mut().map_or(Ok(true), |check| check()) {
                Ok(current) => SourceObservation { current, unavailable: None },
                Err(error) => SourceObservation {
                    current: false,
                    unavailable: Some(format!("assessment source observation unavailable: {error}")),
                },
            };
            let vars = self.acting(
                handle,
                (self.clock)(),
                json!({
                    "slug": slug,
                    "patch": {FIELDS: {WAITS: record}},
                    "set": WAITS_SET,
                    "record": record,
                    "assessment": assessment,
                    "missing": format!("{REFUSED}{}", super::work::no_task(slug)),
                    "checked": expected.is_some(),
                    "expected_status": expected.map(|state| state.status.as_str()),
                    "expected_version": expected.map(|state| state.version),
                    "changed": format!("{REFUSED}task {} changed since its assessment", repr(slug)),
                    "source_current": observed.current,
                    "source_error": observed.unavailable,
                    "source_changed": format!("{REFUSED}assessment source changed before publication"),
                }),
            );
            self.written(Statement::from(pg::record(acting)).posting(), vars, slug)
        })
    }

    /// Keep on task `slug` that its branch first moved past the tip the record of changes `record`
    /// judged, after it was written, at `first` ([`FIXED`]), unless the board already keeps one for
    /// that record, which stands: the pipeline alarm's memory of where that rework ended, kept by
    /// the first digest to learn it; another record of the same tip keeps its own. It writes no
    /// line and no version, deciding nothing but the alarm. Answers what the board keeps after,
    /// none for a task it does not hold.
    pub fn record_fixed(
        &self,
        slug: &str,
        record: Written<'_>,
        first: &Tip,
    ) -> Result<Option<Tip>, Error> {
        if record.sid <= 0
            || !record.at.is_finite()
            || !first.at.is_finite()
            || first.commit.trim().is_empty()
        {
            return Err(refused(format!(
                "invalid rework occurrence or end: {record:?}, {first:?}"
            )));
        }
        let read = || Statement::from(pg::fixed());
        let written = record.key();
        let vars = json!({
            "slug": slug,
            "judged": record.tip,
            "written": &written,
            "patch": {FIELDS: {FIXED: {record.tip: {written.as_str(): first}}}},
        });
        self.retrying(|| self.transact(read(), vars.clone()))
    }
}

fn expected_state(slug: &str, expected: &AssessmentState) -> Result<(), Error> {
    if expected.slug != slug
        || expected.version < 0
        || !super::STATUSES.contains(&expected.status.as_str())
    {
        return Err(refused(format!(
            "invalid assessment state for {}",
            repr(slug)
        )));
    }
    Ok(())
}

fn receipt(task: &Task, record: &str, assessment: &str) -> Result<AssessmentReceipt, Error> {
    let invalid = || {
        Error::Db(format!(
            "assessment {} of {} has no confirmed occurrence receipt",
            repr(assessment),
            repr(&task.slug)
        ))
    };
    let board = task.fields.get(FIELDS).ok_or_else(invalid)?;
    let current = board.get(WAITS).is_some_and(|waits| waits == record)
        && board
            .get(ASSESSED)
            .is_some_and(|value| value["id"] == assessment);
    let kept = board
        .get("assessments")
        .and_then(|values| values.get(assessment))
        .and_then(|values| values.get(record))
        .or_else(|| current.then(|| board.get(ASSESSED)).flatten())
        .ok_or_else(invalid)?;
    let sid = kept["sid"]
        .as_i64()
        .filter(|sid| *sid > 0)
        .ok_or_else(invalid)?;
    let at = kept["at"]
        .as_f64()
        .filter(|at| at.is_finite())
        .ok_or_else(invalid)?;
    Ok(AssessmentReceipt {
        slug: task.slug.clone(),
        id: assessment.into(),
        record: record.into(),
        sid,
        at,
        current,
    })
}
