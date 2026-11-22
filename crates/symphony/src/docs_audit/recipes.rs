//! The reader of the rule of the recipes a text may tell a project to run (`just NAME`): a recipe
//! `symphony init` installs, a name the lists allow anywhere (the consumer's landing, a word of
//! prose), in one of this repository's guides a recipe of its justfile, or, in a text about this
//! repository, one of its own recipes or one filed to be made where the lists name it for that
//! file. The lists are this repository's data, [`LISTS`], read from its checkout, never carried in
//! the binary.
//!
//! One source, two places: the docs audit holds it as a module of its own and judges, in the
//! Symphony's own checkout ([`PROJECT`], holding [`LISTS`]), what [`audited`] names; the hooks test
//! (`crates/symphony/tests/hooks/named_recipes.rs`) takes this file by its path and judges what the
//! Symphony delivers and the strings its binaries print. Each entry of the lists is kept current by
//! the one place that judges its file ([`Rule::errors`]). It names nothing else of the crate's, so
//! either can hold it.

use std::sync::LazyLock;

use regex::Regex;

/// The project whose checkout holds the lists.
pub const PROJECT: &str = "symphony";

/// The lists, from the root of the Symphony's own checkout: a line is a list's name and its words,
/// and `#` starts a comment ([`Rule::read`]).
pub const LISTS: &str = "tools/named-recipes.txt";

/// What the Symphony delivers, from the root of its checkout: the skills, the Codex rules and the
/// recipes init installs ([`INSTALLED`]).
pub const SHIPPED: &str = "crates/symphony/shipped/";

/// The recipes `symphony init` installs, from the root of the Symphony's own checkout.
pub const INSTALLED: &str = "crates/symphony/shipped/symphony.just";

/// What to do about a name the rule refuses.
pub const ADVICE: &str = "add it to crates/symphony/shipped/symphony.just, name the command another way, or list it in \
     tools/named-recipes.txt";

/// The records under `docs/`, which say what was decided or found rather than how things are now:
/// a file, or a folder ending in `/`.
const RECORDS: [&str; 2] = ["docs/DECISIONS.md", "docs/history/"];

/// The folder of an area's analyses, a record as well, at any depth under `docs/`
/// (`docs/workflow/analysis/`).
const ANALYSIS: &str = "analysis";

/// What a guide is called: this repository's own guidance, at its root or in a package.
const GUIDES: [&str; 3] = ["AGENTS.md", "README.md", "RULES.md"];

static HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z][a-z0-9_-]*)(?:\s[^:]*)?:(?:[^=]|$)").unwrap());

/// `just`, the options that leave a recipe to run (a justfile or folder named, quiet, verbose, a
/// dry run), and the recipe's name. The space between words holds at most one line break: a name
/// after a blank line is another paragraph's word.
static COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    let space = r"(?:[ \t]+(?:\r?\n[ \t]*)?|\r?\n[ \t]*)";
    Regex::new(&format!(
        r"(?:^|[^A-Za-z0-9_-])just(?:{space}(?:(?:-f|--justfile|-d|--working-directory)(?:{space}|=)[^\s`]+|-[nqv]+|--(?:dry-run|quiet|verbose|yes)))*{space}([a-z][a-z0-9_-]*)"
    ))
    .unwrap()
});

/// The recipes a justfile's text defines, private ones (`_NAME`) and settings aside.
pub fn recipes(text: &str) -> Vec<&str> {
    let head = |line| Some(HEAD.captures(line)?.get(1)?.as_str());
    text.lines().filter_map(head).collect()
}

/// Each `just NAME` in `text`, with its line counted from `first`.
pub fn named_in(text: &str, first: usize) -> impl Iterator<Item = (usize, &str)> {
    COMMAND.captures_iter(text).filter_map(move |found| {
        let name = found.get(1)?;
        let line = first + text[..name.start()].bytes().filter(|b| *b == b'\n').count();
        Some((line, name.as_str()))
    })
}

/// What a name after `just` is, in the file that names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// A recipe `symphony init` installs.
    Installed,
    /// A name the lists allow anywhere: the consumer's own, or a word of prose.
    Listed,
    /// A recipe of this repository's justfile, in one of its guides ([`guide`]).
    Justfile,
    /// This repository's own, or filed to be made, in the file an entry names it for (its index in
    /// [`Rule::entries`]).
    Entry(usize),
    /// None of those: a recipe a freshly initialised project lacks.
    Unknown,
}

/// A recipe of this repository's that a file names: its own, or filed to be made by `task`.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub file: &'a str,
    pub name: &'a str,
    pub task: Option<&'a str>,
}

/// What a name after `just` is judged by, borrowed from the texts it was read from.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rule<'a> {
    pub installed: Vec<&'a str>,
    pub justfile: Vec<&'a str>,
    pub listed: Vec<&'a str>,
    pub entries: Vec<Entry<'a>>,
}

