//! The reputation relationship tables, from their settings file.
//!
//! The owner tunes `docs/data/relationships.json` (who moves whom) against the sides declared in
//! `docs/data/factions.json`. The Symphony draws the two tables live wherever a page writes
//! `[[relationships:destroy]]` or `[[relationships:help]]` (as `docs/factions/RELATIONSHIPS.md` does), so
//! no document holds a copy of the numbers, and `just content` writes each side's rows into its
//! subfaction. This reads them into [`Tables`], the data both draw from; `symphony relationships --check`
//! (`just relationships`) checks both files against their schemas. A row naming a side or faction
//! `factions.json` lacks, a side with no row, an effect that is not a direction and a size, or a
//! generated table left in the document fails the build of crates/docs-index.
//!
//! Effects are words, not numbers: `up` or `down` and a size, from `slight` to `forces a side`. Each
//! row starts with the side itself (the `self` default, overridable per row); `others` applies one
//! effect to every side the row does not name, except whole factions listed in `except_factions`.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{Flag, parse, refuse};
use crate::schema;
use crate::text::{os_error, read_text, repr, strip};

/// The settings file, under the project root.
pub const SETTINGS: &str = "docs/data/relationships.json";
/// The sides it is tuned against.
pub const FACTIONS: &str = "docs/data/factions.json";
/// The two tables, in the order the page draws them.
pub const TABLES: [&str; 2] = ["destroy", "help"];
/// Each size word and its weight, largest first.
pub const SIZES: [(&str, f64); 5] = [
    ("forces a side", 3.0),
    ("strong", 2.0),
    ("medium", 1.0),
    ("small", 0.5),
    ("slight", 0.25),
];

/// The settings file or the declaration cannot be used as they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsError(pub String);

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An effect's words as a signed size: `up strong` is 2, `down slight` -0.25.
pub fn parse_effect(text: &str) -> Result<f64, SettingsError> {
    let trimmed = strip(text);
    let (direction, size) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
    let weight = SIZES
        .iter()
        .find(|(word, _)| *word == size)
        .map(|(_, w)| *w);
    match (direction, weight) {
        ("up", Some(weight)) => Ok(weight),
        ("down", Some(weight)) => Ok(-weight),
        _ => Err(SettingsError(format!(
            "effect {} must be 'up' or 'down' and one of: {}",
            repr(text),
            SIZES.map(|(word, _)| word).join(", ")
        ))),
    }
}

/// The size word of a weight, whichever its sign: `slight` for 0.25 or -0.25.
pub fn word(value: f64) -> Option<&'static str> {
    SIZES
        .iter()
        .find(|(_, weight)| *weight == value.abs())
        .map(|(word, _)| *word)
}

/// A signed size in words again: `up strong` for 2, `down slight` for -0.25.
pub fn effect_words(value: f64) -> Option<String> {
    let direction = if value > 0.0 { "up" } else { "down" };
    word(value).map(|size| format!("{direction} {size}"))
}

/// One subfaction as `docs/data/factions.json` declares it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Subfaction {
    pub code: String,
    pub icon: String,
    pub name: String,
}

/// One faction as `docs/data/factions.json` declares it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Faction {
    pub name: String,
    pub colour: String,
    pub subfactions: Vec<Subfaction>,
}

/// `docs/data/factions.json`: the factions and their sides, in the order the file lists them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Declaration {
    #[serde(default)]
    pub factions: Vec<Faction>,
}

/// A JSON object's members in the order the text writes them, a repeated key keeping its first place
/// and its last value (as Python's dict reads one), whichever map serde_json was built with.
#[derive(Debug, Clone, Default)]
struct Pairs<V>(Vec<(String, V)>);

impl<V> Pairs<V> {
    fn get(&self, key: &str) -> Option<&V> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Pairs<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Pairs<V>, D::Error> {
        struct Members<V>(PhantomData<V>);
        impl<'de, V: Deserialize<'de>> Visitor<'de> for Members<V> {
            type Value = Pairs<V>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Pairs<V>, A::Error> {
                let mut pairs: Vec<(String, V)> = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, V>()? {
                    match pairs.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = value,
                        None => pairs.push((key, value)),
                    }
                }
                Ok(Pairs(pairs))
            }
        }
        deserializer.deserialize_map(Members(PhantomData))
    }
}

/// `docs/data/relationships.json` as the tables read it: the `self` defaults, and each table's rows
/// with each row's entries, in the order the file writes them.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    defaults: Pairs<Value>,
    tables: BTreeMap<String, Pairs<Pairs<Value>>>,
}

