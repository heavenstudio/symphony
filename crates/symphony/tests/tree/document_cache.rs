//! Inputs outside a page must refresh its cached links, breadcrumbs and live components.

use std::path::PathBuf;
use std::sync::Arc;

use symphony::page::Document;
use symphony::render::{Components, Render};

use super::Tree;

fn html(doc: &Document) -> String {
    doc.sections
        .iter()
        .map(|section| section.html.as_str())
        .collect()
}

#[test]
fn adding_or_replacing_a_card_in_an_existing_file_refreshes_its_links() {
    for before in ["# Register\n", "# Register\n\n- [ ] **old-card** — x\n"] {
        let tree = Tree::new();
        tree.write("docs/REF.md", "# Reference\n\nSee **new-card**.\n");
        let register = tree.write("docs/DECISIONS.md", before);
        let modified = std::fs::metadata(&register).unwrap().modified().unwrap();
        let render = Render::new(&tree.root);
        let first = render.document("docs/REF.md").unwrap();
        assert!(!html(&first).contains("DECISIONS.md#new-card"));
        tree.write(
            "docs/DECISIONS.md",
            "# Register\n\n- [ ] **new-card** — x\n",
        );
        // The replacement is the same length and mtime: both document and card caches need content.
        std::fs::File::open(register)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let after = render.document("docs/REF.md").unwrap();
        assert!(html(&after).contains("href=\"/docs/DECISIONS.md#new-card\""));
        assert_eq!(after.changed_since(&first).len(), 1);
        tree.write("docs/DECISIONS.md", "# Register\n");
        assert!(!html(&render.document("docs/REF.md").unwrap()).contains("DECISIONS.md#new-card"));
    }
}

#[test]
fn moving_a_card_between_existing_documents_refreshes_its_destination() {
    let tree = Tree::new();
    tree.write("docs/REF.md", "See **moving-card**.\n");
    tree.write("docs/A.md", "- [ ] **moving-card** — x\n");
    tree.write("docs/B.md", "# B\n");
    let render = Render::new(&tree.root);
    assert!(html(&render.document("docs/REF.md").unwrap()).contains("/docs/A.md#moving-card"));
    tree.write("docs/A.md", "# A\n");
    tree.write("docs/B.md", "- [ ] **moving-card** — x\n");
    let after = html(&render.document("docs/REF.md").unwrap());
    assert!(after.contains("/docs/B.md#moving-card"));
    assert!(!after.contains("/docs/A.md#moving-card"));
}

#[test]
fn moving_a_wikilink_target_refreshes_its_destination() {
    let tree = Tree::new();
    tree.write("docs/REF.md", "See [[TARGET]].\n");
    let old = tree.write("docs/old/TARGET.md", "# Target\n");
    let render = Render::new(&tree.root);
    assert!(html(&render.document("docs/REF.md").unwrap()).contains("/docs/old/TARGET.md"));
    std::fs::create_dir_all(tree.path("docs/new")).unwrap();
    std::fs::rename(old, tree.path("docs/new/TARGET.md")).unwrap();
    assert!(html(&render.document("docs/REF.md").unwrap()).contains("/docs/new/TARGET.md"));
}

#[test]
fn editing_a_readme_refreshes_child_breadcrumbs() {
    let tree = Tree::new();
    tree.write("docs/topic/README.md", "# Old title\n");
    tree.write("docs/topic/A.md", "# A\n");
    let render = Render::new(&tree.root);
    let first = render.document("docs/topic/A.md").unwrap();
    assert_eq!(first.crumbs[1].name, "Old title");
    tree.write("docs/topic/README.md", "# New title\n");
    let after = render.document("docs/topic/A.md").unwrap();
    assert_eq!(after.crumbs[1].name, "New title");
    assert!(
        after.changed_since(&first).is_empty(),
        "only the breadcrumb changed"
    );
}

struct Live {
    path: PathBuf,
}

impl Components for Live {
    fn render_one(&self, _: &str, _: &str, _: Option<&str>) -> Result<String, String> {
        std::fs::read_to_string(&self.path).map_err(|error| error.to_string())
    }
}

struct Versioned(Live);

impl Components for Versioned {
    fn render_one(&self, name: &str, arg: &str, option: Option<&str>) -> Result<String, String> {
        self.0.render_one(name, arg, option)
    }

    fn cache_version(&self) -> Option<String> {
        Some(std::fs::read_to_string(&self.0.path).unwrap())
    }
}

fn component_update(path: &str, before: &str, after: &str) {
    for versioned in [false, true] {
        let tree = Tree::new();
        tree.write("docs/A.md", "# A\n\n[[ship:REP-F01]]\n");
        let path = tree.write(path, before);
        let live = Live { path: path.clone() };
        let components: Box<dyn Components> = if versioned {
            Box::new(Versioned(live))
        } else {
            Box::new(live)
        };
        let render = Render::new(&tree.root).with_components(components);
        let first = render.document("docs/A.md").unwrap();
        assert!(html(&first).contains(before));
        let unchanged = render.document("docs/A.md").unwrap();
        std::fs::write(path, after).unwrap();
        let next = render.document("docs/A.md").unwrap();
        assert!(html(&next).contains(after));
        assert!(!html(&next).contains(before));
        assert_eq!(next.changed_since(&first).len(), 1);
        assert_eq!(Arc::ptr_eq(&first, &unchanged), versioned);
    }
}

#[test]
fn an_approved_component_picture_refreshes_without_editing_the_document() {
    component_update(
        "artifacts/approved-picture",
        "<img src=\"/old.png\">",
        "<img src=\"/new.png\">",
    );
}

#[test]
fn component_data_refreshes_without_editing_the_document() {
    component_update("docs/data/stats", "<span>1</span>", "<span>2</span>");
}

#[test]
fn replacing_the_component_provider_clears_its_previous_cached_documents() {
    struct Replacement;
    impl Components for Replacement {
        fn render_one(&self, _: &str, _: &str, _: Option<&str>) -> Result<String, String> {
            Ok("<span>new provider</span>".into())
        }

        fn cache_version(&self) -> Option<String> {
            Some(String::new()) // deliberately the same version as Builtin
        }
    }
    let tree = Tree::new();
    tree.write("docs/A.md", "[[ship:REP-F01]]\n");
    // Select Builtin explicitly: the default live provider has its own data fingerprint.
    let render = Render::new(&tree.root).with_components(Box::new(symphony::render::Builtin));
    let first = render.document("docs/A.md").unwrap();
    assert!(html(&first).contains("⚠ unknown component ship"));
    assert!(Arc::ptr_eq(&first, &render.document("docs/A.md").unwrap()));
    let render = render.with_components(Box::new(Replacement));
    let next = render.document("docs/A.md").unwrap();
    assert!(!Arc::ptr_eq(&first, &next));
    assert_eq!(html(&next).trim(), "<span>new provider</span>");
    assert_eq!(next.changed_since(&first).len(), 1);
    assert!(Arc::ptr_eq(&next, &render.document("docs/A.md").unwrap()));
}
