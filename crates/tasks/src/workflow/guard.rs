//! Revision-bound guards (docs/WORKFLOW.md, "Guards"; docs/DECISIONS.md#revision-bound-project-guards
//! B): a guarded edge is taken only with an attestation from the project's own approval
//! authority, signed by the key the project commits for it, naming exactly the operation taken,
//! and at the generation of that authority the board last accepted. The project keeps the ledger
//! of human decisions, the criteria and who may decide; the board keeps only what it accepted
//! from the authority and what it admitted, and refuses whatever it cannot verify now.
//!
//! Nothing here reads or writes the board: the store reads the committed authorities, the
//! accepted states and the task in one snapshot, decides here, and writes fenced on what it read
//! (`crate::store::guard`, `crate::store::admission`).

use super::Validated;
use super::admit::Refusal;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use symphony_api::workflow::{
    ATTESTATION_CONTRACT, Attestation, Attested, Attestee, AuthorityState, Definition, Edge,
    Effect, GuardRefusal, GuardStanding, GuardWhy, Observation, Pair, RefusalCode, Signed,
};
use symphony_plugin::execution::{Principal, PrincipalKind};

/// The longest an attestation may hold, from its issue to its expiry, in seconds: an attestation
/// is fetched for the operation it covers, never kept as a standing permit.
pub const MAX_VALIDITY: f64 = 900.0;

/// How far ahead of the deciding machine's clock an attestation may say it was issued, in
/// seconds.
pub const SKEW: f64 = 30.0;

/// What an attestation's signature covers before its payload: a signature over an observation
/// never verifies as an attestation, nor the other way round.
const ATTESTATION_CONTEXT: &[u8] = b"symphony-workflow-attestation\n";
const OBSERVATION_CONTEXT: &[u8] = b"symphony-workflow-observation\n";

/// A project's approval authority, as its default branch commits it (`[[workflow_authorities]]`,
/// docs/PROJECT_FILE.md): the adapter its guards name, the issuer it signs as, the contract it
/// speaks, and the public half of its signing key, `ed25519:` and 64 hex digits. Activation
/// records them, so a key changed is a new generation, and nothing else extends them: no request,
/// task or payload names a key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attester {
    pub adapter: String,
    pub issuer: String,
    pub contract: u32,
    pub key: String,
}

impl Attester {
    /// Its public key, refused when it is not `ed25519:` and 32 bytes in lowercase hex, or not a
    /// point the signature scheme accepts.
    pub fn verifying_key(&self) -> Result<VerifyingKey, String> {
        let hex = (self.key.strip_prefix("ed25519:")).ok_or_else(|| {
            format!(
                "workflow_authorities: {}'s key is not ed25519:HEX",
                self.adapter
            )
        })?;
        let bytes: [u8; 32] = decoded(hex)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| {
                format!(
                    "workflow_authorities: {}'s key is not 32 bytes of lowercase hex",
                    self.adapter
                )
            })?;
        VerifyingKey::from_bytes(&bytes).map_err(|_| {
            format!(
                "workflow_authorities: {}'s key is no Ed25519 key",
                self.adapter
            )
        })
    }
}

/// The committed authorities checked against the definition they guard: each names an adapter the
/// definition declares, once, with a key that reads. Its contract is the host's to support
/// (`supported_with`), so a project learns which contract it asked for.
pub fn check_attesters(definition: &Validated, attesters: &[Attester]) -> Result<(), String> {
    let declared = &definition.definition().adapters;
    let mut seen = BTreeSet::new();
    for attester in attesters {
        if !declared.contains(&attester.adapter) {
            return Err(format!(
                "workflow_authorities: {} is no adapter the workflow declares",
                attester.adapter
            ));
        }
        if !seen.insert(&attester.adapter) {
            return Err(format!(
                "workflow_authorities: {} is named twice",
                attester.adapter
            ));
        }
        if attester.issuer.is_empty() {
            return Err(format!(
                "workflow_authorities: {} names no issuer",
                attester.adapter
            ));
        }
        attester.verifying_key()?;
    }
    Ok(())
}

