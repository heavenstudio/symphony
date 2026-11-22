//! The default landing of a task a custom workflow holds: the adapter `default_land` on the
//! workflow's external-effect boundary ([`crate::workflow::effect`], docs/workflow/WORKFLOW.md "External
//! effects"). A task the board's own writers keep never comes here, and lands as it always did.
//!
//! A managed task lands only by its graph: the land takes the prepare edge leaving its pair whose
//! effect names `default_land`, and refuses, before anything is merged, a task at a pair no such
//! edge leaves. Evidence is decided as for any land, by the stamps the task needs read at its tip
//! by the board's readers; its `review` is read from the workflow, not from a verdict message,
//! which a workflow's approve writes none of: the approve standing is its last review transition
//! ([`Store::reviewed`]), and the head it read is the `head` input of that transition's request.
//!
//! 1. **Prepared.** Once the stamps stand, the prepare edge is taken and its intent recorded,
//!    bound to the project and task, the repository, the reviewed head, the revision of the
//!    landing policy (the project file the default branch commits), the workflow generation, the
//!    evidence as read and the default branch.
//! 2. **Dispatched.** Each binding is read again from the repository and the board; one that moved
//!    withdraws the intent, and nothing is merged. Then the head bound, by its whole name and never
//!    the branch's, is merged, checked and moved to as any land does, its [`Record`] kept first.
//! 3. **Finalized.** The merge is read back from the repository: the land's own record of it, a
//!    merge of the bound head the default branch holds. The finalize edge is taken with that
//!    commit, and the landed line written. A land killed between the merge and this step leaves
//!    the task at its prepared pair with its intent dispatched: run again, it reads the merge back
//!    and finishes the same operation, merging nothing. Where the policy or the evidence was
//!    another when the merge happened, the board records the merge and the recovery owed.

use super::needs::{self, Needed};
use super::{Landing, MANIFEST, Record, Refusal, Stop, evidence, short, taken_said};
use crate::cli::Env;
use crate::store::workflow::explicit_action;
use crate::sweep::git::{ancestor, rev};
use crate::workflow::effect::{self, Adapter, Bindings, DEFAULT_LAND, Intent, Outcome, Stage};
use crate::workflow::{Host, Reviewed, TaskPosition, agent_session};
use crate::{Error, Store};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use symphony_api::workflow::{InputValue, OperationRequest, PositionSnapshot, WorkflowRead};

/// The default landing as an adapter: what it reads of the repository and the board for the
/// boundary's questions.
pub struct DefaultLand<'a> {
    pub store: &'a Store,
    pub env: &'a Env,
    /// The main checkout.
    pub main: &'a Path,
    /// The lands' folder, where a land keeps its [`Record`].
    pub folder: PathBuf,
    /// The default branch.
    pub branch: &'a str,
}

impl DefaultLand<'_> {
    fn default_ref(&self) -> String {
        format!("refs/heads/{}", self.branch)
    }

    /// The revision of the landing policy `commit` holds: the blob of its project file, `absent`
    /// with none.
    fn policy(&self, commit: &str) -> String {
        rev(self.main, &format!("{commit}:{MANIFEST}")).unwrap_or_else(|| "absent".into())
    }

    /// The bindings of a land of `slug` at `head` as the repository and the board hold them now.
    pub fn bindings(&self, slug: &str, head: &str) -> Result<Bindings, String> {
        let common = crate::git::common_dir(self.main).map_err(|e| e.to_string())?;
        Ok(Bindings {
            project: self.store.database().database().into(),
            task: slug.into(),
            repository: common.display().to_string(),
            pull: None,
            head: head.into(),
            policy: self.policy(&self.default_ref()),
            evidence: self.evidence(slug, head, "HEAD")?,
            destination: self.default_ref(),
        })
    }

    /// The revision of `slug`'s evidence at `head`: a digest of the stamps it needs, which of them
    /// stand there by the board's readers, and the review the workflow recorded. The stamps it
    /// needs are the ones the catalogue `policy` commits names: the main checkout's head before a
    /// merge, the merge's first parent after it.
    fn evidence(&self, slug: &str, head: &str, policy: &str) -> Result<String, String> {
        let task = self.store.get(slug).map_err(|e| e.to_string())?;
        let thread = self.store.thread(slug).map_err(|e| e.to_string())?;
        let needed = Needed::under(&task, self.main, policy, Some(head))?;
        let read = needs::Evidence::of(self.store, self.env, self.main, slug, &thread, &needed);
        let (met, missing) = needs::standing(&needed, head, &read);
        let reviewed = self.store.reviewed(slug).map_err(|e| e.to_string())?;
        Ok(evidence_revision(
            &needed,
            &met,
            &missing,
            reviewed.as_ref(),
            head,
        ))
    }

    /// The merge of `head` the default branch holds, which this land made: by its own record, else
    /// the commit an earlier finalize read back, each checked against the repository.
    fn merge(&self, intent: &Intent) -> Option<String> {
        let (head, slug) = (&intent.bindings.head, &intent.slug);
        let recorded = Record::read(&self.folder, slug)
            .filter(|record| record.tip == *head)
            .map(|record| record.merge);
        recorded.or_else(|| intent.receipt.clone()).filter(|merge| {
            rev(self.main, &format!("{merge}^2")).as_deref() == Some(head.as_str())
                && ancestor(self.main, merge, &self.default_ref())
        })
    }
}

