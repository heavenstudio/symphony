//! The theme package: the defaults, each part a project file sets over a plugin's, the tokens and
//! layouts refused by name, and the ETag that moves with every part.

use super::*;
use crate::test_tree::Tree;

fn project(manifest: &str) -> (Tree, Project) {
    let tree = Tree::new();
    tree.write("docs/README.md", "# Docs\n");
    tree.write(crate::project::FILE, manifest);
    let project = Project::load(&tree.root).unwrap();
    (tree, project)
}

#[test]
fn a_project_without_a_package_has_the_symphonys_defaults() {
    let (_tree, project) = project("[project]\nname = \"Plain\"\n");
    let package = Package::of(&project);
    assert_eq!(
        package,
        Package {
            name: "Plain".into(),
            ..Package::default()
        }
    );
    assert_eq!(package.css(), "", "no tokens is no rule");
    assert!(problems(&project).is_empty());
}

#[test]
fn each_part_the_file_sets_is_taken_and_each_it_leaves_is_the_default() {
    let (_tree, full) = project(
        "[project]\nname = \"Full\"\n[theme]\nname = \"Shown\"\nicon = \"look/icon.svg\"\n\
         stylesheet = \"look/look.css\"\n[theme.tokens]\nprimary = \"#a33\"\nfont-sans = \"Georgia, serif\"\n\
         [theme.layouts]\nheader = \"compact\"\nhome = \"default\"\n",
    );
    let package = Package::of(&full);
    assert_eq!(package.name, "Shown");
    assert_eq!(package.icon.as_deref(), Some("look/icon.svg"));
    assert_eq!(package.stylesheets, ["look/look.css"]);
    assert_eq!(
        package.css(),
        ":root{--font-sans:Georgia, serif;--primary:#a33;}\n"
    );
    assert_eq!(
        package.layouts,
        BTreeMap::from([("header".to_string(), "compact".to_string())]),
        "a region on its default variant needs no attribute"
    );
    for (manifest, check) in [
        ("[theme]\nicon = \"i.svg\"\n", "icon"),
        ("[theme]\nstylesheet = \"s.css\"\n", "stylesheet"),
        ("[theme.tokens]\nprimary = \"red\"\n", "tokens"),
        ("[theme.layouts]\nhome = \"focus\"\n", "layouts"),
        ("[theme]\nname = \"N\"\n", "name"),
    ] {
        let (_tree, project) = project(&format!("[project]\nname = \"P\"\n{manifest}"));
        let package = Package::of(&project);
        assert_eq!(package.name == "N", check == "name", "{check}");
        assert_eq!(package.icon.is_some(), check == "icon", "{check}");
        assert_eq!(
            !package.stylesheets.is_empty(),
            check == "stylesheet",
            "{check}"
        );
        assert_eq!(!package.tokens.is_empty(), check == "tokens", "{check}");
        assert_eq!(!package.layouts.is_empty(), check == "layouts", "{check}");
    }
}

/// Which mark and name a project's header opens with: the Symphony's wordmark only where its
/// theme has neither a name nor an icon of its own, and the Symphony's mark wherever it has no
/// icon, as the project's view carries them.
#[test]
fn a_header_is_the_symphonys_until_the_theme_has_a_name_or_an_icon_of_its_own() {
    for (theme, wordmark, mark) in [
        ("", None, ""),
        ("[theme.tokens]\nprimary = \"red\"\n", None, ""),
        ("[theme]\nstylesheet = \"s.css\"\n", None, ""),
        ("[theme.layouts]\nheader = \"compact\"\n", None, ""),
        // A package no plugin provides names nothing.
        ("[theme]\npackage = \"nowhere\"\n", None, ""),
        // A name alone keeps the Symphony's mark beside it.
        ("[theme]\nname = \"Shown\"\n", Some("Shown"), ""),
        // A theme may name itself as its project is named: the name is still its own.
        ("[theme]\nname = \"Plain\"\n", Some("Plain"), ""),
        // An icon alone stands beside the project's name, never the Symphony's.
        ("[theme]\nicon = \"look/i.svg\"\n", Some("Plain"), "look/i.svg"),
        (
            "[theme]\nname = \"Shown\"\nicon = \"look/i.svg\"\n",
            Some("Shown"),
            "look/i.svg",
        ),
    ] {
        let (_tree, project) = project(&format!("[project]\nname = \"Plain\"\n{theme}"));
        let package = Package::of(&project);
        assert_eq!(package.wordmark(), wordmark, "{theme}");
        let view = project.view();
        assert_eq!(view.wordmark, wordmark.unwrap_or_default(), "{theme}");
        assert_eq!(view.mark, mark, "{theme}");
        // The tab's name is the package's either way.
        assert_eq!(view.name, wordmark.unwrap_or("Plain"), "{theme}");
    }
    // A plugin's package names its icon as it likes: the header's has no slash first, as the
    // client puts it under the project's base.
    let plugins = Package {
        name: "Every".into(),
        icon: Some("/look/i.svg".into()),
        ..Package::default()
    };
    assert_eq!(plugins.mark(), "look/i.svg");
    assert_eq!(plugins.wordmark(), Some("Every"));
    assert_eq!(Package::default().mark(), "");
}

