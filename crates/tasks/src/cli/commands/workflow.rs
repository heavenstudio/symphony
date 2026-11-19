//! `workflow show`, `workflow actions SLUG` and `workflow apply SLUG EDGE --revision N`: the custom
//! workflow a board activated, the edges a session could take from where a task stands, and one
//! taken through the board's one admission ([`Store::apply_operation`], docs/WORKFLOW.md). The
//! principal is the agent session `--as` names, as the local host binds it, never one the
//! command's input names; the owner acts from the Symphony.
//!
//! `apply` sends the task's workflow revision the session read (`--revision`, which `actions`
//! prints), never the one it finds: a task another moved since is refused as stale, so the
//! session reads it again before it acts. Its key (`--key`, by default the task, the revision and
//! the edge) makes a repeated command answer the first one's result.

use super::{Context, Stop};
use crate::Store;
use crate::workflow::{Host, admit::word, agent_session};
use std::collections::BTreeMap;
use symphony_api::workflow::{Authority, InputValue, NextActions, OperationRequest, Pair};

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    match c.parsed.get("action").unwrap_or_default() {
        "show" => show(store, c),
        "actions" => actions(store, c),
        "apply" => apply(store, c),
        other => Err(Stop::Refused(format!(
            "workflow takes show, actions SLUG or apply SLUG EDGE --revision N, not {other:?}"
        ))),
    }
}

/// The active workflow: its revision, its positions and its edges.
fn show(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let Some(read) = store.workflow_read()? else {
        c.say("no custom workflow is active: the board's own writers keep its tasks");
        return Ok(());
    };
    if c.parsed.flag("json") {
        c.say(&serde_json::to_string_pretty(&read).expect("a workflow read serializes"));
        return Ok(());
    }
    let d = &read.definition;
    c.say(&format!(
        "workflow {} (generation {}), committed at {}, starting at {}",
        read.revision.digest,
        read.activation_generation,
        read.revision.source.commit,
        named(&d.initial)
    ));
    for p in &d.positions {
        let mut line = format!("  {}  {}", named(&p.at), word(&p.legacy));
        if let Some(role) = &p.staffing {
            line.push_str(&format!(", staffed by its {}", word(role)));
        }
        if p.terminal {
            line.push_str(", terminal");
        }
        c.say(&line);
    }
    for e in &d.edges {
        c.say(&format!(
            "  {}: {} → {}  {}, by {}{}",
            e.id,
            named(&e.from),
            named(&e.to),
            word(&e.operation),
            authorities(&e.authority),
            if e.human_wait { ", a person's" } else { "" }
        ));
    }
    Ok(())
}

/// What the session could take from where `SLUG` stands, and what stops each edge now.
fn actions(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let slug = required(c, "slug", "workflow actions SLUG")?;
    let principal = agent_session(&c.actor()?);
    let next = store.workflow_actions(&slug, &principal, &Host::local())?;
    if c.parsed.flag("json") {
        c.say(&serde_json::to_string_pretty(&next).expect("next actions serialize"));
        return Ok(());
    }
    for line in action_lines(&slug, &next) {
        c.say(&line);
    }
    Ok(())
}

/// The snapshot, then each edge: where it goes, its label, who takes it and what stops it.
fn action_lines(slug: &str, next: &NextActions) -> Vec<String> {
    let s = &next.snapshot;
    let mut lines = vec![format!(
        "{slug} at {}, workflow revision {} (generation {})",
        named(&s.at),
        s.task_revision,
        s.activation_generation
    )];
    if next.actions.is_empty() {
        lines.push("  no edge leaves it".into());
    }
    for a in &next.actions {
        let mut line = format!(
            "  {} → {}  {}, by {}",
            a.edge,
            named(&a.destination),
            a.label,
            authorities(&a.responsible)
        );
        if !a.required_inputs.is_empty() {
            line.push_str(&format!(", inputs {}", a.required_inputs.join(", ")));
        }
        if !a.unavailable.is_empty() {
            let codes: Vec<String> = a.unavailable.iter().map(word).collect();
            line.push_str(&format!("  (not now: {})", codes.join(", ")));
        }
        lines.push(line);
    }
    lines
}

/// `EDGE` taken on `SLUG` from the revision `--revision` names, as the session.
fn apply(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let usage = "workflow apply SLUG EDGE --revision N";
    let slug = required(c, "slug", usage)?;
    let edge = required(c, "edge", usage)?;
    let revision: u64 = (c.parsed.get("revision"))
        .and_then(|r| r.parse().ok())
        .ok_or_else(|| {
            Stop::Refused(format!(
                "{usage}: --revision is the task's workflow revision you read (`just tasks \
                 workflow actions {slug}` prints it)"
            ))
        })?;
    let principal = agent_session(&c.actor()?);
    let host = Host::local();
    let mut snapshot = store.workflow_actions(&slug, &principal, &host)?.snapshot;
    // The revision the session read fences its intention, never the one found now: a task moved
    // since is refused as stale.
    snapshot.task_revision = revision;
    let mut inputs = BTreeMap::new();
    for given in c.parsed.all("input") {
        let (name, value) = given
            .split_once('=')
            .ok_or_else(|| Stop::Refused(format!("--input is NAME=TEXT, not {given:?}")))?;
        inputs.insert(name.to_string(), InputValue::Text(value.to_string()));
    }
    let key = (c.parsed.get("key").map(str::to_string))
        .unwrap_or_else(|| format!("{slug}@{revision}:{edge}"));
    let request = OperationRequest {
        project: store.database().database().to_string(),
        task: slug.clone(),
        edge,
        expected: snapshot,
        input_revisions: BTreeMap::new(),
        idempotency_key: key,
        inputs,
    };
    // The same command run again sends the request its key made, which answers its result; any
    // other under that key is sent as it is, and refused.
    let request = match store.workflow_receipt(&principal, &request.idempotency_key)? {
        Some(receipt)
            if (receipt.sent.task.as_str(), receipt.sent.edge.as_str())
                == (request.task.as_str(), request.edge.as_str())
                && receipt.sent.expected.task_revision == revision
                && receipt.sent.inputs == request.inputs =>
        {
            receipt.sent
        }
        _ => request,
    };
    let result = store.apply_operation(&principal, &host, &request)?;
    if c.parsed.flag("json") {
        c.say(&serde_json::to_string_pretty(&result).expect("a result serializes"));
        return Ok(());
    }
    c.say(&format!("took {} ({})", request.edge, result.transition));
    for line in action_lines(&slug, &result.next) {
        c.say(&line);
    }
    Ok(())
}

fn required(c: &Context, dest: &str, usage: &str) -> Result<String, Stop> {
    (c.parsed.get(dest).map(str::to_string))
        .filter(|v| !v.is_empty())
        .ok_or_else(|| Stop::Refused(format!("{usage}: {} is missing", dest.to_uppercase())))
}

fn named(pair: &Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

fn authorities(authority: &[Authority]) -> String {
    let each: Vec<String> = (authority.iter())
        .map(|a| match a {
            Authority::TaskRole { role } => format!("its {}", word(role)),
            Authority::ProjectCapability { capability } => capability.clone(),
            Authority::SystemCapability { capability } => format!("the board's {capability}"),
        })
        .collect();
    each.join(" or ")
}
