//! The Symphony's words stay its own: no file of this checkout, by its path or its text, holds a
//! word of a game's vocabulary ([`REFUSED`]). The Symphony is a generic tool, so its fixtures, tests
//! and documents use neutral words (a widget, a report, pictures). A consuming game's own check
//! reads every file of the Symphony it pins and refuses its game's words, so one written here turns
//! that check red at the consumer's next pin move.
//!
//! The files are the ones git lists (tracked, or added and not ignored), and each is read as that
//! check reads it: a word whole and in any case, any character but an ASCII letter or digit
//! parting words, so `-`, `_` and `/` do; a phrase's words one after another with its one parting
//! character between them; the path before the text, and a file holding a NUL byte by its path
//! alone.
//!
//! No word of the list is written in clear, as this file is one of those the scan reads, and so
//! would be any file holding the list: each entry is the first 16 hex digits of the SHA-256 of the
//! word in lower case (`printf %s WORD | shasum -a 256 | cut -c1-16`), and a phrase is its words'
//! entries parted by its character. A failure names each file and line, and the word as that file
//! writes it.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The refused words and phrases, each by its digest: the game vocabulary the Symphony's fixtures
/// and documents keep out, each word with its plural.
const REFUSED: [&str; 19] = [
    // The vessel, its frame and the list of them.
    "e5d5b971139eefeb",
    "ec5a5e086bbc5785",
    "fb4dac7b8936e9d7",
    "1c0451690dde92cc",
    "c6836f7ad1c4da8a",
    "fb76e4203fc88b6f",
    // The word for drawn and painted work, and its plural.
    "d3cec99112255db9",
    "907decfcc13a76fa",
    // The 3D model format, and the element that shows a model.
    "8bc31540bd104a36",
    "9372c470eeadd5ec-d35ca5051b82ffc3",
    // A side, its subdivisions and their plurals, then the names of five sides.
    "ff926d04864c233e",
    "797ff110d2a8c5c8",
    "d9985be68671894a",
    "bfb2529b4a4c4308",
    "5640f1bb0da68091",
    "a1cd94c4a9c0fa36",
    "d602bf8f470bc67c",
    "f6ec582c1001e4d3",
    "16188fca10e34d2a",
];

/// The characters a phrase's words may be parted by.
const PARTINGS: [u8; 3] = *b" -_";

/// The digest of `word`, in lower case: the first 8 bytes of its SHA-256, the 16 hex digits an
/// entry writes.
fn digest(word: &[u8]) -> u64 {
    let hash = Sha256::digest(word.to_ascii_lowercase());
    u64::from_be_bytes(hash[..8].try_into().unwrap())
}

/// The list, read: each word's digest, and each phrase as its words' digests with the character
/// between each two.
struct Refused {
    words: HashSet<u64>,
    phrases: Vec<(Vec<u64>, Vec<u8>)>,
}

impl Refused {
    /// Reads `entries`, refusing one that is not 16 lower-case hex digits to a word, or that parts
    /// its words by another character than [`PARTINGS`].
    fn read(entries: &[&str]) -> Result<Refused, String> {
        let mut refused = Refused {
            words: HashSet::new(),
            phrases: Vec::new(),
        };
        for entry in entries {
            let (mut parts, mut partings) = (Vec::new(), Vec::new());
            let mut rest = entry.as_bytes();
            loop {
                let (hex, after) = rest.split_at(rest.len().min(16));
                let hex_digit = |b: &u8| b.is_ascii_digit() || (b'a'..=b'f').contains(b);
                if hex.len() < 16 || !hex.iter().all(hex_digit) {
                    return Err(format!("{entry}: a word is 16 lower-case hex digits"));
                }
                let hex = std::str::from_utf8(hex).unwrap();
                parts.push(u64::from_str_radix(hex, 16).unwrap());
                match after.split_first() {
                    None => break,
                    Some((parting, after)) if PARTINGS.contains(parting) => {
                        partings.push(*parting);
                        rest = after;
                    }
                    Some(_) => {
                        return Err(format!(
                            "{entry}: a phrase parts its words by a space, `-` or `_`"
                        ));
                    }
                }
            }
            if parts.len() == 1 {
                refused.words.insert(parts[0]);
            } else {
                refused.phrases.push((parts, partings));
            }
        }
        Ok(refused)
    }