impl Settings {
    /// The settings in `text`, as the file holds them.
    pub fn parse(text: &str) -> Result<Settings, SettingsError> {
        serde_json::from_str(text).map_err(|e| SettingsError(format!("{SETTINGS}: {e}")))
    }
}

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Settings, D::Error> {
        struct Top;
        impl<'de> Visitor<'de> for Top {
            type Value = Settings;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("the relationship settings, an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Settings, A::Error> {
                let mut settings = Settings::default();
                while let Some(key) = map.next_key::<String>()? {
                    if key == "self" {
                        settings.defaults = map.next_value()?;
                    } else if TABLES.contains(&key.as_str()) {
                        settings.tables.insert(key, map.next_value()?);
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(settings)
            }
        }
        deserializer.deserialize_map(Top)
    }
}

/// A side: one subfaction, with the faction it belongs to and that faction's colour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Side {
    pub code: String,
    pub icon: String,
    pub name: String,
    pub faction: String,
    pub colour: String,
}

/// A row's `others`: the effect it gives, the sides that took it, and the factions it left out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Others {
    pub value: f64,
    pub members: Vec<String>,
    pub excepted: Vec<String>,
}

/// What acting on one side does to every side it moves: the side itself first, then the sides the row
/// names, in its order, then the members of its `others`, each with its signed size.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub code: String,
    pub effects: Vec<(String, f64)>,
    pub others: Option<Others>,
}

/// Both tables over the declared sides, a row per side in the declaration's order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tables {
    pub sides: Vec<Side>,
    pub destroy: Vec<Row>,
    pub help: Vec<Row>,
}

impl Tables {
    /// The rows of `table` (`destroy` or `help`); none for another name.
    pub fn rows(&self, table: &str) -> &[Row] {
        match table {
            "destroy" => &self.destroy,
            "help" => &self.help,
            _ => &[],
        }
    }

    /// The row of the side `code` in `table`.
    pub fn row(&self, table: &str, code: &str) -> Option<&Row> {
        self.rows(table).iter().find(|row| row.code == code)
    }

    /// The side called `code`.
    pub fn side(&self, code: &str) -> Option<&Side> {
        self.sides.iter().find(|side| side.code == code)
    }

    /// The tables as JSON: the sides, each table's rows by code as `[code, value]` pairs, and each
    /// `others` as `[value, members, excepted]`.
    pub fn to_json(&self) -> Value {
        let mut rows = serde_json::Map::new();
        let mut groups = serde_json::Map::new();
        for table in TABLES {
            let mut by_code = serde_json::Map::new();
            let mut grouped = serde_json::Map::new();
            for row in self.rows(table) {
                by_code.insert(row.code.clone(), json!(row.effects));
                if let Some(others) = &row.others {
                    grouped.insert(
                        row.code.clone(),
                        json!([others.value, others.members, others.excepted]),
                    );
                }
            }
            rows.insert(table.to_string(), Value::Object(by_code));
            groups.insert(table.to_string(), Value::Object(grouped));
        }
        json!({"sides": self.sides, "rows": rows, "groups": groups})
    }
}

