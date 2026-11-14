//! The seams a plugin's process answers besides its components and card kinds: its renderers,
//! routes, home regions and doc checks, each the object its declaration names, forwarding each
//! call. What the declaration says of an object (a renderer's extensions and content type, a
//! check's inputs) is answered from it, with no call. A call that gets no answer is shown where its
//! answer would go, as the seam has room for it: a renderer's drawing as its error in the file's
//! place, a route's answer as not found and a home region as none (each failure in the plugin's
//! log), a doc check's as a problem naming it, and its components as none.

use std::path::Path;

use symphony_plugin::protocol::{
    Ask, CheckDocs, DocComponents, Draw, FileData, PageData, ProblemData, Serve, ShowRegion,
};
use symphony_plugin::vocabulary::Region;
use symphony_plugin::{
    DocCheck, Docs, Drawing, File, HomeRegion, Page, Problem, Renderer, Reply, Route,
};

use super::{Failure, Remote};

/// A renderer the remote declared.
pub(super) struct RemoteRenderer {
    pub(super) remote: &'static Remote,
    pub(super) name: String,
    pub(super) extensions: Vec<&'static str>,
    pub(super) content_type: String,
}

impl Renderer for RemoteRenderer {
    fn name(&self) -> &str {
        &self.name
    }
    fn extensions(&self) -> &[&str] {
        &self.extensions
    }
    fn content_type(&self) -> &str {
        &self.content_type
    }
    fn draw(&self, page: &Page<'_>, file: &File<'_>) -> Result<Drawing, String> {
        let asked = Draw {
            renderer: self.name.clone(),
            page: PageData::of(page),
            file: FileData::of(file),
        };
        match self.remote.ask(&self.name, asked) {
            Ok(drawn) => drawn,
            // Required of every renderer, so in every minor.
            Err(Failure::Newer(agreed)) => Err(self.remote.failed(&format!(
                "it speaks protocol {agreed}, which has no {}",
                Draw::TAG
            ))),
            Err(Failure::Down(why)) => Err(self.remote.failed(&why)),
        }
    }
}

/// A route the remote declared, named by its path.
pub(super) struct RemoteRoute {
    pub(super) remote: &'static Remote,
    pub(super) path: String,
}

impl Route for RemoteRoute {
    fn path(&self) -> &str {
        &self.path
    }
    fn serve(&self, root: &Path, rest: &str) -> Option<Reply> {
        let asked = Serve {
            route: self.path.clone(),
            root: root.to_path_buf(),
            rest: rest.into(),
        };
        // No answer is not found: why is in the plugin's log.
        self.remote.ask(&self.path, asked).ok().flatten()
    }
}

/// A home region the remote declared, named by its id.
pub(super) struct RemoteRegion {
    pub(super) remote: &'static Remote,
    pub(super) id: String,
}

impl HomeRegion for RemoteRegion {
    fn id(&self) -> &str {
        &self.id
    }
    fn region(&self, root: &Path) -> Option<Region> {
        let asked = ShowRegion {
            region: self.id.clone(),
            root: root.to_path_buf(),
        };
        // No answer shows nothing: why is in the plugin's log.
        self.remote.ask(&self.id, asked).ok().flatten()
    }
}

/// A doc check the remote declared: the plugin reads the documents of the checkout the call names
/// itself, once for each call.
pub(super) struct RemoteCheck {
    pub(super) remote: &'static Remote,
    pub(super) name: String,
    pub(super) inputs: Vec<String>,
}

impl DocCheck for RemoteCheck {
    fn name(&self) -> &str {
        &self.name
    }
    fn components(&self, docs: &Docs) -> Vec<String> {
        let asked = DocComponents {
            check: self.name.clone(),
            root: docs.root().to_path_buf(),
        };
        // None named: a component only it names reads unknown, so the gap is seen.
        self.remote.ask(&self.name, asked).unwrap_or_default()
    }
    fn inputs(&self) -> Vec<String> {
        self.inputs.clone()
    }
    fn check(&self, docs: &Docs) -> Vec<Problem> {
        let asked = CheckDocs {
            check: self.name.clone(),
            root: docs.root().to_path_buf(),
        };
        match self.remote.ask(&self.name, asked) {
            Ok(problems) => problems.into_iter().map(ProblemData::problem).collect(),
            Err(Failure::Newer(_)) => Vec::new(),
            // A check that did not run is a problem, never a pass.
            Err(Failure::Down(why)) => vec![Problem::new(
                crate::project::FILE,
                0,
                self.remote
                    .failed(&format!("its doc check {} did not run: {why}", self.name)),
            )],
        }
    }
}
