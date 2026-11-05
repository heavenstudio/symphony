//! A project's plugin whose card kind shows the candidates an agent posted on a card (`![alt](src)`),
//! as they were written.

use std::path::Path;

use symphony_plugin::vocabulary::Picture;
use symphony_plugin::{Card, CardKind, Plugin};

struct Candidates;

static PLUGINS: &[&dyn Plugin] = &[&Candidates];

impl Plugin for Candidates {
    fn name(&self) -> &str {
        "fixture-candidates"
    }

    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![self]
    }
}

impl CardKind for Candidates {
    fn name(&self) -> &str {
        "candidates"
    }

    fn recognises(&self, card: &Card<'_>) -> bool {
        !self.pictures(Path::new(""), card).is_empty()
    }

    fn pictures(&self, _: &Path, card: &Card<'_>) -> Vec<Picture> {
        let posted = regex::Regex::new(r"!\[[^\]]*\]\(([^)]+)\)").unwrap();
        card.thread
            .iter()
            .filter(|line| line.agent)
            .flat_map(|line| posted.captures_iter(line.text))
            .map(|found| Picture {
                src: Some(found[1].to_string()),
                caption: String::new(),
                waiting: false,
            })
            .collect()
    }
}

/// A project file naming [`Candidates`] after `before`; the process holds it.
pub fn candidates(before: &str) -> String {
    symphony::plugins::install(PLUGINS).unwrap();
    format!("{before}[plugins]\ncrates = [\"fixture-candidates\"]\n")
}
