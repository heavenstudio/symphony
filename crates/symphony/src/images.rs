//! Picture sizes for the Symphony, from `tools/docs_images.py`: every `<img>` of a local file leaves
//! its space reserved.
//!
//! A picture without width and height takes no room until it arrives, so each one that does arrive moves
//! the page (layout shift), worst on a phone where the pictures are large and the link slow. [`reserve`]
//! runs over the markup about to be sent and adds the file's real `width` and `height` (read from the PNG
//! header, not the whole file) to every local `<img>` lacking them; the page's `img { height:auto }`
//! turns the pair into an aspect ratio.
//!
//! A big picture is also drawn from a small copy: a concept picture is 2-3 MB, which a phone on a
//! mobile link cannot take by the dozen. [`reserve`] points a PNG over [`THUMB_MIN_BYTES`] at
//! `/thumb/<width>/<token>/<path>.jpg` (`data-full` keeps the original for the viewer), and [`thumbnail`]
//! makes that copy once, into `artifacts/docs-thumbs/`, named by a token of the file's path, time and
//! size (so the copy is cached forever and never stale). The resize is the operating system's own `sips`
//! ([`sips_convert`]); on a machine without it the original is drawn.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::{Captures, Regex};
use sha2::{Digest, Sha256};
use symphony_board::lock::Recovered as _;
use symphony_board::sys::{Command, Stdio};

use crate::markup::{normpath, quote, unquote};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
/// A picture smaller than this is not worth a second request.
pub const THUMB_MIN_BYTES: usize = 120_000;
/// Device pixels: about a sixth of a concept picture's bytes; the viewer opens the original.
pub const THUMB_WIDTH: u32 = 540;
/// For a picture the markup draws up to [`SMALL_SHOWN`] css pixels wide (matrix cells, strips).
pub const THUMB_SMALL: u32 = 160;
pub const SMALL_SHOWN: u32 = 80;
/// The thumbnails' folder in the project's artifacts folder.
pub const THUMB_DIR: &str = "docs-thumbs";
pub const THUMB_ROUTE: &str = "/thumb/";

static IMG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<img\b[^>]*>").expect("the img pattern"));
static SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bsrc="([^"]*)""#).expect("the src pattern"));
static WIDTH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bwidth="(\d+)""#).expect("the width pattern"));
static HEIGHT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bheight=").expect("the height pattern"));

type SizeKey = (PathBuf, i64, u64);
type Sizes = HashMap<SizeKey, Option<(u32, u32)>>;
static SIZES: LazyLock<Mutex<Sizes>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// (width, height) from a PNG's first bytes, `None` for anything else or a header that says 0.
pub fn png_size(head: &[u8]) -> Option<(u32, u32)> {
    if head.len() < 24 || !head.starts_with(PNG) || &head[12..16] != b"IHDR" {
        return None;
    }
    let word = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    let (width, height) = (word(16), word(20));
    (width != 0 && height != 0).then_some((width, height))
}

fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.mtime() * 1_000_000_000 + meta.mtime_nsec()
}

/// A picture file's size, remembered until the file changes.
pub fn file_size(path: &Path) -> Option<(u32, u32)> {
    let meta = std::fs::metadata(path).ok()?;
    let key = (path.to_path_buf(), mtime_ns(&meta), meta.len());
    if let Some(known) = SIZES.lock().recover().get(&key) {
        return *known;
    }
    let mut head = [0u8; 24];
    let read = std::fs::File::open(path)
        .and_then(|mut file| file.read(&mut head))
        .ok()?;
    let size = png_size(&head[..read]);
    SIZES.lock().recover().insert(key, size);
    size
}

