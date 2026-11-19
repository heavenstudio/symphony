//! `tasks workflow`: the active workflow shown, a task's next actions read and an edge taken
//! through the board's one admission, and the board's own commands refusing a task it holds.

use super::Case;
use symphony_api::workflow::Source;
use symphony_board::workflow::{Custom, Host, written};

/// Work built (available, then worked by its executor), then accepted by the owner.
const FLOW: &str = r#"
[workflow]
format = 1
initial = { phase = "build", state = "available" }
task_kinds = ["task"]
phases = [
  { id = "build", label = "Build", order = 0, phone_order = 0, column = true },
  { id = "verify", label = "Verify", order = 1, phone_order = 1, column = true },
]
states = [
  { id = "available", label = "Available" }, { id = "working", label = "Working" },
  { id = "awaiting", label = "Awaiting" }, { id = "accepted", label = "Accepted" },
]
capabilities = ["owner"]
adapters = []
guards = []
positions = [
  { at = { phase = "build", state = "available" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "ready", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "build", state = "working" }, staffing = "executor", slot = "acquire", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "in_progress", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, lost_holder = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  { at = { phase = "verify", state = "awaiting" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "verify", state = "accepted" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "done", outcome = "success", satisfies_dependencies = true, completion = "covered_close" },
]
edges = [
  { id = "claim", label = "Claim", from = { phase = "build", state = "available" }, to = { phase = "build", state = "working" }, operation = "claim", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "acquire_role", role = "executor" }, { kind = "slot", policy = "acquire" }], automatic = false, human_wait = false },
  { id = "submit", label = "Submit", from = { phase = "build", state = "working" }, to = { phase = "verify", state = "awaiting" }, operation = "request_review", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "accept", label = "Accept", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = true },
]
"#;

pub(crate) fn flow() -> Custom {
    let definition = written(FLOW).unwrap().0.unwrap();
    let revision = definition
        .revision(Source {
            commit: "1".repeat(40),
            project_blob: "2".repeat(40),
        })
        .unwrap();
    Custom {
        definition,
        revision,
        mappings: Vec::new(),
        authorities: Vec::new(),
        project: None,
    }
}