/// The attestation `signed` carries, refused unless its signature is `attester`'s key's over it
/// and its payload is an attestation of this contract from that authority.
pub fn verified_attestation(attester: &Attester, signed: &Signed) -> Result<Attestation, String> {
    verify(attester, ATTESTATION_CONTEXT, signed)?;
    let attestation: Attestation = serde_json::from_str(&signed.payload)
        .map_err(|e| format!("its payload is no attestation: {e}"))?;
    from_authority(
        attester,
        attestation.contract,
        &attestation.adapter,
        &attestation.issuer,
    )?;
    Ok(attestation)
}

/// The observation `signed` carries, refused as [`verified_attestation`] refuses.
pub fn verified_observation(attester: &Attester, signed: &Signed) -> Result<Observation, String> {
    verify(attester, OBSERVATION_CONTEXT, signed)?;
    let observation: Observation = serde_json::from_str(&signed.payload)
        .map_err(|e| format!("its payload is no observation: {e}"))?;
    from_authority(
        attester,
        observation.contract,
        &observation.adapter,
        &observation.issuer,
    )?;
    Ok(observation)
}

/// The adapter a signed observation says it is from, read before its signature is checked, so
/// the store knows which committed key to check it with. Nothing else of an unverified payload is
/// read.
pub fn claimed_adapter(signed: &Signed) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Claim {
        adapter: String,
    }
    serde_json::from_str::<Claim>(&signed.payload)
        .map(|c| c.adapter)
        .map_err(|e| format!("its payload names no adapter: {e}"))
}

fn verify(attester: &Attester, context: &[u8], signed: &Signed) -> Result<(), String> {
    let key = attester.verifying_key()?;
    let bytes: [u8; 64] = decoded(&signed.signature)
        .and_then(|b| b.try_into().ok())
        .ok_or("its signature is not 64 bytes of lowercase hex")?;
    let message = [context, signed.payload.as_bytes()].concat();
    key.verify_strict(&message, &Signature::from_bytes(&bytes))
        .map_err(|_| {
            format!(
                "its signature is not {}'s committed key's",
                attester.adapter
            )
        })
}

fn from_authority(
    attester: &Attester,
    contract: u32,
    adapter: &str,
    issuer: &str,
) -> Result<(), String> {
    if contract != ATTESTATION_CONTRACT {
        return Err(format!(
            "it speaks contract {contract}, and this board reads {ATTESTATION_CONTRACT}"
        ));
    }
    if adapter != attester.adapter || issuer != attester.issuer {
        return Err(format!(
            "it names {adapter} as {issuer}, and the key is {}'s as {}",
            attester.adapter, attester.issuer
        ));
    }
    Ok(())
}

/// The message an authority signs for `payload`: what [`Signed::signature`] covers. The
/// project's authority builds the same bytes; tests sign with it.
pub fn signed_message(observation: bool, payload: &str) -> Vec<u8> {
    let context = if observation {
        OBSERVATION_CONTEXT
    } else {
        ATTESTATION_CONTEXT
    };
    [context, payload.as_bytes()].concat()
}

/// An authority as the board accepted it (`workflow:['authority', ADAPTER]`): the state its last
/// accepted observation reported, whether two observations reported different decisions at one
/// generation (then nothing it attests is admitted until it reports a state naming this one),
/// and how many operations were admitted at it, which each admission moves so an observation and
/// an admission never pass each other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accepted {
    pub adapter: String,
    pub issuer: String,
    pub epoch: String,
    pub generation: u64,
    pub state: String,
    #[serde(default)]
    pub conflicted: bool,
    #[serde(default)]
    pub admitted: u64,
}

impl Accepted {
    pub fn state(&self) -> AuthorityState {
        AuthorityState {
            epoch: self.epoch.clone(),
            generation: self.generation,
            state: self.state.clone(),
        }
    }
}

/// What an observation does to the authority the board accepted: accepted as its next state,
/// already the state accepted, or a different decision at the generation accepted, which leaves
/// the authority unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observed {
    Accepted(Accepted),
    Same,
    Conflict(Accepted),
}

