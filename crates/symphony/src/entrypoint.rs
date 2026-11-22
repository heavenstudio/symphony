//! A project's entrypoints: each serves one folder at one URL, shows the files its patterns include
//! that one of its renderers handles, and passes over its history folders when it reads documents.
//! A document is named by its route: the entrypoint's URL and the document's path in the folder,
//! with no leading slash (`docs/game/SPEC.md`; `index.md` at an entrypoint on `/`). With none
//! written, one entrypoint keeps the convention: `/docs/` over `docs/`, Markdown and HTML,
//! `docs/history` passed over.

use std::path::Path;

/// The convention's URL, folder and history folder: a project that writes no entrypoint serves
/// `docs/` at `/docs/` and passes over `docs/history`.
pub const DOCS: &str = "docs";
pub const HISTORY: &str = "history";

/// A renderer: the file types it shows, by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Renderer {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

/// Markdown documents, drawn by the document view.
pub const MARKDOWN: Renderer = Renderer {
    name: "markdown",
    extensions: &["md"],
};

/// HTML pages, each a document of its own: listed beside the Markdown ones and drawn whole in the
/// app, in a sandboxed frame, with none of the Markdown view's features (docs/operations/PROJECT_FILE.md).
pub const HTML: Renderer = Renderer {
    name: "html",
    extensions: &["html", "htm"],
};

/// What the Symphony draws where a page shows it: pictures and `.html` mockups. Their files are
/// served as they are and listed in the folder pages by name, as the files a plugin's renderer
/// draws are; a mockup opened from there is drawn in the app's frame as an [`HTML`] page is.
pub const MEDIA: Renderer = Renderer {
    name: "media",
    extensions: &[
        "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "html", "htm",
    ],
};

/// Every renderer the Symphony has, by the name an entrypoint's `types` gives it. A project's
/// plugins add theirs ([`crate::renderers`]).
pub const RENDERERS: [Renderer; 3] = [MARKDOWN, HTML, MEDIA];

/// The renderers of an entrypoint that names no `types`, the convention's among them.
pub const DEFAULT_TYPES: [Renderer; 2] = [MARKDOWN, HTML];

pub use symphony_api::{SYMPHONY_ROUTES, is_html};

/// One entrypoint, as checked at load: every path spelled as the repository names its files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrypoint {
    /// The route it is served at, without slashes: `docs`, or empty at the project's root.
    pub url: String,
    /// The folder it reads, project-relative; empty is the project's root.
    pub folder: String,
    /// The files shown, as patterns over their path in the folder; see [`matches`].
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// The renderers that show its files, in the order its `types` names them: the Symphony's and
    /// its project's plugins', resolved when the project loads.
    pub types: Vec<Renderer>,
    /// Folders inside it, relative to it, served and listed but never read as documents.
    pub history: Vec<String>,
}

impl Entrypoint {
    /// The convention: `/docs/` over `docs/`, Markdown and HTML, `history/` passed over.
    pub fn convention() -> Self {
        Self {
            url: DOCS.into(),
            folder: DOCS.into(),
            include: vec!["**".into()],
            exclude: Vec::new(),
            types: DEFAULT_TYPES.to_vec(),
            history: vec![HISTORY.into()],
        }
    }

