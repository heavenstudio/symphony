//! `symphony-build split-repair PROGRAM PUBLIC`: the output of `dx bundle --wasm-split` in PUBLIC
//! (its `index.html` and `assets/`) made safe and repeatable, in place, from PROGRAM, the program
//! dx linked for it with its relocations (`wasm32-unknown-unknown/wasm-release/symphony-ui.wasm` in
//! the build folder).
//!
//! dx 0.7.10's splitter (wasm-split-cli 0.7.10) has two faults, and this works around both:
//!
//! - it zeroes, in the main module, merged constants the main module reads: [`constants`] writes
//!   back every byte two data symbols lie over, and fails the run when the main module still reads
//!   zero there;
//! - it writes a chunk's data segments and table entries in the order of a hash map, so the same
//!   sources give other bytes on every build: [`order`] puts them in a fixed one, and joins those
//!   that touch or overlap, a segment a symbol or a function as the splitter writes them, into one.
//!
//! Each module is then named by its bytes (`STEM-HASH.wasm`, HASH the first 16 hex digits of their
//! SHA-256, as `package.sh` names a page kind's script), the scripts and `index.html` that name it
//! are rewritten to the new name, and each script so changed is named by its bytes too: two builds
//! of the same sources give the same files. The main module is the one module that defines its
//! memory; a chunk uses the main module's, imported from the splitter's `__wasm_split` (or none, as
//! the empty shared chunk wasm-opt leaves). It exits 0 with a line on each module and on each
//! script it renamed, 1 when a check fails, and 2 when it cannot read what it is given; it writes
//! nothing unless every check passed.
//!
//! The two faults are dx's to fix: the reports for its tracker are kept with the evidence of
//! `perf/client-shared-runtime/splitter-safe`. Once its splitter zeroes only bytes that no symbol
//! the main module keeps lies over, and writes a chunk's segments in a fixed order, this command
//! goes, with its call after `dx bundle --wasm-split` and its line in `symphony-build`'s listing.
#![forbid(unsafe_code)]

mod constants;
mod order;
mod wasm;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const USAGE: &str = "usage: symphony-build split-repair PROGRAM PUBLIC";

/// Why a run stops: an output it would not stand behind (exit 1), or input it cannot read (2).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Unsafe(String),
    Input(String),
}

impl Failure {
    /// The same failure, said of `what`.
    fn of(self, what: &str) -> Self {
        match self {
            Failure::Unsafe(why) => Failure::Unsafe(format!("{what}: {why}")),
            Failure::Input(why) => Failure::Input(format!("{what}: {why}")),
        }
    }
}

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let [program, public] = args else {
        let _ = writeln!(err, "{USAGE}");
        return 2;
    };
    match repair(Path::new(program), Path::new(public)) {
        Ok(report) => {
            let _ = out.write_all(report.as_bytes());
            0
        }
        Err(Failure::Unsafe(why)) => {
            let _ = writeln!(err, "symphony-build split-repair: {why}");
            1
        }
        Err(Failure::Input(why)) => {
            let _ = writeln!(err, "symphony-build split-repair: {why}");
            2
        }
    }
}

fn read(path: &Path) -> Result<Vec<u8>, Failure> {
    fs::read(path).map_err(|why| Failure::Input(format!("{}: {why}", path.display())))
}

/// A file of the output: its name in its folder, and its bytes.
#[derive(Clone)]
struct File {
    name: String,
    bytes: Vec<u8>,
}

/// What `dx bundle --wasm-split` wrote in a public folder.
struct Output {
    assets: PathBuf,
    main: File,
    chunks: Vec<File>,
    scripts: Vec<File>,
    page: File,
}

impl Output {
    fn read(public: &Path) -> Result<Self, Failure> {
        let assets = public.join("assets");
        let (mut main, mut chunks, mut scripts) = (None::<File>, Vec::new(), Vec::new());
        for name in names(&assets)? {
            let bytes = read(&assets.join(&name))?;
            if name.ends_with(".js") {
                scripts.push(File { name, bytes });
                continue;
            }
            if !name.ends_with(".wasm") {
                continue;
            }
            let defines = wasm::Module::read(&bytes)
                .and_then(|module| module.defines_memory())
                .map_err(|why| Failure::Input(format!("{name}: {why}")))?;
            if !defines {
                chunks.push(File { name, bytes });
            } else if let Some(first) = &main {
                return Err(Failure::Input(format!(
                    "{}: {} and {name} both define a memory, where one main module is expected",
                    assets.display(),
                    first.name
                )));
            } else {
                main = Some(File { name, bytes });
            }
        }
        let Some(main) = main else {
            return Err(Failure::Input(format!(
                "{}: no main module, the one that defines its memory",
                assets.display()
            )));
        };
        if chunks.is_empty() {
            return Err(Failure::Input(format!(
                "{}: no chunk, a module that defines no memory: it is not the output of dx bundle \
                 --wasm-split",
                assets.display()
            )));
        }
        let page = File {
            name: "index.html".into(),
            bytes: read(&public.join("index.html"))?,
        };
        Ok(Self {
            assets,
            main,
            chunks,
            scripts,
            page,
        })
    }
}