#[test]
fn a_package_no_plugin_provides_is_the_defaults_and_named_by_the_lint() {
    let (_tree, project) = project("[theme]\npackage = \"nowhere\"\nicon = \"i.svg\"\n");
    let package = Package::of(&project);
    assert_eq!(package.package, None);
    assert_eq!(
        package.icon.as_deref(),
        Some("i.svg"),
        "the file's parts still apply"
    );
    let found = problems(&project);
    assert_eq!(found.len(), 1);
    assert!(
        found[0].contains("package nowhere is shipped by none"),
        "{found:?}"
    );
}

#[test]
fn a_token_the_client_cannot_read_is_refused_by_name() {
    for good in [
        ("primary", "#a33"),
        ("r_sm", "calc(var(--radius) - 2px)"),
        ("x1", "Georgia, serif"),
    ] {
        assert_eq!(token(good.0, good.1), Ok(()), "{good:?}");
    }
    for (name, value) in [
        ("", "red"),
        ("--primary", "red"),
        ("pri mary", "red"),
        ("primary", ""),
        ("primary", "  "),
        ("primary", "red;}body{display:none"),
        ("primary", "red</style>"),
        ("primary", "url(https://x/y.png)"),
        ("primary", "URL(x)"),
        ("primary", "red /* x */"),
        ("primary", "@import 'x'"),
        ("primary", "red\\3b"),
        ("primary", "red\nblue"),
    ] {
        let why = token(name, value).unwrap_err();
        assert!(why.contains(name) || name.is_empty(), "{why}");
    }
    let refused = Project::parse(
        std::path::Path::new("/tmp/p"),
        "[theme.tokens]\nprimary = \"url(x)\"\n",
    );
    assert!(refused.unwrap_err().contains("[theme.tokens] primary"));
}

#[test]
fn a_layout_is_a_variant_the_client_holds_for_a_region_it_draws() {
    assert_eq!(layout("header", "compact"), Ok(()));
    assert_eq!(layout("home", "focus"), Ok(()));
    let why = layout("header", "wide").unwrap_err();
    assert!(why.contains("default, compact"), "{why}");
    let why = layout("footer", "default").unwrap_err();
    assert!(why.contains("the regions are header, home"), "{why}");
    let root = std::path::Path::new("/tmp/p");
    let refused = Project::parse(root, "[theme.layouts]\nheader = \"wide\"\n").unwrap_err();
    assert!(refused.contains("[theme.layouts] header"), "{refused}");
    for (key, manifest) in [
        ("name", "[theme]\nname = \" \"\n"),
        ("package", "[theme]\npackage = \"\"\n"),
        ("icon", "[theme]\nicon = \"../out.svg\"\n"),
        ("stylesheet", "[theme]\nstylesheet = \"/abs.css\"\n"),
        ("colour", "[theme]\ncolour = \"red\"\n"),
    ] {
        let why = Project::parse(root, manifest).unwrap_err();
        assert!(why.contains(key), "{key}: {why}");
    }
}

#[test]
fn the_etag_moves_with_every_part_and_only_then() {
    let base = Package {
        name: "P".into(),
        ..Package::default()
    };
    let same = base.clone();
    assert_eq!(base.etag(), same.etag());
    assert!(base.etag().starts_with('"') && base.etag().ends_with('"'));
    let changed = [
        Package {
            name: "Q".into(),
            ..base.clone()
        },
        Package {
            icon: Some("i.svg".into()),
            ..base.clone()
        },
        Package {
            stylesheets: vec!["s.css".into()],
            ..base.clone()
        },
        Package {
            tokens: BTreeMap::from([("primary".into(), "red".into())]),
            ..base.clone()
        },
        Package {
            layouts: BTreeMap::from([("home".into(), "focus".into())]),
            ..base.clone()
        },
    ];
    for package in &changed {
        assert_ne!(package.etag(), base.etag(), "{package:?}");
    }
}