/// `observation` from `current`, the authority's state the board accepted (none for one it
/// accepted nothing from). Accepted when it names `current` as its previous state and moves on
/// from it (a higher generation in the same epoch, or any generation of a new epoch, which only
/// an observation naming the accepted state can start); the same state again changes nothing; a
/// different decision at the accepted generation marks the authority conflicted. Refused when it
/// is behind the accepted generation, or names another previous state: it never lowers a
/// generation or passes over one the board did not accept, and the authority reports again from
/// the state the refusal names.
pub fn observe(current: Option<&Accepted>, observation: &Observation) -> Result<Observed, String> {
    let next = Accepted {
        adapter: observation.adapter.clone(),
        issuer: observation.issuer.clone(),
        epoch: observation.epoch.clone(),
        generation: observation.generation,
        state: observation.state.clone(),
        conflicted: false,
        admitted: current.map_or(0, |c| c.admitted),
    };
    let Some(current) = current else {
        return match &observation.previous {
            None => Ok(Observed::Accepted(next)),
            Some(previous) => Err(format!(
                "the board has accepted nothing from {}, and the observation names {} as its \
                 previous state; the first observation names none",
                observation.adapter,
                described(previous)
            )),
        };
    };
    let accepted = current.state();
    if observation.epoch == current.epoch && observation.generation == current.generation {
        if observation.state == current.state && !current.conflicted {
            return Ok(Observed::Same);
        }
        if observation.state != current.state {
            return Ok(Observed::Conflict(Accepted {
                conflicted: true,
                ..current.clone()
            }));
        }
    }
    if observation.epoch == current.epoch && observation.generation < current.generation {
        return Err(format!(
            "the observation is at generation {} of {}, behind the {} the board accepted; a \
             generation is never lowered",
            observation.generation, observation.epoch, current.generation
        ));
    }
    if observation.previous.as_ref() != Some(&accepted) {
        return Err(format!(
            "the board accepted {} from {}, and the observation names {} as its previous \
             state; report again from the state the board accepted",
            described(&accepted),
            observation.adapter,
            observation
                .previous
                .as_ref()
                .map_or_else(|| "none".to_string(), described)
        ));
    }
    if observation.epoch == current.epoch && observation.generation <= current.generation {
        return Err(format!(
            "the observation names generation {} as following {}; a state follows only a \
             lower generation of its epoch",
            observation.generation,
            described(&accepted)
        ));
    }
    Ok(Observed::Accepted(next))
}

fn described(state: &AuthorityState) -> String {
    format!(
        "generation {} of epoch {} (state {})",
        state.generation, state.epoch, state.state
    )
}

/// The exact operation an attestation must cover: the project and task, the edge, the workflow
/// revision and activation, the task's revision and pair it was taken from, the input revisions
/// it is taken on, and its principal.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding<'a> {
    pub project: &'a str,
    pub task: &'a str,
    pub edge: &'a str,
    pub workflow: &'a str,
    pub activation_generation: u64,
    pub task_revision: u64,
    pub at: &'a Pair,
    pub input_revisions: &'a BTreeMap<String, String>,
    pub principal: &'a Principal,
}

/// What admission and dispatch decide guards on: the authorities the active activation commits,
/// those the board accepted, the attestations the caller gave, and the machine's clock.
#[derive(Clone, Copy, Debug)]
pub struct Guarding<'a> {
    /// The project the active activation recorded ([`crate::workflow::attested_project`]): the
    /// one an attestation must name, whatever project a request says it is for.
    pub project: &'a str,
    pub attesters: &'a [Attester],
    pub accepted: &'a [Accepted],
    pub given: &'a [Signed],
    /// The time the attestations are held to: the machine's clock where a store decides, never
    /// one its caller can pin.
    pub now: f64,
}

/// One guard as admitted: which, by whose authority at which accepted state, the digest of the
/// attestation's payload, the ledger facts it rested on, what it bound, until when it held, and
/// the attestation itself, which a dispatch verifies again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checked {
    pub guard: String,
    pub adapter: String,
    pub issuer: String,
    pub epoch: String,
    pub generation: u64,
    pub state: String,
    pub digest: String,
    pub facts: Vec<String>,
    pub input_revisions: BTreeMap<String, String>,
    pub expires_at: f64,
    pub signed: Signed,
}

