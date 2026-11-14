//! Python's text rules, kept exact, so a file this crate writes is byte for byte the file Python would
//! write: the plugin kit's (`symphony_docs::text`), which a plugin reads its own files by, and a
//! string's `repr` and an OS error as Python prints them, the helper `symphony-schema`'s.

pub(crate) use symphony_docs::text::NS;
pub use symphony_docs::text::{
    FileStamp, S, is_space, is_word, lstrip, path_str, prefix_chars, read_text, rstrip, splitlines,
    strip, universal_newlines, write_whole,
};
pub use symphony_schema::text::{os_error, repr};
