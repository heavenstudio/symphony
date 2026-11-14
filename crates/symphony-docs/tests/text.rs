//! Python's text rules and a page's escaping and quoting, as a plugin and the Symphony's library both
//! read and write by them.

use std::path::{Path, PathBuf};

use symphony_docs::markup::{escape, quote, unescape};
use symphony_docs::text::{
    FileStamp, NS, S, is_space, is_word, lstrip, path_str, prefix_chars, read_text, rstrip,
    splitlines, strip, universal_newlines, write_whole,
};

/// A file of this test's own in the system's temporary folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        Scratch(
            std::env::temp_dir().join(format!("symphony-docs-text-{}-{name}", std::process::id())),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn splitlines_breaks_where_python_does_and_drops_a_last_empty_line() {
    assert_eq!(splitlines(""), Vec::<&str>::new());
    assert_eq!(splitlines("a\n"), vec!["a"]);
    assert_eq!(splitlines("a\n\nb"), vec!["a", "", "b"]);
    assert_eq!(
        splitlines("a\r\nb\rc\x0bd\x0ce\x1cf\x1dg\x1eh\u{85}i\u{2028}j\u{2029}k"),
        vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"]
    );
    assert_eq!(splitlines("a\x1fb"), vec!["a\x1fb"]);
    assert_eq!(splitlines("a\r\n\r\nb"), vec!["a", "", "b"]);
}

#[test]
fn strip_takes_pythons_spaces_including_the_four_separators() {
    assert_eq!(strip(" \t\x1c a b\x1f\u{a0}\u{3000}"), "a b");
    assert_eq!(lstrip("  a "), "a ");
    assert_eq!(rstrip("  a \n"), "  a");
    assert_eq!(strip("\x1e\x1d"), "");
    assert!(is_space('\x1d'));
    assert!(!is_space('\u{200b}'));
    assert!(is_word('é') && is_word('_') && is_word('7') && !is_word('-'));
}

#[test]
fn the_space_classes_are_pythons_in_a_pattern() {
    assert_eq!(S, r"[\s\x1C-\x1F]");
    assert_eq!(NS, r"[^\s\x1C-\x1F]");
}

#[test]
fn prefix_counts_characters_not_bytes() {
    assert_eq!(prefix_chars("éèê", 2), "éè");
    assert_eq!(prefix_chars("ab", 80), "ab");
    assert_eq!(prefix_chars("ab", 0), "");
}

#[test]
fn a_path_prints_as_pathlib_prints_it() {
    assert_eq!(path_str("./tmp/x/"), "tmp/x");
    assert_eq!(path_str("/a//b/./c"), "/a/b/c");
    assert_eq!(path_str("//host/a"), "//host/a");
    assert_eq!(path_str("///a"), "/a");
    assert_eq!(path_str("."), ".");
    assert_eq!(path_str(""), ".");
}

#[test]
fn universal_newlines_read_crlf_and_cr_as_lf() {
    assert_eq!(universal_newlines("a\r\nb\rc\n".into()), "a\nb\nc\n");
    assert_eq!(universal_newlines("plain\n".into()), "plain\n");
}

#[test]
fn read_text_reads_universal_newlines_and_sees_a_file_replaced() {
    let file = Scratch::new("read.md");
    std::fs::write(&file.0, "a\r\nb\rc\n").unwrap();
    assert_eq!(read_text(&file.0).unwrap(), "a\nb\nc\n");
    // Read again from memory while the file holds, and afresh once it is replaced.
    assert_eq!(read_text(&file.0).unwrap(), "a\nb\nc\n");
    write_whole(&file.0, "other\n").unwrap();
    assert_eq!(read_text(&file.0).unwrap(), "other\n");
    // A relative path is read from the working folder, the package's own under `cargo test`.
    assert!(
        read_text(Path::new("Cargo.toml"))
            .unwrap()
            .contains("name = \"symphony-docs\"")
    );
}

#[test]
fn read_text_refuses_what_is_not_utf8_and_a_missing_file() {
    let file = Scratch::new("bytes.md");
    std::fs::write(&file.0, [0xff, 0xfe]).unwrap();
    assert_eq!(
        read_text(&file.0).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
    let missing = Path::new("/nonexistent/symphony-docs-text.md");
    assert_eq!(
        read_text(missing).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
}

#[test]
fn a_stamp_changes_with_the_file_and_holds_while_it_does_not() {
    let file = Scratch::new("stamp.md");
    std::fs::write(&file.0, "a").unwrap();
    let first = FileStamp::read(&file.0).unwrap();
    assert!(FileStamp::read(&file.0).unwrap() == first);
    std::fs::write(&file.0, "ab").unwrap();
    assert!(FileStamp::read(&file.0).unwrap() != first);
    assert!(FileStamp::read(Path::new("/nonexistent/stamp.md")).is_err());
}

#[test]
fn write_whole_replaces_the_text_and_keeps_the_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let file = Scratch::new("whole.md");
    std::fs::write(&file.0, "old").unwrap();
    std::fs::set_permissions(&file.0, std::fs::Permissions::from_mode(0o640)).unwrap();
    write_whole(&file.0, "new\r\n").unwrap();
    assert_eq!(std::fs::read(&file.0).unwrap(), b"new\r\n");
    let mode = std::fs::metadata(&file.0).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
    assert!(write_whole(Path::new("/nonexistent/folder/x.md"), "x").is_err());
}

#[test]
fn escape_writes_the_five_references_html_escape_does() {
    assert_eq!(
        escape(r#"<a href="x">it's & </a>"#),
        "&lt;a href=&quot;x&quot;&gt;it&#x27;s &amp; &lt;/a&gt;"
    );
    assert_eq!(escape("plain é"), "plain é");
}

#[test]
fn unescape_reads_the_named_references_and_numbers_back() {
    assert_eq!(
        unescape("&lt;&gt;&amp;&quot;&apos;&#39;&#x27;&#X41;"),
        "<>&\"'''A"
    );
    assert_eq!(unescape(&escape("a <b> & 'c' \"d\"")), "a <b> & 'c' \"d\"");
    // What is no reference stays as written.
    assert_eq!(unescape("a & b"), "a & b");
    assert_eq!(unescape("&nbsp;"), "&nbsp;");
    assert_eq!(unescape("&#xD800;"), "&#xD800;");
    assert_eq!(unescape("&#notanumber;"), "&#notanumber;");
    assert_eq!(unescape("&averylongname;"), "&averylongname;");
    assert_eq!(unescape("&amp"), "&amp");
    // A reference's name is at most nine bytes; an `&` that opens none is kept and read past.
    assert_eq!(unescape("&#x0000041;"), "A");
    assert_eq!(unescape("&#x00000041;"), "&#x00000041;");
    assert_eq!(unescape("&&lt;é&gt"), "&<é&gt");
}

#[test]
fn quote_leaves_letters_digits_the_four_marks_and_the_safe_ones() {
    assert_eq!(quote("docs/a b.md", "/"), "docs/a%20b.md");
    assert_eq!(quote("docs/a b.md", ""), "docs%2Fa%20b.md");
    assert_eq!(quote("A-z_0.9~", ""), "A-z_0.9~");
    assert_eq!(quote("é#?", ""), "%C3%A9%23%3F");
}