/// Each guard `edge` carries, met by an attestation `guarding` gives for it, or the first that is
/// not, refused: `guard_unavailable` for a guard with no committed authority, no accepted or a
/// conflicted state, no attestation, one that does not verify, one for another operation,
/// project, task, guard, edge, workflow, activation, revision or pair, one that does not pass or
/// rests on no fact, one outside its time, or one at another state than the accepted one;
/// `stale_input` for one bound to other input revisions; `unauthorized` for one covering another
/// principal; `unknown_id` for an attestation given for a guard the edge does not carry. Each
/// guard takes one attestation.
pub fn check(
    definition: &Definition,
    edge: &Edge,
    binding: &Binding<'_>,
    guarding: &Guarding<'_>,
) -> Result<Vec<Checked>, Refusal> {
    explained(definition, edge, binding, guarding).map_err(|refused| refused.refusal)
}

/// A guard's refusal with what a page shows of it: the refusal as [`check`] gives it, and, where
/// one guard of the edge refused, which, by whose authority, a word for why, and what the
/// attestation given says once it verified. A refusal of what was given as a whole (an
/// attestation for no guard of the edge, two for one) names no guard.
#[derive(Clone, Debug, PartialEq)]
pub struct Explained {
    pub refusal: Refusal,
    pub guard: Option<Box<GuardRefusal>>,
}

impl From<Refusal> for Explained {
    fn from(refusal: Refusal) -> Self {
        Self {
            refusal,
            guard: None,
        }
    }
}

