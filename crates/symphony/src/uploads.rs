//! The owner's attachments: a screenshot or a document added to an ask, a card line, a comment, a
//! new task or a task thread message.
//!
//! Each file is stored once, under its content's SHA-256, in [`DIR`] of the server's checkout (the
//! main checkout for the shared app, so every branch's pages read the same file), and is never
//! removed with the post that named it: an ask goes after a day, its pictures stay. The post keeps
//! the file as one Markdown line of its text, [`Stored::markup`]: `![name](/artifacts/uploads/<sha>.png)`
//! for a picture, `[name](/artifacts/uploads/<sha>.pdf)` for any other file. A task brief and an
//! agent's line are Markdown already; owner words stay literal text, and only a whole line that is
//! such a reference ([`reference`]) is drawn ([`owner_html`]).
//!
//! A file is accepted by its content, never by the name or type the browser gave it: the pictures
//! a browser draws (PNG, JPEG, GIF, WebP), PDF, plain text, and Word, Excel and PowerPoint files,
//! at most [`MAX_BYTES`]. SVG, HTML and scripts are refused, under a text file's name too; the
//! server sends each stored file with the one type its extension names ([`Kind::mime`]), never as
//! markup.

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::markup::escape;

/// Where uploads live, under the server's checkout. Gitignored, like every picture a card shows.
pub const DIR: &str = "artifacts/uploads";
/// The address of a stored file, followed by `<sha256>.<ext>`.
pub const ROUTE: &str = "/artifacts/uploads/";
/// The largest upload, in bytes.
pub const MAX_BYTES: usize = symphony_api::UPLOAD_MAX_BYTES;
/// The longest file name a reference keeps, in characters.
pub const NAME_MAX: usize = 80;

/// One kind of file the server stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    /// The stored file's extension, which alone decides how it is served.
    pub ext: &'static str,
    /// The one type the server sends it as.
    pub mime: &'static str,
    /// A picture the page draws inline.
    pub picture: bool,
    /// Opened in the browser (pictures, text) rather than downloaded (PDF, Office files).
    pub inline: bool,
    /// Recognised by its extension and UTF-8 content: plain text has no signature.
    text: bool,
    /// Recognised by its extension inside a zip signature.
    office: bool,
}

const fn picture(ext: &'static str, mime: &'static str) -> Kind {
    Kind {
        ext,
        mime,
        picture: true,
        inline: true,
        text: false,
        office: false,
    }
}

const fn text(ext: &'static str) -> Kind {
    Kind {
        ext,
        mime: "text/plain; charset=utf-8",
        picture: false,
        inline: true,
        text: true,
        office: false,
    }
}

const fn office(ext: &'static str, mime: &'static str) -> Kind {
    Kind {
        ext,
        mime,
        picture: false,
        inline: false,
        text: false,
        office: true,
    }
}

