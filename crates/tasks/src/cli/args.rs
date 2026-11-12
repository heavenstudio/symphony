//! The command line read as the board's first, Python, command line's argparse parser read it: its
//! commands, flags and defaults, an abbreviated long flag taken when it names one flag, `--as` on
//! either side of the command, `--help` and argparse's own words for a usage error (exit 2).

use super::usage;
use crate::pyjson::repr as pyrepr;
use std::collections::BTreeMap;

/// What a flag takes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Takes {
    /// Nothing: present or not (`store_true`).
    Bool,
    /// One value, of this type; the last one given wins.
    Value(Ty),
    /// One value each time, all kept (`append`).
    Append,
    /// Two values each time, all kept in order (`nargs=2`, `append`).
    Pair,
    /// Nothing; sets its dest to this (`store_const`).
    Const(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ty {
    Str,
    Int,
    Float,
}

struct Opt {
    flag: &'static str,
    dest: &'static str,
    takes: Takes,
    choices: &'static [&'static str],
    required: bool,
    /// A mutually exclusive group, 0 for none.
    group: u8,
}

struct Pos {
    dest: &'static str,
    /// `nargs="?"`.
    optional: bool,
    choices: &'static [&'static str],
}

struct Command {
    name: &'static str,
    pos: &'static [Pos],
    /// In the order they were declared, which an ambiguous abbreviation lists them in.
    opts: &'static [Opt],
}

const fn opt(flag: &'static str, dest: &'static str, takes: Takes) -> Opt {
    Opt {
        flag,
        dest,
        takes,
        choices: &[],
        required: false,
        group: 0,
    }
}

/// A command of positionals and `--as` alone.
const fn plain(name: &'static str, pos: &'static [Pos]) -> Command {
    Command {
        name,
        pos,
        opts: &[AS],
    }
}

const fn choice(flag: &'static str, dest: &'static str, choices: &'static [&'static str]) -> Opt {
    Opt {
        flag,
        dest,
        takes: Takes::Value(Ty::Str),
        choices,
        required: false,
        group: 0,
    }
}

const fn pos(dest: &'static str) -> Pos {
    Pos {
        dest,
        optional: false,
        choices: &[],
    }
}

const fn maybe(dest: &'static str) -> Pos {
    Pos {
        dest,
        optional: true,
        choices: &[],
    }
}

const STR: Takes = Takes::Value(Ty::Str);
const AS: Opt = opt("--as", "actor", STR);
const KINDS: &[&str] = &["orchestrator", "worker", "reviewer", "chat", "app"];
const STATUSES: &[&str] = &[
    "backlog",
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "done",
    "blocked",
    "cancelled",
];
const SESSION: &[Opt] = &[
    AS,
    choice("--kind", "kind", KINDS),
    opt("--parent", "parent", STR),
    opt("--affinity", "affinity", STR),
    opt("--model", "model", STR),
    opt("--session-url", "session_url", STR),
];
const SLUG: &[Pos] = &[pos("slug")];