/// [`check`], a refusal explained ([`Explained`]): the same decision, read by the same steps.
pub fn explained(
    definition: &Definition,
    edge: &Edge,
    binding: &Binding<'_>,
    guarding: &Guarding<'_>,
) -> Result<Vec<Checked>, Explained> {
    let unavailable = |why: String| Refusal::new(RefusalCode::GuardUnavailable, why);
    let id = &edge.id;
    // Every attestation given is read first: one for a guard the edge does not carry, or a second
    // for one guard, refuses rather than being passed over.
    let mut by_guard: Vec<(Attestation, &Signed)> = Vec::new();
    let mut unverified: BTreeMap<String, String> = BTreeMap::new();
    for signed in guarding.given {
        let adapter = claimed_adapter(signed)
            .map_err(|why| unavailable(format!("an attestation given for {id}: {why}")))?;
        let Some(attester) = guarding.attesters.iter().find(|a| a.adapter == adapter) else {
            return Err(unavailable(format!(
                "an attestation given for {id} names {adapter}, which no authority the active \
                 workflow commits is"
            ))
            .into());
        };
        match verified_attestation(attester, signed) {
            Ok(attestation) => {
                if !edge.guards.contains(&attestation.guard) {
                    return Err(Refusal::new(
                        RefusalCode::UnknownId,
                        format!(
                            "an attestation given is for guard {}, which edge {id} does not \
                             carry",
                            attestation.guard
                        ),
                    )
                    .into());
                }
                if by_guard.iter().any(|(a, _)| a.guard == attestation.guard) {
                    return Err(unavailable(format!(
                        "two attestations were given for guard {} of {id}; give one",
                        attestation.guard
                    ))
                    .into());
                }
                by_guard.push((attestation, signed));
            }
            Err(why) => {
                unverified.insert(adapter, why);
            }
        }
    }
    let mut checked = Vec::new();
    for guard_id in &edge.guards {
        let Some(guard) = definition.guards.iter().find(|g| &g.id == guard_id) else {
            return Err(unavailable(format!(
                "edge {id} is guarded by {guard_id}, which the active workflow does not declare"
            ))
            .into());
        };
        let adapter = &guard.adapter;
        // This guard's refusal, with the word a page reads and nothing an attestation says.
        let told = |why: GuardWhy, refusal: Refusal| Explained {
            refusal,
            guard: Some(Box::new(GuardRefusal {
                guard: guard_id.clone(),
                adapter: adapter.clone(),
                why,
                record: None,
            })),
        };
        if !guarding.attesters.iter().any(|a| &a.adapter == adapter) {
            return Err(told(
                GuardWhy::Uncommitted,
                unavailable(format!(
                    "edge {id} is guarded by {guard_id}, and the active workflow commits no \
                     authority for {adapter}"
                )),
            ));
        }
        let Some(accepted) = guarding.accepted.iter().find(|a| &a.adapter == adapter) else {
            return Err(told(
                GuardWhy::Unreported,
                unavailable(format!(
                    "edge {id} is guarded by {guard_id}, and the board has accepted no \
                     observation from {adapter}: its authority publishes one first"
                )),
            ));
        };
        if accepted.conflicted {
            return Err(told(
                GuardWhy::Conflict,
                unavailable(format!(
                    "{adapter} reported two different decisions at generation {} of {}: its \
                     authority is unknown until it reports a state that follows them",
                    accepted.generation, accepted.epoch
                )),
            ));
        }
        let taken = by_guard.iter().position(|(a, _)| &a.guard == guard_id);
        let Some((attestation, signed)) = taken.map(|at| by_guard.swap_remove(at)) else {
            return Err(match unverified.get(adapter) {
                Some(why) => told(
                    GuardWhy::Unverified,
                    unavailable(format!(
                        "edge {id} is guarded by {guard_id}, and the attestation given from \
                         {adapter} does not verify: {why}"
                    )),
                ),
                None => told(
                    GuardWhy::Missing,
                    unavailable(format!(
                        "edge {id} is guarded by {guard_id}: it is taken with an attestation \
                         from {adapter}"
                    )),
                ),
            });
        };
        // A guard is decided on the word of its own adapter's authority alone: the key that
        // verified the attestation and the state it is held to below are that authority's.
        if attestation.adapter != *adapter {
            return Err(told(
                GuardWhy::Unverified,
                unavailable(format!(
                    "the attestation given for {guard_id} of {id} is from {}, and only \
                     {adapter} attests {guard_id}",
                    attestation.adapter
                )),
            ));
        }
        // From here the attestation is this guard's authority's own, verified: what it says is
        // shown with the refusal.
        let said = |why: GuardWhy, refusal: Refusal| {
            let mut explained = told(why, refusal);
            if let Some(guard) = &mut explained.guard {
                guard.record = Some(attestation.record());
            }
            explained
        };
        bound(&attestation, guard_id, binding).map_err(|refusal| said(GuardWhy::Stale, refusal))?;
        if attestation.principal != attestee(binding.principal) {
            return Err(said(
                GuardWhy::NotYours,
                Refusal::new(
                    RefusalCode::Unauthorized,
                    format!(
                        "the attestation of {guard_id} covers {}/{} ({}), not the principal \
                         taking {id}",
                        attestation.principal.issuer,
                        attestation.principal.subject,
                        attestation.principal.kind
                    ),
                ),
            ));
        }
        if attestation.input_revisions != *binding.input_revisions {
            return Err(said(
                GuardWhy::Stale,
                Refusal::new(
                    RefusalCode::StaleInput,
                    format!(
                        "the attestation of {guard_id} binds the input revisions {:?}, and {id} \
                         is taken on {:?}",
                        attestation.input_revisions, binding.input_revisions
                    ),
                ),
            ));
        }
        match attestation.result {
            Attested::Pass if attestation.facts.is_empty() => {
                return Err(said(
                    GuardWhy::Unfounded,
                    unavailable(format!(
                        "the attestation of {guard_id} passes on no decision of {adapter}'s"
                    )),
                ));
            }
            Attested::Pass => {}
            Attested::Refuse => {
                return Err(said(
                    GuardWhy::Refused,
                    unavailable(format!(
                        "{adapter} refuses {guard_id} for {}: {}",
                        binding.task,
                        attestation.reasons.join("; ")
                    )),
                ));
            }
            Attested::Unknown => {
                return Err(said(
                    GuardWhy::Unknown,
                    unavailable(format!(
                        "{adapter} cannot say whether {guard_id} holds for {}: {}",
                        binding.task,
                        attestation.reasons.join("; ")
                    )),
                ));
            }
        }
        timely(&attestation, guard_id, guarding.now)
            .map_err(|(why, refusal)| said(why, refusal))?;
        if attestation.epoch != accepted.epoch || attestation.generation != accepted.generation {
            let ahead =
                attestation.epoch == accepted.epoch && attestation.generation > accepted.generation;
            return Err(said(
                if ahead {
                    GuardWhy::NotReported
                } else {
                    GuardWhy::DecidedAgain
                },
                unavailable(format!(
                    "the attestation of {guard_id} is at generation {} of {}, and the board \
                     accepted generation {} of {} from {adapter}: {}",
                    attestation.generation,
                    attestation.epoch,
                    accepted.generation,
                    accepted.epoch,
                    if ahead {
                        "its authority publishes that observation first"
                    } else {
                        "a later decision stands, so it attests again"
                    }
                )),
            ));
        }
        checked.push(Checked {
            guard: guard_id.clone(),
            adapter: adapter.clone(),
            issuer: attestation.issuer.clone(),
            epoch: accepted.epoch.clone(),
            generation: accepted.generation,
            state: accepted.state.clone(),
            digest: digest(&signed.payload),
            facts: attestation.facts.clone(),
            input_revisions: attestation.input_revisions.clone(),
            expires_at: attestation.expires_at,
            signed: signed.clone(),
        });
    }
    Ok(checked)
}