/// Repairs the split output in `public` from `program`: what it did, a line a file.
fn repair(program: &Path, public: &Path) -> Result<String, Failure> {
    let linked = constants::Program::read(&read(program)?)
        .map_err(|why| Failure::Input(format!("{}: {why}", program.display())))?;
    let output = Output::read(public)?;
    let mut report = String::new();
    // Each module repaired, then named by its bytes.
    let repaired =
        constants::repair(&linked, &output.main.bytes).map_err(|why| why.of(&output.main.name))?;
    let mut modules = vec![(
        "main module",
        output.main.clone(),
        repaired.bytes,
        format!(
            "{} merged constants written back, {} bytes",
            repaired.runs, repaired.written
        ),
    )];
    for chunk in &output.chunks {
        let ordered = order::order(&chunk.bytes).map_err(|why| why.of(&chunk.name))?;
        modules.push((
            "chunk",
            chunk.clone(),
            ordered.bytes,
            format!(
                "{} of its {} segments put in order, {} joined to the one before",
                ordered.moved, ordered.segments, ordered.joined
            ),
        ));
    }
    let mut scripts = output.scripts.clone();
    let mut page = output.page.clone();
    let (mut written, mut gone) = (Vec::new(), Vec::new());
    for (what, file, bytes, said) in modules {
        let new = named(&file.name, &bytes);
        let mut naming = 0;
        for script in &mut scripts {
            naming += rename_in(&mut script.bytes, &file.name, &new);
        }
        if naming == 0 {
            return Err(Failure::Input(format!(
                "{}: no script in {} names it",
                file.name,
                output.assets.display()
            )));
        }
        // The page may name a module too (a preload of the main one): it follows the new name.
        rename_in(&mut page.bytes, &file.name, &new);
        report.push_str(&format!("{what} {}: {said}\n", renaming(&file.name, &new)));
        if new != file.name {
            gone.push(file.name.clone());
        }
        if new != file.name || bytes != file.bytes {
            written.push(File { name: new, bytes });
        }
    }
    // Each script the new names changed, named by its bytes in the page.
    for (script, before) in scripts.iter().zip(&output.scripts) {
        if script.bytes == before.bytes {
            continue;
        }
        let new = named(&script.name, &script.bytes);
        if let Some(other) = scripts
            .iter()
            .find(|other| other.name != script.name && mentions(&other.bytes, &script.name) > 0)
        {
            return Err(Failure::Input(format!(
                "{}: it names the script {}, whose name changes with its bytes",
                other.name, script.name
            )));
        }
        if rename_in(&mut page.bytes, &script.name, &new) == 0 {
            return Err(Failure::Input(format!(
                "{}: {} does not name it",
                script.name, page.name
            )));
        }
        report.push_str(&format!("script {}\n", renaming(&script.name, &new)));
        if new != script.name {
            gone.push(script.name.clone());
        }
        written.push(File {
            name: new,
            bytes: script.bytes.clone(),
        });
    }

    // Every new file whole before anything is removed: a failure leaves each name it held.
    let write = |path: PathBuf, bytes: &[u8]| {
        fs::write(&path, bytes).map_err(|why| Failure::Input(format!("{}: {why}", path.display())))
    };
    for file in &written {
        write(output.assets.join(&file.name), &file.bytes)?;
    }
    if page.bytes != output.page.bytes {
        write(public.join(&page.name), &page.bytes)?;
    }
    for name in gone {
        let path = output.assets.join(name);
        fs::remove_file(&path)
            .map_err(|why| Failure::Input(format!("{}: {why}", path.display())))?;
    }
    Ok(report)
}

/// `OLD -> NEW`, or the name alone when it stays.
fn renaming(old: &str, new: &str) -> String {
    if old == new {
        old.to_string()
    } else {
        format!("{old} -> {new}")
    }
}

/// The names of the regular files in `folder`, in byte order.
fn names(folder: &Path) -> Result<Vec<String>, Failure> {
    let entries = fs::read_dir(folder)
        .map_err(|why| Failure::Input(format!("{}: {why}", folder.display())))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|why| Failure::Input(format!("{}: {why}", folder.display())))?;
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        if let (true, Some(name)) = (is_file, entry.file_name().to_str()) {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// `name` for a file of `bytes`: its stem, less the hash dx or an earlier run named it by, then the
/// first 16 hex digits of the bytes' SHA-256, then its extension.
fn named(name: &str, bytes: &[u8]) -> String {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let stem = match stem.rsplit_once('-') {
        Some((head, hash)) if is_hash(hash) => head,
        _ => stem,
    };
    let digest: String = Sha256::digest(bytes)[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if extension.is_empty() {
        format!("{stem}-{digest}")
    } else {
        format!("{stem}-{digest}.{extension}")
    }
}

/// A name's hash: dx's (`dxh` and hex digits) or this command's (16 hex digits).
fn is_hash(text: &str) -> bool {
    let hex = |digits: &str| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit());
    text.strip_prefix("dxh").is_some_and(hex) || (text.len() == 16 && hex(text))
}

/// Whether `text` names `name` whole at `at`: no other file name character before or after it.
fn names_at(text: &[u8], at: usize, name: &[u8]) -> bool {
    let part = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.');
    !name.is_empty()
        && text[at..].starts_with(name)
        && (at == 0 || !part(text[at - 1]))
        && text.get(at + name.len()).is_none_or(|&next| !part(next))
}

/// How many times `text` names `name` whole.
fn mentions(text: &[u8], name: &str) -> usize {
    (0..text.len())
        .filter(|&at| names_at(text, at, name.as_bytes()))
        .count()
}

/// `text` with every whole `old` made `new`: how many it made.
fn rename_in(text: &mut Vec<u8>, old: &str, new: &str) -> usize {
    let mut out = Vec::with_capacity(text.len());
    let (mut at, mut made) = (0, 0);
    while at < text.len() {
        if names_at(text, at, old.as_bytes()) {
            out.extend_from_slice(new.as_bytes());
            at += old.len();
            made += 1;
        } else {
            out.push(text[at]);
            at += 1;
        }
    }
    *text = out;
    made
}

#[cfg(test)]
mod tests;
