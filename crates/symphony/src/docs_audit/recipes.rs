//! The one rule of the recipes a text may tell a project to run (`just NAME`): a recipe
//! `symphony init` installs (`shipped/symphony.just`), the consumer's landing, named as the
//! project's own ([`CONSUMERS`]), a word of prose ([`PROSE`]), or, in a text about this repository,
//! one of its own justfile's recipes where [`OWN`] lists it for that file, or one filed to be made
//! ([`FILED`]). The docs audit judges this repository's documents by it ([`Rule::in_documents`]),
//! and the hooks test (`crates/symphony/tests/hooks/named_recipes.rs`) the shipped skills, rules and
//! recipes and the strings the binaries print.
//!
//! The audit judges it only in the Symphony's own checkout ([`symphony_checkout`]: the one holding
//! the recipes init installs), and there only the documents [`judged`] names: the Markdown under
//! `docs/`, but the decision register (`docs/DECISIONS.md`), `docs/analysis/` and
//! `docs/history/`, which are records. A consuming project's documents name its own recipes, and its
//! audit judges none of them.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

#[cfg(test)]
#[path = "recipes_tests.rs"]
mod tests;

/// The consumer's recipes, which the text names as the project's where it has them.
pub const CONSUMERS: [&str; 5] = [
    "land-approve",
    "land-request",
    "land-stamps",
    "merge-feature",
    "merge-risk",
];

/// Words that follow "just" in prose, never a recipe.
pub const PROSE: [&str; 21] = [
    "a",
    "above",
    "after",
    "as",
    "became",
    "before",
    "done",
    "finished",
    "launched",
    "like",
    "now",
    "one",
    "past",
    "published",
    "pushed",
    "ran",
    "started",
    "successful",
    "the",
    "this",
    "written",
];

/// Text of this repository's own development, which names recipes of its own justfile: a source
/// kit's client build, which no project runs, the checks a task here proves its work with, its
/// size budgets, its measurements, and its releases, which the nightly cuts and no project runs,
/// with the test targets a red night's task reruns alone. Each is the file from the root and the
/// recipe it names.
pub const OWN: [(&str, &str); 22] = [
    ("apps/symphony-server/src/api/client.rs", "symphony-ui-dist"),
    ("apps/symphony-server/src/development.rs", "symphony-ui-dev"),
    (
        "crates/symphony/src/cli/commands/size_budget.rs",
        "size-check",
    ),
    ("crates/symphony/src/nightly.rs", "nightly"),
    ("crates/symphony/src/nightly.rs", "verify"),
    ("crates/symphony/src/nightly/board.rs", "nightly"),
    ("crates/symphony/src/nightly/board.rs", "test-rs"),
    ("crates/symphony/src/nightly/cadence.rs", "nightly"),
    ("crates/symphony/src/nightly/record.rs", "nightly"),
    ("crates/symphony/src/nightly/refresh.rs", "nightly"),
    ("crates/symphony/src/nightly/schedule.rs", "nightly"),
    ("docs/CHECKS.md", "client-check"),
    ("docs/CHECKS.md", "client-update"),
    ("docs/CHECKS.md", "lint-rs"),
    ("docs/CHECKS.md", "nightly"),
    ("docs/CHECKS.md", "size-check"),
    ("docs/CHECKS.md", "test-rs"),
    ("docs/CHECKS.md", "verify"),
    ("docs/PLUGIN_PROTOCOL.md", "test-rs"),
    ("docs/RELEASES.md", "nightly"),
    ("docs/RELEASES.md", "test-rs"),
    ("docs/RELEASES.md", "verify"),
];

/// Recipes this repository's justfile does not have yet, each named where a task is filed to make
/// it: the file, the recipe and the task, which the file names as `[[task:SLUG]]` beside it.
pub const FILED: [(&str, &str, &str); 1] = [("docs/CHECKS.md", "check-fast", "tools/check-fast")];

/// Where the lists are, for a message that says to change one.
const LISTS: &str = "crates/symphony/src/docs_audit/recipes.rs";

/// The recipes `symphony init` installs, from the root of the Symphony's own checkout.
pub const SHIPPED: &str = "crates/symphony/shipped/symphony.just";

/// What to do about a name the rule refuses.
pub const ADVICE: &str = "ship it in crates/symphony/shipped/symphony.just, name the command \
     another way, or say it is the project's own (CONSUMERS) or, in a text about this repository, \
     its own justfile's (OWN, in crates/symphony/src/docs_audit/recipes.rs); a word of prose after \
     \"just\" goes in PROSE";

/// The records under `docs/`, which say what was decided or found rather than how things are now:
/// a file, or a folder ending in `/`.
const RECORDS: [&str; 3] = ["docs/DECISIONS.md", "docs/analysis/", "docs/history/"];

static HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z][a-z0-9_-]*)(?:\s[^:]*)?:(?:[^=]|$)").unwrap());
static COMMAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^A-Za-z0-9_-])just\s+([a-z][a-z0-9_-]*)").unwrap());

/// The recipes a justfile's text defines, private ones (`_NAME`) and settings aside.
pub fn recipes(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| HEAD.captures(line))
        .map(|found| found[1].to_string())
        .collect()
}