impl Adapter for DefaultLand<'_> {
    fn name(&self) -> &str {
        DEFAULT_LAND
    }

    fn current(&self, intent: &Intent) -> Result<Bindings, String> {
        let slug = &intent.slug;
        let head = rev(self.main, &format!("refs/heads/{slug}"))
            .ok_or_else(|| format!("no branch {slug} in {}", self.main.display()))?;
        self.bindings(slug, &head)
    }

    fn outcome(&self, intent: &Intent) -> Result<Option<Outcome>, String> {
        let Some(merge) = self.merge(intent) else {
            return Ok(None);
        };
        // The policy in force when it merged is the one the default branch held before the merge:
        // what the branch itself brings is the landing's own change, its stamp catalogue too.
        let before = format!("{merge}^1");
        let policy = self.policy(&before);
        let evidence = self.evidence(&intent.slug, &intent.bindings.head, &before)?;
        Ok(Some(Outcome {
            receipt: merge,
            policy,
            evidence,
        }))
    }
}

/// The digest of what was read of a task's evidence at `head`, its gate decisions that stand among
/// it: one given between the prepare and the merge changes it.
fn evidence_revision(
    needed: &Needed,
    met: &[(String, String)],
    missing: &[needs::Missing],
    reviewed: Option<&Reviewed>,
    head: &str,
) -> String {
    let mut parts: Vec<String> = vec![format!("head {head}")];
    parts.extend(needed.stamps.iter().map(|stamp| format!("needs {stamp}")));
    parts.extend(needed.decisions.iter().map(|decision| decision.line()));
    parts.extend(met.iter().map(|(stamp, _)| format!("met {stamp}")));
    parts.extend(
        missing
            .iter()
            .map(|stamp| format!("missing {}", stamp.name)),
    );
    if let Some(reviewed) = reviewed {
        parts.push(format!(
            "reviewed {} {}",
            reviewed.transition,
            reviewed.head.as_deref().unwrap_or("")
        ));
    }
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    evidence::digest(&parts)
}

/// Where a land finds a task a custom workflow holds.
enum Found {
    /// A prepare edge of the default landing leaves its pair.
    Ready(String),
    /// Its prepare was taken, and its effect is not finalized on the board.
    Open(Intent),
    /// Its finalize edge was taken: what is left is the landed line and the sweep.
    Landed(Intent),
}