#[test]
fn a_session_reads_a_tasks_actions_and_takes_an_edge_from_the_revision_it_read() {
    let c = Case::new();
    let amber = c.register("worker");
    assert_eq!(
        c.ok(&amber, &["workflow", "show"]),
        "no custom workflow is active: the board's own writers keep its tasks\n"
    );
    let custom = flow();
    c.store()
        .activate_workflow(&custom, &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    let shown = c.ok(&amber, &["workflow", "show"]);
    assert!(
        shown.starts_with(&format!(
            "workflow {} (generation 1), committed at {}, starting at build/available\n",
            custom.revision.digest,
            "1".repeat(40)
        )),
        "{shown}"
    );
    assert!(shown.contains(
        "  accept: verify/awaiting → verify/accepted  close_covered, by owner, a person's\n"
    ));
    assert_eq!(
        c.ok(&amber, &["workflow", "actions", "sim/jump"]),
        "sim/jump at build/available, workflow revision 1 (generation 1)\n  claim → build/working  Claim, by its executor\n"
    );
    let stale = c.run_as(
        &amber,
        &["workflow", "apply", "sim/jump", "claim", "--revision", "0"],
        "",
    );
    assert_eq!(stale.code, 1);
    assert!(
        stale.err.starts_with("✖ stale_task: sim/jump is at build/available (workflow revision 1), not build/available (revision 0)"),
        "{}",
        stale.err
    );
    let took = c.ok(
        &amber,
        &["workflow", "apply", "sim/jump", "claim", "--revision", "1"],
    );
    assert_eq!(
        took,
        "took claim (sim/jump@2)\nsim/jump at build/working, workflow revision 2 (generation 1)\n  submit → verify/awaiting  Submit, by its executor\n"
    );
    // The same command again answers the first result: its key is the task, revision and edge.
    assert_eq!(
        c.ok(
            &amber,
            &["workflow", "apply", "sim/jump", "claim", "--revision", "1"]
        ),
        took
    );
    let moved = c.run_as(&amber, &["move", "sim/jump", "review"], "");
    assert_eq!(moved.code, 1);
    assert!(
        moved
            .err
            .starts_with("✖ explicit_action_required: a custom workflow holds sim/jump"),
        "{}",
        moved.err
    );
    let json = c.ok(&amber, &["workflow", "actions", "sim/jump", "--json"]);
    let next: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(next["snapshot"]["task_revision"], 2);
    assert_eq!(next["actions"][0]["edge"], "submit");
    let usage = c.run_as(&amber, &["workflow", "apply", "sim/jump", "submit"], "");
    assert_eq!(usage.code, 1);
    assert!(
        usage
            .err
            .contains("--revision is the task's workflow revision you read"),
        "{}",
        usage.err
    );
}

#[test]
fn an_agent_reads_and_takes_an_edge_through_the_api_as_the_session_it_names() {
    let c = Case::new();
    let amber = c.register("worker");
    let call = |call: serde_json::Value| {
        let ran = super::run(&c.env, &["api"], &call.to_string());
        assert_eq!(ran.code, 0, "{}", ran.err);
        serde_json::from_str::<serde_json::Value>(&ran.out).unwrap()
    };
    assert_eq!(
        call(serde_json::json!({"method": "workflow_read"})),
        serde_json::json!({"ok": null})
    );
    let custom = flow();
    c.store()
        .activate_workflow(&custom, &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    let read = call(serde_json::json!({"method": "workflow_read"}));
    assert_eq!(read["ok"]["revision"]["digest"], custom.revision.digest);
    assert_eq!(read["ok"]["activation_generation"], 1);
    let next = call(serde_json::json!({"method": "workflow_actions",
        "args": {"slug": "sim/jump", "handle": amber}}));
    let snapshot = next["ok"]["snapshot"].clone();
    assert_eq!(snapshot["task_revision"], 1);
    let project = c.store().database().database().to_string();
    let request = serde_json::json!({
        "project": project, "task": "sim/jump", "edge": "claim", "expected": snapshot,
        "input_revisions": {}, "idempotency_key": "api-1", "inputs": {},
    });
    let applied = call(serde_json::json!({"method": "workflow_apply",
        "args": {"handle": amber, "request": request}}));
    assert_eq!(applied["ok"]["transition"], "sim/jump@2");
    assert_eq!(applied["ok"]["next"]["actions"][0]["edge"], "submit");
    let owner = call(serde_json::json!({"method": "workflow_apply",
        "args": {"handle": "owner", "request": request}}));
    assert!(
        owner["refused"]
            .as_str()
            .unwrap()
            .contains("the owner acts from the Symphony")
    );
    let mut again = request.clone();
    again["idempotency_key"] = "api-2".into();
    let stale = call(serde_json::json!({"method": "workflow_apply",
        "args": {"handle": amber, "request": again}}));
    assert!(
        stale["refused"]
            .as_str()
            .unwrap()
            .starts_with("stale_task: sim/jump is at build/working"),
        "{stale}"
    );
}

#[test]
fn a_session_recovers_a_lost_holders_task_which_pull_and_next_offer_to_no_claim() {
    let mut c = Case::new();
    let amber = c.register("worker");
    let birch = c.register("worker");
    c.store()
        .activate_workflow(&flow(), &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    c.ok(
        &amber,
        &["workflow", "apply", "sim/jump", "claim", "--revision", "1"],
    );
    let live = c.run_as(&birch, &["workflow", "recover", "sim/jump", &amber], "");
    assert_eq!(live.code, 1, "{}", live.out);
    assert!(
        live.err.starts_with(&format!(
            "✖ explicit_action_required: {amber} is live on the board: only a lost holder's \
             work is recovered"
        )),
        "{}",
        live.err
    );
    let now: f64 = c.env.vars["TASKS_NOW"].parse().unwrap();
    let lost = now + symphony_board::health::LIVE_SECONDS + symphony_board::health::STALE_SECONDS;
    c.env
        .vars
        .insert("TASKS_NOW".into(), (lost + 1.0).to_string());
    c.store().touch(&birch).unwrap();
    assert_eq!(
        c.ok(&birch, &["workflow", "recover", "sim/jump", &amber]),
        format!(
            "recovered {amber} on sim/jump (sim/jump@3): build/working → build/available\n\
             sim/jump at build/available, workflow revision 3 (generation 1)\n  \
             claim → build/working  Claim, by its executor\n"
        )
    );
    // Ready and unstaffed, yet no claim but its edge takes it.
    let pulled = c.ok(&birch, &["pull"]);
    assert!(pulled.contains("ready_unstaffed=0"), "{pulled}");
    assert!(!pulled.contains("sim/jump"), "{pulled}");
    assert!(c.ok(&birch, &["next"]).starts_with("nothing to take"));
}

/// FLOW with its submit edge guarded by `approved`, which the `approvals` authority attests
/// with the key of seed 7, in the project `sim`.
mod guarded {
    use super::super::{Case, scratch};
    use super::{FLOW, flow};
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::BTreeMap;
    use symphony_api::workflow::{
        ATTESTATION_CONTRACT, Attestation, Attested, Observation, Pair, Signed,
    };
    use symphony_board::workflow::guard::{attestee, signed_message};
    use symphony_board::workflow::{Attester, Custom, Host, agent_session, written};

    const NOW: f64 = 1_800_000_000.0;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn signing(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn signed(seed: u8, observation: bool, payload: String) -> Signed {
        let signature = signing(seed).sign(&signed_message(observation, &payload));
        Signed {
            payload,
            signature: hex(&signature.to_bytes()),
        }
    }

    fn guarded_flow() -> Custom {
        let text = FLOW
            .replacen("adapters = []", "adapters = [\"approvals\"]", 1)
            .replacen(
                "guards = []",
                "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
                1,
            )
            .replacen(
                "operation = \"request_review\", authority = [{ kind = \"task_role\", role = \"executor\" }], inputs = [], dependencies = \"none\", guards = []",
                "operation = \"request_review\", authority = [{ kind = \"task_role\", role = \"executor\" }], inputs = [], dependencies = \"none\", guards = [\"approved\"]",
                1,
            );
        let definition = written(&text).unwrap().0.unwrap();
        Custom {
            definition,
            authorities: vec![Attester {
                adapter: "approvals".into(),
                issuer: "ledger".into(),
                contract: ATTESTATION_CONTRACT,
                key: format!("ed25519:{}", hex(signing(7).verifying_key().as_bytes())),
            }],
            project: Some("sim".into()),
            ..flow()
        }
    }

    fn observation(seed: u8, generation: u64) -> Signed {
        let observed = Observation {
            contract: ATTESTATION_CONTRACT,
            adapter: "approvals".into(),
            issuer: "ledger".into(),
            epoch: "e1".into(),
            generation,
            state: format!("s{generation}"),
            previous: None,
            at: NOW,
        };
        signed(seed, true, serde_json::to_string(&observed).unwrap())
    }

    /// amber's submit of sim/jump from its workflow revision 2, passed at generation 3, on the
    /// input revision design=r7.
    fn submit_attestation(custom: &Custom, amber: &str) -> Signed {
        let attestation = Attestation {
            contract: ATTESTATION_CONTRACT,
            adapter: "approvals".into(),
            issuer: "ledger".into(),
            epoch: "e1".into(),
            generation: 3,
            project: "sim".into(),
            task: "sim/jump".into(),
            guard: "approved".into(),
            edge: "submit".into(),
            workflow: custom.revision.digest.clone(),
            activation_generation: 1,
            task_revision: 2,
            at: Pair {
                phase: "build".into(),
                state: "working".into(),
            },
            input_revisions: BTreeMap::from([("design".to_string(), "r7".to_string())]),
            principal: attestee(&agent_session(amber)),
            result: Attested::Pass,
            facts: vec!["ledger:decision/41".into()],
            reasons: Vec::new(),
            issued_at: NOW - 1.0,
            expires_at: NOW + 300.0,
        };
        signed(7, false, serde_json::to_string(&attestation).unwrap())
    }

    /// A board with `guarded_flow` active and sim/jump claimed by a new worker session.
    fn claimed() -> (Case, String, Custom) {
        let c = Case::new();
        let amber = c.register("worker");
        let custom = guarded_flow();
        c.store()
            .activate_workflow(&custom, &Host::local())
            .unwrap();
        c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
        c.ok(
            &amber,
            &["workflow", "apply", "sim/jump", "claim", "--revision", "1"],
        );
        (c, amber, custom)
    }

    #[test]
    fn a_session_takes_a_guarded_edge_with_the_authoritys_signed_word_and_no_other() {
        let (c, amber, custom) = claimed();
        let dir = scratch("workflow-guarded-cli");
        let file = |name: &str, signed: &Signed| {
            let path = dir.join(name);
            std::fs::write(&path, serde_json::to_string(signed).unwrap()).unwrap();
            path.to_string_lossy().into_owned()
        };
        assert!(c.ok(&amber, &["workflow", "actions", "sim/jump"]).contains(
            "submit → verify/awaiting  Submit, by its executor  (not now: guard_unavailable)"
        ));
        let submit = ["workflow", "apply", "sim/jump", "submit", "--revision", "2"];
        let unobserved = c.run_as(&amber, &submit, "");
        assert!(
            unobserved.err.starts_with(
                "✖ guard_unavailable: edge submit is guarded by approved, and the board has \
                 accepted no observation from approvals"
            ),
            "{}",
            unobserved.err
        );
        let observed = file("observed.json", &observation(7, 3));
        assert_eq!(
            c.ok(&amber, &["workflow", "observe", &observed]),
            "approvals: accepted generation 3 of e1 (s3)\n"
        );
        assert_eq!(
            c.ok(&amber, &["workflow", "observe", &observed]),
            "approvals: already at generation 3 of e1 (s3)\n"
        );
        let json: serde_json::Value =
            serde_json::from_str(&c.ok(&amber, &["workflow", "observe", &observed, "--json"]))
                .unwrap();
        assert_eq!(json["observed"], "already at");
        assert_eq!(json["authority"]["generation"], 3);
        assert_eq!(json["authority"]["state"], "s3");
        let forged = file("forged.json", &observation(8, 4));
        let refused = c.run_as(&amber, &["workflow", "observe", &forged], "");
        assert!(refused.err.contains("committed key"), "{}", refused.err);

        let attested = file("attested.json", &submit_attestation(&custom, &amber));
        let mut with = submit.to_vec();
        with.extend(["--attestation", &attested]);
        let drifted = c.run_as(&amber, &with, "");
        assert!(drifted.err.starts_with("✖ stale_input:"), "{}", drifted.err);
        let missing = c.run_as(
            &amber,
            &[
                &submit[..],
                &["--attestation", "/nonexistent/attested.json"],
            ]
            .concat(),
            "",
        );
        assert!(
            missing
                .err
                .contains("--attestation /nonexistent/attested.json"),
            "{}",
            missing.err
        );
        std::fs::write(dir.join("garbage.json"), "{}").unwrap();
        let garbage = dir.join("garbage.json").to_string_lossy().into_owned();
        let unread = c.run_as(
            &amber,
            &[&submit[..], &["--attestation", &garbage]].concat(),
            "",
        );
        assert!(
            unread.err.contains("not a signed statement"),
            "{}",
            unread.err
        );
        let bad_revision = c.run_as(
            &amber,
            &[&with[..], &["--input-revision", "design"]].concat(),
            "",
        );
        assert!(
            bad_revision
                .err
                .contains("--input-revision is NAME=REVISION"),
            "{}",
            bad_revision.err
        );
        with.extend(["--input-revision", "design=r7"]);
        assert_eq!(
            c.ok(&amber, &with),
            "took submit (sim/jump@3)\nsim/jump at verify/awaiting, workflow revision 3 (generation 1)\n  accept → verify/accepted  Accept, by owner  (not now: human_wait, unauthorized)\n"
        );
        // The same command again answers the first result.
        assert!(c.ok(&amber, &with).starts_with("took submit (sim/jump@3)"));
        let admission = c
            .store()
            .workflow_admission("sim/jump", 3)
            .unwrap()
            .unwrap();
        assert_eq!(admission.guards[0].guard, "approved");
    }

    #[test]
    fn an_agent_takes_a_guarded_edge_through_the_api_with_the_attestation_it_carries() {
        let (c, amber, custom) = claimed();
        let call = |call: serde_json::Value| {
            let ran = super::super::run(&c.env, &["api"], &call.to_string());
            serde_json::from_str::<serde_json::Value>(&ran.out).unwrap()
        };
        let observed = call(serde_json::json!({"method": "workflow_observe",
            "args": {"signed": observation(7, 3)}}));
        assert_eq!(observed["ok"]["observed"], "accepted", "{observed}");
        assert_eq!(observed["ok"]["authority"]["generation"], 3);
        let next = call(serde_json::json!({"method": "workflow_actions",
            "args": {"slug": "sim/jump", "handle": amber}}));
        let request = serde_json::json!({
            "project": c.store().database().database(), "task": "sim/jump", "edge": "submit",
            "expected": next["ok"]["snapshot"], "input_revisions": {"design": "r7"},
            "idempotency_key": "api-submit", "inputs": {},
        });
        let bare = call(serde_json::json!({"method": "workflow_apply",
            "args": {"handle": amber, "request": request}}));
        assert!(
            bare["refused"]
                .as_str()
                .unwrap()
                .starts_with("guard_unavailable:"),
            "{bare}"
        );
        let applied = call(serde_json::json!({"method": "workflow_apply",
            "args": {"handle": amber, "request": request,
                "attestations": [submit_attestation(&custom, &amber)]}}));
        assert_eq!(applied["ok"]["transition"], "sim/jump@3", "{applied}");
        let forged = call(serde_json::json!({"method": "workflow_observe",
            "args": {"signed": observation(8, 4)}}));
        assert!(
            forged["refused"]
                .as_str()
                .unwrap()
                .contains("committed key"),
            "{forged}"
        );
    }
}