/// The file a root-relative `src` names, if it is one under `root`: a document's neighbour named by
/// its route (`/docs/…`) is in the folder of the entrypoint serving it.
fn local(root: &Path, src: &str) -> Option<PathBuf> {
    let src = src.replace("&amp;", "&");
    let path = src.split(['?', '#']).next().unwrap_or("");
    if !path.starts_with('/') || path.starts_with("//") {
        return None; // a scheme, a host or a relative path is not a file of the project's
    }
    let base = root.canonicalize().ok()?;
    let file = crate::project::Project::of(root).file(unquote(path).trim_start_matches('/'));
    let joined = base.join(file);
    let found = PathBuf::from(normpath(&joined.to_string_lossy()));
    let found = found.canonicalize().unwrap_or(found);
    (found != base && found.starts_with(&base)).then_some(found)
}

/// Whether the operating system can resize a picture (`sips`).
pub fn sips_available() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join("sips").is_file()))
}

/// Write a JPEG of `src`, `width` pixels across, to `dest` with the system's `sips`; false when it could not.
pub fn sips_convert(src: &Path, dest: &Path, width: u32) -> bool {
    sips_convert_within(src, dest, width, Duration::from_secs(30))
}

/// [`sips_convert`], reaping `sips` once it has run for `budget`. A test's own `sips` is a file it has
/// just written, whose first exec macOS holds until its assessment of the new file ends, so the test
/// waits for its exit instead.
fn sips_convert_within(src: &Path, dest: &Path, width: u32, budget: Duration) -> bool {
    let mut command = Command::new("sips");
    command
        .args([
            "-s",
            "format",
            "jpeg",
            "-s",
            "formatOptions",
            "70",
            "--resampleWidth",
        ])
        .arg(width.to_string())
        .arg(src);
    convert(command, dest, budget)
}

/// Reserve a distinct output for every call, including overlapping requests for the same thumbnail.
fn part_file(dest: &Path) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let part = dest.with_extension(format!("jpg.{}.{n}.part", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&part) {
            Ok(_) => return Ok(part),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

/// Run the converter, publish only a complete output, and reap it when it exceeds its time budget.
/// It starts without a fork and inherits only its streams (symphony_board::sys::Command), so a connection
/// the server has accepted but not yet marked close-on-exec stays out of it.
fn convert(mut command: Command, dest: &Path, timeout: Duration) -> bool {
    let Ok(part) = part_file(dest) else {
        return false;
    };
    let made = crate::children::sessionless(&mut command)
        .arg("--out")
        .arg(&part)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok_and(|mut child| {
            let start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => return status.success(),
                    Ok(None) if start.elapsed() < timeout => {
                        #[expect(clippy::disallowed_methods, reason = "no-timed-waits baseline: looks again for its child's end, where a wait on the child under a deadline is the event")]
                        std::thread::sleep(
                            Duration::from_millis(10).min(timeout - start.elapsed().min(timeout)),
                        );
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return false;
                    }
                }
            }
        });
    if made
        && std::fs::metadata(&part).is_ok_and(|meta| meta.len() > 0)
        && std::fs::rename(&part, dest).is_ok()
    {
        return true;
    }
    let _ = std::fs::remove_file(&part);
    false
}

#[cfg(test)]
#[path = "images/conversion_tests.rs"]
mod conversion_tests;

/// Names a picture as it is now: its path, change time and size, and the width of its thumbnail.
pub fn token(root: &Path, path: &Path, width: u32) -> String {
    let meta = std::fs::metadata(path).ok();
    let base = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let rel = path
        .strip_prefix(&base)
        .unwrap_or(&path)
        .to_string_lossy()
        .to_string();
    let key = format!(
        "{rel}|{}|{}|{width}",
        meta.as_ref().map_or(0, mtime_ns),
        meta.as_ref().map_or(0, std::fs::Metadata::len)
    );
    Sha256::digest(key.as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The thumbnail of the PNG at `rel` and whether it is the current one (`tok` is its token), or `None`
/// when `rel` is no picture of the project or the copy could not be made. `convert` makes a missing copy.
pub fn thumbnail(
    root: &Path,
    width: u32,
    tok: &str,
    rel: &str,
    convert: &dyn Fn(&Path, &Path, u32) -> bool,
    can_convert: bool,
) -> Option<(PathBuf, bool)> {
    let path = local(root, &format!("/{rel}"))?;
    let is_png = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));
    if !is_png || !path.is_file() || ![THUMB_WIDTH, THUMB_SMALL].contains(&width) {
        return None;
    }
    let current = token(root, &path, width);
    let made = root
        .canonicalize()
        .ok()?
        .join(crate::project::Project::of(root).artifact(THUMB_DIR))
        .join(format!("{current}.jpg"));
    if !made.is_file() {
        std::fs::create_dir_all(made.parent()?).ok()?;
        if !can_convert || !convert(&path, &made, width) {
            return None;
        }
    }
    Some((made, tok == current))
}