    /// Each refused word and phrase `text` holds, with the line it is on (1 for the first), as
    /// `text` writes it. `seen` keeps each word's digest, so each is hashed once per scan.
    fn found(&self, text: &[u8], seen: &mut HashMap<Vec<u8>, u64>) -> Vec<(usize, String)> {
        // Each word: where it starts and ends, its line, its digest.
        let mut words: Vec<(usize, usize, usize, u64)> = Vec::new();
        let (mut at, mut line) = (0, 1);
        while at < text.len() {
            if !text[at].is_ascii_alphanumeric() {
                line += usize::from(text[at] == b'\n');
                at += 1;
                continue;
            }
            let start = at;
            while at < text.len() && text[at].is_ascii_alphanumeric() {
                at += 1;
            }
            let word = &text[start..at];
            let hash = match seen.get(word) {
                Some(hash) => *hash,
                None => *seen.entry(word.to_vec()).or_insert_with(|| digest(word)),
            };
            words.push((start, at, line, hash));
        }
        let mut found = Vec::new();
        let written =
            |start: usize, end: usize| String::from_utf8_lossy(&text[start..end]).into_owned();
        for (i, &(start, end, line, hash)) in words.iter().enumerate() {
            if self.words.contains(&hash) {
                found.push((line, written(start, end)));
            }
            for (parts, partings) in &self.phrases {
                let Some(run) = words.get(i..i + parts.len()) else {
                    continue;
                };
                let follows = run.windows(2).zip(partings).all(|(pair, parting)| {
                    pair[1].0 == pair[0].1 + 1 && text[pair[0].1] == *parting
                });
                let same = run.iter().zip(parts).all(|(word, part)| word.3 == *part);
                if same && follows {
                    found.push((line, written(start, run[parts.len() - 1].1)));
                }
            }
        }
        found
    }
}

/// The Symphony's checkout.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Every file git lists in `root`, tracked or added, but none it ignores: NUL-parted, so a name
/// outside ASCII is not quoted.
fn listed(root: &Path) -> Vec<String> {
    let done = std::process::Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
    String::from_utf8(done.stdout)
        .unwrap()
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(String::from)
        .collect()
}

/// The text of `path` to read: none for a folder, a file holding a NUL byte, or one git lists that
/// the checkout no longer holds.
fn text(path: &Path) -> Vec<u8> {
    if path.is_dir() {
        return Vec::new();
    }
    match std::fs::read(path) {
        Ok(bytes) if bytes.contains(&0) => Vec::new(),
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("{}: {error}", path.display()),
    }
}

/// Each refused word in the files `paths` of `root`: `path: named WORD` for a path, `path:LINE:
/// WORD` for a text.
fn scan(refused: &Refused, root: &Path, paths: &[String]) -> Vec<String> {
    let mut seen = HashMap::new();
    let mut found = Vec::new();
    for path in paths {
        for (_, word) in refused.found(path.as_bytes(), &mut seen) {
            found.push(format!("{path}: named {word}"));
        }
        for (line, word) in refused.found(&text(&root.join(path)), &mut seen) {
            found.push(format!("{path}:{line}: {word}"));
        }
    }
    found
}

