use super::*;

/// The items of the menu titled `title`.
fn items(title: &str) -> &'static [Standard] {
    MAIN.iter()
        .find(|(named, _)| *named == title)
        .map_or(&[], |(_, items)| *items)
}

#[test]
fn the_edit_menu_holds_every_key_a_pages_text_takes_and_the_app_menu_quits() {
    for needed in [
        Standard::Undo,
        Standard::Redo,
        Standard::Cut,
        Standard::Copy,
        Standard::Paste,
        Standard::SelectAll,
    ] {
        assert!(items("Edit").contains(&needed), "{needed:?}");
    }
    let app = MAIN[0];
    assert_eq!(app.0, "Symphony", "the app's own menu comes first");
    assert_eq!(app.1.last(), Some(&Standard::Quit));
    assert!(items("Window").contains(&Standard::CloseWindow));
}

#[test]
fn each_item_is_in_the_bar_once_and_no_menu_starts_or_ends_with_a_line() {
    let all: Vec<Standard> = MAIN
        .iter()
        .flat_map(|(_, items)| items.iter().copied())
        .filter(|item| *item != Standard::Separator)
        .collect();
    for item in &all {
        assert_eq!(
            all.iter().filter(|other| *other == item).count(),
            1,
            "{item:?}"
        );
    }
    for (title, items) in MAIN {
        assert!(!items.is_empty(), "{title}");
        assert_ne!(items.first(), Some(&Standard::Separator), "{title}");
        assert_ne!(items.last(), Some(&Standard::Separator), "{title}");
    }
}
