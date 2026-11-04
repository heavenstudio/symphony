//! Star Foundry's card kinds: a hull model (`*Model:*`), a picture filling a slot (`*Shows:*`),
//! and a golden frame (`*Golden:* frame`).

use std::path::Path;

use symphony_plugin::vocabulary::Picture as Shown;
use symphony_plugin::{Card, CardKind};

use crate::art;

pub(super) static KINDS: [&(dyn CardKind + 'static); 4] = [
    &HullModel,
    &Picture,
    &GoldenFrame,
    &super::home::HullMentions,
];

fn agent_lines<'a>(card: &'a Card<'a>) -> impl Iterator<Item = &'a str> + 'a {
    card.thread
        .iter()
        .filter(|line| line.agent)
        .map(|line| line.text)
}

/// One card per hull model version, decided by the owner alone: never approved on a prediction.
struct HullModel;

impl CardKind for HullModel {
    fn name(&self) -> &str {
        "hull model"
    }

    fn recognises(&self, card: &Card<'_>) -> bool {
        crate::model::tagged(card.text).is_some()
    }

    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        self.recognises(card)
            .then(|| format!("hull model · {}", card.topic.to_lowercase()))
    }

    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        !self.recognises(card)
    }
}

/// A picture for a slot a page names (`Republic · ship`), with up to three candidates.
struct Picture;

impl CardKind for Picture {
    fn name(&self) -> &str {
        "art"
    }

    fn recognises(&self, card: &Card<'_>) -> bool {
        art::shows_in(card.text, agent_lines(card)).is_some()
    }

    /// Its first four candidates, each captioned with its letter when there are several.
    fn pictures(&self, _root: &Path, card: &Card<'_>) -> Vec<Shown> {
        let candidates = art::candidates_in(agent_lines(card));
        candidates
            .iter()
            .take(4)
            .map(|(alt, src)| {
                let letter = alt.rsplit(' ').next().unwrap_or("");
                let lettered = candidates.len() > 1
                    && letter.len() == 1
                    && letter.bytes().all(|b| b.is_ascii_uppercase());
                Shown {
                    src: Some(src.clone()),
                    caption: if lettered {
                        letter.into()
                    } else {
                        String::new()
                    },
                    waiting: false,
                }
            })
            .collect()
    }

    fn slot(&self, card: &Card<'_>) -> Option<String> {
        art::shows_in(card.text, agent_lines(card)).map(|(sub, kind)| format!("{sub} · {kind}"))
    }

    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        let (parent, stem) = parent_and_stem(card.doc);
        if self.recognises(card) && parent == art::SHIP_PAGES {
            return Some(format!("roster art · {stem}"));
        }
        // The second case: a picture card posted before cards named their slot.
        (self.recognises(card)
            || (parent == art::PAGES && !art::candidates_in(agent_lines(card)).is_empty()))
        .then(|| "faction art".to_string())
    }
}

/// The folder of a `/`-separated path, and its last part without its extension.
fn parent_and_stem(doc: &str) -> (&str, &str) {
    let (parent, name) = doc.rsplit_once('/').unwrap_or(("", doc));
    let stem = match name.rfind('.') {
        Some(at) if at > 0 => &name[..at],
        _ => name,
    };
    (parent, stem)
}

/// A captured frame for a golden test: its approval authorizes an executor's blessing, so it waits
/// for the agent that applies it.
struct GoldenFrame;

/// What ends a golden frame's card text.
pub const GOLDEN_FRAME: &str = "*Golden:* frame";

impl CardKind for GoldenFrame {
    fn name(&self) -> &str {
        "golden frame"
    }

    fn recognises(&self, card: &Card<'_>) -> bool {
        card.text.ends_with(GOLDEN_FRAME)
    }

    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        self.recognises(card)
    }
}