/// Each `just NAME` in `text`, with its line counted from `first`.
pub fn named_in(text: &str, first: usize) -> Vec<(usize, String)> {
    COMMAND
        .captures_iter(text)
        .map(|found| {
            let at = found.get(1).unwrap().start();
            (
                first + text[..at].matches('\n').count(),
                found[1].to_string(),
            )
        })
        .collect()
}

/// What a name after `just` is, in the file that names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// A recipe `symphony init` installs.
    Installed,
    /// The consumer's, named as the project's own.
    Consumer,
    /// A word of prose.
    Prose,
    /// This repository's own, in a file the rule's own list names it for (its index there).
    Own(usize),
    /// Filed to be made, in a file the rule's filed list names it for (its index there).
    Filed(usize),
    /// None of those: a recipe a freshly initialised project lacks.
    Unknown,
}

/// The lists a name after `just` is judged by.
pub struct Rule {
    pub installed: BTreeSet<String>,
    pub consumers: &'static [&'static str],
    pub prose: &'static [&'static str],
    pub own: &'static [(&'static str, &'static str)],
    pub filed: &'static [(&'static str, &'static str, &'static str)],
}

/// The Symphony's rule: the recipes this build installs and the lists above.
pub static RULE: LazyLock<Rule> = LazyLock::new(|| Rule {
    installed: recipes(crate::init::SHIPPED_RECIPES),
    consumers: &CONSUMERS,
    prose: &PROSE,
    own: &OWN,
    filed: &FILED,
});

/// The recipes `symphony init` installs.
pub fn installed() -> &'static BTreeSet<String> {
    &RULE.installed
}

impl Rule {
    /// What `name`, after `just` in `file` (its path from the root), is.
    pub fn standing(&self, file: &str, name: &str) -> Standing {
        if self.installed.contains(name) {
            Standing::Installed
        } else if self.consumers.contains(&name) {
            Standing::Consumer
        } else if self.prose.contains(&name) {
            Standing::Prose
        } else if let Some(at) = self
            .own
            .iter()
            .position(|(at, own)| *at == file && *own == name)
        {
            Standing::Own(at)
        } else if let Some(at) = self
            .filed
            .iter()
            .position(|(at, filed, _)| *at == file && *filed == name)
        {
            Standing::Filed(at)
        } else {
            Standing::Unknown
        }
    }

    /// The docs audit's errors for `documents`, each its file from the root and its text, of the
    /// Symphony's own checkout, those [`judged`] says alone: each `just NAME` none of the lists
    /// allows, by its file and line; each entry of the own and filed lists for a judged document
    /// that the document no longer names (or that is not among them); and each filed recipe named on
    /// no line beside the task filed to make it.
    pub fn in_documents<'a>(
        &self,
        documents: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        let mut own = HashSet::new();
        let mut filed: HashMap<usize, usize> = HashMap::new();
        let mut texts = HashMap::new();
        for (file, text) in documents {
            if !judged(file) {
                continue;
            }
            texts.insert(file, text);
            for (line, name) in named_in(text, 1) {
                match self.standing(file, &name) {
                    Standing::Unknown => errors.push(format!(
                        "{file}:{line}: names `just {name}`, a recipe a freshly initialised project \
                         lacks: {ADVICE}"
                    )),
                    Standing::Own(at) => {
                        own.insert(at);
                    }
                    Standing::Filed(at) => {
                        filed.entry(at).or_insert(line);
                    }
                    Standing::Installed | Standing::Consumer | Standing::Prose => {}
                }
            }
        }
        for (at, (file, name)) in self.own.iter().enumerate() {
            if judged(file) && !own.contains(&at) {
                errors.push(format!(
                    "{file}: names no `just {name}`, which OWN lists as its own: take that entry \
                     out of OWN, in {LISTS}"
                ));
            }
        }
        for (at, (file, name, task)) in self.filed.iter().enumerate() {
            if !judged(file) {
                continue;
            }
            let Some(line) = filed.get(&at) else {
                errors.push(format!(
                    "{file}: names no `just {name}`, which FILED lists as filed to be made: take \
                     that entry out of FILED, in {LISTS}"
                ));
                continue;
            };
            let (command, filing) = (format!("`just {name}`"), format!("[[task:{task}]]"));
            let beside = texts[file]
                .lines()
                .any(|text| text.contains(&command) && text.contains(&filing));
            if !beside {
                errors.push(format!(
                    "{file}:{line}: names `just {name}`, which this repository's justfile does not \
                     have yet, on no line beside the task filed to make it: write {filing} on the \
                     line that names {command}"
                ));
            }
        }
        errors
    }
}

/// Whether the docs audit judges the recipes the document at `file` (its path from the root of the
/// Symphony's own checkout) names: Markdown under `docs/`, but the records (`RECORDS`).
pub fn judged(file: &str) -> bool {
    file.starts_with("docs/")
        && file.ends_with(".md")
        && !RECORDS.iter().any(|record| {
            if record.ends_with('/') {
                file.starts_with(record)
            } else {
                file == *record
            }
        })
}

/// Whether the checkout at `root` is the Symphony's own, whose documents' recipes the docs audit
/// judges: it holds the recipes `symphony init` installs ([`SHIPPED`]). A consuming project takes
/// the Symphony through a pinned dependency and holds no such file.
pub fn symphony_checkout(root: &Path) -> bool {
    root.join(SHIPPED).is_file()
}