const COMMANDS: &[Command] = &[
    Command {
        name: "callback",
        pos: &[maybe("address")],
        opts: &[
            AS,
            opt("--chat", "chat", Takes::Bool),
            opt("--if-free", "if_free", Takes::Bool),
            opt("--off", "off", Takes::Bool),
            opt("--session-url", "session_url", STR),
        ],
    },
    Command {
        name: "register",
        pos: &[],
        opts: SESSION,
    },
    Command {
        name: "attach",
        pos: &[],
        opts: &[
            AS,
            choice("--kind", "kind", KINDS),
            opt("--parent", "parent", STR),
            opt("--affinity", "affinity", STR),
            opt("--model", "model", STR),
            opt("--resume", "resume", STR),
            opt("--interval", "interval", Takes::Value(Ty::Float)),
            opt("--beat", "beat", Takes::Value(Ty::Float)),
            opt("--polls", "polls", Takes::Value(Ty::Int)),
        ],
    },
    Command {
        name: "list",
        pos: &[],
        opts: &[
            AS,
            choice("--status", "status", STATUSES),
            opt("--mine", "mine", Takes::Bool),
            opt("--doubled", "doubled", Takes::Bool),
            opt("--json", "json", Takes::Bool),
        ],
    },
    Command {
        name: "search",
        pos: &[pos("query")],
        opts: &[
            AS,
            choice("--status", "status", STATUSES),
            opt("--limit", "limit", Takes::Value(Ty::Int)),
            opt("--json", "json", Takes::Bool),
            choice("--mode", "mode", &["keyword", "vector", "hybrid", "exact"]),
        ],
    },
    Command {
        name: "stats",
        pos: &[],
        opts: &[
            AS,
            opt("--days", "days", Takes::Value(Ty::Int)),
            opt("--json", "json", Takes::Bool),
        ],
    },
    Command {
        name: "embed",
        pos: &[],
        opts: &[
            opt("--maintenance", "maintenance", Takes::Bool),
            opt("--threads", "threads", Takes::Value(Ty::Int)),
            AS,
            opt("--task", "task", STR),
            opt("--batch-size", "batch_size", Takes::Value(Ty::Int)),
        ],
    },
    plain("next", &[]),
    Command {
        name: "create",
        pos: &[pos("slug"), pos("title")],
        opts: &[
            AS,
            opt("--body", "body", STR),
            opt("--body-file", "body_file", STR),
            opt("--priority", "priority", Takes::Value(Ty::Int)),
            choice(
                "--stage",
                "stage",
                &["backlog", "concept", "design", "ready"],
            ),
            opt("--backlog", "stage", Takes::Const("backlog")),
            opt("--parent", "parent", STR),
            opt("--for", "assignee", STR),
            opt("--instruction", "instruction", Takes::Bool),
            opt("--depends", "depends", Takes::Append),
            opt("--link", "link", Takes::Append),
            opt("--fast-fix", "fast_fix", STR),
        ],
    },
    Command {
        name: "priority",
        pos: &[
            pos("slug"),
            Pos {
                dest: "priority",
                optional: false,
                choices: &["0", "1", "2", "3", "4"],
            },
        ],
        opts: &[AS],
    },
    plain("field", &[pos("slug"), pos("name"), pos("value")]),
    plain("claim", SLUG),
    plain("reserve", SLUG),
    Command {
        name: "release",
        pos: SLUG,
        opts: &[
            AS,
            choice(
                "--role",
                "role",
                &["orchestrator", "executor", "reviewer", "reserved"],
            ),
        ],
    },
    Command {
        name: "show",
        pos: SLUG,
        opts: &[AS, opt("--json", "json", Takes::Bool)],
    },
    Command {
        name: "parent",
        pos: &[pos("slug"), maybe("parent")],
        opts: &[AS, opt("--none", "none", Takes::Bool)],
    },
    Command {
        name: "depends",
        pos: SLUG,
        opts: &[
            AS,
            opt("--add", "add", Takes::Append),
            opt("--remove", "remove", Takes::Append),
            opt("--replace", "replace", Takes::Pair),
        ],
    },
    plain("done", &[pos("slug"), pos("note")]),
    Command {
        name: "role",
        pos: &[
            pos("slug"),
            Pos {
                dest: "role",
                optional: false,
                choices: &["orchestrator", "reviewer"],
            },
        ],
        opts: &[AS],
    },
    plain("handover", &[pos("slug"), pos("to")]),
    Command {
        name: "move",
        pos: &[
            pos("slug"),
            Pos {
                dest: "status",
                optional: false,
                choices: STATUSES,
            },
        ],
        opts: &[AS],
    },
    Command {
        name: "verdict",
        pos: &[
            pos("slug"),
            Pos {
                dest: "verdict",
                optional: false,
                choices: &["approve", "changes"],
            },
            maybe("text"),
        ],
        opts: &[AS],
    },
    Command {
        name: "say",
        pos: &[pos("slug"), pos("text")],
        opts: &[AS, opt("--to", "to", STR)],
    },
    Command {
        name: "launch",
        pos: SLUG,
        opts: &[
            AS,
            opt("--model", "model", STR),
            choice("--kind", "kind", &["codex", "claude", "worker", "reviewer"]),
            choice("--role", "role", &["executor", "reviewer"]),
            Opt {
                group: 1,
                ..opt("--branch", "branch", STR)
            },
            Opt {
                group: 1,
                ..opt("--detached", "detached", Takes::Bool)
            },
            opt("--worktree", "worktree", STR),
            opt("--tab", "tab", STR),
            opt("--prompt", "prompt", STR),
        ],
    },
    plain("restore", &[pos("slug"), maybe("handle")]),
    Command {
        name: "cleanup",
        pos: &[],
        opts: &[AS, opt("--dry-run", "dry_run", Takes::Bool)],
    },
    Command {
        name: "clean",
        pos: &[],
        opts: &[AS, opt("--dry-run", "dry_run", Takes::Bool)],
    },
    plain("close", &[pos("slug"), pos("handle")]),
    Command {
        name: "reap",
        pos: &[],
        opts: &[AS, opt("--dry-run", "dry_run", Takes::Bool)],
    },
    Command {
        name: "staff",
        pos: &[],
        opts: &[AS, opt("--dry-run", "dry_run", Takes::Bool)],
    },
    Command {
        name: "agents",
        pos: &[],
        opts: &[
            AS,
            opt("--json", "json", Takes::Bool),
            opt("--all", "all", Takes::Bool),
        ],
    },
    Command {
        name: "who",
        pos: &[],
        opts: &[
            AS,
            opt("--all", "all", Takes::Bool),
            opt("--json", "json", Takes::Bool),
        ],
    },
    Command {
        name: "status",
        pos: &[],
        opts: &[AS, opt("--json", "json", Takes::Bool)],
    },
    plain("inbox", &[]),
    plain("pull", &[]),
    Command {
        name: "wait",
        pos: &[],
        opts: &[AS, opt("--timeout", "timeout", Takes::Value(Ty::Float))],
    },
    plain("leave", &[]),
    plain("beat", &[]),
    Command {
        name: "export",
        pos: &[],
        opts: &[AS, opt("--out", "out", STR)],
    },
    plain("finish", SLUG),
    Command {
        name: "sweep",
        pos: &[],
        opts: &[
            AS,
            opt("--dry-run", "dry_run", Takes::Bool),
            opt("--recent-hours", "recent_hours", Takes::Value(Ty::Float)),
            opt("--task", "task", Takes::Append),
            opt("--background", "background", STR),
        ],
    },
    plain("land", SLUG),
];

