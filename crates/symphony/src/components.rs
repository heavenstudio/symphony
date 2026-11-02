//! Live `[[name:argument|option]]` components. A page context loads each input once, and a new render
//! reads it afresh so a verdict, a plan edit or a screen capture is visible immediately.

#[path = "component_icons.rs"]
mod icons;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::art;
use crate::markup::escape;
use crate::register::Decision;
use crate::render::{Builtin, Components};
use crate::ship_art::{self, Data, Hull, array, optional_json, string};
use crate::text::{read_text, splitlines};

/// The default provider. Its per-page copy is a snapshot for one render, never a cache across edits.
pub struct Live {
    root: PathBuf,
    doc_path: String,
    data: OnceLock<Result<Data, String>>,
    shapes: OnceLock<Result<Value, String>>,
    catalogue: OnceLock<Result<Value, String>>,
    manifest: OnceLock<Result<Value, String>>,
    relationships: OnceLock<Result<crate::relationships::Tables, String>>,
    cards: Mutex<BTreeMap<String, Result<Vec<Decision>, String>>>,
}

impl Live {
    pub fn new(root: impl Into<PathBuf>, doc_path: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            doc_path: doc_path.into(),
            data: OnceLock::new(),
            shapes: OnceLock::new(),
            catalogue: OnceLock::new(),
            manifest: OnceLock::new(),
            relationships: OnceLock::new(),
            cards: Mutex::new(BTreeMap::new()),
        }
    }

    fn data(&self) -> Result<&Data, String> {
        self.data
            .get_or_init(|| Data::load(&self.root))
            .as_ref()
            .map_err(Clone::clone)
    }

    fn cards(&self, rel: &str) -> Result<Vec<Decision>, String> {
        let mut cards = self.cards.lock().unwrap_or_else(PoisonError::into_inner);
        cards
            .entry(rel.into())
            .or_insert_with(|| {
                let path = self.root.join(rel);
                if !path.is_file() {
                    return Ok(Vec::new());
                }
                let text = read_text(&path).map_err(|e| crate::text::os_error(&e, &path))?;
                Ok(art::register_cards(&splitlines(&text)))
            })
            .clone()
    }

    fn shown(&self, hull: &Hull) -> Result<Option<(String, bool)>, String> {
        art::choose(&self.cards(&hull.page())?, &hull.label(), "ship").map_err(|e| e.0)
    }

    fn ship_html(&self, hull: &Hull, size: &str) -> Result<String, String> {
        let picture = self.shown(hull)?;
        let state = match &picture {
            None => "no picture yet",
            Some((_, true)) => "pending review",
            Some((_, false)) => "approved",
        };
        let data = self.data()?;
        let sub = string(&data.plan[&hull.id], "subfaction");
        let facts = [&hull.class, sub]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let name = escape(&hull.label());
        let title = escape(&format!("{} · {facts} · {state}", hull.label()));
        let image = picture.as_ref().map_or_else(
            || "<span class=\"ship-none\">no picture yet</span>".into(),
            |(src, _)| {
                format!(
                    "<img src=\"{}\" alt=\"{name}\" loading=\"lazy\">",
                    escape(src)
                )
            },
        );
        let note = if state == "approved" || picture.is_none() {
            ""
        } else {
            " · pending review"
        };
        let caption = if size == "tile" {
            format!("<span class=\"ship-caption\">{}</span>", escape(&hull.id))
        } else {
            format!(
                "<span class=\"ship-caption\">{name}<small>{}{note}</small></span>",
                escape(&facts)
            )
        };
        Ok(format!(
            "<a class=\"ship ship-{size}\" href=\"{}\" title=\"{title}\">{image}{caption}</a>",
            hull.href()
        ))
    }

    fn art(&self, arg: &str) -> Result<String, String> {
        let (sub, kinds) = art::parse_slot(arg).map_err(|e| e.0)?;
        let data = self.data()?;
        let known = ship_art::TITLES.iter().any(|(_, name)| *name == sub)
            || data.hulls.iter().any(|h| h.label() == sub)
            || array(&data.factions, "factions").iter().any(|f| {
                string(f, "name") == sub
                    || array(f, "subfactions")
                        .iter()
                        .any(|s| string(s, "name") == sub)
            });
        if !known {
            return Err(format!(
                "art: no subfaction, faction or hull is called {sub}"
            ));
        }
        art::slot_html(&self.cards(&self.doc_path)?, &sub, &kinds).map_err(|e| e.0)
    }

    fn screen(&self, arg: &str) -> Result<String, String> {
        let name = arg.trim();
        let catalogue = self
            .catalogue
            .get_or_init(|| optional_json(&self.root, crate::screens::CATALOGUE))
            .as_ref()
            .map_err(Clone::clone)?;
        let entry = array(catalogue, "screens")
            .iter()
            .find(|e| string(e, "id") == name)
            .ok_or_else(|| {
                format!("screen: no screen is called {name} (docs/data/screens.json lists them)")
            })?;
        let title = escape(string(entry, "title"));
        let manifest = self
            .manifest
            .get_or_init(|| {
                crate::screens::read_manifest(&self.root.join(crate::screens::OUT))
                    .map(|manifest| manifest.map_or(Value::Null, Value::Object))
            })
            .as_ref()
            .map_err(Clone::clone)?;
        let shot = &manifest["screens"][name];
        if shot.is_null()
            || !self
                .root
                .join(crate::screens::OUT)
                .join(string(shot, "file"))
                .is_file()
        {
            return Ok(format!(
                "<div class=\"screen screen-missing\" data-screen=\"{}\">{title}: not captured yet <small>(<code>just screens</code> shoots it)</small></div>",
                escape(name)
            ));
        }
        let commit = shot
            .get("commit")
            .unwrap_or(&manifest["commit"])
            .as_str()
            .unwrap_or("");
        let day = shot
            .get("date")
            .unwrap_or(&manifest["date"])
            .as_str()
            .unwrap_or("");
        let caption = format!(
            "{title} · {} · {}",
            escape(&prefix(commit, 12)),
            escape(day)
        );
        Ok(format!(
            "<figure class=\"screen\" data-screen=\"{}\"><img src=\"/{}/{}?h={}\" alt=\"{title}\" loading=\"lazy\"><figcaption>{caption}</figcaption></figure>",
            escape(name),
            crate::screens::OUT,
            escape(string(shot, "file")),
            escape(&prefix(string(shot, "hash"), 12))
        ))
    }

    fn matrix(&self, arg: &str) -> Result<String, String> {
        let data = self.data()?;
        let known = data.known_factions();
        let wanted = arg.trim().to_lowercase();
        let wanted = if wanted.is_empty() { "all" } else { &wanted };
        if wanted != "all" && !known.iter().any(|f| f == wanted) {
            return Err(format!(
                "ship-matrix: no faction is called {}; use all or one of {}",
                arg.trim(),
                known.join(", ")
            ));
        }
        let factions = if wanted == "all" {
            known
        } else {
            vec![wanted.into()]
        };
        let cards = factions
            .iter()
            .map(|f| {
                Ok((
                    f.clone(),
                    self.cards(&format!("{}/{f}.md", art::SHIP_PAGES))?,
                ))
            })
            .collect::<Result<_, String>>()?;
        ship_art::matrix_table(data, &cards, &factions)
    }

    fn subfaction(&self, arg: &str, option: Option<&str>) -> Result<String, String> {
        let data = self.data()?;
        let declared = array(&data.factions, "factions")
            .iter()
            .flat_map(|f| array(f, "subfactions"))
            .map(|s| string(s, "name"));
        let planned = data
            .plan
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, e)| string(e, "subfaction"));
        let name = declared
            .chain(planned)
            .find(|n| n.to_lowercase() == arg.trim().to_lowercase())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| format!("subfaction-ships: no subfaction is called {}", arg.trim()))?;
        let size = size(option.filter(|s| !s.is_empty()).or(Some("small")))?;
        let classes = data.classes();
        let mut flown: Vec<_> = data
            .hulls
            .iter()
            .filter(|h| string(&data.plan[&h.id], "subfaction") == name)
            .collect();
        if flown.is_empty() {
            return Ok(format!(
                "<p class=\"muted subfaction-ships\">{} flies no hulls in the roster yet.</p>",
                escape(name)
            ));
        }
        flown.sort_by_key(|h| {
            classes
                .iter()
                .position(|c| *c == h.class)
                .unwrap_or(classes.len())
        });
        let items = flown
            .into_iter()
            .map(|h| self.ship_html(h, size))
            .collect::<Result<Vec<_>, _>>()?
            .join("");
        Ok(format!(
            "<span class=\"ship-strip subfaction-ships\">{items}</span>"
        ))
    }

    fn shape_matrix(&self, arg: &str, option: Option<&str>) -> Result<String, String> {
        if !self.root.join(ship_art::SHAPES).is_file() {
            return Err(format!("shape-matrix: {} is missing", ship_art::SHAPES));
        }
        let shapes = self
            .shapes
            .get_or_init(|| optional_json(&self.root, ship_art::SHAPES))
            .as_ref()
            .map_err(Clone::clone)?;
        let families = array(shapes, "families");
        let data = self.data()?;
        let known = data.known_factions();
        let wanted = arg.trim().to_lowercase();
        let wanted = if wanted.is_empty() { "all" } else { &wanted };
        if !["all", "free"].contains(&wanted) && !known.iter().any(|f| f == wanted) {
            return Err(format!(
                "shape-matrix: no faction is called {}; use all, free or one of {}",
                arg.trim(),
                known.join(", ")
            ));
        }
        let pivot = option.unwrap_or("").trim().to_lowercase();
        if !["", "class"].contains(&pivot.as_str()) {
            return Err(format!(
                "shape-matrix: unknown option {}; use class",
                option.unwrap_or("")
            ));
        }
        let hulls: Vec<_> = data
            .hulls
            .iter()
            .filter(|h| ["all", "free"].contains(&wanted) || h.faction == wanted)
            .collect();
        let columns: Vec<(String, String)> = if pivot == "class" {
            ship_art::unique(
                data.classes()
                    .into_iter()
                    .chain(hulls.iter().map(|h| h.class.clone())),
            )
            .into_iter()
            .map(|c| (c.clone(), c))
            .collect()
        } else {
            known
                .into_iter()
                .filter(|f| hulls.iter().any(|h| h.faction == *f))
                .map(|f| (f.clone(), ship_art::title(&f)))
                .collect()
        };
        let donors = ship_art::donors(&self.root)?;
        let mut rows = Vec::new();
        for family in families {
            let id = string(family, "id");
            if wanted == "free"
                && data
                    .hulls
                    .iter()
                    .any(|h| string(&data.plan[&h.id], "shape") == id)
            {
                continue;
            }
            let members: Vec<_> = hulls
                .iter()
                .filter(|h| string(&data.plan[&h.id], "shape") == id)
                .collect();
            let count = match members.len() {
                0 => "free".into(),
                1 => "1 hull".into(),
                n => format!("{n} hulls"),
            };
            let head = format!(
                "<b>{}</b><small class=\"family-reads\">{} · {count}</small>",
                escape(string(family, "name")),
                escape(string(family, "reads_as"))
            );
            let mut cells = String::new();
            if wanted != "free" {
                for (column, _) in &columns {
                    cells.push_str("<td>");
                    for hull in &members {
                        if (if pivot == "class" {
                            &hull.class
                        } else {
                            &hull.faction
                        }) != column
                        {
                            continue;
                        }
                        let note = donors
                            .get(&hull.id)
                            .filter(|_| {
                                string(&data.plan[&hull.id], "outline")
                                    .to_lowercase()
                                    .contains("taken-over")
                            })
                            .map_or_else(String::new, |donor| {
                                format!(
                                    " <small title=\"drawn on its donor's outline\">← {}</small>",
                                    escape(donor)
                                )
                            });
                        cells.push_str(&format!(
                            "<div>{}{note}</div>",
                            self.ship_html(hull, "tile")?
                        ));
                    }
                    cells.push_str("</td>");
                }
            }
            let refs = array(family, "references")
                .iter()
                .map(|reference| self.reference_html(reference))
                .collect::<Vec<_>>()
                .join("<br>");
            rows.push(format!(
                "<tr><th scope=\"row\">{head}</th>{cells}<td>{}</td></tr>",
                if refs.is_empty() {
                    "<small>original</small>"
                } else {
                    &refs
                }
            ));
        }
        let titles = if wanted == "free" {
            String::new()
        } else {
            columns
                .iter()
                .map(|(_, t)| format!("<th>{}</th>", escape(t)))
                .collect()
        };
        let mut table = vec![
            "<table class=\"matrix shape-matrix\">".into(),
            format!("<thead><tr><th>Family</th>{titles}<th>References</th></tr></thead>"),
            "<tbody>".into(),
        ];
        table.extend(rows);
        table.extend(["</tbody>".into(), "</table>".into()]);
        let loose: Vec<_> = hulls
            .iter()
            .filter(|h| {
                !families
                    .iter()
                    .any(|f| string(f, "id") == string(&data.plan[&h.id], "shape"))
            })
            .map(|h| h.label())
            .collect();
        if !loose.is_empty() && wanted != "free" {
            table.push(format!(
                "<p class=\"matrix-scale\">Not classified: {}</p>",
                escape(&loose.join(", "))
            ));
        }
        Ok(table.join("\n"))
    }

    fn reference_html(&self, reference: &Value) -> String {
        let name = escape(&format!(
            "{}: {}",
            string(reference, "game"),
            string(reference, "ship")
        ));
        let atlas = string(reference, "atlas");
        let rel = format!("artifacts/references/shape-atlas/2026-09-21/ships/{atlas}.svg");
        if !atlas.is_empty() && self.root.join(&rel).is_file() {
            return format!(
                "<span class=\"shape-ref\"><img src=\"/{}\" alt=\"{name}\" loading=\"lazy\" style=\"height:28px;vertical-align:middle;background:#fff;border-radius:3px\"> {name}</span>",
                escape(&rel)
            );
        }
        format!("<span class=\"shape-ref\">{name}</span>")
    }
}

