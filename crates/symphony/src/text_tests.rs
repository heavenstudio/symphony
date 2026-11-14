use super::{NS, S, lstrip, path_str, prefix_chars, rstrip};

#[test]
fn the_one_sided_strips_take_pythons_spaces_including_the_four_separators() {
    assert_eq!(lstrip("  a "), "a ");
    assert_eq!(rstrip("  a \n"), "  a");
    assert_eq!(lstrip("\x1c\u{3000}a\x1f"), "a\x1f");
    assert_eq!(rstrip("\x1ca\x1f\u{a0}"), "\x1ca");
    assert_eq!(lstrip("\x1e\x1d"), "");
    assert_eq!(rstrip(""), "");
    // A zero-width space is no space to Python.
    assert_eq!(rstrip("a\u{200b}"), "a\u{200b}");
}

#[test]
fn not_a_space_is_the_space_class_turned_round() {
    assert_eq!(NS, r"[^\s\x1C-\x1F]");
    assert_eq!(NS.replacen('^', "", 1), S);
}

#[test]
fn prefix_counts_characters_not_bytes() {
    assert_eq!(prefix_chars("éèê", 2), "éè");
    assert_eq!(prefix_chars("ab", 80), "ab");
    assert_eq!(prefix_chars("ab", 2), "ab");
    assert_eq!(prefix_chars("ab", 0), "");
    assert_eq!(prefix_chars("", 3), "");
}

#[test]
fn a_path_prints_as_pathlib_prints_it() {
    assert_eq!(path_str("./tmp/x/"), "tmp/x");
    assert_eq!(path_str("/a//b/./c"), "/a/b/c");
    assert_eq!(path_str("//host/a"), "//host/a");
    assert_eq!(path_str("///a"), "/a");
    assert_eq!(path_str("/"), "/");
    assert_eq!(path_str("."), ".");
    assert_eq!(path_str(""), ".");
    assert_eq!(path_str("a/../b"), "a/../b");
}