/// A command line read: the command and each dest given (a flag's as "", `append`'s each value).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    pub command: String,
    values: BTreeMap<String, Vec<String>>,
}

impl Parsed {
    /// The value given for `dest`, the last if it was given more than once.
    pub fn get(&self, dest: &str) -> Option<&str> {
        self.values
            .get(dest)
            .and_then(|all| all.last())
            .map(String::as_str)
    }

    pub fn or<'a>(&'a self, dest: &str, default: &'a str) -> &'a str {
        self.get(dest).unwrap_or(default)
    }

    pub fn flag(&self, dest: &str) -> bool {
        self.values.contains_key(dest)
    }

    /// Every value given for an `append` dest, in order.
    pub fn all(&self, dest: &str) -> &[String] {
        self.values.get(dest).map_or(&[], Vec::as_slice)
    }

    /// An `int` dest, checked when it was read.
    pub fn int(&self, dest: &str, default: i64) -> i64 {
        self.get(dest).and_then(int).unwrap_or(default)
    }

    /// A `float` dest, checked when it was read.
    pub fn float(&self, dest: &str, default: f64) -> f64 {
        self.get(dest).and_then(float).unwrap_or(default)
    }
}

/// What reading the command line came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Run(Parsed),
    /// `--help`: this text on stdout, exit 0.
    Help(&'static str),
    /// A usage error: this text on stderr, exit 2.
    Usage(String),
}

/// Read `args` (the command line without the program name).
pub fn parse(args: &[String]) -> Outcome {
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // What nothing takes, with where it stood: argparse names them in the order given.
    let mut extras: Vec<(usize, String)> = Vec::new();
    let top =
        |message: String| Outcome::Usage(format!("{}just tasks: error: {message}\n", usage::USAGE));
    let mut i = 0;
    let command = loop {
        let Some(token) = args.get(i) else {
            return top("the following arguments are required: command".into());
        };
        i += 1;
        match optional(token, &[AS]) {
            Flag::Positional => break token,
            Flag::Help => return Outcome::Help(usage::HELP),
            Flag::Unknown => extras.push((i - 1, token.clone())),
            Flag::Ambiguous(names) => {
                return top(format!(
                    "ambiguous option: {} could match {names}",
                    flag_name(token)
                ));
            }
            Flag::Known(found, explicit) => {
                match take(found, explicit, args, &mut i, &mut values) {
                    Ok(()) => {}
                    Err(message) => return top(message),
                }
            }
        }
    };
    let Some(cmd) = COMMANDS.iter().find(|c| c.name == command) else {
        let all: Vec<String> = COMMANDS.iter().map(|c| pyrepr(c.name)).collect();
        return top(format!(
            "argument command: invalid choice: {} (choose from {})",
            pyrepr(command),
            all.join(", ")
        ));
    };
    let (usage, help) = usage::COMMANDS
        .iter()
        .find(|(name, _, _)| *name == cmd.name)
        .map(|(_, usage, help)| (*usage, *help))
        .expect("every command has its usage");
    let refuse = |message: String| {
        Outcome::Usage(format!(
            "{usage}just tasks {}: error: {message}\n",
            cmd.name
        ))
    };
    let mut positionals: Vec<(usize, String)> = Vec::new();
    // Where the first `--` stood: after it, everything is a value.
    let mut dash: Option<usize> = None;
    let mut grouped: BTreeMap<u8, &'static str> = BTreeMap::new();
    while let Some(token) = args.get(i) {
        i += 1;
        if dash.is_some() {
            positionals.push((i - 1, token.clone()));
            continue;
        }
        if token == "--" {
            dash = Some(i - 1);
            continue;
        }
        match optional(token, cmd.opts) {
            Flag::Positional => positionals.push((i - 1, token.clone())),
            Flag::Help => return Outcome::Help(help),
            Flag::Unknown => extras.push((i - 1, token.clone())),
            Flag::Ambiguous(names) => {
                return refuse(format!(
                    "ambiguous option: {} could match {names}",
                    flag_name(token)
                ));
            }
            Flag::Known(found, explicit) => {
                if found.group != 0 {
                    match grouped.get(&found.group) {
                        Some(other) if *other != found.flag => {
                            return refuse(format!(
                                "argument {}: not allowed with argument {other}",
                                found.flag
                            ));
                        }
                        _ => {
                            grouped.insert(found.group, found.flag);
                        }
                    }
                }
                if let Err(message) = take(found, explicit, args, &mut i, &mut values) {
                    return refuse(message);
                }
            }
        }
    }
    let mut given = positionals.into_iter();
    let mut missing: Vec<&str> = Vec::new();
    // argparse drops the `--` only when a positional's share of the command line takes it in: one that
    // stood right before it, or one after it, or an optional one left with nothing else (`-*A?-*`).
    let mut dash_taken = false;
    for p in cmd.pos {
        match given.next() {
            Some((at, value)) => {
                dash_taken |= dash.is_some_and(|dash| at + 1 == dash || at > dash);
                if !p.choices.is_empty() && !p.choices.contains(&value.as_str()) {
                    return refuse(invalid_choice(p.dest, &value, p.choices));
                }
                values.insert(p.dest.into(), vec![value]);
            }
            None if p.optional => dash_taken = true,
            None => missing.push(p.dest),
        }
    }
    if let Some(at) = dash.filter(|_| !dash_taken) {
        extras.push((at, "--".into()));
    }
    missing.extend(
        cmd.opts
            .iter()
            .filter(|o| o.required && !values.contains_key(o.dest))
            .map(|o| o.flag),
    );
    if !missing.is_empty() {
        return refuse(format!(
            "the following arguments are required: {}",
            missing.join(", ")
        ));
    }
    extras.extend(given);
    if !extras.is_empty() {
        extras.sort_by_key(|(at, _)| *at);
        let named: Vec<String> = extras.into_iter().map(|(_, token)| token).collect();
        return top(format!("unrecognized arguments: {}", named.join(" ")));
    }
    Outcome::Run(Parsed {
        command: cmd.name.into(),
        values,
    })
}

