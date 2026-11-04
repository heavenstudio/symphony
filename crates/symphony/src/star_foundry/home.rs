//! The ship art the roster's hulls show: the home's progress region, and the pictures of the hulls
//! a card names (`[[ship:REP-F01]]`). Read once per checkout until the roster or a ships page
//! changes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use regex::Regex;
use serde::Deserialize;
use symphony_plugin::vocabulary::{Picture, ProgressBar, Region};
use symphony_plugin::{Card, CardKind, HomeRegion};

use crate::art;
use crate::text::{read_text, splitlines};

static HULLS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[ships?:([^\]|]+)").unwrap());

#[derive(Default, Deserialize)]
struct Roster {
    #[serde(default)]
    hulls: Vec<Hull>,
    #[serde(default)]
    factions: Vec<Faction>,
}

#[derive(Deserialize)]
struct Hull {
    id: String,
    name: String,
    faction: String,
}

#[derive(Deserialize)]
struct Faction {
    id: String,
    name: String,
}

/// Each hull's chosen picture and whether it waits on the owner's review.
struct Shown {
    roster: Roster,
    shown: BTreeMap<String, Option<(String, bool)>>,
}

type Stamps = Vec<Option<(u64, SystemTime)>>;

/// Each root's roster and pictures, beside the stamps of the files they were read from.
type Cache = HashMap<PathBuf, (Stamps, Arc<Result<Shown, String>>)>;

static SHOWN: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn stamps(root: &Path, files: &[String]) -> Stamps {
    files
        .iter()
        .map(|rel| {
            std::fs::metadata(root.join(rel))
                .ok()
                .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        })
        .collect()
}

impl Shown {
    /// The checkout at `root`'s, from the cache while its inputs are unchanged.
    fn of(root: &Path) -> Arc<Result<Shown, String>> {
        let mut files = vec![art::ROSTER.to_string()];
        files.extend(
            art::ship_pages(root)
                .iter()
                .filter_map(|page| page.strip_prefix(root).ok())
                .map(|rel| rel.to_string_lossy().into_owned()),
        );
        let now = stamps(root, &files);
        let mut cache = SHOWN.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((held, shown)) = cache.get(root)
            && *held == now
        {
            return shown.clone();
        }
        let shown = Arc::new(Self::load(root));
        cache.insert(root.to_path_buf(), (now, shown.clone()));
        shown
    }

    fn load(root: &Path) -> Result<Shown, String> {
        let path = root.join(art::ROSTER);
        let roster: Roster = match std::fs::read_to_string(&path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Roster::default(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut registers = BTreeMap::new();
        let mut shown = BTreeMap::new();
        for hull in &roster.hulls {
            let rel = format!("{}/{}.md", art::SHIP_PAGES, hull.faction);
            let cards = registers.entry(rel.clone()).or_insert_with(|| {
                read_text(&root.join(&rel))
                    .map(|text| crate::register::parse_decisions(&splitlines(&text)))
                    .unwrap_or_default()
            });
            let label = format!("{} {}", hull.id, hull.name);
            let chosen = art::choose(cards, &label, "ship").map_err(|e| e.to_string())?;
            shown.insert(hull.id.clone(), chosen);
        }
        Ok(Shown { roster, shown })
    }

    fn region(&self) -> Option<Region> {
        let mut order: Vec<String> = self.roster.factions.iter().map(|f| f.id.clone()).collect();
        for hull in &self.roster.hulls {
            if !order.contains(&hull.faction) {
                order.push(hull.faction.clone());
            }
        }
        let bars: Vec<ProgressBar> = order
            .into_iter()
            .filter_map(|faction| {
                let hulls: Vec<_> = self
                    .roster
                    .hulls
                    .iter()
                    .filter(|h| h.faction == faction)
                    .collect();
                if hulls.is_empty() {
                    return None;
                }
                let state = |waiting: bool| {
                    count(
                        hulls
                            .iter()
                            .filter(|h| self.shown[&h.id].as_ref().is_some_and(|s| s.1 == waiting))
                            .count(),
                    )
                };
                let (done, waiting, total) = (state(false), state(true), count(hulls.len()));
                let name = self
                    .roster
                    .factions
                    .iter()
                    .find(|f| f.id == faction)
                    .map_or_else(|| faction.clone(), |f| f.name.clone());
                Some(ProgressBar {
                    title: format!(
                        "{done} approved, {waiting} waiting on review, {} not drawn yet",
                        total.saturating_sub(done + waiting)
                    ),
                    page: format!("{}/{faction}.md", art::SHIP_PAGES),
                    key: faction,
                    name,
                    done,
                    waiting,
                    total,
                })
            })
            .collect();
        if bars.is_empty() {
            return None;
        }
        let done: u32 = bars.iter().map(|bar| bar.done).sum();
        let total: u32 = bars.iter().map(|bar| bar.total).sum();
        Some(Region {
            id: "ship-art".into(),
            title: "Ship art".into(),
            note: format!("{done} of {total} hulls approved"),
            bars,
            legend: [
                "approved".into(),
                "waiting on you".into(),
                "not drawn".into(),
            ],
        })
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The home's ship-art progress, a bar per faction.
pub(super) struct ShipArt;

impl HomeRegion for ShipArt {
    fn id(&self) -> &str {
        "ship-art"
    }

    fn region(&self, root: &Path) -> Option<Region> {
        match Shown::of(root).as_ref() {
            Ok(shown) => shown.region(),
            Err(problem) => {
                eprintln!("ship art: {problem}");
                None
            }
        }
    }
}

/// A card naming hulls (`[[ship:REP-F01]]`, `[[ships:A,B]]`) shows their pictures, six at most.
pub(super) struct HullMentions;

impl CardKind for HullMentions {
    fn name(&self) -> &str {
        "hull mention"
    }

    fn recognises(&self, card: &Card<'_>) -> bool {
        HULLS.is_match(card.text)
    }

    fn pictures(&self, root: &Path, card: &Card<'_>) -> Vec<Picture> {
        let shown = Shown::of(root);
        let Ok(shown) = shown.as_ref() else {
            return Vec::new();
        };
        let mut seen = BTreeSet::new();
        HULLS
            .captures_iter(card.text)
            .flat_map(|c| {
                c[1].split(',')
                    .map(|id| id.trim().to_uppercase())
                    .collect::<Vec<_>>()
            })
            .filter(|id| seen.insert(id.clone()))
            .filter_map(|id| {
                let hull = shown.roster.hulls.iter().find(|h| h.id == id)?;
                let chosen = shown.shown.get(&id).and_then(Option::as_ref);
                Some(Picture {
                    src: chosen.map(|s| s.0.clone()),
                    caption: format!("{} {}", hull.id, hull.name),
                    waiting: chosen.is_some_and(|s| s.1),
                })
            })
            .take(6)
            .collect()
    }

    /// The distinct hulls the card names, as written.
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        let named: BTreeSet<&str> = HULLS
            .captures_iter(card.text)
            .flat_map(|c| c.get(1).map_or("", |ids| ids.as_str()).split(','))
            .map(crate::text::strip)
            .filter(|id| !id.is_empty())
            .collect();
        Some((named.len(), "hull".into()))
    }
}