/// The revision of the transition that last brought a task into `pair` before its transition
/// `before`, of its recorded `transitions` (oldest first): going back past each that stayed in
/// `pair` (an edge or a recovery from it to it, an activation that kept it there) and each resume
/// into it with everything back to the block that left it, to the transition that entered it from
/// another pair. None where the record says the task stood elsewhere just before `before`, where
/// what a resume walks back to is not a block from `pair` (admission resumes a task at a guarded
/// pair only where its block saved it), or where it holds no such transition: what is not found is
/// never taken as met.
pub fn entry(transitions: &[super::Transition], before: u64, pair: &Pair) -> Option<u64> {
    // While walking back through a block: the resume returns to the pair its block left, so the
    // block is the latest transition before it that left `pair`.
    let mut blocked = false;
    for t in (transitions.iter().rev()).skip_while(|t| t.revision >= before) {
        let from = t.from.as_ref().unwrap_or(&t.to);
        if blocked {
            if from == pair && t.to != *pair {
                // What left the pair must be the block the resume undid, or the record is not one
                // admission lets stand: none is found.
                if !(t.effects.iter()).any(|e| matches!(e, Effect::Block { .. })) {
                    return None;
                }
                blocked = false;
            }
        } else if t.to != *pair {
            return None;
        } else if t.effects.contains(&Effect::ClearBlock) {
            blocked = true;
        } else if from != pair {
            return Some(t.revision);
        }
    }
    None
}

/// Why the guards an admission met (`met`) no longer stand for an effect that cannot be taken
/// back, none while they do; what a dispatch asks right before one (`Store::dispatch_effect`).
/// Each guard's authority is still the one the active activation commits (`attesters`), its
/// attestation still verifies by the key committed for it, and the board still accepts the very
/// state it was met at (`accepted`), not in conflict. A later decision (a revocation, a
/// withdrawal, a lost approver) moves that state, so it no longer stands. The attestation's own
/// time was held when it was admitted: a dispatch comes whenever its land runs, and the
/// authority's state, not the receipt's age, is its current word.
pub fn unstanding(
    met: &[Checked],
    attesters: &[Attester],
    accepted: &[Accepted],
) -> Option<String> {
    (met.iter()).find_map(|checked| standing(checked, attesters, accepted).1)
}

/// Whether one guard an admission met still stands, as a word a page says it by, with the reason
/// [`unstanding`] gives while it does not.
pub fn standing(
    checked: &Checked,
    attesters: &[Attester],
    accepted: &[Accepted],
) -> (GuardStanding, Option<String>) {
    let (guard, adapter) = (&checked.guard, &checked.adapter);
    let Some(attester) = (attesters.iter()).find(|a| a.adapter == *adapter) else {
        return (
            GuardStanding::Uncommitted,
            Some(format!(
                "{guard} was met by {adapter}, which the active workflow no longer commits"
            )),
        );
    };
    if let Err(why) = verified_attestation(attester, &checked.signed) {
        return (
            GuardStanding::Unverified,
            Some(format!(
                "the attestation that met {guard} no longer verifies by the key committed for \
                 {adapter}: {why}"
            )),
        );
    }
    let met_at = format!("generation {} of {}", checked.generation, checked.epoch);
    let Some(now) = accepted.iter().find(|a| a.adapter == *adapter) else {
        return (
            GuardStanding::Unreported,
            Some(format!(
                "{guard} was met at {met_at} of {adapter}, and the board accepts no state from it now"
            )),
        );
    };
    if now.conflicted {
        return (
            GuardStanding::Conflict,
            Some(format!(
                "{adapter} is in conflict at generation {} of {}: nothing it attested is \
                 dispatched until it reports a state following that one",
                now.generation, now.epoch
            )),
        );
    }
    let same = now.epoch == checked.epoch
        && now.generation == checked.generation
        && now.state == checked.state;
    if same {
        return (GuardStanding::Met, None);
    }
    (
        GuardStanding::DecidedAgain,
        Some(format!(
            "{adapter} decided again since {guard} was met at {met_at}: a later decision \
             stands (generation {} of {} now)",
            now.generation, now.epoch
        )),
    )
}

