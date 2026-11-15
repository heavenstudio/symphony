//! The app's menu bar, which the binary makes of the system's standard items (muda's predefined
//! ones, each with its key equivalent). macOS hands a key equivalent such as ⌘C to the menu bar
//! before the page: with no Edit menu a page takes no undo, cut, copy, paste or select-all from the
//! keyboard, and with no app menu ⌘Q quits nothing. It is data here, as a menu is made only on the
//! main thread of a running application, where no test runs.

/// One of the system's standard menu items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standard {
    /// Hide the app (⌘H).
    Hide,
    /// Hide the other apps (⌥⌘H).
    HideOthers,
    /// Show every app again.
    ShowAll,
    /// Quit the app (⌘Q).
    Quit,
    /// Undo in the page (⌘Z).
    Undo,
    /// Redo in the page (⇧⌘Z).
    Redo,
    /// Cut the page's selection (⌘X).
    Cut,
    /// Copy the page's selection (⌘C).
    Copy,
    /// Paste into the page (⌘V).
    Paste,
    /// Select all of the page's field or text (⌘A).
    SelectAll,
    /// Minimize the window (⌘M).
    Minimize,
    /// Close the window (⌘W), which hides it, as its close button does.
    CloseWindow,
    /// A line between groups.
    Separator,
}

/// The menu bar, each menu by its title: the app's own (which macOS titles with the app's name
/// whatever is written here), Edit and Window.
pub const MAIN: [(&str, &[Standard]); 3] = [
    (
        "Symphony",
        &[
            Standard::Hide,
            Standard::HideOthers,
            Standard::ShowAll,
            Standard::Separator,
            Standard::Quit,
        ],
    ),
    (
        "Edit",
        &[
            Standard::Undo,
            Standard::Redo,
            Standard::Separator,
            Standard::Cut,
            Standard::Copy,
            Standard::Paste,
            Standard::SelectAll,
        ],
    ),
    ("Window", &[Standard::Minimize, Standard::CloseWindow]),
];

#[cfg(test)]
#[path = "main_menu_tests.rs"]
mod tests;