impl<'a> Rule<'a> {
    /// The rule of `lists` (the text of [`LISTS`]), with the recipes `installed` (the text of
    /// [`INSTALLED`]) and `justfile` (this repository's justfile) define; the first line of `lists`
    /// that does not read as a list, naming [`LISTS`] and that line, if any. A line is
    /// `consumers NAME...` or `prose WORD...` (allowed anywhere), `own FILE NAME...` or
    /// `filed FILE NAME TASK`.
    pub fn read(lists: &'a str, installed: &'a str, justfile: &'a str) -> Result<Self, String> {
        let mut rule = Rule {
            installed: recipes(installed),
            justfile: recipes(justfile),
            ..Rule::default()
        };
        for (at, line) in lists.lines().enumerate() {
            let line = line.split_once('#').map_or(line, |(line, _)| line);
            let words: Vec<&str> = line.split_whitespace().collect();
            match words[..] {
                [] => {}
                ["consumers" | "prose", ref names @ ..] => rule.listed.extend(names),
                ["own", file, ref names @ ..] if !names.is_empty() => {
                    let entry = |&name| Entry {
                        file,
                        name,
                        task: None,
                    };
                    rule.entries.extend(names.iter().map(entry));
                }
                ["filed", file, name, task] => rule.entries.push(Entry {
                    file,
                    name,
                    task: Some(task),
                }),
                _ => {
                    return Err(format!(
                        "{LISTS}:{}: not `consumers NAME...`, `prose WORD...`, `own FILE NAME...` \
                         or `filed FILE NAME TASK`",
                        at + 1
                    ));
                }
            }
        }
        Ok(rule)
    }

    /// What `name`, after `just` in `file` (its path from the root), is.
    pub fn standing(&self, file: &str, name: &str) -> Standing {
        let entry = |entry: &Entry| entry.file == file && entry.name == name;
        if self.installed.contains(&name) {
            Standing::Installed
        } else if self.listed.contains(&name) {
            Standing::Listed
        } else if guide(file) && self.justfile.contains(&name) {
            Standing::Justfile
        } else if let Some(at) = self.entries.iter().position(entry) {
            Standing::Entry(at)
        } else {
            Standing::Unknown
        }
    }

    /// The errors of `texts`, each a file from the root, the line its text starts on and the text:
    /// each `just NAME` the rule allows nowhere; then, of the entries whose file `owns` says this
    /// place judges, each that no text of its file names, and each filed recipe its file names on no
    /// line beside the task filed to make it.
    pub fn errors(&self, texts: &[(String, usize, String)], owns: fn(&str) -> bool) -> Vec<String> {
        let mut errors = Vec::new();
        let mut first = vec![None; self.entries.len()];
        for (file, start, text) in texts {
            for (line, name) in named_in(text, *start) {
                match self.standing(file, name) {
                    Standing::Unknown => errors.push(format!(
                        "{file}:{line}: names `just {name}`, a recipe a freshly initialised project \
                         lacks: {ADVICE}"
                    )),
                    Standing::Entry(at) => {
                        first[at].get_or_insert(line);
                    }
                    _ => {}
                }
            }
        }
        for (Entry { file, name, task }, line) in self.entries.iter().zip(first) {
            if !owns(file) {
                continue;
            }
            let Some(line) = line else {
                errors.push(format!(
                    "{file}: names no `just {name}`, which {LISTS} lists for it: take that entry out"
                ));
                continue;
            };
            let Some(task) = task else { continue };
            let (command, filing) = (format!("`just {name}`"), format!("[[task:{task}]]"));
            let beside = texts.iter().any(|(at, _, text)| {
                at == file
                    && (text.lines()).any(|line| line.contains(&command) && line.contains(&filing))
            });
            if !beside {
                errors.push(format!(
                    "{file}:{line}: names {command}, which this repository's justfile does not \
                     have yet, on no line beside the task filed to make it: write {filing} on the \
                     line that names it"
                ));
            }
        }
        errors
    }
}

/// Whether `file`, a path from the root of the Symphony's own checkout, is one of its guides: an
/// `AGENTS.md`, `README.md` or `RULES.md` at its root or in a package under `crates/` or `apps/`,
/// what it delivers aside. A guide may name any recipe of this repository's justfile.
pub fn guide(file: &str) -> bool {
    let (folder, name) = file.rsplit_once('/').unwrap_or(("", file));
    GUIDES.contains(&name)
        && (folder.is_empty() || folder.starts_with("crates/") || folder.starts_with("apps/"))
        && !file.starts_with(SHIPPED)
}

/// Whether the docs audit judges the names in `file`, a path from the root of the Symphony's own
/// checkout, beside the documents it walks: a file of what the Symphony delivers ([`SHIPPED`])
/// or a guide ([`guide`]).
pub fn beside_documents(file: &str) -> bool {
    file.starts_with(SHIPPED) || guide(file)
}

/// Whether the docs audit judges the names in `file`, a path from the root of the Symphony's own
/// checkout, and keeps the lists' entries for it current: Markdown under `docs/` but the records
/// (`RECORDS`, `ANALYSIS`), and what [`beside_documents`] names. The hooks test keeps the entries
/// for every other file current.
pub fn audited(file: &str) -> bool {
    let record = |record: &&str| {
        if record.ends_with('/') {
            file.starts_with(record)
        } else {
            file == *record
        }
    };
    let mut folders = file.split('/').skip(1).collect::<Vec<_>>();
    folders.pop();
    let document = file.starts_with("docs/")
        && file.ends_with(".md")
        && !RECORDS.iter().any(record)
        && !folders.contains(&ANALYSIS);
    document || beside_documents(file)
}