/// The live three-column relationship table: gain/loss symbols, faction-coloured side chips, and
/// the compact "everyone else" group rather than a repeated chip for every member.
pub fn render_table(tables: &Tables, table: &str) -> Result<String, String> {
    use crate::markup::escape;
    let (first, left, right) = match table {
        "destroy" => ("You destroy a ship of", "goes down", "goes up"),
        "help" => ("You help", "goes up", "goes down"),
        _ => {
            return Err(format!(
                "relationships: no table called {table}; use destroy or help"
            ));
        }
    };
    fn mark(value: f64, title: &str) -> Result<String, String> {
        let index = SIZES
            .iter()
            .position(|(_, weight)| *weight == value.abs())
            .ok_or_else(|| format!("unknown relationship size {value}"))?;
        let (colour, symbols) = if value > 0.0 {
            ("#137a3c", ["▲▲▲", "▲▲", "▲", "△", "▴"])
        } else {
            ("#a3161b", ["▼▼▼", "▼▼", "▼", "▽", "▾"])
        };
        Ok(format!(
            "<span style=\"color:{colour};font-weight:700\" title=\"{}\">{}</span>",
            escape(title),
            symbols[index]
        ))
    }
    fn cell(tables: &Tables, table: &str, code: &str, up: bool) -> Result<String, String> {
        let row = tables
            .row(table, code)
            .ok_or_else(|| format!("{table}: no row for {code}"))?;
        let mut parts = Vec::new();
        let action = if up { "gain" } else { "loss" };
        for (other, value) in &row.effects {
            if (*value > 0.0) != up
                || row
                    .others
                    .as_ref()
                    .is_some_and(|g| g.members.contains(other))
            {
                continue;
            }
            let side = tables
                .side(other)
                .ok_or_else(|| format!("{table}: unknown side {other}"))?;
            let size = word(*value).ok_or_else(|| format!("unknown relationship size {value}"))?;
            let symbol = mark(*value, &format!("{}: {size} {action}", side.name))?;
            let chip = format!(
                "<span style=\"white-space:nowrap;border-bottom:3px solid {}\" title=\"{}\">{} {}</span>",
                escape(&side.colour),
                escape(&side.name),
                escape(&side.icon),
                escape(&side.code)
            );
            parts.push(format!("{symbol}&nbsp;{chip}"));
        }
        if let Some(group) = &row.others
            && (group.value > 0.0) == up
            && !group.members.is_empty()
        {
            let size = word(group.value)
                .ok_or_else(|| format!("unknown relationship size {}", group.value))?;
            let symbol = mark(group.value, &format!("{size} {action}"))?;
            let except = if group.excepted.is_empty() {
                String::new()
            } else {
                format!(" except the {}", escape(&group.excepted.join(" and the ")))
            };
            parts.push(format!("{symbol}&nbsp;everyone else{except}"));
        }
        Ok(format!("<td>{}</td>", parts.join(" &nbsp; ")))
    }
    let mut out = vec![format!(
        "<table style=\"border-collapse:collapse;font-size:.92em\"><thead><tr><th style=\"text-align:left\">{first}</th><th style=\"text-align:left\">{left}</th><th style=\"text-align:left\">{right}</th></tr></thead><tbody>"
    )];
    for side in &tables.sides {
        out.push(format!("<tr><td style=\"white-space:nowrap;border-left:4px solid {};padding-left:6px\">{} {} <small>{}</small></td>{}{}</tr>", escape(&side.colour), escape(&side.icon), escape(&side.name), escape(&side.code), cell(tables, table, &side.code, left == "goes up")?, cell(tables, table, &side.code, left != "goes up")?));
    }
    out.push("</tbody></table>".into());
    Ok(out.join("\n"))
}

/// A value as Python's `str()` writes it, for an effect that is not a string.
fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Null => "None".to_string(),
        other => other.to_string(),
    }
}

/// The tables `settings` gives over the sides `declaration` declares. A row for a side that is not
/// declared, a side with no row, an effect that is not words, a side naming itself by code or an
/// unknown faction left out of `others` is refused.
pub fn load(declaration: &Declaration, settings: &Settings) -> Result<Tables, SettingsError> {
    let sides: Vec<Side> = declaration
        .factions
        .iter()
        .flat_map(|faction| {
            faction.subfactions.iter().map(move |sub| Side {
                code: sub.code.clone(),
                icon: sub.icon.clone(),
                name: sub.name.clone(),
                faction: faction.name.clone(),
                colour: faction.colour.clone(),
            })
        })
        .collect();
    let codes: Vec<&str> = sides.iter().map(|side| side.code.as_str()).collect();
    let factions: Vec<&str> = sides.iter().map(|side| side.faction.as_str()).collect();
    let fail = |message: String| Err(SettingsError(message));
    let mut built: BTreeMap<&str, Vec<Row>> = BTreeMap::new();
    for table in TABLES {
        let empty = Pairs::default();
        let spec = settings.tables.get(table).unwrap_or(&empty);
        if let Some((code, _)) = spec
            .0
            .iter()
            .find(|(code, _)| !codes.contains(&code.as_str()))
        {
            return fail(format!("{table}: unknown side {code}"));
        }
        let mut rows = Vec::new();
        for code in &codes {
            let Some(entry) = spec.get(code) else {
                return fail(format!("{table}: no row for {code}"));
            };
            let Some(own) = entry.get("self").or_else(|| settings.defaults.get(table)) else {
                return fail(format!("{table}: no self effect for {code}"));
            };
            let mut effects = vec![((*code).to_string(), parse_effect(&py_str(own))?)];
            for (other, effect) in entry.0.iter().filter(|(k, _)| k != "self" && k != "others") {
                if other == code {
                    return fail(format!(
                        "{table} {code}: name the side itself as 'self', not {other}"
                    ));
                }
                if !codes.contains(&other.as_str()) {
                    return fail(format!("{table} {code}: unknown side {other}"));
                }
                effects.push((other.clone(), parse_effect(&py_str(effect))?));
            }
            let mut others = None;
            if let Some(group) = entry.get("others") {
                let excepted: Vec<String> = match group.get("except_factions") {
                    None => Vec::new(),
                    Some(Value::Array(names)) => names.iter().map(py_str).collect(),
                    Some(_) => {
                        return fail(format!("{table} {code}: except_factions is not a list"));
                    }
                };
                if let Some(name) = excepted.iter().find(|n| !factions.contains(&n.as_str())) {
                    return fail(format!("{table} {code}: unknown faction {name}"));
                }
                let Some(effect) = group.get("effect") else {
                    return fail(format!("{table} {code}: others names no effect"));
                };
                let value = parse_effect(&py_str(effect))?;
                let named: Vec<String> = effects.iter().map(|(c, _)| c.clone()).collect();
                let members: Vec<String> = sides
                    .iter()
                    .filter(|s| !named.contains(&s.code) && !excepted.contains(&s.faction))
                    .map(|s| s.code.clone())
                    .collect();
                effects.extend(members.iter().map(|c| (c.clone(), value)));
                others = Some(Others {
                    value,
                    members,
                    excepted,
                });
            }
            rows.push(Row {
                code: (*code).to_string(),
                effects,
                others,
            });
        }
        built.insert(table, rows);
    }
    Ok(Tables {
        sides,
        destroy: built.remove("destroy").unwrap_or_default(),
        help: built.remove("help").unwrap_or_default(),
    })
}

