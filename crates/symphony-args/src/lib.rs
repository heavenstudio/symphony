//! Command-line arguments with stable option matching and refusals.

#![forbid(unsafe_code)]

use std::io::Write;

/// A refusal of the command line `symphony COMMAND …`, as argparse words one: the usage, then the
/// error, written to `err`. Its exit code, 2.
pub fn refuse(err: &mut dyn Write, usage: &str, command: &str, problem: &str) -> i32 {
    let _ = writeln!(err, "{usage}\nsymphony {command}: error: {problem}");
    2
}

/// An option a command takes: its name, and whether a value follows it.
pub struct Flag {
    pub name: &'static str,
    pub value: bool,
}

/// A command line read as Python's argparse reads one: `--name VALUE`, `--name=VALUE`, a unique
/// prefix of a name, positionals anywhere, `--` ending the options.
pub struct Parsed {
    values: Vec<(&'static str, String)>,
    pub positionals: Vec<String>,
    pub help: bool,
}

impl Parsed {
    /// All occurrences of a repeatable option, in command-line order.
    pub fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.values
            .iter()
            .filter(move |(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every option given, by its full name, with its value (empty for a flag), in command-line
    /// order across names: what a command whose options are steps runs, one after another.
    pub fn options(&self) -> impl Iterator<Item = (&'static str, &str)> {
        self.values
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
    }

    pub fn flag(&self, name: &str) -> bool {
        self.values.iter().any(|(n, _)| *n == name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Whether argparse would read `arg` as an option rather than a positional.
fn looks_like_option(arg: &str) -> bool {
    arg.starts_with('-') && arg.len() > 1 && !arg.contains(' ') && arg.parse::<f64>().is_err()
}

/// Where a subcommand's name is in `args`: the first argument that is neither an option nor the
/// value of one of `flags` (the options that come before it); the end when there is none.
pub fn command_at(args: &[String], flags: &[Flag]) -> usize {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if !looks_like_option(arg) {
            return at;
        }
        let name = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
        let takes_next = !arg.contains('=')
            && flags.iter().any(|flag| {
                flag.value && (flag.name == name || (name.len() > 2 && flag.name.starts_with(name)))
            });
        at += if takes_next { 2 } else { 1 };
    }
    at.min(args.len())
}

pub fn parse(args: &[String], flags: &[Flag]) -> Result<Parsed, String> {
    let mut parsed = Parsed {
        values: Vec::new(),
        positionals: Vec::new(),
        help: false,
    };
    let mut unknown: Vec<&str> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--" {
            parsed.positionals.extend(rest.by_ref().cloned());
            break;
        }
        if !looks_like_option(arg) {
            parsed.positionals.push(arg.clone());
            continue;
        }
        if arg == "-h" || arg == "--help" {
            parsed.help = true;
            continue;
        }
        let (name, given) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (arg.as_str(), None),
        };
        let exact = flags.iter().find(|f| f.name == name);
        let prefixed: Vec<&Flag> = flags
            .iter()
            .filter(|f| name.len() > 2 && f.name.starts_with(name))
            .collect();
        let flag = match (exact, prefixed.as_slice()) {
            (Some(flag), _) | (None, &[flag]) => flag,
            (None, []) => {
                unknown.push(arg);
                continue;
            }
            (None, many) => {
                let names: Vec<&str> = many.iter().map(|f| f.name).collect();
                return Err(format!(
                    "ambiguous option: {name} could match {}",
                    names.join(", ")
                ));
            }
        };
        if !flag.value {
            if given.is_some() {
                return Err(format!(
                    "argument {}: ignored explicit argument {:?}",
                    flag.name,
                    given.unwrap_or_default()
                ));
            }
            parsed.values.push((flag.name, String::new()));
            continue;
        }
        let value = match given {
            Some(value) => value,
            None => match rest.next() {
                Some(value) if !looks_like_option(value) => value.clone(),
                _ => return Err(format!("argument {}: expected one argument", flag.name)),
            },
        };
        parsed.values.push((flag.name, value));
    }
    if !unknown.is_empty() {
        // The nearest option to the first unknown one, by its name without a value.
        let first = unknown[0]
            .split_once('=')
            .map_or(unknown[0], |(name, _)| name);
        let names = flags.iter().map(|flag| flag.name).chain(["--help"]);
        let near = match nearest(first, names).as_slice() {
            [] => String::new(),
            [one] => format!("; did you mean {one}?"),
            [rest @ .., last] => format!("; did you mean {} or {last}?", rest.join(", ")),
        };
        return Err(format!(
            "unrecognized arguments: {}{near}",
            unknown.join(" ")
        ));
    }
    Ok(parsed)
}

/// The words of `among` nearest to `word`, taken for a mistyping of one, in `among`'s order: those
/// it takes the fewest single edits to reach (a letter added, dropped or changed, or two side by
/// side swapped; case, and `-` against `_`, are no edit), when that is at most a third of `word`'s
/// letters and at least one; else the one word `word` begins, when it begins just one. When every
/// word of several is as near as the nearest (a one-letter word among one-letter choices), none is
/// named: such a list says no more than the choices themselves.
pub fn nearest<'a>(word: &str, among: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let folded = |text: &str| text.trim().to_lowercase().replace('-', "_");
    let word = folded(word);
    let among: Vec<&str> = among.into_iter().collect();
    let limit = (word.chars().count() / 3).max(1);
    let scored: Vec<(usize, &str)> = among
        .iter()
        .map(|candidate| (edits(&word, &folded(candidate)), *candidate))
        .filter(|(distance, _)| *distance <= limit)
        .collect();
    if let Some(best) = scored.iter().map(|(distance, _)| *distance).min() {
        let near: Vec<&str> = scored
            .into_iter()
            .filter(|(distance, _)| *distance == best)
            .map(|(_, candidate)| candidate)
            .collect();
        return if near.len() > 1 && near.len() == among.len() {
            Vec::new()
        } else {
            near
        };
    }
    let begun: Vec<&str> = among
        .into_iter()
        .filter(|candidate| word.chars().count() >= 2 && folded(candidate).starts_with(&word))
        .collect();
    if begun.len() == 1 { begun } else { Vec::new() }
}

/// The single edits from `a` to `b`: a character added, dropped or changed, or two adjacent ones
/// swapped (the optimal string alignment distance).
fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let change = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + change);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}
