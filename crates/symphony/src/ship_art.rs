//! Faction ship pages and their live matrix, from the roster and the art plan. The register of each
//! generated page is kept verbatim; the rest is rebuilt by `symphony ship-art --write`.

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use symphony_api::{Block, Cell, ContainerKind, FigureSize, Image, InlineKind, Presentation, Span};

use crate::art;
use crate::cli::{Flag, parse, refuse};
use crate::edit::heading_anchor;
use crate::register::{Decision, REGION_END, REGION_START};
use crate::text::{os_error, read_text};

pub const PLAN: &str = "docs/data/ship-art-plan.json";
pub const FACTIONS: &str = "docs/data/factions.json";
pub const SHAPES: &str = "docs/data/ship-shapes.json";
pub const ROSTER_PAGE: &str = "docs/ships/SHIP_ROSTER.md";
pub const TITLES: [(&str, &str); 6] = [
    ("republic", "Republic"),
    ("empire", "Empire"),
    ("coalition", "Alien Coalition"),
    ("biological", "Biological"),
    ("machine", "Machine"),
    ("dimensional", "Dimensional"),
];
const FIELDS: [&str; 5] = ["subfaction", "outline", "weapons", "defences", "thrusters"];
const INTRO: &str = "Every hull in the roster, one picture each, drawn from above in the colours of the subfaction that flies it. Each card carries up to three candidates; approve it and name the letter in the note.";

pub fn title(faction: &str) -> String {
    TITLES
        .iter()
        .find(|(key, _)| *key == faction)
        .map_or_else(|| capitalize(faction), |(_, t)| (*t).into())
}

pub(crate) fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
    })
}

pub(crate) fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

pub(crate) fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map_or(&[], Vec::as_slice)
}

pub(crate) fn read_json(root: &Path, rel: &str) -> Result<Value, String> {
    let path = root.join(rel);
    let text = read_text(&path).map_err(|e| os_error(&e, &path))?;
    serde_json::from_str(&text).map_err(|e| format!("{rel}: {e}"))
}