#[test]
fn no_file_of_the_checkout_holds_a_refused_word() {
    let refused = Refused::read(&REFUSED).unwrap();
    let root = workspace();
    let paths = listed(&root);
    // The listing holds the workspace and this file, so the scan reads its own list too.
    for path in ["Cargo.toml", "crates/symphony/tests/hooks/vocabulary.rs"] {
        assert!(paths.iter().any(|listed| listed == path), "{path}");
    }
    let found = scan(&refused, &root, &paths);
    assert!(
        found.is_empty(),
        "{} refused word(s): say it in the Symphony's own neutral words\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn the_list_reads_to_one_entry_a_word_or_phrase_and_none_twice() {
    let refused = Refused::read(&REFUSED).unwrap();
    assert_eq!(refused.words.len() + refused.phrases.len(), REFUSED.len());
    assert_eq!(refused.phrases.len(), 1);
    // An entry is what `shasum -a 256` prints for the word, cut to 16 digits.
    assert_eq!(digest(b"abc"), 0xba7816bf8f01cfea);
    assert_eq!(digest(b"ABC"), digest(b"abc"));
    for entry in [
        "BA7816BF8F01CFEA",
        "ba7816bf8f01cfe",
        "ba7816bf8f01cfeax",
        "",
    ] {
        assert!(Refused::read(&[entry]).is_err(), "{entry}");
    }
    let phrase = "ba7816bf8f01cfea/ba7816bf8f01cfea";
    assert!(Refused::read(&[phrase]).is_err());
    assert!(Refused::read(&["ba7816bf8f01cfea-"]).is_err());
}

/// The list of `words` and `phrases` in clear, as the tests below give it.
fn listing(words: &[&str], phrases: &[(&str, u8)]) -> Refused {
    let hex = |word: &str| format!("{:016x}", digest(word.as_bytes()));
    let mut entries: Vec<String> = words.iter().map(|word| hex(word)).collect();
    for (phrase, parting) in phrases {
        let parts: Vec<String> = phrase.split(*parting as char).map(hex).collect();
        entries.push(parts.join(&(*parting as char).to_string()));
    }
    let entries: Vec<&str> = entries.iter().map(String::as_str).collect();
    Refused::read(&entries).unwrap()
}

#[test]
fn a_word_is_found_whole_in_any_case_and_a_phrase_only_with_its_one_parting() {
    let refused = listing(
        &["lamp", "lamps"],
        &[("blue moon", b' '), ("tide-pool", b'-')],
    );
    let text = b"A Lamp.\nlamp_post and is-lamp, lamp/lamps\nlamplight, clamp, lamp2\n\
                 Blue Moon, blue  moon, blue-moon, blue\nmoon\ntide-pool tide pool TIDE-POOL\n\
                 caf\xc3\xa9lamp \xe9lamp\n";
    let found = refused.found(text, &mut HashMap::new());
    assert_eq!(
        found,
        [
            (1, "Lamp".into()),
            (2, "lamp".into()),
            (2, "lamp".into()),
            (2, "lamp".into()),
            (2, "lamps".into()),
            (4, "Blue Moon".into()),
            (6, "tide-pool".into()),
            (6, "TIDE-POOL".into()),
            (7, "lamp".into()),
            (7, "lamp".into()),
        ]
    );
    assert_eq!(refused.found(b"", &mut HashMap::new()), []);
    assert_eq!(
        refused.found(b"lamp", &mut HashMap::new()),
        [(1, "lamp".into())]
    );
}

#[test]
fn a_path_is_read_before_its_text_and_a_file_with_no_text_by_its_path_alone() {
    let root = std::env::temp_dir().join(format!("symphony-vocabulary-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("lamp_room/folder")).unwrap();
    std::fs::write(root.join("lamp_room/notes.md"), "one\nthe LAMPS\n").unwrap();
    std::fs::write(root.join("lamp_room/picture.png"), b"\x89PNG\0lamp").unwrap();
    std::fs::write(root.join("plain.txt"), "a lampshade\n").unwrap();
    let paths: Vec<String> = [
        "lamp_room/notes.md",
        "lamp_room/picture.png",
        "lamp_room/folder",
        "lamp_room/gone.md",
        "plain.txt",
    ]
    .map(String::from)
    .into();
    let found = scan(&listing(&["lamp", "lamps"], &[]), &root, &paths);
    assert_eq!(
        found,
        [
            "lamp_room/notes.md: named lamp",
            "lamp_room/notes.md:2: LAMPS",
            "lamp_room/picture.png: named lamp",
            "lamp_room/folder: named lamp",
            "lamp_room/gone.md: named lamp",
        ]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_listing_holds_files_added_and_not_ignored_and_a_name_outside_ascii_as_it_is() {
    let root = std::env::temp_dir().join(format!("symphony-vocabulary-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let done = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(done.status.success(), "{args:?} {done:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(root.join(".gitignore"), "/out/\n").unwrap();
    std::fs::write(root.join("café.md"), "x\n").unwrap();
    std::fs::create_dir_all(root.join("out")).unwrap();
    std::fs::write(root.join("out/lamp.md"), "lamp\n").unwrap();
    std::fs::write(root.join("tracked.md"), "y\n").unwrap();
    git(&["add", "tracked.md"]);
    let mut names = listed(&root);
    names.sort();
    assert_eq!(names, [".gitignore", "café.md", "tracked.md"]);
    std::fs::remove_dir_all(&root).unwrap();
}
