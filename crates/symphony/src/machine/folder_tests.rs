//! The machine folder on macOS and elsewhere, and a test's own when it names one.

use std::path::Path;

use super::*;

fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect();
    move |name| {
        pairs
            .iter()
            .find(|(named, _)| named == name)
            .map(|(_, value)| OsString::from(value))
    }
}

#[test]
fn the_machine_folder_is_the_platforms_unless_a_test_names_its_own() {
    let home = [("HOME", "/Users/owner")];
    assert_eq!(
        folder(vars(&home), true).unwrap(),
        Path::new("/Users/owner/Library/Application Support/Symphony")
    );
    assert_eq!(
        folder(vars(&home), false).unwrap(),
        Path::new("/Users/owner/.local/share/symphony")
    );
    let xdg = [("HOME", "/home/o"), ("XDG_DATA_HOME", "/data")];
    assert_eq!(
        folder(vars(&xdg), false).unwrap(),
        Path::new("/data/symphony")
    );
    // A relative XDG folder is not one: the specification's fallback is used.
    let relative = [("HOME", "/home/o"), ("XDG_DATA_HOME", "data")];
    assert_eq!(
        folder(vars(&relative), false).unwrap(),
        Path::new("/home/o/.local/share/symphony")
    );
    let named = [("HOME", "/home/o"), (DIR_ENV, "/tmp/machine")];
    for macos in [true, false] {
        assert_eq!(
            folder(vars(&named), macos).unwrap(),
            Path::new("/tmp/machine")
        );
    }
    let empty = [("HOME", "/home/o"), (DIR_ENV, "")];
    assert_eq!(
        folder(vars(&empty), true).unwrap(),
        Path::new("/home/o/Library/Application Support/Symphony")
    );
    let bad = folder(vars(&[(DIR_ENV, "machine")]), true).unwrap_err();
    assert!(bad.contains("not an absolute folder"), "{bad}");
    for none in [&[][..], &[("HOME", "")][..], &[("HOME", "rel")][..]] {
        let why = folder(vars(none), true).unwrap_err();
        assert!(why.contains("needs HOME"), "{why}");
    }
}