fn read(root: &Path, rel: &str) -> Result<String, SettingsError> {
    let path = root.join(rel);
    read_text(&path).map_err(|e| SettingsError(os_error(&e, &path)))
}

/// The tables as the settings under `root` give them now.
pub fn tables(root: &Path) -> Result<Tables, SettingsError> {
    let declaration: Declaration = serde_json::from_str(&read(root, FACTIONS)?)
        .map_err(|e| SettingsError(format!("{FACTIONS}: {e}")))?;
    load(&declaration, &Settings::parse(&read(root, SETTINGS)?)?)
}

/// A settings file checked against the schema its own `$schema` names, relative to the file: each
/// line prefixed with the file's name.
pub fn schema_errors(data: &Value, path: &Path) -> Result<Vec<String>, SettingsError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let Some(reference) = data.get("$schema").and_then(Value::as_str) else {
        return Err(SettingsError(format!("{name} names no $schema")));
    };
    let schema_path: PathBuf = path.parent().unwrap_or(Path::new("")).join(reference);
    let text = read_text(&schema_path).map_err(|e| SettingsError(os_error(&e, &schema_path)))?;
    let schema: Value = serde_json::from_str(&text)
        .map_err(|e| SettingsError(format!("{}: {e}", schema_path.display())))?;
    let errors = schema::validate(data, &schema).map_err(|e| SettingsError(e.0))?;
    Ok(errors.into_iter().map(|e| format!("{name} {e}")).collect())
}

const USAGE: &str = "usage: symphony relationships [-h] [--root ROOT] --check";
const HELP: &str = "Check the reputation relationship settings against their schemas.

options:
  -h, --help   show this help message and exit
  --root ROOT  project root holding docs/
  --check      exit 2 if a file does not match its schema";

/// `symphony relationships --check [--root ROOT]` (`just relationships`): both files against their
/// schemas, exit 2 when one does not match.
pub fn check_main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let flags = [
        Flag {
            name: "--root",
            value: true,
        },
        Flag {
            name: "--check",
            value: false,
        },
    ];
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        Ok(parsed) if !parsed.positionals.is_empty() => {
            let problem = format!("unrecognized arguments: {}", parsed.positionals.join(" "));
            return refuse(err, USAGE, "relationships", &problem);
        }
        Ok(parsed) if !parsed.flag("--check") => {
            let problem = "the following arguments are required: --check";
            return refuse(err, USAGE, "relationships", problem);
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, USAGE, "relationships", &problem),
    };
    let root = PathBuf::from(parsed.value("--root").unwrap_or("."));
    let checked = (|| {
        let mut errors = Vec::new();
        for rel in [FACTIONS, SETTINGS] {
            let path = root.join(rel);
            let data: Value = serde_json::from_str(&read(&root, rel)?)
                .map_err(|e| SettingsError(format!("{rel}: {e}")))?;
            errors.extend(schema_errors(&data, &path)?);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            let mut lines = vec!["files do not match their schemas:".to_string()];
            lines.extend(errors);
            Err(SettingsError(lines.join("\n  ")))
        }
    })();
    match checked {
        Ok(()) => {
            let _ = writeln!(
                out,
                "relationships: {SETTINGS} and {FACTIONS} match their schemas"
            );
            0
        }
        Err(problem) => {
            let _ = writeln!(err, "✖ relationships: {problem}");
            2
        }
    }
}