impl Landing<'_> {
    /// Whether a custom workflow holds the task, and where: none for a task the board's own
    /// writers keep, on a board with no workflow active in one small read.
    pub(super) fn held(&self) -> Result<Option<TaskPosition>, Error> {
        if !self.task || self.store.workflow_active()?.is_none() {
            return Ok(None);
        }
        self.store.workflow_position(self.slug)
    }

    fn adapter(&self) -> DefaultLand<'_> {
        DefaultLand {
            store: self.store,
            env: self.env,
            main: &self.main,
            folder: self.folder.clone(),
            branch: &self.settings.default_branch,
        }
    }

    /// The land of a task a custom workflow holds at `position` (the module's doc).
    pub(super) fn land_held(&mut self, position: &TaskPosition) -> Result<String, Stop> {
        let slug = self.slug;
        if self.delivers() {
            return Err(Stop::Refused(format!(
                "{}; a land delivered by a pull request ([land] delivery) takes no workflow edge \
                 yet, so {slug} does not land here",
                explicit_action(slug)
            )));
        }
        let workflow = self.store.workflow_read()?.ok_or_else(|| {
            Stop::Refused(format!(
                "{slug} has a workflow position and no workflow is active"
            ))
        })?;
        match self.found(&workflow, position)? {
            Found::Ready(edge) => self.prepare(&workflow, position, &edge),
            Found::Open(intent) => self.resume(intent),
            Found::Landed(intent) => self.landed_before(&intent),
        }
    }

    fn found(&self, workflow: &WorkflowRead, position: &TaskPosition) -> Result<Found, Stop> {
        let slug = self.slug;
        let at = position.at();
        if let Some(edge) = effect::prepare_edge(&workflow.definition, &at, DEFAULT_LAND) {
            return Ok(Found::Ready(edge.id.clone()));
        }
        let ours = |intent: &Intent| intent.adapter == DEFAULT_LAND;
        if let Some(intent) = (self.store.effect_intent(slug, position.revision)?).filter(ours) {
            return Ok(Found::Open(intent));
        }
        // The finalize edge left the prepared pair for this one: its intent is the revision before.
        let before = position.revision.saturating_sub(1);
        let finalized = Some(crate::workflow::Transition::id_of(slug, position.revision));
        if let Some(intent) = (self.store.effect_intent(slug, before)?).filter(ours)
            && intent.finalized == finalized
        {
            return Ok(Found::Landed(intent));
        }
        Err(Stop::Refused(format!(
            "{}; it is at {}/{}, which no prepare_land edge of the adapter {DEFAULT_LAND} leaves, \
             so nothing is merged",
            explicit_action(slug),
            at.phase,
            at.state
        )))
    }

    /// Step 1: the stamps read at the tip, the review read from the workflow, then the prepare
    /// edge taken with its intent; then [`Landing::dispatch`].
    fn prepare(
        &mut self,
        workflow: &WorkflowRead,
        position: &TaskPosition,
        edge: &str,
    ) -> Result<String, Stop> {
        let slug = self.slug;
        let task = self.store.get(slug)?;
        let thread = self.store.thread(slug)?;
        let tip = rev(&self.main, &format!("refs/heads/{slug}")).ok_or_else(|| {
            self.refuse(Refusal::thread(format!(
                "no branch {slug} in {}: nothing to land",
                self.main.display()
            )))
        })?;
        let needed = Needed::of(&task, &self.main, Some(&tip))
            .map_err(|line| self.refuse(Refusal::here(line)))?;
        if needed.review() {
            self.reviewed_at(&tip)?;
        }
        self.clean().map_err(|line| self.refuse(line.into()))?;
        let base = self.base().map_err(|line| self.refuse(line.into()))?;
        if ancestor(&self.main, &tip, &base) {
            return Err(self.refuse(Refusal::here(format!(
                "{slug}'s tip {} is on {} already, merged outside land: no effect was prepared \
                 for it, so nothing is finalized",
                short(&tip),
                self.settings.default_branch
            ))));
        }
        self.owner_lines(&base, &tip)
            .map_err(|refusal| self.refuse(refusal))?;
        let raised = self.raised(&base, &tip, &needed)?;
        let read = needs::Evidence::of(self.store, self.env, &self.main, slug, &thread, &needed);
        let (met, missing) = needs::standing(&needed, &tip, &read);
        if !missing.is_empty() {
            let (line, to) = needs::refusal(slug, &tip, &missing);
            let then = format!(
                "Give them at {}, then land it again; {slug} stays at {}/{}",
                short(&tip),
                position.phase,
                position.state
            );
            return Err(self.refuse(Refusal::to(line, to, then)));
        }
        if needed.told() {
            self.say(&needed.line(slug, &tip, &met));
        }
        self.say_raised(&raised);
        let bindings = self
            .adapter()
            .bindings(slug, &tip)
            .map_err(|line| self.refuse(Refusal::here(line)))?;
        let declared = (workflow.definition.edges.iter())
            .find(|e| e.id == edge)
            .is_some_and(|e| e.inputs.iter().any(|input| input == "head"));
        let mut inputs = BTreeMap::new();
        if declared {
            inputs.insert("head".to_string(), InputValue::Revision(tip.clone()));
        }
        let request = OperationRequest {
            project: bindings.project.clone(),
            task: slug.to_string(),
            edge: edge.to_string(),
            expected: PositionSnapshot {
                workflow: workflow.revision.clone(),
                activation_generation: workflow.activation_generation,
                task_revision: position.revision,
                at: position.at(),
            },
            input_revisions: BTreeMap::new(),
            // One prepare from one position of one head: a run again repeats it, and answers it.
            idempotency_key: format!("land:{tip}:{}", position.revision),
            inputs,
        };
        let intent = self.store.prepare_effect(
            &agent_session(self.actor),
            &Host::local(),
            &self.adapter(),
            &request,
            &bindings,
        )?;
        self.say(&format!(
            "land {slug}: prepared {} by its edge {edge}, bound to its head {}",
            intent.id,
            short(&tip)
        ));
        self.dispatch(&intent)
    }

    /// The approve the workflow recorded stands on `tip`, the head its request named: refused,
    /// said to the orchestrator, with no approve standing or one of another head.
    fn reviewed_at(&self, tip: &str) -> Result<(), Stop> {
        let slug = self.slug;
        let line = match self.store.reviewed(slug)? {
            // Who approved is the edge's to decide: admission refuses the reviewer's role to a
            // session that wrote, executed or orchestrated the work.
            Some(Reviewed {
                head: Some(head), ..
            }) if head == tip => return Ok(()),
            Some(Reviewed {
                head: Some(head),
                transition,
                ..
            }) => format!(
                "{slug}'s approve ({transition}) read {}, and its tip is {}: the head reviewed is \
                 the head landed",
                short(&head),
                short(tip)
            ),
            Some(Reviewed { transition, .. }) => format!(
                "{slug}'s approve ({transition}) names no head: its approve edge takes the head \
                 read as its `head` input, which the land binds"
            ),
            None => format!("{slug} needs review, and no approve of its workflow stands on it"),
        };
        Err(self.refuse(Refusal::to(
            line,
            "@orchestrator",
            format!(
                "A fresh reviewer approves the tip by the workflow's approve edge, naming it \
                 (`just tasks workflow actions {slug}`)"
            ),
        )))
    }

    /// A land run again on a task whose prepare was taken: an effect that happened is finalized,
    /// nothing merged again; one that did not is dispatched again.
    fn resume(&mut self, intent: Intent) -> Result<String, Stop> {
        let slug = self.slug;
        if intent.stage == Stage::Withdrawn {
            return Err(Stop::Refused(format!(
                "{} is withdrawn ({}), and nothing lands under it: take an edge out of its pair \
                 (`just tasks workflow actions {slug}`), then land it again",
                intent.id,
                intent
                    .withdrawn
                    .as_deref()
                    .unwrap_or("its reservation no longer stands")
            )));
        }
        let merged = self.adapter().merge(&intent);
        let Some(merge) = merged else {
            self.say(&format!(
                "land {slug}: {} is {}, and nothing of it is on {}: dispatching it again",
                intent.id,
                intent.stage.as_str(),
                self.settings.default_branch
            ));
            return self.dispatch(&intent);
        };
        let record = Record::read(&self.folder, slug).filter(|record| record.merge == merge);
        let (top, taken) = match record {
            Some(record) => (record.checked, record.regenerated),
            None => (merge.clone(), Vec::new()),
        };
        self.say(&format!(
            "land {slug}: its head {} is on {} already, by this land's merge {}: finishing the \
             same operation ({}), merging nothing",
            short(&intent.bindings.head),
            self.settings.default_branch,
            short(&merge),
            intent.id
        ));
        self.finalize(&intent, &merge, &top, &taken, true)
    }

    /// Step 2: the bindings read again and the intent dispatched, or withdrawn and the land
    /// refused; then the bound head merged, checked and moved to; then [`Landing::finalize`].
    fn dispatch(&mut self, intent: &Intent) -> Result<String, Stop> {
        let slug = self.slug;
        let dispatched = self
            .store
            .dispatch_effect(&self.adapter(), slug, intent.revision);
        if let Err(Error::Refused(why)) = &dispatched {
            self.post(
                &format!(
                    "land refused: {why}. Nothing was merged. Take an edge out of its pair (`just \
                     tasks workflow actions {slug}`), then land it again"
                ),
                Some("@orchestrator"),
            );
        }
        let intent = dispatched?;
        let tip = intent.bindings.head.clone();
        self.clean().map_err(|line| self.refuse(line.into()))?;
        let mut base = self.base().map_err(|line| self.refuse(line.into()))?;
        let (merge, top, taken) = self.merge_until_moved(&mut base, &tip)?;
        self.owner_lines_after(&base, &tip);
        self.finalize(&intent, &merge, &top, &taken, false)
    }

    /// Step 3, once `merge` is on the default branch at `top`: the push, the finalize edge taken
    /// with the merge read back, the landed line, and what follows a land.
    fn finalize(
        &mut self,
        intent: &Intent,
        merge: &str,
        top: &str,
        taken: &[String],
        finishing: bool,
    ) -> Result<String, Stop> {
        let (slug, default) = (self.slug, self.settings.default_branch.clone());
        let mut landed = format!(
            "landed {slug} on {default} at {}, by its workflow ({})",
            &merge[..merge.len().min(12)],
            intent.id
        );
        if finishing {
            landed.push_str(", finishing a land interrupted after its merge");
        }
        landed.push_str(&taken_said(taken, &default, &self.settings.generators));
        landed.push_str(&self.tokens_said());
        self.go_on(Some(merge)).map_err(Stop::Refused)?;
        let pushed = self.push(top).said();
        self.go_on(Some(merge)).map_err(Stop::Refused)?;
        let mut line = format!("{landed}{pushed}");
        let finalized = self
            .store
            .finalize_effect(
                &agent_session(self.actor),
                &Host::local(),
                &self.adapter(),
                slug,
                intent.revision,
            )
            .map_err(|error| {
                Stop::Refused(format!(
                    "{line}, but the board did not finalize {}: {error}; run `just land {slug}` \
                     again to finish it",
                    intent.id
                ))
            })?;
        if let Some(recovery) = &finalized.intent.recovery {
            line.push_str(&format!("; RECOVERY REQUIRED: {recovery}"));
        }
        if finalized.transition.is_none() {
            // The outcome is recorded and its edge not taken: the record stays for the run that
            // takes it, and nothing says the task is done.
            return Err(Stop::Refused(format!(
                "{line}; its finalize edge is not taken, and {slug} stays where it is: a person \
                 recovers it"
            )));
        }
        self.landed_line(&line);
        Record::drop_in(&self.folder, slug);
        self.after(merge, true);
        Ok(line)
    }

    /// The landed line on the thread: the board's own where the position landed at projects to
    /// done, as a land's always is, else a plain line, the graph having said where the task is.
    fn landed_line(&self, line: &str) {
        let slug = self.slug;
        if (self.store.landed(slug, self.actor, line, true, &["done"])).is_err() {
            self.post(line, None);
        }
    }

    /// A land run again on a task whose finalize edge was taken: the landed line where the thread
    /// has none, and what follows a land.
    fn landed_before(&mut self, intent: &Intent) -> Result<String, Stop> {
        let slug = self.slug;
        let merge = intent.receipt.clone().unwrap_or_default();
        let said = format!("landed {slug} on {}", self.settings.default_branch);
        let thread = self.store.thread(slug)?;
        if thread.iter().any(|m| m.text.starts_with(&said)) {
            return Err(Stop::Refused(format!(
                "{slug} landed already, at {} ({}): nothing to land",
                short(&merge),
                intent.id
            )));
        }
        let line = format!(
            "{said} at {}, by its workflow ({}), finishing a land interrupted after its finalize",
            &merge[..merge.len().min(12)],
            intent.id
        );
        self.landed_line(&line);
        Record::drop_in(&self.folder, slug);
        self.after(&merge, true);
        Ok(line)
    }
}