impl Components for Live {
    fn cache_version(&self) -> Option<String> {
        let mut digest = Sha256::new();
        // The document fingerprint covers registers. These are the remaining inputs a component
        // reads; content rather than mtimes also catches a same-length edit with a preserved time.
        for rel in [
            art::ROSTER,
            ship_art::PLAN,
            ship_art::FACTIONS,
            ship_art::SHAPES,
            crate::screens::CATALOGUE,
            crate::relationships::SETTINGS,
            "artifacts/screens/manifest.json",
        ] {
            let bytes = std::fs::read(self.root.join(rel));
            digest.update(rel.as_bytes());
            digest.update([u8::from(bytes.is_ok())]);
            let bytes = bytes.unwrap_or_default();
            digest.update(bytes.len().to_le_bytes());
            digest.update(&bytes);
            if rel == ship_art::SHAPES
                && let Ok(data) = serde_json::from_slice::<Value>(&bytes)
            {
                for family in array(&data, "families") {
                    for reference in array(family, "references") {
                        let path = format!(
                            "artifacts/references/shape-atlas/2026-09-21/ships/{}.svg",
                            string(reference, "atlas")
                        );
                        digest.update([u8::from(self.root.join(path).is_file())]);
                    }
                }
            }
            if rel == "artifacts/screens/manifest.json"
                && let Ok(data) = serde_json::from_slice::<Value>(&bytes)
            {
                for (_, shot) in data["screens"].as_object().into_iter().flatten() {
                    digest.update([u8::from(
                        self.root
                            .join(crate::screens::OUT)
                            .join(string(shot, "file"))
                            .is_file(),
                    )]);
                }
            }
        }
        Some(
            digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    }

    fn for_page(&self, doc_path: &str) -> Option<Box<dyn Components>> {
        Some(Box::new(Self::new(&self.root, doc_path)))
    }

    fn render_one(&self, name: &str, arg: &str, option: Option<&str>) -> Result<String, String> {
        match name {
            "task" => Builtin.render_one(name, arg, option.filter(|s| !s.is_empty())),
            "ship" => self.ship_html(self.data()?.hull(arg)?, size(option)?),
            "ships" => {
                let ids: Vec<_> = arg
                    .split(',')
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .collect();
                if ids.is_empty() {
                    return Err("ships: name at least one hull".into());
                }
                let size = size(option)?;
                let data = self.data()?;
                let items: String = ids
                    .into_iter()
                    .map(|id| {
                        data.hull(id)
                            .and_then(|h| self.ship_html(h, size))
                            .unwrap_or_else(|e| error(&e))
                    })
                    .collect();
                Ok(format!("<span class=\"ship-strip\">{items}</span>"))
            }
            "art" => self.art(arg),
            "screen" => self.screen(arg),
            "ship-matrix" => self.matrix(arg),
            "shape-matrix" => self.shape_matrix(arg, option),
            "subfaction-ships" => self.subfaction(arg, option),
            "relationships" => {
                let table = arg.trim();
                if !crate::relationships::TABLES.contains(&table) {
                    return Err(format!(
                        "relationships: no table called {table}; use destroy or help"
                    ));
                }
                let tables = self
                    .relationships
                    .get_or_init(|| {
                        crate::relationships::tables(&self.root)
                            .map_err(|e| format!("relationships: {e}"))
                    })
                    .as_ref()
                    .map_err(Clone::clone)?;
                crate::relationships::render_table(tables, table)
            }
            "capability" | "weapon" | "gear" | "status" | "defence" => {
                icons::render(name, arg, option)
            }
            other => Err(format!("unknown component {other}")),
        }
    }
}

fn prefix(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn size(option: Option<&str>) -> Result<&str, String> {
    let size = option.unwrap_or("row").trim();
    let size = if size.is_empty() { "row" } else { size };
    if ["small", "row", "large"].contains(&size) {
        Ok(size)
    } else {
        Err(format!("unknown size {size}; use small, row, large"))
    }
}

pub fn error(message: &str) -> String {
    format!(
        "<span class=\"component-error\" title=\"component error\">⚠ {}</span>",
        escape(message)
    )
}

/// Render one document component into HTML and collect its rendering errors.
pub fn render_one(
    root: &Path,
    doc_path: &str,
    name: &str,
    arg: &str,
    option: Option<&str>,
) -> String {
    Live::new(root, doc_path)
        .render_one(name, arg, option)
        .unwrap_or_else(|problem| error(&problem))
}