/// Refused unless `attestation` covers exactly the operation `binding` names.
fn bound(attestation: &Attestation, guard: &str, binding: &Binding<'_>) -> Result<(), Refusal> {
    let mismatched: Option<(&str, String, String)> = if attestation.project != binding.project {
        Some((
            "project",
            attestation.project.clone(),
            binding.project.into(),
        ))
    } else if attestation.task != binding.task {
        Some(("task", attestation.task.clone(), binding.task.into()))
    } else if attestation.guard != guard {
        Some(("guard", attestation.guard.clone(), guard.into()))
    } else if attestation.edge != binding.edge {
        Some(("edge", attestation.edge.clone(), binding.edge.into()))
    } else if attestation.workflow != binding.workflow {
        Some((
            "workflow",
            attestation.workflow.clone(),
            binding.workflow.into(),
        ))
    } else if attestation.activation_generation != binding.activation_generation {
        Some((
            "activation generation",
            attestation.activation_generation.to_string(),
            binding.activation_generation.to_string(),
        ))
    } else if attestation.task_revision != binding.task_revision {
        Some((
            "task revision",
            attestation.task_revision.to_string(),
            binding.task_revision.to_string(),
        ))
    } else if attestation.at != *binding.at {
        Some(("pair", named(&attestation.at), named(binding.at)))
    } else {
        None
    };
    match mismatched {
        None => Ok(()),
        Some((what, attested, taken)) => Err(Refusal::new(
            RefusalCode::GuardUnavailable,
            format!(
                "the attestation of {guard} is for {what} {attested}, and the operation's is \
                 {taken}: it covers another operation"
            ),
        )),
    }
}

/// Refused unless `now` is within `attestation`'s time: issued no later than [`SKEW`] ahead of
/// the machine's clock, not yet expired, and held for no longer than [`MAX_VALIDITY`].
/// Each refusal comes with the word a page says it by ([`GuardWhy`]).
fn timely(attestation: &Attestation, guard: &str, now: f64) -> Result<(), (GuardWhy, Refusal)> {
    let refuse =
        |word: GuardWhy, why: String| Err((word, Refusal::new(RefusalCode::GuardUnavailable, why)));
    let (issued, expires) = (attestation.issued_at, attestation.expires_at);
    if !(issued.is_finite() && expires.is_finite()) || expires <= issued {
        return refuse(
            GuardWhy::Expired,
            format!(
                "the attestation of {guard} expires ({expires}) no later than it was issued \
                 ({issued})"
            ),
        );
    }
    if expires - issued > MAX_VALIDITY {
        return refuse(
            GuardWhy::Overlong,
            format!(
                "the attestation of {guard} holds for {} s, longer than the {MAX_VALIDITY} s an \
                 attestation may",
                expires - issued
            ),
        );
    }
    if issued > now + SKEW {
        return refuse(
            GuardWhy::Early,
            format!(
                "the attestation of {guard} says it was issued at {issued}, ahead of this \
                 machine's clock ({now})"
            ),
        );
    }
    if now >= expires {
        return refuse(
            GuardWhy::Expired,
            format!(
                "the attestation of {guard} expired at {expires}; this machine's clock is {now}"
            ),
        );
    }
    Ok(())
}

/// `principal` as an attestation names it.
pub fn attestee(principal: &Principal) -> Attestee {
    Attestee {
        kind: match principal.kind {
            PrincipalKind::Human => "human",
            PrincipalKind::Agent => "agent",
            PrincipalKind::Service => "service",
            _ => "unknown",
        }
        .into(),
        issuer: principal.issuer.clone(),
        subject: principal.subject.clone(),
    }
}