pub(crate) fn optional_json(root: &Path, rel: &str) -> Result<Value, String> {
    if root.join(rel).exists() {
        read_json(root, rel)
    } else {
        Ok(json!({}))
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Hull {
    pub id: String,
    pub name: String,
    pub faction: String,
    #[serde(rename = "class")]
    pub class: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub length_m: Value,
    #[serde(default)]
    pub beam_m: Value,
}

impl Hull {
    pub fn label(&self) -> String {
        format!("{} {}", self.id, self.name)
    }
    pub fn page(&self) -> String {
        format!("{}/{}.md", art::SHIP_PAGES, self.faction)
    }
    pub fn href(&self) -> String {
        format!("/{}#{}", self.page(), heading_anchor(&self.label()))
    }
}

/// Data shared by all ship components in one render, in the order the roster lists it.
pub struct Data {
    pub hulls: Vec<Hull>,
    pub roster: Value,
    pub plan: Value,
    pub factions: Value,
    pub subfactions: BTreeMap<String, Vec<String>>,
}

impl Data {
    pub fn load(root: &Path) -> Result<Self, String> {
        let roster = optional_json(root, art::ROSTER)?;
        let hulls = array(&roster, "hulls")
            .iter()
            .map(|h| serde_json::from_value(h.clone()).map_err(|e| format!("{}: {e}", art::ROSTER)))
            .collect::<Result<_, _>>()?;
        let plan = optional_json(root, PLAN)?["hulls"].clone();
        let factions = optional_json(root, FACTIONS)?;
        let subfactions = array(&factions, "factions")
            .iter()
            .map(|f| {
                let name = string(f, "name");
                let key = TITLES
                    .iter()
                    .find(|(_, t)| t.eq_ignore_ascii_case(name))
                    .map_or_else(|| name.to_lowercase(), |(key, _)| (*key).into());
                (
                    key,
                    array(f, "subfactions")
                        .iter()
                        .map(|s| string(s, "name").into())
                        .collect(),
                )
            })
            .collect();
        Ok(Self {
            hulls,
            roster,
            plan,
            factions,
            subfactions,
        })
    }

    pub fn classes(&self) -> Vec<String> {
        array(&self.roster, "classes")
            .iter()
            .map(|c| string(c, "name").into())
            .collect()
    }
    pub fn known_factions(&self) -> Vec<String> {
        unique(self.hulls.iter().map(|h| h.faction.clone()))
    }
    pub fn hull(&self, id: &str) -> Result<&Hull, String> {
        let wanted = id.trim().to_uppercase();
        self.hulls
            .iter()
            .find(|h| h.id == wanted)
            .ok_or_else(|| format!("unknown hull {}", id.trim()))
    }
    pub fn row_name(&self, hull: &Hull) -> String {
        self.plan.get(&hull.id).map_or_else(
            || title(&hull.faction),
            |entry| string(entry, "subfaction").into(),
        )
    }
}

pub(crate) fn unique(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

pub fn faction_link(faction: &str, heading: &str) -> String {
    let page = if faction == "coalition" {
        "alien-coalition"
    } else {
        faction
    };
    let target = if heading.is_empty() {
        format!("factions/{page}")
    } else {
        format!("factions/{page}#{heading}")
    };
    let label = if heading.is_empty() {
        title(faction)
    } else {
        heading.into()
    };
    format!("[[{target}|{label}]]")
}

pub fn page(faction: &str, data: &Data, existing: &str) -> String {
    let mut lines = vec![
        format!("# {} ships", title(faction)),
        String::new(),
        INTRO.into(),
        String::new(),
        format!("Faction page: {}.", faction_link(faction, "")),
        String::new(),
        format!("[[ship-matrix:{faction}]]"),
        String::new(),
    ];
    let mut shown = "";
    for hull in data.hulls.iter().filter(|h| h.faction == faction) {
        if hull.class != shown {
            shown = &hull.class;
            lines.extend([format!("## {shown}s"), String::new()]);
        }
        let label = hull.label();
        let facts = format!(
            "{} · {} · {} × {} m",
            hull.class,
            hull.role.as_deref().unwrap_or("None"),
            hull.length_m,
            hull.beam_m
        );
        lines.extend([format!("### {label}"), String::new()]);
        if let Some(entry) = data.plan.get(&hull.id) {
            let sub = string(entry, "subfaction");
            let link = faction_link(faction, if sub == title(faction) { "" } else { sub });
            let body = FIELDS[1..]
                .iter()
                .map(|key| {
                    format!(
                        "**{}:** {}.",
                        capitalize(key),
                        string(entry, key).trim_end_matches('.')
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            lines.extend([
                format!("*{facts} · {link}*"),
                String::new(),
                body,
                String::new(),
            ]);
        } else {
            lines.extend([
                format!("*{facts}*"),
                String::new(),
                "Not planned yet.".into(),
                String::new(),
            ]);
        }
        lines.extend([format!("[[art:{label} · ship]]"), String::new()]);
    }
    let register = existing.find(REGION_START).and_then(|start| {
        existing
            .find(REGION_END)
            .filter(|end| *end >= start)
            .map(|end| &existing[start..end + REGION_END.len()])
    });
    format!(
        "{}\n{}\n",
        lines.join("\n"),
        register.map_or_else(|| format!("{REGION_START}\n{REGION_END}"), str::to_string)
    )
}

pub fn picture_width(length: f64) -> i64 {
    (40.0 + 30.0 * (length.max(1.0) / 20.0).log2())
        .round_ties_even()
        .max(40.0) as i64
}

/// A hull in a matrix cell: its picture at its length's width, linked to the full picture, its
/// name linked to its section and its length and state.
fn hull_figure(hull: &Hull, chosen: Option<(String, bool)>) -> Block {
    let alt = hull.label();
    let mut caption = vec![
        Span::Link {
            href: hull.href(),
            target: None,
            title: String::new(),
            spans: vec![Span::Text { text: alt.clone() }],
        },
        Span::LineBreak,
    ];
    let small = |text: String| Span::Styled {
        tag: InlineKind::Small,
        presentation: Presentation::default(),
        spans: vec![Span::Text { text }],
    };
    let Some((src, pending)) = chosen else {
        caption.push(small(format!("{} m · no picture yet", hull.length_m)));
        return Block::Figure {
            picture: None,
            caption,
            href: None,
            badge: None,
            size: FigureSize::Cell,
        };
    };
    let width = picture_width(hull.length_m.as_f64().unwrap_or(1.0));
    let state = if pending {
        "pending review"
    } else {
        "approved"
    };
    caption.push(small(format!("{} m · {state}", hull.length_m)));
    Block::Figure {
        picture: Some(Image {
            src: src.clone(),
            alt,
            presentation: Presentation {
                attributes: [
                    ("width".to_string(), width.to_string()),
                    (
                        "style".to_string(),
                        format!("width:{width}px;max-width:none"),
                    ),
                ]
                .into(),
                ..Presentation::default()
            },
        }),
        caption,
        href: Some(src),
        badge: None,
        size: FigureSize::Cell,
    }
}

/// The scale's note and the matrix of `factions`' hulls: a row per subfaction, a column per class.
pub fn matrix(
    data: &Data,
    cards: &BTreeMap<String, Vec<Decision>>,
    factions: &[String],
) -> Result<Vec<Block>, String> {
    let columns = unique(
        data.classes()
            .into_iter()
            .chain(data.hulls.iter().map(|h| h.class.clone())),
    );
    let text = |text: String| Span::Text { text };
    let note = Block::Container {
        tag: ContainerKind::Div,
        presentation: Presentation::class("matrix-note"),
        blocks: vec![Block::Paragraph {
            spans: vec![text(format!(
                "Scale: a 20 m hull is drawn 40 px wide and every doubling of length adds 30 px, so a 2400 m dreadnought is {} px wide.",
                picture_width(2400.0)
            ))],
        }],
    };
    let cell = |header: bool, spans: Vec<Span>, presentation: Presentation| Cell {
        header,
        spans,
        presentation,
    };
    let mut head = vec![cell(
        true,
        vec![text("Subfaction".into())],
        Presentation::default(),
    )];
    head.extend(
        columns
            .iter()
            .map(|c| cell(true, vec![text(c.clone())], Presentation::default())),
    );
    let mut rows = Vec::new();
    for faction in unique(
        TITLES
            .iter()
            .map(|(key, _)| (*key).into())
            .chain(data.known_factions()),
    ) {
        if !factions.contains(&faction) {
            continue;
        }
        let names = unique(
            data.subfactions
                .get(&faction)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .chain(
                    data.hulls
                        .iter()
                        .filter(|h| h.faction == faction)
                        .map(|h| data.row_name(h)),
                ),
        );
        for name in names {
            let title = title(&faction);
            let mut heading = vec![text(name.clone())];
            if name != title {
                heading.push(Span::LineBreak);
                heading.push(Span::Styled {
                    tag: InlineKind::Small,
                    presentation: Presentation::default(),
                    spans: vec![text(title)],
                });
            }
            let mut row = vec![cell(
                true,
                heading,
                Presentation {
                    attributes: [("scope".to_string(), "row".to_string())].into(),
                    ..Presentation::default()
                },
            )];
            for column in &columns {
                let mut figures = Vec::new();
                for hull in data.hulls.iter().filter(|h| {
                    h.faction == faction && h.class == *column && data.row_name(h) == name
                }) {
                    let chosen = art::choose(
                        cards.get(&faction).map_or(&[], Vec::as_slice),
                        &hull.label(),
                        "ship",
                    )
                    .map_err(|e| e.0)?;
                    figures.push(Span::Blocks {
                        blocks: vec![hull_figure(hull, chosen)],
                    });
                }
                row.push(cell(false, figures, Presentation::default()));
            }
            rows.push(row);
        }
    }
    Ok(vec![
        note,
        Block::Table {
            head,
            rows,
            presentation: Presentation::class("matrix"),
        },
    ])
}

/// Biological donor ids are declared in the roster document's donor table.
pub fn donors(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let path = root.join(ROSTER_PAGE);
    let source = if path.is_file() {
        read_text(&path).map_err(|e| os_error(&e, &path))?
    } else {
        String::new()
    };
    let Some((_, section)) = source.split_once("## Biological donor hulls") else {
        return Ok(BTreeMap::new());
    };
    let pattern = Regex::new(r"^[A-Z]{3}-[A-Z]+\d+").expect("donor id");
    Ok(section
        .split("\n## ")
        .next()
        .unwrap_or("")
        .lines()
        .filter_map(|line| {
            let cells: Vec<_> = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            if cells.len() < 3 {
                return None;
            }
            let first = pattern.find(cells[0])?;
            let donor = pattern.find(cells[2])?;
            cells[0][first.end()..]
                .starts_with(' ')
                .then(|| (first.as_str().into(), donor.as_str().into()))
        })
        .collect())
}

pub fn check(data: &Data, shapes: &Value, donors: &BTreeMap<String, String>) -> Result<(), String> {
    let hulls: BTreeMap<_, _> = data.hulls.iter().map(|h| (h.id.as_str(), h)).collect();
    for (id, entry) in data.plan.as_object().into_iter().flatten() {
        let hull = hulls
            .get(id.as_str())
            .ok_or_else(|| format!("the plan names {id}, which is not in the roster"))?;
        let missing: Vec<_> = FIELDS
            .iter()
            .filter(|key| string(entry, key).trim().is_empty())
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(format!("{id} has no {}", missing.join(", ")));
        }
        let sub = string(entry, "subfaction");
        if data
            .subfactions
            .get(&hull.faction)
            .is_some_and(|allowed| !allowed.iter().any(|s| s == sub))
        {
            return Err(format!(
                "{id} is flown by {sub}, which is not a {} subfaction",
                hull.faction
            ));
        }
    }
    let mut codes = HashSet::new();
    for class in array(&data.roster, "classes") {
        if let Some(code) = class["code"].as_str()
            && !codes.insert(code)
        {
            return Err(format!(
                "two classes take the code {code} in {}",
                art::ROSTER
            ));
        }
    }
    for hull in &data.hulls {
        let class = array(&data.roster, "classes")
            .iter()
            .find(|c| string(c, "name") == hull.class)
            .ok_or_else(|| {
                format!(
                    "{} is a {}, which is not a class in the roster's classes",
                    hull.id, hull.class
                )
            })?;
        if let Some(code) = class["code"].as_str()
            && !Regex::new(&format!(r"^[A-Z]+-{}[0-9]+$", regex::escape(code)))
                .map_err(|e| e.to_string())?
                .is_match(&hull.id)
        {
            return Err(format!(
                "{} is a {}, so its id takes the code {code}",
                hull.id, hull.class
            ));
        }
    }
    let grammars: HashSet<_> = array(&data.roster, "geometries")
        .iter()
        .map(|g| string(g, "id"))
        .collect();
    let mut families = HashSet::new();
    for family in array(shapes, "families") {
        let id = string(family, "id");
        if !families.insert(id) {
            return Err(format!("two families are called {id} in {SHAPES}"));
        }
        for grammar in array(family, "grammars").iter().filter_map(Value::as_str) {
            if !grammars.contains(grammar) {
                return Err(format!(
                    "family {id} names grammar {grammar}, which the roster does not have"
                ));
            }
        }
    }
    for hull in &data.hulls {
        let Some(entry) = data.plan.get(&hull.id) else {
            continue;
        };
        let shape = string(entry, "shape");
        if shape.trim().is_empty() {
            return Err(format!(
                "{} has no shape; name a family from {SHAPES}",
                hull.id
            ));
        }
        if !families.contains(shape) {
            return Err(format!(
                "{} names shape {shape}, which is not a family in {SHAPES}",
                hull.id
            ));
        }
    }
    for (id, donor) in donors {
        if let (Some(entry), Some(source)) = (data.plan.get(id), data.plan.get(donor))
            && string(entry, "outline")
                .to_lowercase()
                .contains("taken-over")
            && entry["shape"] != source["shape"]
        {
            return Err(format!(
                "{id} is taken over from {donor}, so its shape is {}, not {}",
                string(source, "shape"),
                string(entry, "shape")
            ));
        }
    }
    Ok(())
}

pub fn pages(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    read_json(root, art::ROSTER)?;
    let data = Data::load(root)?;
    if !root.join(SHAPES).is_file() {
        return Err(format!("{SHAPES} is missing"));
    }
    let shapes = read_json(root, SHAPES)?;
    let reference = shapes["$schema"]
        .as_str()
        .ok_or_else(|| format!("{SHAPES} names no $schema"))?;
    let schema = read_json(root, &format!("docs/data/{reference}"))?;
    let errors = crate::schema::validate(&shapes, &schema).map_err(|e| e.0)?;
    if !errors.is_empty() {
        return Err(format!(
            "{SHAPES} does not match its schema: {}",
            errors.join("; ")
        ));
    }
    check(&data, &shapes, &donors(root)?)?;
    data.known_factions()
        .into_iter()
        .map(|faction| {
            let path = root.join(art::SHIP_PAGES).join(format!("{faction}.md"));
            // A read failure must never turn an existing register into an empty one on --write.
            let existing = if path.exists() {
                read_text(&path).map_err(|e| os_error(&e, &path))?
            } else {
                String::new()
            };
            let text = page(&faction, &data, &existing);
            Ok((path, text))
        })
        .collect()
}

pub fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    const USAGE: &str = "usage: symphony ship-art [-h] [--root ROOT] [--write] [--check]";
    let flags = [
        Flag {
            name: "--root",
            value: true,
        },
        Flag {
            name: "--write",
            value: false,
        },
        Flag {
            name: "--check",
            value: false,
        },
    ];
    let parsed = match parse(args, &flags) {
        Ok(p) if p.help => {
            let _ = writeln!(
                out,
                "{USAGE}\n\nBuild each faction's ships page from the roster and the art plan."
            );
            return 0;
        }
        Ok(p) if p.positionals.is_empty() => p,
        Ok(p) => {
            return refuse(
                err,
                USAGE,
                "ship-art",
                &format!("unrecognized arguments: {}", p.positionals.join(" ")),
            );
        }
        Err(e) => return refuse(err, USAGE, "ship-art", &e),
    };
    let root = PathBuf::from(parsed.value("--root").unwrap_or("."));
    let wanted = match pages(&root) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "✖ {e}");
            return 1;
        }
    };
    let mut stale = false;
    for (path, text) in wanted {
        if read_text(&path).is_ok_and(|existing| existing == text) {
            continue;
        }
        let rel = path.strip_prefix(&root).unwrap_or(&path).display();
        if parsed.flag("--write") {
            let written = std::fs::create_dir_all(path.parent().unwrap_or(&root))
                .and_then(|()| std::fs::write(&path, &text));
            if let Err(e) = written {
                let _ = writeln!(err, "✖ {}", os_error(&e, &path));
                return 1;
            }
            let _ = writeln!(out, "rebuilt {rel}");
        } else if parsed.flag("--check") {
            stale = true;
            let _ = writeln!(err, "stale ships page: {rel} (run `just ship-art`)");
        }
    }
    if parsed.flag("--write") {
        return 0;
    }
    if parsed.flag("--check") {
        if stale {
            return 1;
        }
        let _ = writeln!(
            out,
            "ships pages: every hull matches the roster, the plan and its register"
        );
        return 0;
    }
    let _ = writeln!(out, "{USAGE}");
    2
}