/// Every kind the server stores, by its stored extension.
pub const KINDS: [Kind; 13] = [
    picture("png", "image/png"),
    picture("jpg", "image/jpeg"),
    picture("gif", "image/gif"),
    picture("webp", "image/webp"),
    Kind {
        ext: "pdf",
        mime: "application/pdf",
        picture: false,
        inline: false,
        text: false,
        office: false,
    },
    text("txt"),
    text("md"),
    text("csv"),
    text("json"),
    text("log"),
    office(
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    office(
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    office(
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
];

/// The kind a stored extension names.
pub fn kind_of(ext: &str) -> Option<Kind> {
    KINDS.iter().copied().find(|kind| kind.ext == ext)
}

/// Why an upload was refused; the file is not stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Empty,
    TooLarge {
        bytes: usize,
    },
    Unsupported {
        name: String,
    },
    /// Text that is markup or a script ([`active`]).
    Active {
        name: String,
    },
}

impl Refusal {
    /// The HTTP status the refusal answers with.
    pub fn status(&self) -> u16 {
        match self {
            Self::Empty => 400,
            Self::TooLarge { .. } => 413,
            Self::Unsupported { .. } | Self::Active { .. } => 415,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named = |name: &str| {
            if name.is_empty() {
                "the file".to_string()
            } else {
                name.to_string()
            }
        };
        match self {
            Self::Empty => f.write_str("the file is empty"),
            Self::TooLarge { bytes } => write!(
                f,
                "the file is {:.1} MB; uploads are at most {} MB",
                megabytes(*bytes),
                MAX_BYTES / (1024 * 1024)
            ),
            Self::Unsupported { name } => write!(
                f,
                "{} is not a picture (PNG, JPEG, GIF, WebP) or a document (PDF, text, Word, Excel, PowerPoint)",
                named(name)
            ),
            Self::Active { name } => write!(
                f,
                "{} holds markup or a script, which is never stored",
                named(name)
            ),
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a size shown to one decimal place of a megabyte"
)]
fn megabytes(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// The extension a name ends in, lower case; empty when it has none.
fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(stem, ext)| {
            if stem.is_empty() {
                String::new()
            } else {
                ext.to_ascii_lowercase()
            }
        })
        .unwrap_or_default()
}

/// The kind `bytes` are, read from their content: a picture or a PDF by its signature, an Office
/// file by the zip signature and its name's extension, plain text by its name's extension and
/// content that is UTF-8 with no NUL byte and is not markup or a script ([`active`]). Anything
/// else, SVG and HTML among it, is refused.
pub fn sniff(name: &str, bytes: &[u8]) -> Result<Kind, Refusal> {
    if bytes.is_empty() {
        return Err(Refusal::Empty);
    }
    if bytes.len() > MAX_BYTES {
        return Err(Refusal::TooLarge { bytes: bytes.len() });
    }
    let signed = |ext: &str| kind_of(ext).expect("a known kind");
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(signed("png"));
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Ok(signed("jpg"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok(signed("gif"));
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Ok(signed("webp"));
    }
    if bytes.starts_with(b"%PDF-") {
        return Ok(signed("pdf"));
    }
    let named = kind_of(&extension(name));
    let refused = || Refusal::Unsupported { name: clean(name) };
    if bytes.starts_with(b"PK\x03\x04") {
        return named.filter(|kind| kind.office).ok_or_else(refused);
    }
    let body = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let (Some(kind), Ok(words)) = (named.filter(|kind| kind.text), std::str::from_utf8(body))
    else {
        return Err(refused());
    };
    if words.contains('\0') {
        return Err(refused());
    }
    if active(words) {
        return Err(Refusal::Active { name: clean(name) });
    }
    Ok(kind)
}

/// Tags that start a document or run, style, load or embed something, found anywhere in text.
const ACTIVE_TAGS: [&str; 20] = [
    "<!doctype",
    "<?xml",
    "<html",
    "<head",
    "<body",
    "<script",
    "<style",
    "<svg",
    "<math",
    "<iframe",
    "<frame",
    "<frameset",
    "<object",
    "<embed",
    "<applet",
    "<link",
    "<meta",
    "<base",
    "<form",
    "<img",
];

/// Whether text is markup or a script: it opens (spaces aside) with a tag, a comment, a
/// declaration, a processing instruction or a `#!` line, or holds one of [`ACTIVE_TAGS`]
/// anywhere, in any case, ended as a tag name ends (a space, `/`, `>` or the end). Angle brackets
/// in words (`a < b`, `Vec<u8>`) and a tag such as `<b>` after the first words stay text. A script
/// without a `#!` line cannot be told from text; it is served as text only.
fn active(text: &str) -> bool {
    let opens = match text.trim_start().as_bytes() {
        [b'#', b'!', ..] => true,
        [b'<', next, ..] => next.is_ascii_alphabetic() || matches!(next, b'!' | b'?' | b'/'),
        _ => false,
    };
    opens
        || text.match_indices('<').any(|(at, _)| {
            let rest = &text.as_bytes()[at..];
            ACTIVE_TAGS.iter().any(|tag| {
                rest.get(..tag.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(tag.as_bytes()))
                    && rest
                        .get(tag.len())
                        .is_none_or(|after| after.is_ascii_whitespace() || b"/>".contains(after))
            })
        })
}

/// A file name made safe for one Markdown line: its last path part, with letters, digits, spaces
/// and `.-_()+,` kept and every other character `_`, at most [`NAME_MAX`] characters.
fn clean(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut out = String::new();
    for c in base.chars() {
        let c = if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '(' | ')' | '+' | ',') {
            c
        } else if c == ' ' || c == '\t' {
            ' '
        } else {
            '_'
        };
        if !(c == ' ' && (out.is_empty() || out.ends_with(' '))) {
            out.push(c);
        }
    }
    let out = out.trim_start_matches(['.', ' ']).trim_end().to_string();
    if out.chars().count() <= NAME_MAX {
        return out;
    }
    match out.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.chars().count() < NAME_MAX / 2 => {
            let keep = NAME_MAX - ext.chars().count() - 1;
            format!(
                "{}.{ext}",
                stem.chars().take(keep).collect::<String>().trim_end()
            )
        }
        _ => out
            .chars()
            .take(NAME_MAX)
            .collect::<String>()
            .trim_end()
            .into(),
    }
}

/// The name a reference shows: [`clean`], named after its kind when nothing is left, with the
/// stored extension added when it has none and the name cleaned again with it, so a name at
/// [`NAME_MAX`] gives up characters for the extension and [`reference`] reads the line back.
pub fn display_name(name: &str, kind: Kind) -> String {
    let name = clean(name);
    let name = if name.is_empty() {
        if kind.picture { "image" } else { "file" }.to_string()
    } else {
        name
    };
    if extension(&name).is_empty() {
        clean(&format!("{}.{}", name.trim_end_matches('.'), kind.ext))
    } else {
        name
    }
}

/// A stored upload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    /// The content's SHA-256, lower-case hex: the file's name.
    pub sha: String,
    pub kind: Kind,
    pub name: String,
    pub bytes: usize,
}

impl Stored {
    /// `/artifacts/uploads/<sha>.<ext>`.
    pub fn url(&self) -> String {
        format!("{ROUTE}{}.{}", self.sha, self.kind.ext)
    }

    /// The line a post carries to keep the file.
    pub fn markup(&self) -> String {
        markup(&self.name, &self.url(), self.kind.picture)
    }

    pub fn wire(&self) -> symphony_api::Upload {
        symphony_api::Upload {
            url: self.url(),
            name: self.name.clone(),
            picture: self.kind.picture,
            bytes: self.bytes as u64,
            markup: self.markup(),
        }
    }
}

fn markup(name: &str, url: &str, picture: bool) -> String {
    format!("{}[{name}]({url})", if picture { "!" } else { "" })
}

/// Why storing an upload failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub status: u16,
    pub error: String,
}