/// The hex digest of `payload`, by which an admission names the attestation it admitted.
pub fn digest(payload: &str) -> String {
    Sha256::digest(payload.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The edges that enter `pair` from another: every edge to it but one leaving it already and a
/// resume, which admission lets return a task to a pair a guarded edge enters only where its block
/// saved that very pair (`admit::blockers`), so a resume is never a way in past the guard.
fn entering<'a>(definition: &'a Definition, pair: &Pair) -> impl Iterator<Item = &'a Edge> {
    (definition.edges.iter()).filter(move |e| {
        e.to == *pair && e.from != *pair && !e.effects.contains(&Effect::ClearBlock)
    })
}

/// The guards every edge entering `pair` carries, in the order a validated definition sorts them,
/// none where no guarded edge enters it.
pub fn guarded_into<'d>(definition: &'d Definition, pair: &Pair) -> Option<&'d [String]> {
    entering(definition, pair)
        .find(|e| !e.guards.is_empty())
        .map(|e| e.guards.as_slice())
}

/// Refused where a task could reach a pair a guarded edge enters by a route that lacks that
/// edge's guards: another edge entering it under other guards or none, the initial pair a new
/// task is filed at, a recovery that moves a task there from another position, or a retry bound
/// that sends it there once exhausted. A graph never offers a second way to the same acceptance.
pub fn coverage(definition: &Definition) -> Result<(), String> {
    for position in &definition.positions {
        let pair = &position.at;
        let Some(guards) = guarded_into(definition, pair) else {
            continue;
        };
        let guarding = (entering(definition, pair))
            .find(|e| !e.guards.is_empty())
            .expect("guarded_into() found a guarded edge");
        for edge in entering(definition, pair) {
            if edge.guards != guards {
                return Err(format!(
                    "unguarded route: edge {} enters {} under {}, and edge {} under {}; every \
                     edge entering a guarded pair carries the same guards",
                    edge.id,
                    named(pair),
                    listed(&edge.guards),
                    guarding.id,
                    listed(guards)
                ));
            }
        }
        if definition.initial == *pair {
            return Err(format!(
                "unguarded route: {} is the initial pair, and edge {} enters it under {}",
                named(pair),
                guarding.id,
                listed(guards)
            ));
        }
        for other in &definition.positions {
            let Some(recovery) = &other.recovery else {
                continue;
            };
            for (name, action) in [
                ("release", &recovery.release),
                ("lost_holder", &recovery.lost_holder),
                ("attempt_failure", &recovery.attempt_failure),
                (
                    "cancellation_acknowledged",
                    &recovery.cancellation_acknowledged,
                ),
            ] {
                if action.target.as_ref() == Some(pair) && other.at != *pair {
                    return Err(format!(
                        "unguarded route: the {name} recovery of {} moves a task to {}, which \
                         edge {} enters under {}",
                        named(&other.at),
                        named(pair),
                        guarding.id,
                        listed(guards)
                    ));
                }
            }
        }
        // An exhausted retry bound sends the task to its pair as a recovery, no edge taken.
        let exhausting = (definition.edges.iter()).find(|e| {
            e.from != *pair && (e.retry.as_ref()).is_some_and(|retry| retry.exhausted == *pair)
        });
        if let Some(edge) = exhausting {
            return Err(format!(
                "unguarded route: the retry bound of edge {} sends a task to {} once exhausted, \
                 which edge {} enters under {}",
                edge.id,
                named(pair),
                guarding.id,
                listed(guards)
            ));
        }
    }
    Ok(())
}

fn listed(guards: &[String]) -> String {
    if guards.is_empty() {
        return "no guard".into();
    }
    guards.join(", ")
}

fn named(pair: &Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

/// Lowercase hex as bytes; none for anything else.
fn decoded(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            let pair = hex.get(i..i + 2)?;
            if pair.bytes().any(|c| c.is_ascii_uppercase()) {
                return None;
            }
            u8::from_str_radix(pair, 16).ok()
        })
        .collect()
}

/// The word `admit` refuses a guard's preview with, kept beside the rest of the guard's words.
pub(super) fn needs_attestation(edge: &Edge) -> Option<Refusal> {
    let guard = edge.guards.first()?;
    Some(Refusal::new(
        RefusalCode::GuardUnavailable,
        format!(
            "edge {} is guarded by {guard}: it is taken with an attestation from the project's \
             authority",
            edge.id
        ),
    ))
}

#[cfg(test)]
#[path = "guard_tests.rs"]
pub(crate) mod tests;
