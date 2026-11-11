//! Command-line arguments with stable option matching and refusals.

#![forbid(unsafe_code)]

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
        return Err(format!("unrecognized arguments: {}", unknown.join(" ")));
    }
    Ok(parsed)
}