/// Python's `round` of a ratio: halves to the even number.
fn round_half_even(value: f64) -> u32 {
    let floor = value.floor();
    let rounded = match value - floor {
        diff if diff < 0.5 => floor,
        diff if diff > 0.5 => floor + 1.0,
        _ if floor % 2.0 == 0.0 => floor,
        _ => floor + 1.0,
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a picture's height in pixels"
    )]
    let pixels = rounded as u32;
    pixels
}

fn sized(tag: &str, root: &Path, lead: &mut bool, can_convert: bool) -> String {
    let Some(src) = SRC.captures(tag) else {
        return tag.to_string();
    };
    if HEIGHT.is_match(tag) {
        return tag.to_string();
    }
    let Some(path) = local(root, &src[1]) else {
        return tag.to_string();
    };
    let Some((width, height)) = file_size(&path) else {
        return tag.to_string();
    };
    let shown: Option<u32> = WIDTH.captures(tag).and_then(|w| w[1].parse().ok());
    let dims = match shown {
        Some(chosen) => format!(
            " height=\"{}\"",
            round_half_even(f64::from(chosen) * f64::from(height) / f64::from(width)).max(1)
        ),
        None => format!(" width=\"{width}\" height=\"{height}\""),
    };
    let mut extra = if std::mem::take(lead) {
        // the page's first picture: what the reader waits for, so it goes first
        " fetchpriority=\"high\"".to_string()
    } else if tag.contains("loading=") {
        String::new()
    } else {
        " loading=\"lazy\"".to_string()
    };
    if !tag.contains("decoding=") {
        extra.push_str(" decoding=\"async\"");
    }
    // the markup chose a width: keep it and the picture's ratio
    let mut tag = format!(
        "{}{dims}{extra}>",
        tag[..tag.len() - 1].trim_end_matches(['/', ' '])
    );
    let big = std::fs::metadata(&path)
        .is_ok_and(|meta| usize::try_from(meta.len()).is_ok_and(|len| len >= THUMB_MIN_BYTES));
    if big && can_convert {
        let at = if shown.is_some_and(|w| w <= SMALL_SHOWN) {
            THUMB_SMALL
        } else {
            THUMB_WIDTH
        };
        let rel = path
            .strip_prefix(root.canonicalize().unwrap_or_default())
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        let small = format!("{THUMB_ROUTE}{at}/{}/{rel}.jpg", token(root, &path, at));
        tag = SRC
            .replace(&tag, |found: &Captures<'_>| {
                format!("src=\"{}\" data-full=\"{}\"", quote(&small, "/"), &found[1])
            })
            .into_owned();
    }
    tag
}

/// `markup` with a width and height on every local picture that lacks them; with `page`, the first of
/// them is fetched at high priority and the others load as they near the screen. `can_convert` is
/// whether thumbnails can be made on this machine ([`sips_available`]).
pub fn reserve(markup: &str, root: &Path, page: bool, can_convert: bool) -> String {
    let mut lead = page;
    IMG.replace_all(markup, |found: &Captures<'_>| {
        sized(&found[0], root, &mut lead, can_convert)
    })
    .into_owned()
}