    /// `route`'s path inside this entrypoint (empty for its own route); `None` outside its URL.
    pub fn inside_url<'a>(&self, route: &'a str) -> Option<&'a str> {
        below(&self.url, route)
    }

    /// The project-relative `file`'s path inside the folder (empty for the folder); `None` outside.
    pub fn inside_folder<'a>(&self, file: &'a str) -> Option<&'a str> {
        below(&self.folder, file)
    }

    /// The route of the path `inside` the folder.
    pub fn route(&self, inside: &str) -> String {
        join(&self.url, inside)
    }

    /// The project-relative file of the path `inside` the folder.
    pub fn file(&self, inside: &str) -> String {
        join(&self.folder, inside)
    }

    /// Whether the patterns include the path `inside` the folder, whatever its type. A name
    /// starting with a dot, an empty one and anything [`Entrypoint::excluded`] are never included.
    pub fn includes(&self, inside: &str) -> bool {
        !inside
            .split('/')
            .any(|name| name.is_empty() || name.starts_with('.'))
            && self.include.iter().any(|pattern| matches(pattern, inside))
            && !self.excluded(inside)
    }

    /// Whether an exclude pattern matches the path `inside` the folder or a folder above it: an
    /// excluded folder takes everything in it.
    pub fn excluded(&self, inside: &str) -> bool {
        let mut path = inside;
        loop {
            if self.exclude.iter().any(|pattern| matches(pattern, path)) {
                return true;
            }
            match path.rsplit_once('/') {
                Some((parent, _)) => path = parent,
                None => return false,
            }
        }
    }

    /// The renderer that shows the file `inside` the folder, if the patterns include it and one of
    /// the entrypoint's renderers handles its type.
    pub fn renderer(&self, inside: &str) -> Option<Renderer> {
        let extension = Path::new(inside)
            .extension()?
            .to_str()?
            .to_ascii_lowercase();
        self.renderers()
            .find(|renderer| renderer.extensions.contains(&extension.as_str()))
            .filter(|_| self.includes(inside))
    }

    /// Whether the file `inside` the folder is shown: listed and served.
    pub fn shows(&self, inside: &str) -> bool {
        self.renderer(inside).is_some()
    }

    /// Whether the file `inside` the folder is a document page: Markdown it shows.
    pub fn page(&self, inside: &str) -> bool {
        self.renderer(inside) == Some(MARKDOWN)
    }

    /// Whether the file `inside` the folder is an HTML page it shows, as a document (its renderer
    /// [`HTML`]) or among its media: either way the app draws it in its sandboxed frame.
    pub fn html_page(&self, inside: &str) -> bool {
        is_html(inside) && self.shows(inside)
    }

    /// Whether the path `inside` the folder lies in one of its history folders.
    pub fn in_history(&self, inside: &str) -> bool {
        self.history
            .iter()
            .any(|folder| below(folder, inside).is_some())
    }

    /// Whether the file `inside` the folder is read as a document: a page outside its history.
    pub fn document(&self, inside: &str) -> bool {
        self.page(inside) && !self.in_history(inside)
    }

    /// The entrypoint's renderers, in the order it names them: the Symphony's and its plugins'.
    pub fn renderers(&self) -> impl Iterator<Item = Renderer> + '_ {
        self.types.iter().copied()
    }

    /// How a message names it: its URL as the browser shows it.
    pub fn label(&self) -> String {
        if self.url.is_empty() {
            "/".into()
        } else {
            format!("/{}/", self.url)
        }
    }
}

/// The path of `path` below `folder` (empty for `folder` itself, every path below the empty
/// folder); `None` when it lies elsewhere.
pub fn below<'a>(folder: &str, path: &'a str) -> Option<&'a str> {
    if folder.is_empty() {
        return Some(path);
    }
    let rest = path.strip_prefix(folder)?;
    if rest.is_empty() {
        return Some(rest);
    }
    rest.strip_prefix('/')
}

/// `a/b`, either side possibly empty.
pub fn join(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.into(),
        (_, true) => a.into(),
        _ => format!("{a}/{b}"),
    }
}

/// Whether `path` (slash-separated, relative) matches `pattern`: `**` as a whole segment is any
/// number of folders, none included; in a segment `*` is any run of characters and `?` any one,
/// neither crossing a `/`; anything else is itself. A pattern names paths from the folder's top:
/// `*.md` only the files there, `**/*.md` every one below it too.
pub fn matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segments(&pattern, &path)
}

fn segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| segments(rest, &path[skip..])),
        Some((first, rest)) => path
            .split_first()
            .is_some_and(|(name, below)| segment(first, name) && segments(rest, below)),
    }
}

