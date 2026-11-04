//! Star Foundry's plugin: the game's components, card kinds, home region, hull models and their
//! route and watch, the screen catalogue's check, the faction pages' prose rule and the
//! approve-to-land action. It still sits in the Symphony's folder, behind the traits of
//! `symphony-plugin`, and reads the game's modules of this library.

mod cards;
mod components;
mod home;
mod icons;
pub mod models;

use std::path::{Component as Part, Path};

use symphony_plugin::vocabulary::{NavItem, TaskOffer};
use symphony_plugin::{
    BoardAction, CardKind, Component, DocCheck, HomeRegion, Plugin, ProseRule, Reply, Route,
    Submission, Submitted, TaskRef, Watch,
};

pub use cards::GOLDEN_FRAME;

/// The plugin's name, as `[plugins] crates` in `symphony.toml` lists it.
pub const NAME: &str = "star-foundry-symphony";

/// The hull gallery's document.
pub const HULLS_DOC: &str = "docs/ships/HULLS.md";

pub struct StarFoundry;

pub static PLUGIN: StarFoundry = StarFoundry;

impl Plugin for StarFoundry {
    fn name(&self) -> &str {
        NAME
    }

    fn components(&self) -> Vec<&dyn Component> {
        components::COMPONENTS.to_vec()
    }

    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        cards::KINDS.to_vec()
    }

    fn routes(&self) -> Vec<&dyn Route> {
        vec![&Hulls]
    }

    fn nav(&self) -> Vec<NavItem> {
        vec![NavItem {
            label: "⬡ hulls".into(),
            title: "Hull models".into(),
            href: "/hulls".into(),
            document: HULLS_DOC.into(),
        }]
    }

    fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        vec![&home::ShipArt]
    }

    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        vec![&ApproveToLand]
    }

    fn watches(&self) -> Vec<&dyn Watch> {
        vec![&Gallery]
    }

    fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        vec![&Screens]
    }

    fn prose_rules(&self) -> Vec<&dyn ProseRule> {
        vec![&FactionVerdicts]
    }
}

/// The route a hull's model is served by.
const HULLS_ROUTE: &str = "hulls";

/// `/x/hulls/<id>.glb`: a hull's model file, from the checkout's models folder.
struct Hulls;

impl Route for Hulls {
    fn path(&self) -> &str {
        HULLS_ROUTE
    }

    fn serve(&self, root: &Path, rest: &str) -> Option<Reply> {
        let plain = !rest.is_empty()
            && Path::new(rest)
                .components()
                .all(|part| matches!(part, Part::Normal(_)));
        (plain && rest.ends_with(".glb") && !rest.contains('/'))
            .then(|| Reply::File(root.join(models::HULLS).join(rest)))
    }
}

/// The gallery's inputs: a model, a concept picture or an outline moving drops the checkout's
/// cached gallery, which is built again at once.
struct Gallery;

impl Watch for Gallery {
    fn paths(&self) -> Vec<String> {
        vec![
            models::MODELS.into(),
            models::CONCEPTS.into(),
            models::OUTLINES.into(),
        ]
    }

    fn changed(&self, root: &Path) -> Option<String> {
        models::invalidate(root);
        let hulls = models::warm(root);
        Some(format!(
            "hull gallery warmed: {}: {hulls} models",
            root.display()
        ))
    }

    fn forget(&self, root: &Path) {
        models::invalidate(root);
    }
}

/// The screen catalogue against its schema and what the schema cannot say.
struct Screens;

impl DocCheck for Screens {
    fn name(&self) -> &str {
        "screens"
    }

    fn check(&self, root: &Path) -> Vec<String> {
        if !root.join(crate::screens::CATALOGUE).is_file() {
            return Vec::new();
        }
        match crate::screens::load_catalogue(root) {
            Ok(catalogue) => crate::screens::problems(&catalogue, root)
                .into_iter()
                .map(|problem| format!("{}: {problem}", crate::screens::CATALOGUE))
                .collect(),
            Err(problem) => vec![format!("{}: {problem}", crate::screens::CATALOGUE)],
        }
    }
}

/// A faction page (a lowercase page of `docs/factions`, not its README) shows the design as it is:
/// which picture or trait was approved, rejected or replaced is what its register records.
struct FactionVerdicts;

impl ProseRule for FactionVerdicts {
    fn name(&self) -> &str {
        "verdict words on a faction page"
    }

    fn check(&self, rel: &str, text: &str) -> Vec<(usize, String)> {
        let Some(name) = rel.strip_prefix(&format!("{}/", crate::art::PAGES)) else {
            return Vec::new();
        };
        if name.contains('/')
            || !name.ends_with(".md")
            || name != name.to_lowercase()
            || name == "readme.md"
        {
            return Vec::new();
        }
        crate::lint::verdict_words(text)
    }
}

/// The owner's approval of a task in review lands its branch: the offer's payload is the patch
/// `sf-land` prepares for it.
struct ApproveToLand;

impl BoardAction for ApproveToLand {
    fn name(&self) -> &str {
        "approve to land"
    }

    fn offer(&self, root: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        if task.status != "review" {
            return Ok(TaskOffer::default());
        }
        let payload = crate::commit::land_call(
            root,
            &serde_json::json!({"method": "owner_offer", "branch": task.branch}),
        )?
        .as_str()
        .filter(|patch| !patch.is_empty())
        .map(str::to_string);
        Ok(TaskOffer {
            action: self.name().into(),
            label: "Approve to land".into(),
            branch: task.branch.into(),
            payload,
        })
    }

    /// Records the owner's approval of the patch `sf-land` offered, then asks which tasks its
    /// stamps held blocked on it. A release that cannot be read leaves them for the next landing or
    /// stamp edit, which re-reads them.
    fn submit(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        let problem = crate::commit::land_call(
            root,
            &serde_json::json!({"method": "record_owner_approval", "branch": task.branch,
                "patch": submission.payload, "note": submission.note}),
        )?;
        if let Some(problem) = problem.as_str().filter(|problem| !problem.is_empty()) {
            return Err(problem.into());
        }
        let released = crate::commit::land_call(
            root,
            &serde_json::json!({"method": "stamps_released", "branch": task.branch}),
        );
        Ok(Submitted {
            said: format!("approved {} to land", task.branch),
            released: released
                .iter()
                .flat_map(|held| held.as_array().into_iter().flatten())
                .filter_map(|held| held.as_str().map(str::to_string))
                .collect(),
        })
    }
}
