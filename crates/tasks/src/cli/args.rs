//! The command line read as the board's first, Python, command line's argparse parser read it: its
//! commands, flags and defaults, an abbreviated long flag taken when it names one flag, `--as` on
//! either side of the command, `--help` and argparse's own words for a usage error (exit 2). Beyond
//! argparse: `help [COMMAND]`, a choice spelled as people spell it (`p1` for the priority `1`,
//! `in-progress` for the status `in_progress`, any case: [`chosen`]), and a usage error that names
//! the command or choice nearest to a mistyped one ([`nearest`]).

use super::usage;
use crate::quote::repr as pyrepr;
use std::collections::BTreeMap;
use symphony_args::nearest;

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
    /// `nargs="+"`: it takes every positional left, one at least, all kept in order.
    many: bool,
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
        many: false,
    }
}

const fn maybe(dest: &'static str) -> Pos {
    Pos {
        dest,
        optional: true,
        choices: &[],
        many: false,
    }
}

/// A last positional taking every one left, one at least (`nargs="+"`).
const fn many(dest: &'static str) -> Pos {
    Pos {
        dest,
        optional: false,
        choices: &[],
        many: true,
    }
}

const STR: Takes = Takes::Value(Ty::Str);
const AS: Opt = opt("--as", "actor", STR);
const KINDS: &[&str] = &["orchestrator", "worker", "reviewer", "chat", "app"];
const PRIORITIES: &[&str] = &["0", "1", "2", "3", "4"];
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
            opt("--all", "all", Takes::Bool),
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
    plain("next", &[]),
    Command {
        name: "workflow",
        pos: &[pos("action"), maybe("slug"), maybe("edge")],
        opts: &[
            AS,
            opt("--revision", "revision", Takes::Value(Ty::Int)),
            opt("--key", "key", STR),
            opt("--input", "input", Takes::Append),
            opt("--input-revision", "input_revision", Takes::Append),
            opt("--attestation", "attestation", Takes::Append),
            opt("--accept", "accept", Takes::Append),
            opt("--json", "json", Takes::Bool),
        ],
    },
    plain("limits", &[]),
    Command {
        name: "settings",
        pos: &[pos("action"), maybe("key"), maybe("value")],
        opts: &[AS, opt("--json", "json", Takes::Bool)],
    },
    Command {
        name: "people",
        pos: &[
            pos("action"),
            Pos {
                dest: "words",
                optional: true,
                choices: &[],
                many: true,
            },
        ],
        opts: &[
            AS,
            opt("--display", "display", STR),
            opt("--signin", "signin", Takes::Append),
            opt("--git", "git", Takes::Append),
            opt("--power", "power", Takes::Append),
            opt("--group", "group", Takes::Append),
            opt("--json", "json", Takes::Bool),
        ],
    },
    Command {
        name: "create",
        pos: &[pos("slug"), pos("title")],
        opts: &[
            AS,
            opt("--body", "body", STR),
            opt("--body-file", "body_file", STR),
            choice("--priority", "priority", PRIORITIES),
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
            opt("--no-role", "no_role", Takes::Bool),
            opt("--arm", "arm", STR),
            choice("--kind", "kind", &crate::kind::KINDS),
        ],
    },
    Command {
        name: "priority",
        pos: &[
            pos("slug"),
            Pos {
                dest: "priority",
                optional: false,
                choices: PRIORITIES,
                many: false,
            },
        ],
        opts: &[AS],
    },
    Command {
        name: "kind",
        pos: &[
            pos("slug"),
            Pos {
                dest: "kind",
                optional: false,
                choices: &crate::kind::KINDS,
                many: false,
            },
        ],
        opts: &[AS],
    },
    plain("field", &[pos("slug"), pos("name"), pos("value")]),
    plain("stamps", &[maybe("slug")]),
    Command {
        name: "runs",
        pos: SLUG,
        opts: &[AS, opt("--tip", "tip", STR)],
    },
    Command {
        name: "needs",
        pos: &[pos("slug"), many("stamps")],
        opts: &[
            AS,
            opt("--tip", "tip", STR),
            opt("--import", "import", STR),
            Opt {
                required: true,
                ..opt("--why", "why", STR)
            },
        ],
    },
    Command {
        name: "gate",
        pos: &[pos("slug"), pos("gate")],
        opts: &[
            AS,
            Opt {
                group: 1,
                ..opt("--raise", "raise", STR)
            },
            Opt {
                group: 1,
                ..opt("--waive", "waive", Takes::Bool)
            },
            opt("--tip", "tip", STR),
            Opt {
                required: true,
                ..opt("--why", "why", STR)
            },
        ],
    },
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
    Command {
        name: "done",
        pos: &[pos("slug"), pos("note")],
        opts: &[AS, opt("--covered-by", "covered_by", STR)],
    },
    Command {
        name: "role",
        pos: &[
            pos("slug"),
            Pos {
                dest: "role",
                optional: false,
                choices: &["orchestrator", "reviewer"],
                many: false,
            },
        ],
        opts: &[AS, opt("--beside", "beside", STR)],
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
                many: false,
            },
        ],
        opts: &[
            AS,
            opt("--reason", "reason", STR),
            opt("--on", "on", STR),
            opt("--why", "why", STR),
        ],
    },
    Command {
        name: "verdict",
        pos: &[
            pos("slug"),
            Pos {
                dest: "verdict",
                optional: false,
                choices: &["approve", "changes"],
                many: false,
            },
            maybe("text"),
        ],
        opts: &[AS],
    },
    Command {
        name: "say",
        pos: &[pos("slug"), maybe("text")],
        opts: &[AS, opt("--to", "to", STR), opt("--file", "file", STR)],
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
            opt("--beside", "beside", STR),
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
    Command {
        name: "schema",
        pos: &[],
        opts: &[AS, opt("--accept", "accept", STR)],
    },
    Command {
        name: "snapshot",
        pos: &[],
        opts: &[AS, opt("--list", "list", Takes::Bool)],
    },
    Command {
        name: "asks",
        pos: &[
            Pos {
                dest: "action",
                optional: true,
                choices: &["close"],
                many: false,
            },
            maybe("id"),
            maybe("why"),
        ],
        opts: &[
            AS,
            opt("--all", "all", Takes::Bool),
            opt("--json", "json", Takes::Bool),
        ],
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
    Command {
        name: "land",
        pos: &[maybe("slug")],
        opts: &[
            AS,
            opt("--dispatch", "dispatch", Takes::Bool),
            opt("--watch", "watch", Takes::Bool),
        ],
    },
    Command {
        name: "regenerate",
        pos: &[],
        opts: &[
            AS,
            opt("--status", "status", Takes::Bool),
            opt("--wait", "wait", Takes::Bool),
        ],
    },
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
    /// `--help` or `help [COMMAND]`: the top help, or this command's ([`usage::shown`]), on stdout,
    /// exit 0.
    Help(Option<&'static str>),
    /// A usage error: this text on stderr, exit 2.
    Usage(String),
}

/// The word that asks for help: `just tasks help [COMMAND]`.
const HELP: &str = "help";

/// Read `args` (the command line without the program name), with nothing before them: what the
/// tests read a command line with ([`parse_after`] is what the binary reads one with).
#[cfg(test)]
pub fn parse(args: &[String]) -> Outcome {
    parse_after("just tasks", args)
}

/// Read `args`, the words after the board's own (`--database NAME`), on the board `board` reaches:
/// the command that reaches it again, as the board's refusals spell it
/// ([`crate::store::board_command`]), which every line a refusal suggests begins with, so typed
/// again in any shell it reaches the same board.
pub fn parse_after(board: &str, args: &[String]) -> Outcome {
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // What nothing takes, with where it stood: argparse names them in the order given.
    let mut extras: Vec<(usize, String)> = Vec::new();
    let top =
        |message: String| Outcome::Usage(format!("{}just tasks: error: {message}\n", usage::USAGE));
    let mut i = 0;
    let command = loop {
        let Some(token) = args.get(i) else {
            return top("name a command: `just tasks help` lists them".into());
        };
        i += 1;
        match optional(token, &[AS]) {
            Flag::Positional => break token,
            Flag::Help => return Outcome::Help(None),
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
    if command == HELP {
        return match &args[i..] {
            [] => Outcome::Help(None),
            [word, ..] if word == "-h" || word == "--help" => Outcome::Help(None),
            [word, ..] => match usage::names().find(|name| name == word) {
                Some(name) if name == HELP => Outcome::Help(None),
                Some(name) => Outcome::Help(Some(name)),
                None => top(unknown_command(word)),
            },
        };
    }
    let Some(cmd) = COMMANDS.iter().find(|c| c.name == command) else {
        return top(unknown_command(command));
    };
    let usage = usage::COMMANDS
        .iter()
        .find(|(name, _, _)| *name == cmd.name)
        .map(|(_, usage, _)| *usage)
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
            Flag::Help => return Outcome::Help(Some(cmd.name)),
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
            Some((at, value)) if p.many => {
                let mut all = vec![(at, value)];
                all.extend(given.by_ref());
                let last = all.last().map_or(at, |(at, _)| *at);
                dash_taken |= dash.is_some_and(|dash| last + 1 == dash || last > dash);
                values.insert(p.dest.into(), all.into_iter().map(|(_, v)| v).collect());
            }
            Some((at, value)) => {
                dash_taken |= dash.is_some_and(|dash| at + 1 == dash || at > dash);
                let value = if p.choices.is_empty() {
                    value
                } else {
                    match chosen(&value, p.choices) {
                        Some(choice) => choice.to_string(),
                        None => return refuse(invalid_choice(p.dest, &value, p.choices)),
                    }
                };
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
        crate::order::sort_by_key(&mut extras, &|(at, _)| *at);
        return refuse(unrecognized(&extras, board, args, dash, cmd));
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
            let value = if found.choices.is_empty() {
                value
            } else {
                chosen(&value, found.choices)
                    .ok_or_else(|| invalid_choice(found.flag, &value, found.choices))?
                    .to_string()
            };
            values.insert(found.dest.into(), vec![value]);
        }
    }
    Ok(())
}

/// The choice `value` names: itself, or as people spell it, in any case, with `-` for `_`
/// (`in-progress`), or a priority's `p` before its digit (`p1`).
fn chosen(value: &str, choices: &[&'static str]) -> Option<&'static str> {
    let spelled = respelled(value);
    let digit = spelled
        .strip_prefix('p')
        .filter(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
    choices
        .iter()
        .find(|choice| **choice == value)
        .or_else(|| choices.iter().find(|choice| **choice == spelled))
        .or_else(|| digit.and_then(|digit| choices.iter().find(|choice| **choice == digit)))
        .copied()
}

/// `word` lowercased, with `-` for `_`, as [`chosen`] reads a choice.
fn respelled(word: &str) -> String {
    word.trim().to_ascii_lowercase().replace('-', "_")
}

fn invalid_choice(name: &str, value: &str, choices: &[&str]) -> String {
    let all: Vec<String> = choices.iter().map(|c| pyrepr(c)).collect();
    format!(
        "argument {name}: invalid choice: {} (choose from {}){}",
        pyrepr(value),
        all.join(", "),
        did_you_mean(value, choices.iter().copied())
    )
}

/// The refusal of `extras`, what nothing of `cmd` took (each with where it stood in `args`, whose
/// first `--` stood at `dash`). When each mistyped flag among them is nearest one of `cmd`'s, the
/// line to type instead, `board` (the command that reaches the board) and then `args` with those
/// flags set right; else where its flags are listed.
fn unrecognized(
    extras: &[(usize, String)],
    board: &str,
    args: &[String],
    dash: Option<usize>,
    cmd: &Command,
) -> String {
    let named: Vec<&str> = extras.iter().map(|(_, token)| token.as_str()).collect();
    let flags = cmd.opts.iter().map(|o| o.flag).chain(["--help"]);
    let mut line: Vec<String> = args.to_vec();
    let mut mistyped = 0;
    let mut fixed = 0;
    for (at, token) in extras {
        // After `--` a word is a value, whatever it looks like.
        if !token.starts_with('-') || dash.is_some_and(|dash| *at >= dash) {
            continue;
        }
        mistyped += 1;
        let name = flag_name(token);
        if let [one] = nearest(name, flags.clone()).as_slice() {
            fixed += 1;
            line[*at] = format!("{one}{}", &token[name.len()..]);
        }
    }
    let said = format!("unrecognized arguments: {}", named.join(" "));
    if mistyped > 0 && fixed == mistyped {
        let line: Vec<String> = line.iter().map(|word| shell_word(word)).collect();
        return format!("{said}; did you mean `{board} {}`?", line.join(" "));
    }
    format!(
        "{said} (`just tasks help {}` lists what it takes)",
        cmd.name
    )
}

/// `word` as a shell reads it back: as it is when it holds nothing the shell would read otherwise,
/// else in single quotes.
pub(super) fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@%+=,#".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// The refusal of `word`, which names no command: the nearest commands, and where to find them all.
fn unknown_command(word: &str) -> String {
    format!(
        "no command {}{} (`just tasks help` lists them)",
        pyrepr(word),
        did_you_mean(word, usage::names())
    )
}

/// `; did you mean 'x'?` for the `among` nearest to `word` ([`nearest`]), or nothing when none is.
fn did_you_mean<'a>(word: &str, among: impl IntoIterator<Item = &'a str>) -> String {
    let near: Vec<String> = nearest(word, among).iter().map(|n| pyrepr(n)).collect();
    match near.as_slice() {
        [] => String::new(),
        [one] => format!("; did you mean {one}?"),
        [rest @ .., last] => format!("; did you mean {} or {last}?", rest.join(", ")),
    }
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
    use crate::cli::Env;
    use std::collections::BTreeSet;

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    fn parse_of(args: &[&str]) -> Outcome {
        parse(&strings(args))
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

    /// What `args` asks help of, as `--help` prints it.
    fn help_of(args: &[&str]) -> String {
        match parse_of(args) {
            Outcome::Help(command) => usage::shown(command, &Env::default()),
            other => panic!("{args:?} gave {other:?}"),
        }
    }

    #[test]
    fn every_command_of_the_usage_is_read_and_has_its_help_and_its_line() {
        let named: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        let helped: Vec<&str> = usage::COMMANDS.iter().map(|(n, _, _)| *n).collect();
        assert_eq!(named, helped);
        // The top help lists each once, with `help` and the commands read before this table.
        let mut listed: Vec<&str> = usage::names().collect();
        listed.sort_unstable();
        let mut every: Vec<&str> = named
            .iter()
            .copied()
            .chain(usage::BEFORE_TABLE)
            .chain([HELP])
            .collect();
        every.sort_unstable();
        assert_eq!(listed, every);
        for name in usage::names() {
            assert!(
                usage::summary(name).is_some_and(|said| !said.is_empty()),
                "{name}"
            );
        }
    }

    #[test]
    fn the_top_help_lists_every_command_once_with_its_line_within_78_columns() {
        let top = usage::top();
        assert!(top.starts_with("usage: just tasks [-h] [--as ACTOR] COMMAND ...\n\n"));
        for name in usage::names() {
            let said = usage::summary(name).unwrap();
            let rows: Vec<&str> = top
                .lines()
                .filter(|line| line.split_whitespace().next() == Some(name))
                .collect();
            assert_eq!(rows.len(), 1, "{name}: {rows:?}");
            assert!(rows[0].ends_with(said), "{name}: {}", rows[0]);
        }
        // The summaries start in one column.
        let columns: BTreeSet<usize> = usage::names()
            .map(|name| {
                let row = top
                    .lines()
                    .find(|line| line.split_whitespace().next() == Some(name))
                    .unwrap();
                row.find(usage::summary(name).unwrap()).unwrap()
            })
            .collect();
        assert_eq!(columns.len(), 1, "{columns:?}");
        for line in top.lines() {
            assert!(line.chars().count() <= 78, "{line}");
        }
        for args in [
            &["-h"][..],
            &["--help"],
            &["--as", "x", "--help"],
            &["help"],
            &["help", "help"],
        ] {
            assert_eq!(help_of(args), top, "{args:?}");
        }
    }

    #[test]
    fn help_names_a_command_and_says_what_it_does() {
        // `help CMD` is `CMD --help`, its line after the usage where its own text says none.
        for name in usage::COMMANDS.iter().map(|(n, _, _)| *n) {
            let help = help_of(&["help", name]);
            assert_eq!(help, help_of(&[name, "--help"]), "{name}");
            assert!(
                help.starts_with(&format!("usage: just tasks {name} ")),
                "{name}: {help}"
            );
            let (_, rest) = help.split_once("\n\n").unwrap();
            assert!(
                !rest.starts_with("positional arguments:") && !rest.starts_with("options:"),
                "{name} says nothing of what it does: {help}"
            );
        }
        assert!(
            help_of(&["help", "claim"]).contains("\n\ntake a task as its executor.\n\npositional")
        );
        // A new project has none of this repository's own files: staff names the setting it reads
        // in any checkout.
        assert!(help_of(&["help", "staff"]).contains(
            "\n\nstaff ready work once, if tools/task_staff.json enables it.\n\noptions:"
        ));
        // A task is no longer filed as a fast fix: what it needs is its assessor's.
        assert!(!help_of(&["help", "create"]).contains("--fast-fix"));
        assert!(
            help_of(&["help", "create"]).contains(
                "  --link LINK           a document it implements, docs/X.md#Heading\n                        (repeatable)\n  --no-role "
            )
        );
        for name in usage::COMMANDS.iter().map(|(n, _, _)| *n) {
            assert!(!help_of(&["help", name]).contains("RULES.md"), "{name}");
        }
        assert!(help_of(&["help", "run"]).starts_with("usage: tasks run --phase build|test"));
        // A command read before the table answers with its own help.
        assert_eq!(help_of(&["help", "migrate"]), crate::cli::migrate::HELP);
        assert!(
            help_of(&["--as", "x", "help", "show", "extra"]).starts_with("usage: just tasks show ")
        );
        for (word, near) in [("shwo", "show"), ("migrat", "migrate")] {
            assert_eq!(
                refused(&["help", word]),
                format!(
                    "{}just tasks: error: no command '{word}'; did you mean '{near}'? \
                     (`just tasks help` lists them)\n",
                    usage::USAGE
                )
            );
        }
        assert!(refused(&["migrat"]).contains("did you mean 'migrate'?"));
        // Help of help is the list.
        for args in [&["help", "--help"][..], &["help", "-h"]] {
            assert_eq!(help_of(args), usage::top(), "{args:?}");
        }
    }

    #[test]
    fn the_top_help_says_where_database_goes() {
        let top = usage::top();
        assert!(
            top.contains(
                "\n  --database NAME  the scratch board NAME, not the project's; it comes first,\n"
            ),
            "{top}"
        );
    }

    /// Every `.rs` file under `dir`, with its text.
    fn sources(dir: &std::path::Path, found: &mut Vec<(std::path::PathBuf, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                found.push((path, text));
            }
        }
    }

    /// `text` with each `\` that ends a line, the line break and the next line's leading blanks
    /// taken out, as a Rust string literal reads them.
    fn joined(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find("\\\n") {
            out.push_str(&rest[..at]);
            rest = rest[at + 2..].trim_start_matches([' ', '\t', '\n']);
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn a_string_broken_across_lines_reads_as_one() {
        assert_eq!(
            joined("\"see just tasks \\\n             role x\"\nnext"),
            "\"see just tasks role x\"\nnext"
        );
        assert_eq!(joined("\"just \\\n   tasks done\""), "\"just tasks done\"");
        assert_eq!(joined("no break"), "no break");
    }

    #[test]
    fn every_command_a_message_names_is_one_the_help_lists() {
        // Words after "just tasks" in the crate's own text that are prose, not a command.
        const PROSE: [&str; 1] = ["again"];
        let mut found = Vec::new();
        sources(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        let mut unknown = Vec::new();
        for (path, text) in &found {
            // A string broken across lines (`"… just tasks \` then the command on the next) reads
            // as the one line it prints.
            let text = joined(text);
            for (at, _) in text.match_indices("just tasks ") {
                let word: String = text[at + "just tasks ".len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                let word = word.trim_end_matches('-');
                // An option before the command (`just tasks --database NAME`) names none.
                if word.is_empty() || word.starts_with('-') || PROSE.contains(&word) {
                    continue;
                }
                if !usage::names().any(|name| name == word) {
                    unknown.push(format!("{}: {word}", path.display()));
                }
            }
        }
        assert!(
            unknown.is_empty(),
            "a message names a command the help does not list: list it in usage::GROUPS, or add \
             a prose word to PROSE: {unknown:#?}"
        );
    }

    #[test]
    fn every_command_the_binary_dispatches_before_its_table_is_one_the_help_lists() {
        // What `cli::run` and `process_main` compare the first word with, as the source spells it.
        let text = include_str!("mod.rs");
        let mut compared: BTreeSet<String> = BTreeSet::new();
        for line in text.lines() {
            for marker in ["arg == ", "word == ", "args[0] == "] {
                for (at, _) in line.match_indices(marker) {
                    let rest = &line[at + marker.len()..];
                    let operand: String = rest
                        .chars()
                        .take_while(|c| !matches!(c, ')' | ' ' | '{' | ';'))
                        .collect();
                    compared.insert(operand);
                }
            }
        }
        let compared: BTreeSet<String> = compared.iter().map(|op| operand_value(op)).collect();
        // Each word read before the table: one the help's own table of them names, or internal
        // and named here, which no agent types and the help does not list.
        const INTERNAL: [&str; 3] = ["--database", "api", "dispatch"];
        let named: BTreeSet<String> = usage::BEFORE_TABLE
            .iter()
            .chain(&INTERNAL)
            .map(|word| word.to_string())
            .collect();
        assert_eq!(
            compared, named,
            "a command dispatched before the board's table: list it in usage::GROUPS and \
             usage::BEFORE_TABLE (or, if no agent types it, here as INTERNAL)"
        );
        for word in INTERNAL {
            assert!(!usage::names().any(|name| name == word), "{word}");
        }
    }

    /// The word an operand mod.rs compares the first word with stands for: a string literal's
    /// text, or the `COMMAND` the module of `cli` it names declares, as its source spells it.
    fn operand_value(operand: &str) -> String {
        if let Some(text) = operand.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
            return text.into();
        }
        let module = operand
            .strip_suffix("::COMMAND")
            .unwrap_or_else(|| panic!("{operand} is neither a literal nor a module's COMMAND"));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/cli")
            .join(format!("{module}.rs"));
        let source = std::fs::read_to_string(&path).unwrap();
        let (_, rest) = source
            .split_once("const COMMAND: &str = \"")
            .unwrap_or_else(|| panic!("{} declares no COMMAND", path.display()));
        rest.split('"').next().unwrap().into()
    }

    #[test]
    fn help_hides_internal_flags_and_lists_the_values_a_flag_takes() {
        let sweep = help_of(&["sweep", "--help"]);
        assert!(!sweep.contains("--background"), "{sweep}");
        // Still read: work finishing passes it.
        assert_eq!(
            run(&["sweep", "--background", "why"]).get("background"),
            Some("why")
        );
        let say = help_of(&["say", "--help"]);
        for to in ["owner", "@orchestrator", "@executor", "@reviewer", "HANDLE"] {
            assert!(say.contains(to), "{to}: {say}");
        }
        assert!(help_of(&["create", "--help"]).contains("--priority {0,1,2,3,4}"));
        let launch = help_of(&["launch", "--help"]);
        assert!(launch.contains("[--kind {codex,claude}] [--role {executor,reviewer}]"));
        assert!(launch.contains("the role it takes on the task (default: executor)"));
        let callback = help_of(&["callback", "--help"]);
        assert!(callback.starts_with("usage: just tasks callback [-h] [--as ACTOR] [--chat]"));
        for flag in ["  --chat ", "  --if-free ", "  --off ", "  address "] {
            assert!(callback.contains(flag), "{flag}: {callback}");
        }
        // Launch's last line names the project's settings file by its path, as long as it is.
        for help in [sweep, say, launch, callback] {
            for line in help
                .lines()
                .filter(|l| !l.contains("{backlog,") && !l.contains("symphony.toml"))
            {
                assert!(line.chars().count() <= 80, "{line}");
            }
        }
    }

    #[test]
    fn reap_reads_its_dry_run_and_its_help_is_argparses() {
        assert_eq!(
            help_of(&["reap", "-h"]),
            "usage: just tasks reap [-h] [--as ACTOR] [--dry-run]\n\nthe idle sweep: close terminals whose agents ended or sat idle.\n\noptions:\n  -h, --help  show this help message and exit\n  --as ACTOR\n  --dry-run   name what the next sweep closes; count nothing\n"
        );
        let parsed = run(&["reap", "--dry-run", "--as", "amber-heron"]);
        assert_eq!(
            (parsed.flag("dry_run"), parsed.get("actor")),
            (true, Some("amber-heron"))
        );
        assert!(!run(&["reap"]).flag("dry_run"));
    }

    #[test]
    fn status_reads_its_json_flag_and_names_itself_in_the_usage() {
        let parsed = run(&["status", "--json", "--as", "amber-heron"]);
        assert_eq!(
            (parsed.command.as_str(), parsed.flag("json")),
            ("status", true)
        );
        assert!(!run(&["status"]).flag("json"));
        assert!(
            help_of(&["status", "-h"])
                .starts_with("usage: just tasks status [-h] [--as ACTOR] [--json]\n\n")
        );
    }

    #[test]
    fn asks_lists_with_its_flags_and_closes_one_by_its_id_and_reason() {
        let listed = run(&["asks", "--all", "--json"]);
        assert_eq!(listed.command, "asks");
        assert!(listed.flag("all") && listed.flag("json"));
        assert_eq!(listed.get("action"), None);
        let closed = run(&["asks", "close", "3", "answered on the card", "--as", "a"]);
        assert_eq!(
            (
                closed.get("action"),
                closed.get("id"),
                closed.get("why"),
                closed.get("actor")
            ),
            (
                Some("close"),
                Some("3"),
                Some("answered on the card"),
                Some("a")
            )
        );
        let Outcome::Usage(refused) = parse_of(&["asks", "open"]) else {
            panic!("asks takes close alone");
        };
        assert!(
            refused.ends_with(
                "just tasks asks: error: argument action: invalid choice: 'open' (choose from 'close')\n"
            ),
            "{refused}"
        );
        let help = help_of(&["asks", "-h"]);
        assert!(help.contains("close ID WHY  close ask ID"), "{help}");
    }

    #[test]
    fn handover_help_says_only_the_new_orchestrator_hears_it() {
        let help = help_of(&["handover", "-h"]);
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
        let help = help_of(&["wait", "-h"]);
        assert!(help.starts_with("usage: just tasks wait [-h] [--as ACTOR] [--timeout TIMEOUT]\n\nPrint what a callback would push ACTOR"), "{help}");
    }

    #[test]
    fn clean_reads_its_dry_run_and_its_help_says_what_it_runs() {
        assert_eq!(
            help_of(&["clean", "-h"]),
            "usage: just tasks clean [-h] [--as ACTOR] [--dry-run]\n\ncleanup, then one idle sweep: what closed and why each open terminal stays\n\noptions:\n  -h, --help  show this help message and exit\n  --as ACTOR\n  --dry-run   name what it would close and why the rest stay; close nothing\n"
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
                    "{}just tasks: error: name a command: `just tasks help` lists them\n",
                    usage::USAGE
                )
            );
        }
    }

    #[test]
    fn an_unknown_command_names_the_nearest_in_two_lines() {
        for (word, near) in [
            ("shwo", "; did you mean 'show'?"),
            ("claime", "; did you mean 'claim'?"),
            ("Verdict", "; did you mean 'verdict'?"),
            ("stat", "; did you mean 'stats'?"),
            ("handove", "; did you mean 'handover'?"),
            ("ver", "; did you mean 'verdict'?"),
            ("nope", ""),
            ("re", ""),
        ] {
            let said = refused(&[word, "x"]);
            assert_eq!(
                said,
                format!(
                    "{}just tasks: error: no command '{word}'{near} (`just tasks help` lists them)\n",
                    usage::USAGE
                )
            );
            assert_eq!(said.lines().count(), 2, "{said}");
        }
    }

    #[test]
    fn a_choice_reads_as_people_spell_it() {
        for (args, dest, value) in [
            (
                &["create", "a/b", "T", "--priority", "p1"][..],
                "priority",
                "1",
            ),
            (&["create", "a/b", "T", "--priority", "P4"], "priority", "4"),
            (&["create", "a/b", "T", "--priority", "0"], "priority", "0"),
            (&["priority", "a/b", "p0"], "priority", "0"),
            (&["move", "a/b", "in-progress"], "status", "in_progress"),
            (&["move", "a/b", "Review"], "status", "review"),
            (
                &["list", "--status", "in-progress"],
                "status",
                "in_progress",
            ),
            (
                &["search", "q", "--status=in-progress"],
                "status",
                "in_progress",
            ),
            (&["verdict", "a/b", "approve"], "verdict", "approve"),
        ] {
            assert_eq!(run(args).get(dest), Some(value), "{args:?}");
        }
        for (args, said) in [
            (
                &["priority", "a/b", "p5"][..],
                "argument priority: invalid choice: 'p5' (choose from '0', '1', '2', '3', '4')",
            ),
            (
                &["create", "a/b", "T", "--priority", "high"],
                "argument --priority: invalid choice: 'high' (choose from '0', '1', '2', '3', '4')",
            ),
            // Every priority is one edit from any other digit: the range is said once.
            (
                &["priority", "a/b", "9"],
                "argument priority: invalid choice: '9' (choose from '0', '1', '2', '3', '4')",
            ),
            (
                &["move", "a/b", "inprogress"],
                "argument status: invalid choice: 'inprogress' (choose from 'backlog', 'concept', \
                 'design', 'ready', 'in_progress', 'review', 'done', 'blocked', 'cancelled'); did \
                 you mean 'in_progress'?",
            ),
            (
                &["role", "a/b", "reveiwer"],
                "argument role: invalid choice: 'reveiwer' (choose from 'orchestrator', \
                 'reviewer'); did you mean 'reviewer'?",
            ),
            (
                &["list", "--status", "don"],
                "argument --status: invalid choice: 'don' (choose from 'backlog', 'concept', \
                 'design', 'ready', 'in_progress', 'review', 'done', 'blocked', 'cancelled'); did \
                 you mean 'done'?",
            ),
        ] {
            assert!(
                refused(args).ends_with(&format!("{said}\n")),
                "{args:?}: {}",
                refused(args)
            );
        }
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
            (&["say"][..], "say", "slug"),
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
    fn needs_takes_every_stamp_after_its_slug_its_reason_required() {
        let needs = run(&[
            "needs", "a/b", "tests", "--tip", "1a2b", "review", "--why", "w",
        ]);
        assert_eq!(needs.get("slug"), Some("a/b"));
        assert_eq!(needs.all("stamps"), ["tests", "review"]);
        assert_eq!(
            (needs.get("tip"), needs.get("why")),
            (Some("1a2b"), Some("w"))
        );
        let none = run(&["needs", "--why=docs only", "a/b", "none"]);
        assert_eq!(none.all("stamps"), ["none"]);
        assert_eq!(none.get("tip"), None);
        // After `--` a stamp may start with a dash, and the `--` is taken.
        assert_eq!(
            run(&["needs", "a/b", "--why", "w", "--", "-x", "tests"]).all("stamps"),
            ["-x", "tests"]
        );
        assert!(
            refused(&["needs", "a/b", "--why", "w"]).ends_with(
                "just tasks needs: error: the following arguments are required: stamps\n"
            )
        );
        assert!(
            refused(&["needs", "a/b", "tests"]).ends_with(
                "just tasks needs: error: the following arguments are required: --why\n"
            )
        );
        assert_eq!(run(&["stamps"]).get("slug"), None);
        assert_eq!(run(&["stamps", "a/b"]).get("slug"), Some("a/b"));
    }

    #[test]
    fn a_gate_decision_raises_or_waives_never_both_and_says_why() {
        let raised = run(&[
            "gate",
            "a/b",
            "tools/size-budget.toml:symphony",
            "--raise",
            "12_400_000",
            "--why",
            "w",
        ]);
        assert_eq!(
            (
                raised.get("gate"),
                raised.get("raise"),
                raised.flag("waive")
            ),
            (
                Some("tools/size-budget.toml:symphony"),
                Some("12_400_000"),
                false
            )
        );
        let waived = run(&[
            "gate", "a/b", "pictures", "--waive", "--tip", "1a2b", "--why=w",
        ]);
        assert_eq!(
            (waived.get("raise"), waived.flag("waive"), waived.get("tip")),
            (None, true, Some("1a2b"))
        );
        assert!(
            refused(&["gate", "a/b", "x", "--raise", "1", "--waive", "--why", "w"]).ends_with(
                "just tasks gate: error: argument --waive: not allowed with argument --raise\n"
            )
        );
        assert!(
            refused(&["gate", "a/b", "pictures", "--waive"])
                .ends_with("just tasks gate: error: the following arguments are required: --why\n")
        );
        assert!(
            refused(&["gate", "a/b", "--waive", "--why", "w"])
                .ends_with("just tasks gate: error: the following arguments are required: gate\n")
        );
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
            refused(&["stats", "--days", "x"])
                .ends_with("just tasks stats: error: argument --days: invalid int value: 'x'\n")
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
    fn what_no_command_takes_is_unrecognized_after_the_commands_usage_in_order() {
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
            let command = args.iter().find(|word| !word.starts_with('-')).unwrap();
            let usage = usage::COMMANDS
                .iter()
                .find(|(name, _, _)| name == command)
                .unwrap()
                .1;
            assert_eq!(
                refused(args),
                format!(
                    "{usage}just tasks {command}: error: unrecognized arguments: {extra} \
                     (`just tasks help {command}` lists what it takes)\n"
                ),
                "{args:?}"
            );
        }
    }

    #[test]
    fn a_mistyped_flag_names_the_line_to_type_with_the_nearest_flag() {
        for (args, line) in [
            (
                &["create", "sim/x", "X", "--priorty", "1"][..],
                "unrecognized arguments: --priorty 1; did you mean \
                 `just tasks create sim/x X --priority 1`?",
            ),
            (
                &["list", "--stauts=ready", "--as", "me"],
                "unrecognized arguments: --stauts=ready; did you mean \
                 `just tasks list --status=ready --as me`?",
            ),
            (
                &["say", "sim/x", "two words", "--ot", "@orchestrator"],
                "unrecognized arguments: --ot @orchestrator; did you mean \
                 `just tasks say sim/x 'two words' --to @orchestrator`?",
            ),
            // One of two with no nearest: no line, but where the flags are.
            (
                &["list", "--stauts", "ready", "--frobnicate"],
                "unrecognized arguments: --stauts ready --frobnicate (`just tasks help list` \
                 lists what it takes)",
            ),
        ] {
            let said = refused(args);
            assert!(said.ends_with(&format!("{line}\n")), "{args:?}: {said}");
            let command = args[0];
            assert!(
                said.starts_with(&format!("usage: just tasks {command} ")),
                "{said}"
            );
        }
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word(""), "''");
        // The command that reaches the board begins the line, so typed again it reaches the board
        // it was typed for, never the project's.
        let board = "TASKS_SERVER=127.0.0.1:1 just tasks --database scratch1";
        let Outcome::Usage(said) = parse_after(board, &strings(&["list", "--stauts", "ready"]))
        else {
            panic!("a mistyped flag is refused");
        };
        assert!(
            said.ends_with(&format!(
                "unrecognized arguments: --stauts ready; did you mean `{board} list --status \
                 ready`?\n"
            )),
            "{said}"
        );
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
        assert_eq!(parse_of(&["-h"]), Outcome::Help(None));
        assert_eq!(parse_of(&["--as", "x", "--help"]), Outcome::Help(None));
        assert_eq!(parse_of(&["claim", "-h"]), Outcome::Help(Some("claim")));
        assert_eq!(
            parse_of(&["claim", "s", "-h", "--bogus"]),
            Outcome::Help(Some("claim"))
        );
        assert_eq!(parse_of(&["show", "--he"]), Outcome::Help(Some("show")));
        assert_eq!(parse_of(&["help", "show"]), Outcome::Help(Some("show")));
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
        // A line from a file names no text; the command says which it takes.
        let filed = run(&["say", "s", "--file", "post.md"]);
        assert_eq!(
            (filed.get("text"), filed.get("file")),
            (None, Some("post.md"))
        );
        assert_eq!(
            run(&["create", "a", "b", "--body-file", "-"]).get("body_file"),
            Some("-")
        );
        assert_eq!(run(&["say", "s", "it's"]).get("text"), Some("it's"));
        // Python 3.14's argparse takes anything that starts like a negative number (`-\.?\d`) as one.
        assert_eq!(run(&["say", "s", "-1x"]).get("text"), Some("-1x"));
        assert_eq!(run(&["say", "s", "-.5x"]).get("text"), Some("-.5x"));
        // What does not start like one is an option, and `say` has none by that name.
        assert!(
            refused(&["say", "s", "-.x"]).ends_with(
                "just tasks say: error: unrecognized arguments: -.x (`just tasks help say` lists \
                 what it takes)\n"
            ),
            "{}",
            refused(&["say", "s", "-.x"])
        );
    }

    #[test]
    fn numbers_read_as_python_reads_them() {
        for (text, n) in [(" 3 ", 3), ("+3", 3), ("1_0", 10), ("-2", -2), ("0", 0)] {
            assert_eq!(
                run(&["stats", "--days", text]).int("days", 14),
                n,
                "{text:?}"
            );
        }
        for text in ["", "1.5", "_1", "1_", "1__0", "x"] {
            assert!(
                matches!(parse_of(&["stats", "--days", text]), Outcome::Usage(_)),
                "{text:?}"
            );
        }
        assert_eq!(run(&["stats"]).int("days", 14), 14);
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
