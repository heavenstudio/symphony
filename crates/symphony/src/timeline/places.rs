//! The places a custom workflow put a task at ([`PlaceSpan`]), from its recorded transitions
//! (`Store::workflow_transitions`), each named by the definition its move was made under: a move
//! made before a relabel keeps the words it was made with.

use std::collections::HashMap;

use symphony_api::timeline::PlaceSpan;
use symphony_api::workflow::Definition;
use symphony_board::workflow::Transition;

/// Each of `transitions`' places, in their order, its phase and state named by `named(digest)`, the
/// definition recorded under the digest its move was made under, read once a digest: an ID that
/// definition does not name, or a definition not recorded, reads as the ID.
pub fn spans(
    transitions: &[Transition],
    mut named: impl FnMut(&str) -> Option<Definition>,
) -> Vec<PlaceSpan> {
    let mut definitions: HashMap<&str, Option<Definition>> = HashMap::new();
    transitions
        .iter()
        .map(|moved| {
            let definition = definitions
                .entry(moved.digest.as_str())
                .or_insert_with(|| named(&moved.digest))
                .as_ref();
            let (phase, state) = (&moved.to.phase, &moved.to.state);
            let phase_label = definition
                .and_then(|d| d.phases.iter().find(|one| &one.id == phase))
                .map_or(phase, |one| &one.label);
            let state_label = definition
                .and_then(|d| d.states.iter().find(|one| &one.id == state))
                .map_or(state, |one| &one.label);
            PlaceSpan {
                phase: phase.clone(),
                phase_label: phase_label.clone(),
                state: state.clone(),
                state_label: state_label.clone(),
                status: moved.status.clone(),
                from: moved.at,
                edge: moved.edge.clone(),
                by: moved.principal.subject.clone(),
            }
        })
        .collect()
}

/// The places of the task `slug` as its board recorded them: none where its transitions cannot be
/// read, as a gate that cannot be read leaves its lane out, never the timeline.
pub fn read(store: &symphony_board::Store, slug: &str) -> Vec<PlaceSpan> {
    let Ok(transitions) = store.workflow_transitions(slug) else {
        return Vec::new();
    };
    spans(&transitions, |digest| {
        let recorded = store.workflow_definition(digest).ok().flatten()?;
        Some(recorded.definition)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_api::workflow::{Pair, Phase, State};
    use symphony_plugin::execution::{Principal, PrincipalKind};

    /// `revision`'s move of the task to `phase`/`state`, under the definition `digest`, by `by`,
    /// at `at`; by `edge`, or an activation where there is none.
    fn moved(
        revision: u64,
        digest: &str,
        (phase, state): (&str, &str),
        edge: Option<&str>,
        by: &str,
        at: f64,
    ) -> Transition {
        let kind = if by == "owner" {
            PrincipalKind::Human
        } else {
            PrincipalKind::Agent
        };
        Transition {
            id: Transition::id_of("sim/jump", revision),
            slug: "sim/jump".into(),
            revision,
            generation: 1,
            digest: digest.into(),
            edge: edge.map(Into::into),
            operation: None,
            activation: edge.is_none().then_some(1),
            from: None,
            to: Pair {
                phase: phase.into(),
                state: state.into(),
            },
            resume: None,
            status: if state == "awaiting" {
                "review"
            } else {
                "in_progress"
            }
            .into(),
            principal: Principal::new(kind, "local".into(), by.into()),
            effects: vec![],
            key: None,
            recovery: None,
            at,
        }
    }

    /// A definition naming the phases and states `phases` and `states` give, `(id, label)` each.
    fn definition(phases: &[(&str, &str)], states: &[(&str, &str)]) -> Definition {
        let mut definition = symphony_board::workflow::builtin();
        definition.phases = phases
            .iter()
            .enumerate()
            .map(|(order, (id, label))| Phase {
                id: (*id).into(),
                label: (*label).into(),
                order: u32::try_from(order).unwrap(),
                phone_order: u32::try_from(order).unwrap(),
                column: true,
            })
            .collect();
        definition.states = states
            .iter()
            .map(|(id, label)| State {
                id: (*id).into(),
                label: (*label).into(),
            })
            .collect();
        definition
    }

    #[test]
    fn each_move_is_a_place_named_by_its_own_definition_read_once_a_digest() {
        let first = definition(&[("build", "Build")], &[("working", "Working")]);
        let relabelled = definition(
            &[("build", "Make"), ("review", "Review")],
            &[("working", "Making"), ("awaiting", "Awaiting review")],
        );
        let transitions = [
            moved(1, "d1", ("build", "working"), None, "owner", 10.0),
            moved(
                2,
                "d1",
                ("build", "working"),
                Some("rework"),
                "misty-otter-30",
                20.0,
            ),
            moved(
                3,
                "d2",
                ("build", "working"),
                Some("restart"),
                "owner",
                30.0,
            ),
            moved(
                4,
                "d2",
                ("review", "awaiting"),
                Some("request_review"),
                "misty-otter-30",
                40.0,
            ),
        ];
        let mut asked = Vec::new();
        let spans = spans(&transitions, |digest| {
            asked.push(digest.to_string());
            match digest {
                "d1" => Some(first.clone()),
                _ => Some(relabelled.clone()),
            }
        });
        assert_eq!(asked, ["d1", "d2"]);
        let named: Vec<(&str, &str, f64, Option<&str>, &str)> = spans
            .iter()
            .map(|s| {
                (
                    s.phase_label.as_str(),
                    s.state_label.as_str(),
                    s.from,
                    s.edge.as_deref(),
                    s.by.as_str(),
                )
            })
            .collect();
        assert_eq!(
            named,
            [
                ("Build", "Working", 10.0, None, "owner"),
                ("Build", "Working", 20.0, Some("rework"), "misty-otter-30"),
                ("Make", "Making", 30.0, Some("restart"), "owner"),
                (
                    "Review",
                    "Awaiting review",
                    40.0,
                    Some("request_review"),
                    "misty-otter-30"
                ),
            ]
        );
        assert_eq!(
            (spans[3].phase.as_str(), spans[3].state.as_str()),
            ("review", "awaiting")
        );
        assert_eq!(
            spans.iter().map(|s| s.status.as_str()).collect::<Vec<_>>(),
            ["in_progress", "in_progress", "in_progress", "review"]
        );
    }

    #[test]
    fn an_id_its_definition_does_not_name_or_a_definition_not_recorded_reads_as_the_id() {
        let named = definition(&[("build", "Build")], &[]);
        let transitions = [
            moved(1, "d1", ("verify", "working"), Some("e"), "owner", 1.0),
            moved(2, "gone", ("build", "working"), Some("e"), "owner", 2.0),
        ];
        let spans = spans(&transitions, |digest| {
            (digest == "d1").then(|| named.clone())
        });
        let labels: Vec<(&str, &str)> = spans
            .iter()
            .map(|s| (s.phase_label.as_str(), s.state_label.as_str()))
            .collect();
        assert_eq!(labels, [("verify", "working"), ("build", "working")]);
        assert_eq!(super::spans(&[], |_| unreachable!()), vec![]);
    }
}