impl From<Refusal> for Failure {
    fn from(refusal: Refusal) -> Self {
        Self {
            status: refusal.status(),
            error: refusal.to_string(),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Store `bytes`, named `name` by the owner, under `root`'s [`DIR`]: once per content, written
/// to a temporary file and renamed into place, so a reader never sees half a file and two posts of
/// the same picture share it.
pub fn store(root: &Path, name: &str, bytes: &[u8]) -> Result<Stored, Failure> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let kind = sniff(name, bytes)?;
    let stored = Stored {
        sha: hex(&Sha256::digest(bytes)),
        kind,
        name: display_name(name, kind),
        bytes: bytes.len(),
    };
    let failed = |e: std::io::Error| Failure {
        status: 500,
        error: format!("could not store the upload: {e}"),
    };
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir).map_err(failed)?;
    let path = dir.join(format!("{}.{}", stored.sha, kind.ext));
    if path
        .metadata()
        .is_ok_and(|meta| meta.is_file() && meta.len() == bytes.len() as u64)
    {
        return Ok(stored);
    }
    let temp = dir.join(format!(
        ".{}.{}.{}.part",
        stored.sha,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temp, &path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(failed(e));
    }
    Ok(stored)
}

/// The stored file `file` (`<sha256>.<ext>`, as [`ROUTE`] is followed) names under `root`, and
/// its kind. Nothing else under the folder is ever served from it.
pub fn path(root: &Path, file: &str) -> Option<(PathBuf, Kind)> {
    let (sha, ext) = file.split_once('.')?;
    if sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let kind = kind_of(ext)?;
    Some((root.join(DIR).join(file), kind))
}

/// An upload a line names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub name: String,
    pub url: String,
    pub picture: bool,
}

impl Reference {
    /// The reference written as [`Stored::markup`] writes it.
    pub fn markup(&self) -> String {
        markup(&self.name, &self.url, self.picture)
    }
}

/// The upload `line` names, when the whole line (spaces aside) is one reference: `![name](url)` of
/// a stored picture, or `[name](url)` of any stored file, the url [`ROUTE`] and a [`path`] name.
/// The name is one an upload can have ([`clean`]), so no markup typed into it is ever drawn.
pub fn reference(line: &str) -> Option<Reference> {
    let line = line.trim();
    let (picture, rest) = match line.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    let (name, rest) = rest.strip_prefix('[')?.split_once("](")?;
    let url = rest.strip_suffix(')')?;
    if name.trim().is_empty() || clean(name.trim()) != name.trim() {
        return None;
    }
    let (_, kind) = path(Path::new(""), url.strip_prefix(ROUTE)?)?;
    if picture && !kind.picture {
        return None;
    }
    Some(Reference {
        name: name.trim().to_string(),
        url: url.to_string(),
        picture,
    })
}

/// Whether any line of `text` is an upload [`reference`].
pub fn has_reference(text: &str) -> bool {
    text.lines().any(|line| reference(line).is_some())
}

/// The stored files `text` names, as paths under `root`: what an agent opens.
pub fn named_files(root: &Path, text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(reference)
        .filter_map(|found| path(root, found.url.strip_prefix(ROUTE)?).map(|(path, _)| path))
        .collect()
}

/// Owner words as HTML: each line literal text, except a line that is an upload [`reference`],
/// which `convert` draws from its Markdown (a picture, or a link), lines joined by `<br>`.
pub fn owner_html(text: &str, convert: impl Fn(&str) -> String) -> String {
    text.split('\n')
        .map(|line| match reference(line) {
            Some(found) => convert(&found.markup()),
            None => escape(line),
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03";
    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn scratch(test: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "sf-uploads-{}-{test}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn pictures_and_pdfs_are_known_by_their_content_whatever_their_name() {
        let webp = b"RIFF\x10\0\0\0WEBPVP8 ";
        for (name, bytes, ext) in [
            ("shot.png", PNG, "png"),
            ("renamed.txt", PNG, "png"),
            ("", PNG, "png"),
            (
                "photo.jpeg",
                b"\xff\xd8\xff\xe0\0\x10JFIF".as_slice(),
                "jpg",
            ),
            ("old.gif", b"GIF87a\x01\0".as_slice(), "gif"),
            ("new.gif", b"GIF89a\x01\0".as_slice(), "gif"),
            ("clip.webp", webp.as_slice(), "webp"),
            ("paper", b"%PDF-1.7\n".as_slice(), "pdf"),
        ] {
            assert_eq!(sniff(name, bytes).map(|k| k.ext), Ok(ext), "{name}");
        }
        assert!(sniff("a.png", PNG).unwrap().picture);
        assert!(!sniff("a.pdf", b"%PDF-1.4").unwrap().picture);
    }

    #[test]
    fn text_and_office_files_need_their_extension_and_a_matching_content() {
        for (name, ext) in [
            ("notes.txt", "txt"),
            ("README.MD", "md"),
            ("rows.csv", "csv"),
            ("data.json", "json"),
            ("run.log", "log"),
        ] {
            assert_eq!(sniff(name, "naïve, text\n".as_bytes()).unwrap().ext, ext);
        }
        assert_eq!(
            sniff("bom.txt", b"\xef\xbb\xbfhello").unwrap().ext,
            "txt",
            "a byte order mark is still text"
        );
        for (name, ext) in [("a.docx", "docx"), ("b.XLSX", "xlsx"), ("c.pptx", "pptx")] {
            assert_eq!(sniff(name, b"PK\x03\x04rest").unwrap().ext, ext);
        }
        for (name, bytes) in [
            ("binary.txt", b"text\0with a NUL".as_slice()),
            ("latin1.txt", b"caf\xe9".as_slice()),
            ("archive.zip", b"PK\x03\x04rest".as_slice()),
            ("disguised.txt", b"PK\x03\x04rest".as_slice()),
            ("note", b"plain words".as_slice()),
            (".txt", b"hidden file without a stem".as_slice()),
        ] {
            assert!(
                matches!(sniff(name, bytes), Err(Refusal::Unsupported { .. })),
                "{name}"
            );
        }
    }

    #[test]
    fn markup_and_scripts_are_refused_whatever_they_are_called() {
        for (name, bytes) in [
            (
                "vector.svg",
                b"<svg xmlns='http://www.w3.org/2000/svg'></svg>".as_slice(),
            ),
            ("page.html", b"<!doctype html><script>1</script>".as_slice()),
            ("page.htm", b"<html>".as_slice()),
            ("code.js", b"alert(1)".as_slice()),
            ("module.mjs", b"export {}".as_slice()),
            ("data.xml", b"<?xml version='1.0'?>".as_slice()),
            ("run.sh", b"#!/bin/sh".as_slice()),
            ("model.glb", b"glTF".as_slice()),
        ] {
            let refused = sniff(name, bytes).unwrap_err();
            assert_eq!(refused.status(), 415, "{name}");
            assert!(refused.to_string().starts_with(name), "{refused}");
        }
        assert!(
            Refusal::Unsupported {
                name: String::new()
            }
            .to_string()
            .starts_with("the file is not a picture")
        );
    }

    #[test]
    fn markup_and_scripts_under_a_text_name_are_refused_by_their_content() {
        for name in ["notes.txt", "README.md", "rows.csv", "data.json", "run.log"] {
            for bytes in [
                "<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>",
                "<!DOCTYPE html><p>hi</p>",
                "<html><body>",
                "\u{feff}  \n\t<p onclick=alert(1)>",
                "<?xml version='1.0'?><svg/>",
                "<!-- a comment first --><b>",
                "</div>",
                "#!/bin/sh\nrm -rf ~\n",
                "  #!/usr/bin/env node\n",
                "words first\n<SCRIPT src=x></SCRIPT>",
                "a picture: <svg onload=alert(1)>",
                "framed <iframe\nsrcdoc=x>",
                "<img/src=x onerror=alert(1)>",
            ] {
                let Err(refused) = sniff(name, bytes.as_bytes()) else {
                    panic!("{name}: {bytes:?} was taken as text");
                };
                assert_eq!(refused.status(), 415);
                assert_eq!(
                    refused.to_string(),
                    format!("{name} holds markup or a script, which is never stored")
                );
            }
        }
        // Words that only use angle brackets or a hash stay text.
        for words in [
            "a < b and c > d",
            "x<y, Vec<u8> and Option<String>",
            "2 <3 you",
            "# Heading\n\n<!-- register:start -->\nsee <b>bold</b> mid-line\n",
            "#hashtag #!not-at-the-start",
            "the <scripts> folder",
            "{\"tag\": \"<html\"}",
        ] {
            assert_eq!(
                sniff("notes.md", words.as_bytes()).map(|kind| kind.ext),
                Ok("md"),
                "{words:?}"
            );
        }
    }

    #[test]
    fn an_empty_file_and_one_past_the_cap_are_refused_and_the_cap_itself_is_kept() {
        assert_eq!(sniff("a.txt", b""), Err(Refusal::Empty));
        assert_eq!(Refusal::Empty.status(), 400);
        let mut at_cap = vec![b'a'; MAX_BYTES];
        assert_eq!(sniff("big.txt", &at_cap).unwrap().ext, "txt");
        at_cap.push(b'a');
        let refused = sniff("big.txt", &at_cap).unwrap_err();
        assert_eq!(
            refused,
            Refusal::TooLarge {
                bytes: MAX_BYTES + 1
            }
        );
        assert_eq!(refused.status(), 413);
        assert_eq!(
            refused.to_string(),
            "the file is 10.0 MB; uploads are at most 10 MB"
        );
    }

    #[test]
    fn names_keep_one_markdown_line_and_their_extension() {
        for (given, kind, shown) in [
            (
                "Screenshot 2026-10-28 at 10.12.03.png",
                "png",
                "Screenshot 2026-10-28 at 10.12.03.png",
            ),
            ("C:\\Users\\me\\report.pdf", "pdf", "report.pdf"),
            ("../../etc/passwd.txt", "txt", "passwd.txt"),
            ("a](javascript:x)[b.png", "png", "a_(javascript_x)_b.png"),
            ("line\nbreak.txt", "txt", "line_break.txt"),
            ("   spaced   out .md", "md", "spaced out .md"),
            ("...hidden.txt", "txt", "hidden.txt"),
            ("", "png", "image.png"),
            ("", "pdf", "file.pdf"),
            ("<>", "txt", "__.txt"),
            ("image", "png", "image.png"),
            ("trailing.", "jpg", "trailing.jpg"),
            ("résumé.pdf", "pdf", "résumé.pdf"),
        ] {
            assert_eq!(
                display_name(given, kind_of(kind).unwrap()),
                shown,
                "{given:?}"
            );
            assert_eq!(clean(shown), shown, "a reference keeps {shown:?} as it is");
        }
        let long = format!("{}.pdf", "x".repeat(200));
        let shown = display_name(&long, kind_of("pdf").unwrap());
        assert_eq!(shown.chars().count(), NAME_MAX);
        assert!(shown.ends_with("x.pdf"), "{shown}");
        let no_ext = "y".repeat(200);
        assert_eq!(
            display_name(&no_ext, kind_of("txt").unwrap()),
            format!("{}.txt", "y".repeat(NAME_MAX - 4))
        );
    }

    #[test]
    fn a_long_name_reads_back_from_the_line_its_upload_wrote() {
        let root = scratch("long-names");
        for length in [
            NAME_MAX - 5,
            NAME_MAX - 4,
            NAME_MAX - 3,
            NAME_MAX,
            NAME_MAX + 1,
            200,
        ] {
            for (given, bytes, ext) in [
                ("x".repeat(length), PNG, "png"),
                (
                    format!("{}.pdf", "z".repeat(length)),
                    b"%PDF-1.7".as_slice(),
                    "pdf",
                ),
            ] {
                let stored = store(&root, &given, bytes).unwrap();
                assert!(
                    stored.name.chars().count() <= NAME_MAX && stored.name.ends_with(ext),
                    "{length}: {}",
                    stored.name
                );
                let line = stored.markup();
                let found =
                    reference(&line).unwrap_or_else(|| panic!("{length}: {line} is not read back"));
                assert_eq!(found.markup(), line);
                assert_eq!((found.name, found.url), (stored.name.clone(), stored.url()));
                assert_eq!(
                    owner_html(&line, |markdown| format!("[drawn {markdown}]")),
                    format!("[drawn {line}]")
                );
            }
        }
        assert_eq!(
            store(&root, &"x".repeat(NAME_MAX - 4), PNG).unwrap().name,
            format!("{}.png", "x".repeat(NAME_MAX - 4)),
            "a name that fits with its extension keeps every character"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_stored_file_is_written_once_by_content_and_read_back_by_its_reference() {
        let root = scratch("store");
        let first = store(&root, "Screen Shot.png", PNG).unwrap();
        assert_eq!(first.sha.len(), 64);
        assert_eq!(first.name, "Screen Shot.png");
        assert_eq!(first.url(), format!("/artifacts/uploads/{}.png", first.sha));
        assert_eq!(
            first.markup(),
            format!("![Screen Shot.png](/artifacts/uploads/{}.png)", first.sha)
        );
        let wire = first.wire();
        assert!(wire.picture);
        assert_eq!(wire.bytes, PNG.len() as u64);
        assert_eq!(wire.markup, first.markup());
        let (file, kind) = path(&root, &format!("{}.png", first.sha)).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), PNG);
        assert_eq!(kind.mime, "image/png");
        // The same bytes under another name share the stored file.
        let again = store(&root, "copy.png", PNG).unwrap();
        assert_eq!(again.sha, first.sha);
        assert_eq!(again.name, "copy.png");
        let entries: Vec<_> = std::fs::read_dir(root.join(DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            entries,
            [format!("{}.png", first.sha)],
            "no temporary file left"
        );
        let document = store(&root, "notes.pdf", b"%PDF-1.4 notes").unwrap();
        assert_eq!(
            document.markup(),
            format!("[notes.pdf](/artifacts/uploads/{}.pdf)", document.sha)
        );
        assert!(!document.wire().picture);
        assert_eq!(
            reference(&document.markup()).unwrap().url,
            document.url(),
            "a reference reads back what the upload wrote"
        );
        let refused = store(&root, "page.html", b"<html>").unwrap_err();
        assert_eq!(refused.status, 415);
        assert_eq!(std::fs::read_dir(root.join(DIR)).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_truncated_stored_file_is_written_again() {
        let root = scratch("truncated");
        let stored = store(&root, "a.png", PNG).unwrap();
        let (file, _) = path(&root, &format!("{}.png", stored.sha)).unwrap();
        std::fs::write(&file, &PNG[..4]).unwrap();
        store(&root, "a.png", PNG).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), PNG);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn storing_reports_a_folder_it_cannot_make() {
        let root = scratch("unwritable");
        std::fs::write(root.join("artifacts"), "a file where the folder goes").unwrap();
        let failed = store(&root, "a.png", PNG).unwrap_err();
        assert_eq!(failed.status, 500);
        assert!(
            failed.error.starts_with("could not store the upload"),
            "{failed:?}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_that_cannot_take_its_place_is_reported_and_leaves_no_part_behind() {
        let root = scratch("blocked");
        let sha = hex(&Sha256::digest(PNG));
        let folder = root.join(DIR);
        // Something that is not the stored file holds its name, so the rename fails.
        std::fs::create_dir_all(folder.join(format!("{sha}.png")).join("inside")).unwrap();
        let failed = store(&root, "a.png", PNG).unwrap_err();
        assert_eq!(failed.status, 500);
        assert!(
            failed.error.starts_with("could not store the upload"),
            "{failed:?}"
        );
        let left: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            left,
            [format!("{sha}.png")],
            "the temporary file is removed"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_a_sha_and_a_stored_extension_name_a_file() {
        let root = Path::new("/root");
        assert_eq!(
            path(root, &format!("{SHA}.pdf")).map(|(p, k)| (p, k.ext)),
            Some((root.join(DIR).join(format!("{SHA}.pdf")), "pdf"))
        );
        for file in [
            format!("{SHA}.svg"),
            format!("{SHA}.html"),
            format!("{SHA}.PNG"),
            format!("{SHA}.png.html"),
            format!("{}.png", SHA.to_uppercase()),
            format!("{}.png", &SHA[1..]),
            format!("{SHA}a.png"),
            format!("{}g.png", &SHA[1..]),
            format!("../{SHA}.png"),
            SHA.to_string(),
            String::new(),
        ] {
            assert!(path(root, &file).is_none(), "{file}");
        }
    }

    #[test]
    fn a_reference_is_a_whole_line_naming_a_stored_file() {
        let picture = format!("![shot.png](/artifacts/uploads/{SHA}.png)");
        assert_eq!(
            reference(&format!("  {picture}  ")),
            Some(Reference {
                name: "shot.png".into(),
                url: format!("/artifacts/uploads/{SHA}.png"),
                picture: true,
            })
        );
        let link = format!("[ notes ](/artifacts/uploads/{SHA}.pdf)");
        let found = reference(&link).unwrap();
        assert!(!found.picture);
        assert_eq!(
            found.markup(),
            format!("[notes](/artifacts/uploads/{SHA}.pdf)")
        );
        assert!(
            reference(&format!("[shot](/artifacts/uploads/{SHA}.png)")).is_some(),
            "a picture may be linked"
        );
        for line in [
            format!("![notes](/artifacts/uploads/{SHA}.pdf)"),
            format!("see ![shot](/artifacts/uploads/{SHA}.png)"),
            format!("![shot](/artifacts/uploads/{SHA}.png) and more"),
            format!("![shot](/artifacts/other/{SHA}.png)"),
            format!("![shot](https://example.com/artifacts/uploads/{SHA}.png)"),
            format!("![shot](/artifacts/uploads/{SHA}.svg)"),
            format!("![](/artifacts/uploads/{SHA}.png)"),
            format!("![a]b](/artifacts/uploads/{SHA}.png)"),
            format!("[<img src=x onerror=alert(1)>](/artifacts/uploads/{SHA}.pdf)"),
            format!("[**bold**](/artifacts/uploads/{SHA}.pdf)"),
            format!("![`code`](/artifacts/uploads/{SHA}.png)"),
            format!("![a](/artifacts/uploads/{SHA}.png"),
            "![shot](/artifacts/uploads/short.png)".into(),
            String::new(),
        ] {
            assert!(reference(&line).is_none(), "{line}");
        }
    }

    #[test]
    fn owner_words_stay_literal_and_only_reference_lines_are_drawn() {
        let text = format!(
            "Look <here> & there\n![shot.png](/artifacts/uploads/{SHA}.png)\n\n<b>![x](/artifacts/uploads/{SHA}.svg)</b>"
        );
        let html = owner_html(&text, |markdown| format!("[drawn {markdown}]"));
        assert_eq!(
            html,
            format!(
                "Look &lt;here&gt; &amp; there<br>[drawn ![shot.png](/artifacts/uploads/{SHA}.png)]<br><br>&lt;b&gt;![x](/artifacts/uploads/{SHA}.svg)&lt;/b&gt;"
            )
        );
        assert_eq!(owner_html("plain", |_| unreachable!()), "plain");
        assert!(has_reference(&text));
        assert!(!has_reference("no files here"));
        assert_eq!(
            named_files(Path::new("/main"), &text),
            [Path::new("/main").join(DIR).join(format!("{SHA}.png"))]
        );
    }
}
