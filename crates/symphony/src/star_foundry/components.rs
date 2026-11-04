//! Star Foundry's `[[name:argument|option]]` components, rendered to blocks. A page loads each
//! input once, and a new render reads it afresh so a verdict, a plan edit or a screen capture is
//! visible immediately.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;
use sha2::{Digest, Sha256};
use symphony_plugin::vocabulary::{
    Block, Cell, ContainerKind, FigureSize, Image, InlineKind, Media, Presentation, Span,
};
use symphony_plugin::{Component, Page};

use crate::art;
use crate::register::Decision;
use crate::ship_art::{self, Data, Hull, array, optional_json, string};
use crate::text::{read_text, splitlines};

/// Every component of the plugin.
pub(super) static COMPONENTS: [&(dyn Component + 'static); 14] = [
    &Ship,
    &Ships,
    &Art,
    &Screen,
    &ShipMatrix,
    &ShapeMatrix,
    &SubfactionShips,
    &Relationships,
    &Icon("capability"),
    &Icon("weapon"),
    &Icon("gear"),
    &Icon("status"),
    &Icon("defence"),
    &HullGallery,
];

/// The page's inputs, each read at most once per render.
struct Inputs<'p, 'a> {
    page: &'p Page<'a>,
}

type Loaded<T> = Arc<Result<T, String>>;

impl Inputs<'_, '_> {
    fn root(&self) -> &Path {
        self.page.root
    }

    fn data(&self) -> Result<Arc<Data>, String> {
        let loaded: Loaded<Arc<Data>> = self.page.once("star-foundry.data", || {
            Data::load(self.root()).map(Arc::new)
        });
        loaded.as_ref().clone()
    }

    fn json(&self, key: &'static str, rel: &'static str) -> Result<Arc<Value>, String> {
        let root = self.root();
        let loaded: Loaded<Arc<Value>> = self
            .page
            .once(key, || optional_json(root, rel).map(Arc::new));
        loaded.as_ref().clone()
    }

    /// The art cards of the register of `rel`, a project-relative page.
    fn cards(&self, rel: &str) -> Result<Vec<Decision>, String> {
        let registers = self.page.once("star-foundry.cards", || {
            Mutex::new(BTreeMap::<String, Result<Vec<Decision>, String>>::new())
        });
        let mut registers = registers.lock().unwrap_or_else(PoisonError::into_inner);
        registers
            .entry(rel.into())
            .or_insert_with(|| {
                let path = self.root().join(rel);
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

    /// A hull as a figure of `size`: its approved or waiting picture, its name and facts, linked
    /// to its section.
    fn ship(&self, hull: &Hull, size: FigureSize) -> Result<Block, String> {
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
        let name = hull.label();
        let note = if state == "approved" || picture.is_none() {
            ""
        } else {
            " · pending review"
        };
        let caption = if size == FigureSize::Tile {
            vec![text(&hull.id)]
        } else {
            vec![text(&name), small(vec![text(&format!("{facts}{note}"))])]
        };
        Ok(Block::Figure {
            badge: picture.is_none().then(|| "no picture yet".to_string()),
            picture: picture.map(|(src, _)| Image {
                src,
                alt: name.clone(),
                presentation: Presentation {
                    title: format!("{name} · {facts} · {state}"),
                    attributes: [("loading".to_string(), "lazy".to_string())].into(),
                    ..Presentation::default()
                },
            }),
            caption,
            href: Some(hull.href()),
            size,
        })
    }

    /// A version of every input the components read under the root; content rather than mtimes,
    /// so a same-length edit with a preserved time is seen too. The document's own fingerprint
    /// covers the registers.
    fn version(&self) -> String {
        let root = self.root();
        let version: Arc<String> = self.page.once("star-foundry.version", || {
            let mut digest = Sha256::new();
            for rel in [
                art::ROSTER,
                ship_art::PLAN,
                ship_art::FACTIONS,
                ship_art::SHAPES,
                crate::screens::CATALOGUE,
                crate::relationships::SETTINGS,
                "artifacts/screens/manifest.json",
            ] {
                let bytes = std::fs::read(root.join(rel));
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
                            digest.update([u8::from(
                                root.join(atlas(string(reference, "atlas"))).is_file(),
                            )]);
                        }
                    }
                }
                if rel == "artifacts/screens/manifest.json"
                    && let Ok(data) = serde_json::from_slice::<Value>(&bytes)
                {
                    for (_, shot) in data["screens"].as_object().into_iter().flatten() {
                        digest.update([u8::from(
                            root.join(crate::screens::OUT)
                                .join(string(shot, "file"))
                                .is_file(),
                        )]);
                    }
                }
            }
            hex(&digest.finalize())
        });
        version.as_ref().clone()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn atlas(name: &str) -> String {
    format!("artifacts/references/shape-atlas/2026-09-21/ships/{name}.svg")
}

fn text(text: &str) -> Span {
    Span::Text { text: text.into() }
}

fn small(spans: Vec<Span>) -> Span {
    Span::Styled {
        tag: InlineKind::Small,
        presentation: Presentation::default(),
        spans,
    }
}

/// Figures in a line of their own, side by side.
fn strip(items: Vec<Span>) -> Vec<Block> {
    vec![Block::Paragraph {
        spans: vec![Span::Styled {
            tag: InlineKind::Span,
            presentation: Presentation::class("figure-strip"),
            spans: items,
        }],
    }]
}

/// A figure in a line of text.
fn inline(figure: Block) -> Span {
    Span::Blocks {
        blocks: vec![figure],
    }
}

fn note(words: String) -> Block {
    Block::Container {
        tag: ContainerKind::Div,
        presentation: Presentation::class("matrix-note"),
        blocks: vec![Block::Paragraph {
            spans: vec![text(&words)],
        }],
    }
}

fn size(option: Option<&str>) -> Result<FigureSize, String> {
    let size = option.unwrap_or("row").trim();
    let size = if size.is_empty() { "row" } else { size };
    match size {
        "small" => Ok(FigureSize::Small),
        "row" => Ok(FigureSize::Row),
        "large" => Ok(FigureSize::Large),
        _ => Err(format!("unknown size {size}; use small, row, large")),
    }
}

/// A component reading the project's ship data, whose version is that data's.
macro_rules! data_component {
    ($type:ident, $name:literal, |$inputs:ident, $argument:ident, $option:ident| $body:expr) => {
        struct $type;

        impl Component for $type {
            fn name(&self) -> &str {
                $name
            }

            fn render(
                &self,
                page: &Page<'_>,
                $argument: &str,
                $option: Option<&str>,
            ) -> Result<Vec<Block>, String> {
                let $inputs = Inputs { page };
                $body
            }

            fn cache_version(&self, page: &Page<'_>) -> Option<String> {
                Some(Inputs { page }.version())
            }
        }
    };
}

data_component!(Ship, "ship", |inputs, argument, option| {
    let figure = inputs.ship(inputs.data()?.hull(argument)?, size(option)?)?;
    Ok(vec![Block::Paragraph {
        spans: vec![inline(figure)],
    }])
});

data_component!(Ships, "ships", |inputs, argument, option| {
    let ids: Vec<_> = argument
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .collect();
    if ids.is_empty() {
        return Err("ships: name at least one hull".into());
    }
    let size = size(option)?;
    let data = inputs.data()?;
    let items = ids
        .into_iter()
        .map(
            |id| match data.hull(id).and_then(|h| inputs.ship(h, size)) {
                Ok(figure) => inline(figure),
                Err(problem) => error_span(&problem),
            },
        )
        .collect();
    Ok(strip(items))
});

data_component!(Art, "art", |inputs, argument, _option| {
    let (sub, kinds) = art::parse_slot(argument).map_err(|e| e.0)?;
    let data = inputs.data()?;
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
    let slot = art::slot(&inputs.cards(inputs.page.doc_path)?, &sub, &kinds).map_err(|e| e.0)?;
    Ok(vec![slot])
});

data_component!(Screen, "screen", |inputs, argument, _option| {
    let name = argument.trim();
    let catalogue = inputs.json("star-foundry.screens", crate::screens::CATALOGUE)?;
    let entry = array(&catalogue, "screens")
        .iter()
        .find(|e| string(e, "id") == name)
        .ok_or_else(|| {
            format!("screen: no screen is called {name} (docs/data/screens.json lists them)")
        })?;
    let title = string(entry, "title");
    let root = inputs.root();
    let manifest: Loaded<Arc<Value>> = inputs.page.once("star-foundry.manifest", || {
        crate::screens::read_manifest(&root.join(crate::screens::OUT))
            .map(|manifest| Arc::new(manifest.map_or(Value::Null, Value::Object)))
    });
    let manifest = manifest.as_ref().clone()?;
    let shot = &manifest["screens"][name];
    if shot.is_null()
        || !root
            .join(crate::screens::OUT)
            .join(string(shot, "file"))
            .is_file()
    {
        return Ok(vec![Block::Figure {
            picture: None,
            caption: vec![
                text(&format!("{title}: not captured yet ")),
                small(vec![
                    text("("),
                    Span::Code {
                        text: "just screens".into(),
                    },
                    text(" shoots it)"),
                ]),
            ],
            href: None,
            badge: None,
            size: FigureSize::Wide,
        }]);
    }
    let field = |key: &str| {
        shot.get(key)
            .unwrap_or(&manifest[key])
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    Ok(vec![Block::Figure {
        picture: Some(Image {
            src: format!(
                "/{}/{}?h={}",
                crate::screens::OUT,
                string(shot, "file"),
                prefix(string(shot, "hash"), 12)
            ),
            alt: title.into(),
            presentation: Presentation {
                attributes: [("loading".to_string(), "lazy".to_string())].into(),
                ..Presentation::default()
            },
        }),
        caption: vec![text(&format!(
            "{title} · {} · {}",
            prefix(&field("commit"), 12),
            field("date")
        ))],
        href: None,
        badge: None,
        size: FigureSize::Wide,
    }])
});

data_component!(ShipMatrix, "ship-matrix", |inputs, argument, _option| {
    let data = inputs.data()?;
    let known = data.known_factions();
    let wanted = argument.trim().to_lowercase();
    let wanted = if wanted.is_empty() { "all" } else { &wanted };
    if wanted != "all" && !known.iter().any(|f| f == wanted) {
        return Err(format!(
            "ship-matrix: no faction is called {}; use all or one of {}",
            argument.trim(),
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
                inputs.cards(&format!("{}/{f}.md", art::SHIP_PAGES))?,
            ))
        })
        .collect::<Result<_, String>>()?;
    ship_art::matrix(&data, &cards, &factions)
});

data_component!(
    SubfactionShips,
    "subfaction-ships",
    |inputs, argument, option| {
        let data = inputs.data()?;
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
            .find(|n| n.to_lowercase() == argument.trim().to_lowercase())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| {
                format!(
                    "subfaction-ships: no subfaction is called {}",
                    argument.trim()
                )
            })?;
        let size = size(option.filter(|s| !s.is_empty()).or(Some("small")))?;
        let classes = data.classes();
        let mut flown: Vec<_> = data
            .hulls
            .iter()
            .filter(|h| string(&data.plan[&h.id], "subfaction") == name)
            .collect();
        if flown.is_empty() {
            return Ok(vec![Block::Container {
                tag: ContainerKind::Div,
                presentation: Presentation::class("muted"),
                blocks: vec![Block::Paragraph {
                    spans: vec![text(&format!("{name} flies no hulls in the roster yet."))],
                }],
            }]);
        }
        flown.sort_by_key(|h| {
            classes
                .iter()
                .position(|c| *c == h.class)
                .unwrap_or(classes.len())
        });
        let items = flown
            .into_iter()
            .map(|h| inputs.ship(h, size).map(inline))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(strip(items))
    }
);

data_component!(ShapeMatrix, "shape-matrix", |inputs, argument, option| {
    if !inputs.root().join(ship_art::SHAPES).is_file() {
        return Err(format!("shape-matrix: {} is missing", ship_art::SHAPES));
    }
    let shapes = inputs.json("star-foundry.shapes", ship_art::SHAPES)?;
    let families = array(&shapes, "families");
    let data = inputs.data()?;
    let known = data.known_factions();
    let wanted = argument.trim().to_lowercase();
    let wanted = if wanted.is_empty() { "all" } else { &wanted };
    if !["all", "free"].contains(&wanted) && !known.iter().any(|f| f == wanted) {
        return Err(format!(
            "shape-matrix: no faction is called {}; use all, free or one of {}",
            argument.trim(),
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
    let donors = ship_art::donors(inputs.root())?;
    let cell = |header: bool, spans: Vec<Span>| Cell {
        header,
        spans,
        presentation: Presentation::default(),
    };
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
        let mut row = vec![Cell {
            header: true,
            spans: vec![
                Span::Strong {
                    spans: vec![text(string(family, "name"))],
                },
                Span::Styled {
                    tag: InlineKind::Small,
                    presentation: Presentation::class("family-reads"),
                    spans: vec![text(&format!("{} · {count}", string(family, "reads_as")))],
                },
            ],
            presentation: Presentation {
                attributes: [("scope".to_string(), "row".to_string())].into(),
                ..Presentation::default()
            },
        }];
        if wanted != "free" {
            for (column, _) in &columns {
                let mut items = Vec::new();
                for hull in &members {
                    if (if pivot == "class" {
                        &hull.class
                    } else {
                        &hull.faction
                    }) != column
                    {
                        continue;
                    }
                    let mut line = vec![inline(inputs.ship(hull, FigureSize::Tile)?)];
                    if let Some(donor) = donors.get(&hull.id).filter(|_| {
                        string(&data.plan[&hull.id], "outline")
                            .to_lowercase()
                            .contains("taken-over")
                    }) {
                        line.push(text(" "));
                        line.push(Span::Styled {
                            tag: InlineKind::Small,
                            presentation: Presentation {
                                title: "drawn on its donor's outline".into(),
                                ..Presentation::default()
                            },
                            spans: vec![text(&format!("← {donor}"))],
                        });
                    }
                    items.push(Span::Blocks {
                        blocks: vec![Block::Paragraph { spans: line }],
                    });
                }
                row.push(cell(false, items));
            }
        }
        let mut references = Vec::new();
        for (index, reference) in array(family, "references").iter().enumerate() {
            if index > 0 {
                references.push(Span::LineBreak);
            }
            references.push(inputs.reference(reference));
        }
        if references.is_empty() {
            references.push(small(vec![text("original")]));
        }
        row.push(cell(false, references));
        rows.push(row);
    }
    let mut head = vec![cell(true, vec![text("Family")])];
    if wanted != "free" {
        head.extend(columns.iter().map(|(_, t)| cell(true, vec![text(t)])));
    }
    head.push(cell(true, vec![text("References")]));
    let mut blocks = vec![Block::Table {
        head,
        rows,
        presentation: Presentation::class("matrix shape-matrix"),
    }];
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
        blocks.push(note(format!("Not classified: {}", loose.join(", "))));
    }
    Ok(blocks)
});

impl Inputs<'_, '_> {
    /// A shape family's reference ship: its atlas silhouette beside its name when the atlas has one.
    fn reference(&self, reference: &Value) -> Span {
        let name = format!(
            "{}: {}",
            string(reference, "game"),
            string(reference, "ship")
        );
        let rel = atlas(string(reference, "atlas"));
        let mut spans = Vec::new();
        if !string(reference, "atlas").is_empty() && self.root().join(&rel).is_file() {
            spans.push(Span::Image {
                image: Image {
                    src: format!("/{rel}"),
                    alt: name.clone(),
                    presentation: Presentation {
                        attributes: [
                            ("loading".to_string(), "lazy".to_string()),
                            (
                                "style".to_string(),
                                "height:28px;vertical-align:middle;background:#fff;border-radius:3px"
                                    .to_string(),
                            ),
                        ]
                        .into(),
                        ..Presentation::default()
                    },
                },
            });
            spans.push(text(&format!(" {name}")));
        } else {
            spans.push(text(&name));
        }
        Span::Styled {
            tag: InlineKind::Span,
            presentation: Presentation::class("shape-ref"),
            spans,
        }
    }
}

data_component!(
    Relationships,
    "relationships",
    |inputs, argument, _option| {
        let table = argument.trim();
        if !crate::relationships::TABLES.contains(&table) {
            return Err(format!(
                "relationships: no table called {table}; use destroy or help"
            ));
        }
        let root = inputs.root();
        let tables: Loaded<Arc<crate::relationships::Tables>> =
            inputs.page.once("star-foundry.relationships", || {
                crate::relationships::tables(root)
                    .map(Arc::new)
                    .map_err(|e| format!("relationships: {e}"))
            });
        let tables = tables.as_ref().clone()?;
        Ok(vec![crate::relationships::table(&tables, table)?])
    }
);

/// One of the icon families: the catalogue is built in, so its version never moves.
struct Icon(&'static str);

impl Component for Icon {
    fn name(&self) -> &str {
        self.0
    }

    fn render(
        &self,
        _page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        super::icons::render(self.0, argument, option)
    }

    fn cache_version(&self, _page: &Page<'_>) -> Option<String> {
        Some(String::new())
    }
}

/// `[[hull-gallery:all]]`: every hull model in the checkout, one open at a time.
struct HullGallery;

impl Component for HullGallery {
    fn name(&self) -> &str {
        "hull-gallery"
    }

    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        _option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        if argument.trim() != "all" {
            return Err(format!(
                "hull-gallery: no gallery is called {}; use all",
                argument.trim()
            ));
        }
        let items = super::models::gallery(page.root);
        Ok(vec![
            Block::Container {
                tag: ContainerKind::Div,
                presentation: Presentation::class("muted"),
                blocks: vec![Block::Paragraph {
                    spans: vec![text(&format!("{} hull models.", items.len()))],
                }],
            },
            Block::Media {
                media: Media::Gallery { items },
            },
        ])
    }

    fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        let items = super::models::gallery(page.root);
        let bytes = serde_json::to_vec(&items).unwrap_or_default();
        Some(hex(&Sha256::digest(&bytes)))
    }
}

fn prefix(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn error_span(problem: &str) -> Span {
    match crate::plugins::error(problem) {
        Block::Paragraph { mut spans } if spans.len() == 1 => spans.remove(0),
        _ => text(problem),
    }
}