/// What a token on the command line is to a parser with `opts` (argparse's `_parse_optional`).
enum Flag {
    Positional,
    Help,
    /// Looks like a flag, names none: left over.
    Unknown,
    /// An abbreviation of more than one flag, named in the order they were declared.
    Ambiguous(String),
    /// A flag, with the value given after its `=`.
    Known(&'static Opt, Option<String>),
}

fn optional(token: &str, opts: &'static [Opt]) -> Flag {
    if !token.starts_with('-') || token.len() == 1 {
        return Flag::Positional;
    }
    if token.starts_with("-h") && !token.starts_with("--") {
        return Flag::Help;
    }
    let (name, explicit) = match token.split_once('=') {
        Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
        _ => (token, None),
    };
    if name.starts_with("--") {
        if let Some(found) = opts.iter().find(|o| o.flag == name) {
            return Flag::Known(found, explicit);
        }
        if name == "--help" {
            return Flag::Help;
        }
        let mut matches: Vec<&'static str> = Vec::new();
        if "--help".starts_with(name) {
            matches.push("--help");
        }
        matches.extend(opts.iter().map(|o| o.flag).filter(|f| f.starts_with(name)));
        match matches.as_slice() {
            [] => {}
            ["--help"] => return Flag::Help,
            [one] => {
                let found = opts.iter().find(|o| o.flag == *one).expect("matched above");
                return Flag::Known(found, explicit);
            }
            _ => return Flag::Ambiguous(matches.join(", ")),
        }
    }
    if negative_number(token) || token.contains(' ') {
        return Flag::Positional;
    }
    Flag::Unknown
}

/// The flag a token names, without a value after `=`.
fn flag_name(token: &str) -> &str {
    token.split_once('=').map_or(token, |(name, _)| name)
}

/// Python 3.14's argparse: `-\.?\d` at the token's start (its `_negative_number_matcher`, matched, not
/// searched), so `-1x` and `-.5` read as values too.
fn negative_number(token: &str) -> bool {
    let Some(rest) = token.strip_prefix('-') else {
        return false;
    };
    let rest = rest.strip_prefix('.').unwrap_or(rest);
    rest.starts_with(|c: char| c.is_ascii_digit())
}

/// Take flag `found`'s value (after its `=`, else the next token) into `values`.
fn take(
    found: &Opt,
    explicit: Option<String>,
    args: &[String],
    i: &mut usize,
    values: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), String> {
    let mut value = || -> Result<String, String> {
        if let Some(value) = explicit.clone() {
            return Ok(value);
        }
        match args.get(*i) {
            Some(next) if next != "--" && matches!(optional(next, &[]), Flag::Positional) => {
                *i += 1;
                Ok(next.clone())
            }
            _ => Err(format!("argument {}: expected one argument", found.flag)),
        }
    };
    let ignored = |explicit: &Option<String>| match explicit {
        Some(value) => Err(format!(
            "argument {}: ignored explicit argument {}",
            found.flag,
            pyrepr(value)
        )),
        None => Ok(()),
    };
    match found.takes {
        Takes::Bool => {
            ignored(&explicit)?;
            values.insert(found.dest.into(), vec![String::new()]);
        }
        Takes::Const(constant) => {
            ignored(&explicit)?;
            values.insert(found.dest.into(), vec![constant.into()]);
        }
        Takes::Append => {
            let value = value()?;
            values.entry(found.dest.into()).or_default().push(value);
        }
        Takes::Pair => {
            let two = format!("argument {}: expected 2 arguments", found.flag);
            let pair = args.get(*i..*i + 2).filter(|pair| {
                explicit.is_none()
                    && pair
                        .iter()
                        .all(|next| next != "--" && matches!(optional(next, &[]), Flag::Positional))
            });
            let pair = pair.ok_or(two)?;
            *i += 2;
            values
                .entry(found.dest.into())
                .or_default()
                .extend_from_slice(pair);
        }
        Takes::Value(ty) => {
            let value = value()?;
            let (fits, name) = match ty {
                Ty::Str => (true, ""),
                Ty::Int => (int(&value).is_some(), "int"),
                Ty::Float => (float(&value).is_some(), "float"),
            };
            if !fits {
                return Err(format!(
                    "argument {}: invalid {name} value: {}",
                    found.flag,
                    pyrepr(&value)
                ));
            }
            if !found.choices.is_empty() && !found.choices.contains(&value.as_str()) {
                return Err(invalid_choice(found.flag, &value, found.choices));
            }
            values.insert(found.dest.into(), vec![value]);
        }
    }
    Ok(())
}

fn invalid_choice(name: &str, value: &str, choices: &[&str]) -> String {
    let all: Vec<String> = choices.iter().map(|c| pyrepr(c)).collect();
    format!(
        "argument {name}: invalid choice: {} (choose from {})",
        pyrepr(value),
        all.join(", ")
    )
}

/// Digits with single `_` between them, as Python reads a number's digits.
fn python_digits(text: &str) -> Option<String> {
    if text.starts_with('_') || text.ends_with('_') || text.contains("__") {
        return None;
    }
    Some(text.replace('_', ""))
}

/// Python's `int()` of a flag's value: surrounding space, a sign, `_` between digits.
fn int(text: &str) -> Option<i64> {
    let digits = python_digits(text.trim())?;
    let unsigned = digits.strip_prefix(['+', '-']).unwrap_or(&digits);
    if unsigned.is_empty() || !unsigned.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.strip_prefix('+').unwrap_or(&digits).parse().ok()
}

/// Python's `float()` of a flag's value.
fn float(text: &str) -> Option<f64> {
    python_digits(text.trim())?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_of(args: &[&str]) -> Outcome {
        parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    fn run(args: &[&str]) -> Parsed {
        match parse_of(args) {
            Outcome::Run(parsed) => parsed,
            other => panic!("{args:?} gave {other:?}"),
        }
    }

    /// The command's usage, then argparse's error line, for `args`.
    fn refused(args: &[&str]) -> String {
        match parse_of(args) {
            Outcome::Usage(text) => text,
            other => panic!("{args:?} gave {other:?}"),
        }
    }

    fn usage_of(command: &str) -> &'static str {
        usage::COMMANDS
            .iter()
            .find(|(name, _, _)| *name == command)
            .map(|(_, usage, _)| *usage)
            .unwrap()
    }

    const ALL: &str = "'callback', 'register', 'attach', 'list', 'search', 'stats', 'embed', 'next', 'create', 'priority', 'field', 'claim', 'reserve', 'release', 'show', 'parent', 'depends', 'done', 'role', 'handover', 'move', 'verdict', 'say', 'launch', 'restore', 'cleanup', 'clean', 'close', 'reap', 'staff', 'agents', 'who', 'status', 'inbox', 'pull', 'wait', 'leave', 'beat', 'export', 'finish', 'sweep', 'land'";

    #[test]
    fn every_command_of_the_usage_is_read_and_has_its_help() {
        let named: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        let helped: Vec<&str> = usage::COMMANDS.iter().map(|(n, _, _)| *n).collect();
        assert_eq!(named, helped);
        assert_eq!(ALL.replace(['\'', ' '], ""), named.join(","));
    }

    #[test]
    fn reap_reads_its_dry_run_and_its_help_is_argparses() {
        assert_eq!(
            parse_of(&["reap", "-h"]),
            Outcome::Help(
                "usage: just tasks reap [-h] [--as ACTOR] [--dry-run]\n\noptions:\n  -h, --help  show this help message and exit\n  --as ACTOR\n  --dry-run   name what the next sweep closes; count nothing\n"
            )
        );
        let parsed = run(&["reap", "--dry-run", "--as", "amber-heron"]);
        assert_eq!(
            (parsed.flag("dry_run"), parsed.get("actor")),
            (true, Some("amber-heron"))
        );
        assert!(!run(&["reap"]).flag("dry_run"));
        assert!(usage::USAGE.contains(",cleanup,clean,close,reap,staff,agents,"));
        assert_eq!(
            usage::HELP
                .matches(",cleanup,clean,close,reap,staff,agents,")
                .count(),
            2
        );
    }

    #[test]
    fn status_reads_its_json_flag_and_names_itself_in_the_usage() {
        let parsed = run(&["status", "--json", "--as", "amber-heron"]);
        assert_eq!(
            (parsed.command.as_str(), parsed.flag("json")),
            ("status", true)
        );
        assert!(!run(&["status"]).flag("json"));
        assert_eq!(usage::HELP.matches(",agents,who,status,inbox,").count(), 2);
        assert!(usage::USAGE.contains(",agents,who,status,inbox,"));
        let Outcome::Help(help) = parse_of(&["status", "-h"]) else {
            panic!("status -h is its help");
        };
        assert!(help.starts_with("usage: just tasks status [-h] [--as ACTOR] [--json]\n\n"));
    }

    #[test]
    fn handover_help_says_only_the_new_orchestrator_hears_it() {
        let Outcome::Help(help) = parse_of(&["handover", "-h"]) else {
            panic!("handover -h is its help");
        };
        assert!(
            help.contains("the thread names the new orchestrator, who hears one ROLE event; you made\nthe move and are not told it.\n"),
            "{help}"
        );
    }

    #[test]
    fn wait_reads_its_timeout_in_seconds() {
        let parsed = run(&["wait", "--timeout", "2.5", "--as", "amber-heron"]);
        assert_eq!(
            (parsed.float("timeout", 0.0), parsed.get("actor")),
            (2.5, Some("amber-heron"))
        );
        assert_eq!(run(&["wait"]).get("timeout"), None);
        assert!(refused(&["wait", "--timeout", "soon"]).ends_with(
            "just tasks wait: error: argument --timeout: invalid float value: 'soon'\n"
        ));
        let Outcome::Help(help) = parse_of(&["wait", "-h"]) else {
            panic!("wait -h is its help");
        };
        assert!(help.starts_with("usage: just tasks wait [-h] [--as ACTOR] [--timeout TIMEOUT]\n\nPrint what a callback would push ACTOR"), "{help}");
    }

    #[test]
    fn clean_reads_its_dry_run_and_its_help_says_what_it_runs() {
        assert_eq!(
            parse_of(&["clean", "-h"]),
            Outcome::Help(
                "usage: just tasks clean [-h] [--as ACTOR] [--dry-run]\n\ncleanup, then one idle sweep: what closed and why each open terminal stays\n\noptions:\n  -h, --help  show this help message and exit\n  --as ACTOR\n  --dry-run   name what it would close and why the rest stay; close nothing\n"
            )
        );
        let parsed = run(&["clean", "--dry-run", "--as", "amber-heron"]);
        assert_eq!(
            (parsed.flag("dry_run"), parsed.get("actor")),
            (true, Some("amber-heron"))
        );
        assert!(!run(&["clean"]).flag("dry_run"));
    }

    #[test]
    fn no_command_is_a_usage_error_with_the_top_usage() {
        for args in [&[][..], &["--as", "x"][..]] {
            assert_eq!(
                refused(args),
                format!(
                    "{}just tasks: error: the following arguments are required: command\n",
                    usage::USAGE
                )
            );
        }
    }

    #[test]
    fn an_unknown_command_lists_every_command() {
        assert_eq!(
            refused(&["nope"]),
            format!(
                "{}just tasks: error: argument command: invalid choice: 'nope' (choose from {ALL})\n",
                usage::USAGE
            )
        );
    }

    #[test]
    fn a_missing_argument_is_named_after_the_commands_usage() {
        for (args, command, missing) in [
            (&["claim"][..], "claim", "slug"),
            (&["create", "a"][..], "create", "title"),
            (&["create"][..], "create", "slug, title"),
            (&["search"][..], "search", "query"),
            (&["launch"][..], "launch", "slug"),
            (&["verdict", "s"][..], "verdict", "verdict"),
            (&["say", "s", "-x"][..], "say", "text"),
            (&["--as", "x", "--as", "y", "claim"][..], "claim", "slug"),
        ] {
            assert_eq!(
                refused(args),
                format!(
                    "{}just tasks {command}: error: the following arguments are required: {missing}\n",
                    usage_of(command)
                ),
                "{args:?}"
            );
        }
        let launch = run(&["launch", "s"]);
        assert_eq!(launch.get("slug"), Some("s"));
        assert_eq!(launch.get("model"), None);
    }

    #[test]
    fn a_value_outside_the_choices_is_named_with_them() {
        assert_eq!(
            refused(&["move", "x", "nostatus"]),
            format!(
                "{}just tasks move: error: argument status: invalid choice: 'nostatus' (choose from 'backlog', 'concept', 'design', 'ready', 'in_progress', 'review', 'done', 'blocked', 'cancelled')\n",
                usage_of("move")
            )
        );
        assert!(
            refused(&["register", "--kind", "x"]).ends_with(
                "just tasks register: error: argument --kind: invalid choice: 'x' (choose from 'orchestrator', 'worker', 'reviewer', 'chat', 'app')\n"
            )
        );
        assert!(refused(&["role", "s", "executor"]).ends_with(
            "just tasks role: error: argument role: invalid choice: 'executor' (choose from 'orchestrator', 'reviewer')\n"
        ));
        assert!(refused(&["move", "s", "it's"]).contains("invalid choice: \"it's\" (choose"));
    }

    #[test]
    fn a_value_of_the_wrong_type_is_named() {
        assert!(
            refused(&["create", "a", "b", "--priority", "x"]).ends_with(
                "just tasks create: error: argument --priority: invalid int value: 'x'\n"
            )
        );
        assert!(refused(&["attach", "--interval", "x"]).ends_with(
            "just tasks attach: error: argument --interval: invalid float value: 'x'\n"
        ));
        assert!(
            refused(&["attach", "--polls", "x"])
                .ends_with("just tasks attach: error: argument --polls: invalid int value: 'x'\n")
        );
    }

    #[test]
    fn a_flag_with_no_value_after_it_expects_one() {
        for (args, flag) in [
            (
                &["create", "a", "b", "--body"][..],
                "create: error: argument --body",
            ),
            (
                &["create", "a", "b", "--body", "-x"][..],
                "create: error: argument --body",
            ),
            (
                &["create", "a", "b", "--body", "--x"][..],
                "create: error: argument --body",
            ),
            (&["claim", "--as"][..], "claim: error: argument --as"),
            (&["say", "s", "t", "--to"][..], "say: error: argument --to"),
        ] {
            assert!(
                refused(args).ends_with(&format!("{flag}: expected one argument\n")),
                "{args:?}: {}",
                refused(args)
            );
        }
        assert_eq!(
            refused(&["--as"]),
            format!(
                "{}just tasks: error: argument --as: expected one argument\n",
                usage::USAGE
            )
        );
    }

    #[test]
    fn what_no_command_takes_is_unrecognized_after_the_top_usage_in_order() {
        for (args, extra) in [
            (&["claim", "a", "--bogus"][..], "--bogus"),
            (&["claim", "a", "b"][..], "b"),
            (&["verdict", "s", "approve", "why", "extra"][..], "extra"),
            (&["--bogus", "claim", "a"][..], "--bogus"),
            (&["claim", "s", "--", "--as"][..], "--as"),
            // A `--` no positional took is left over with what follows it, as argparse leaves it.
            (&["list", "--", "--json"][..], "-- --json"),
            (&["claim", "s", "x", "--"][..], "x --"),
            (&["parent", "a", "--", "b", "c"][..], "c"),
            (&["claim", "a", "-x", "b", "--y=2"][..], "-x b --y=2"),
            (&["parent", "a", "b", "c"][..], "c"),
        ] {
            assert_eq!(
                refused(args),
                format!(
                    "{}just tasks: error: unrecognized arguments: {extra}\n",
                    usage::USAGE
                ),
                "{args:?}"
            );
        }
    }

    #[test]
    fn an_abbreviation_naming_more_than_one_flag_is_ambiguous() {
        assert!(refused(&["create", "a", "b", "--b", "x"]).ends_with(
            "just tasks create: error: ambiguous option: --b could match --body, --body-file, --backlog\n"
        ));
        assert!(refused(&["create", "a", "b", "--bo", "x"]).ends_with(
            "just tasks create: error: ambiguous option: --bo could match --body, --body-file\n"
        ));
    }

    #[test]
    fn flags_of_one_group_exclude_each_other() {
        assert!(
            refused(&["launch", "s", "--model", "m", "--branch", "b", "--detached"])
                .ends_with("just tasks launch: error: argument --detached: not allowed with argument --branch\n")
        );
    }

    #[test]
    fn a_flag_that_takes_nothing_refuses_a_value() {
        assert!(
            refused(&["list", "--json=1"]).ends_with(
                "just tasks list: error: argument --json: ignored explicit argument '1'\n"
            )
        );
    }

    #[test]
    fn help_is_the_commands_own_wherever_it_comes() {
        let help_of = |command: &str| {
            usage::COMMANDS
                .iter()
                .find(|(n, _, _)| *n == command)
                .unwrap()
                .2
        };
        assert_eq!(parse_of(&["-h"]), Outcome::Help(usage::HELP));
        assert_eq!(
            parse_of(&["--as", "x", "--help"]),
            Outcome::Help(usage::HELP)
        );
        assert_eq!(parse_of(&["claim", "-h"]), Outcome::Help(help_of("claim")));
        assert_eq!(
            parse_of(&["claim", "s", "-h", "--bogus"]),
            Outcome::Help(help_of("claim"))
        );
        assert_eq!(parse_of(&["show", "--he"]), Outcome::Help(help_of("show")));
        assert!(matches!(
            parse_of(&["create", "a", "b", "--priority", "x", "-h"]),
            Outcome::Usage(_)
        ));
    }

    #[test]
    fn values_are_read_wherever_they_come() {
        let p = run(&["verdict", "s", "approve", "--as", "x", "why"]);
        assert_eq!(
            (
                p.command.as_str(),
                p.get("slug"),
                p.get("verdict"),
                p.get("text"),
                p.get("actor")
            ),
            (
                "verdict",
                Some("s"),
                Some("approve"),
                Some("why"),
                Some("x")
            )
        );
        let p = run(&["say", "--to", "o", "s", "t"]);
        assert_eq!(
            (p.get("slug"), p.get("text"), p.get("to")),
            (Some("s"), Some("t"), Some("o"))
        );
        let p = run(&["parent", "a", "--none"]);
        assert_eq!((p.get("parent"), p.flag("none")), (None, true));
        assert_eq!(
            run(&["--as", "x", "claim", "s", "--as", "y"]).get("actor"),
            Some("y")
        );
        assert_eq!(run(&["--as", "x", "claim", "s"]).get("actor"), Some("x"));
        assert_eq!(run(&["--a", "x", "claim", "s"]).get("actor"), Some("x"));
        assert_eq!(run(&["--as=x", "next"]).get("actor"), Some("x"));
        assert_eq!(run(&["next", "--as="]).get("actor"), Some(""));
        assert_eq!(run(&["claim", "--", "s"]).get("slug"), Some("s"));
        assert_eq!(run(&["claim", "--as=y", "s"]).get("actor"), Some("y"));
    }

    #[test]
    fn a_pair_flag_takes_two_values_each_time_or_is_a_usage_error() {
        let p = run(&[
            "depends",
            "s",
            "--replace",
            "a",
            "b",
            "--add",
            "c",
            "--rep",
            "d",
            "e",
        ]);
        assert_eq!(p.all("replace"), ["a", "b", "d", "e"]);
        assert_eq!(
            (p.get("slug"), p.all("add")),
            (Some("s"), &["c".to_string()][..])
        );
        for args in [
            &["depends", "s", "--replace", "a"][..],
            &["depends", "s", "--replace", "a", "--add", "b"][..],
            &["depends", "s", "--replace", "a", "--", "b"][..],
            &["depends", "s", "--replace=a", "b"][..],
        ] {
            assert_eq!(
                refused(args),
                format!(
                    "{}just tasks depends: error: argument --replace: expected 2 arguments\n",
                    usage_of("depends")
                ),
                "{args:?}"
            );
        }
    }

    #[test]
    fn appended_flags_keep_every_value_and_a_const_sets_its_dest() {
        let p = run(&[
            "create",
            "a",
            "b",
            "--depends",
            "c",
            "--depends",
            "d",
            "--link=e",
        ]);
        assert_eq!(
            (p.all("depends"), p.all("link")),
            (
                &["c".to_string(), "d".to_string()][..],
                &["e".to_string()][..]
            )
        );
        assert_eq!(p.all("nothing"), &[] as &[String]);
        assert_eq!(
            run(&["create", "a", "b", "--backlog"]).get("stage"),
            Some("backlog")
        );
        assert_eq!(
            run(&["create", "a", "b", "--backlog", "--stage", "design"]).get("stage"),
            Some("design")
        );
        assert_eq!(
            run(&["create", "a", "b", "--body-f", "x"]).get("body_file"),
            Some("x")
        );
        assert_eq!(
            run(&["list", "--stat", "ready"]).get("status"),
            Some("ready")
        );
        assert!(run(&["list", "--mine", "--mine"]).flag("mine"));
        assert!(!run(&["list"]).flag("mine"));
    }

    #[test]
    fn a_dash_or_a_negative_number_is_a_value() {
        assert_eq!(run(&["say", "s", "-5"]).get("text"), Some("-5"));
        assert_eq!(run(&["say", "s", "-"]).get("text"), Some("-"));
        assert_eq!(
            run(&["create", "a", "b", "--body-file", "-"]).get("body_file"),
            Some("-")
        );
        assert_eq!(run(&["say", "s", "it's"]).get("text"), Some("it's"));
        // Python 3.14's argparse takes anything that starts like a negative number (`-\.?\d`) as one.
        assert_eq!(run(&["say", "s", "-1x"]).get("text"), Some("-1x"));
        assert_eq!(run(&["say", "s", "-.5x"]).get("text"), Some("-.5x"));
        assert!(
            refused(&["say", "s", "-.x"])
                .ends_with("just tasks say: error: the following arguments are required: text\n")
        );
    }

    #[test]
    fn numbers_read_as_python_reads_them() {
        for (text, n) in [(" 3 ", 3), ("+3", 3), ("1_0", 10), ("-2", -2), ("0", 0)] {
            assert_eq!(
                run(&["create", "a", "b", "--priority", text]).int("priority", 2),
                n,
                "{text:?}"
            );
        }
        for text in ["", "1.5", "_1", "1_", "1__0", "x"] {
            assert!(
                matches!(
                    parse_of(&["create", "a", "b", "--priority", text]),
                    Outcome::Usage(_)
                ),
                "{text:?}"
            );
        }
        assert_eq!(run(&["create", "a", "b"]).int("priority", 2), 2);
        assert_eq!(
            run(&["attach", "--interval", "0.5"]).float("interval", 3.0),
            0.5
        );
        assert_eq!(
            run(&["attach", "--interval", "2"]).float("interval", 3.0),
            2.0
        );
        assert_eq!(run(&["attach"]).float("beat", 20.0), 20.0);
    }
}