/// One name against one segment of a pattern: the classic two-pointer match, going back to the
/// last `*` on a mismatch.
fn segment(pattern: &str, name: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((at, from)) = star {
            pi = at + 1;
            ti = from + 1;
            star = Some((at, from + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_matches_names_within_a_segment_and_double_stars_across_folders() {
        for (pattern, path) in [
            ("**", "a.md"),
            ("**", "a/b/c.md"),
            ("*.md", "a.md"),
            ("**/*.md", "a.md"),
            ("**/*.md", "a/b/c.md"),
            ("guide/**", "guide/a/b.md"),
            ("guide/**", "guide"),
            ("a?c.md", "abc.md"),
            ("*", "anything"),
            ("a/**/z.md", "a/z.md"),
            ("a/**/z.md", "a/b/c/z.md"),
            ("/guide/*.md/", "guide/a.md"),
            ("*a*b*", "xaxxbx"),
        ] {
            assert!(matches(pattern, path), "{pattern} ~ {path}");
        }
        for (pattern, path) in [
            ("*.md", "a/b.md"),
            ("*.md", "a.mdx"),
            ("guide/**", "guides/a.md"),
            ("a?c.md", "ac.md"),
            ("a?c.md", "a/c.md"),
            ("a/**/z.md", "b/z.md"),
            ("*a*b*", "xbxa"),
            ("x", ""),
        ] {
            assert!(!matches(pattern, path), "{pattern} !~ {path}");
        }
    }

    #[test]
    fn below_and_join_treat_the_empty_folder_as_the_root() {
        assert_eq!(below("docs", "docs/a.md"), Some("a.md"));
        assert_eq!(below("docs", "docs"), Some(""));
        assert_eq!(below("docs", "docsx/a.md"), None);
        assert_eq!(below("docs", "view/docs/a.md"), None);
        assert_eq!(below("", "a/b.md"), Some("a/b.md"));
        assert_eq!(below("", ""), Some(""));
        assert_eq!(join("", "a.md"), "a.md");
        assert_eq!(join("docs", ""), "docs");
        assert_eq!(join("", ""), "");
        assert_eq!(join("docs", "a.md"), "docs/a.md");
    }

    #[test]
    fn an_entrypoint_shows_what_its_patterns_include_and_its_renderers_handle() {
        let entry = Entrypoint {
            url: String::new(),
            folder: "spec".into(),
            include: vec!["**".into()],
            exclude: vec!["drafts/**".into()],
            types: vec![MARKDOWN, MEDIA],
            history: vec!["old".into()],
        };
        assert_eq!(entry.renderer("index.md"), Some(MARKDOWN));
        assert_eq!(entry.renderer("media/a.PNG"), Some(MEDIA));
        assert_eq!(entry.renderer("drafts/x.md"), None, "excluded");
        let folder = Entrypoint {
            exclude: vec!["drafts".into(), "*/old".into()],
            ..entry.clone()
        };
        for excluded in [
            "drafts",
            "drafts/x.md",
            "drafts/deep/y.md",
            "guide/old/z.md",
        ] {
            assert!(
                folder.excluded(excluded) && !folder.includes(excluded),
                "{excluded}"
            );
        }
        for kept in ["draftsx/a.md", "guide/older/z.md", "index.md"] {
            assert!(!folder.excluded(kept) && folder.includes(kept), "{kept}");
        }
        assert_eq!(entry.renderer("main.rs"), None, "no renderer");
        assert_eq!(entry.renderer("Makefile"), None, "no type");
        assert!(entry.page("old/x.md") && !entry.document("old/x.md"));
        assert!(entry.document("guide/x.md"));
        assert!(!entry.page("media/a.png"));
        assert!(entry.includes("main.rs") && !entry.shows("main.rs"));
        assert_eq!(entry.label(), "/");
        assert_eq!(entry.route("index.md"), "index.md");
        assert_eq!(entry.file("index.md"), "spec/index.md");
        assert_eq!(entry.inside_url("guide/x.md"), Some("guide/x.md"));
        assert_eq!(entry.inside_folder("spec/guide"), Some("guide"));
        assert_eq!(entry.inside_folder("specs/a.md"), None);
        let convention = Entrypoint::convention();
        assert_eq!(convention.label(), "/docs/");
        assert!(convention.document("game/SPEC.md"));
        assert!(convention.page("history/OLD.md") && !convention.document("history/OLD.md"));
        assert!(!convention.shows("media/a.png"), "Markdown and HTML only");
        // An HTML page is the convention's document of its own, and a media entrypoint's mockup:
        // the app draws either in its frame; neither is a Markdown page.
        assert_eq!(convention.renderer("analysis/a.html"), Some(HTML));
        assert_eq!(convention.renderer("analysis/b.HTM"), Some(HTML));
        assert_eq!(entry.renderer("mock/a.html"), Some(MEDIA));
        for (entrypoint, inside) in [(&convention, "analysis/a.html"), (&entry, "mock/b.htm")] {
            assert!(entrypoint.html_page(inside), "{inside}");
            assert!(
                !entrypoint.page(inside) && !entrypoint.document(inside),
                "{inside}"
            );
        }
        for not_html in ["game/SPEC.md", "a.png", "a.html.md", "drafts/x.html"] {
            assert!(!folder.html_page(not_html), "{not_html}");
        }
        let markdown_only = Entrypoint {
            types: vec![MARKDOWN],
            ..convention.clone()
        };
        assert!(!markdown_only.html_page("analysis/a.html") && !markdown_only.shows("a.html"));
    }
}
