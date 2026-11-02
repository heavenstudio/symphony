//! The reader of the screen catalogue and its manifest, the one Symphony's
//! `[[screen:ID]]` and the nightly's stale-screen report link.
//!
//! The catalogue is `docs/data/screens.json` (schema: `docs/data/schemas/screens.schema.json`): one
//! entry per menu or HUD state, with the film script that reaches it. A script step is a list of ui key
//! names (`["enter"]`, one menu frame), a film line as written (`{"dock": true}`) or `{"hold": N}`, N
//! frames with nothing pressed (the sim runs on); `{"hold": N, "with": {"fire": true}}` holds that line
//! instead. `just screens` films each entry through `tools/film.gd` into `artifacts/screens/`, with a
//! `manifest.json` naming each picture's hash, commit and date. `symphony screens --check` checks the
//! catalogue against its schema and against what the schema cannot say ([`problems`]).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::cli::{Flag, parse, refuse};
use crate::schema;
use crate::text::{os_error, read_text, repr};

/// The catalogue, under the project root.
pub const CATALOGUE: &str = "docs/data/screens.json";
/// Its schema.
pub const SCHEMA: &str = "docs/data/schemas/screens.schema.json";
/// Where `just screens` puts the latest set (gitignored: pictures never reach main).
pub const OUT: &str = "artifacts/screens";
/// The set's manifest, in [`OUT`].
pub const MANIFEST: &str = "manifest.json";
/// The size every screen is filmed at.
pub const RESOLUTION: &str = "1280x800";
/// The router whose `named_keys` a script's keys must be.
const ROUTER: &str = "view/ui_router.gd";

fn id_pattern() -> &'static Regex {
    static ID: OnceLock<Regex> = OnceLock::new();
    // Python's `$` also takes a final newline.
    ID.get_or_init(|| Regex::new(r"^[a-z][a-z0-9]*(-[a-z0-9]+)*\n?\z").expect("the id pattern"))
}

fn quoted_key() -> &'static Regex {
    static KEY: OnceLock<Regex> = OnceLock::new();
    KEY.get_or_init(|| Regex::new(r#""([a-z0-9-]+)""#).expect("the key pattern"))
}

/// The catalogue under `root`, as its file holds it.
pub fn load_catalogue(root: &Path) -> Result<Value, String> {
    let path = root.join(CATALOGUE);
    let text = read_text(&path).map_err(|e| os_error(&e, &path))?;
    serde_json::from_str(&text).map_err(|e| format!("{CATALOGUE}: {e}"))
}

/// The ui key names the router takes (`UiRouter.named_keys`), read from its source under `root`.
pub fn known_keys(root: &Path) -> Result<BTreeSet<String>, String> {
    let path = root.join(ROUTER);
    let source = read_text(&path).map_err(|e| os_error(&e, &path))?;
    let Some((_, after)) = source.split_once("static func named_keys") else {
        return Err(format!("{ROUTER} has no named_keys"));
    };
    let body = after.split("\n\n\n").next().unwrap_or("");
    Ok(quoted_key()
        .captures_iter(body)
        .map(|c| c[1].to_string())
        .collect())
}

/// A value as Python's `str()` writes it, for a name in a problem.
fn py_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::Bool(false)) => "False".to_string(),
        Some(other) => other.to_string(),
    }
}

/// A hold's count as Python's `isinstance(n, int)` takes one (a boolean is one), else none.
fn count(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(flag) => Some(i64::from(*flag)),
        Value::Number(number) => number.as_i64(),
        _ => None,
    }
}

/// Everything wrong with a catalogue beyond what its schema says: the resolution, an id that is not a
/// slug or is there twice, a script with no steps, a key the router does not name, a hold that is not
/// a count, a step that is neither keys nor a film line.
pub fn problems(catalogue: &Value, root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let resolution = catalogue.get("resolution");
    if resolution.and_then(Value::as_str) != Some(RESOLUTION) {
        let shown = match resolution {
            Some(Value::String(text)) => repr(text),
            other => py_str(other),
        };
        out.push(format!(
            "resolution is {shown}, every screen is filmed at {RESOLUTION}"
        ));
    }
    // Without the router's keys no key can be checked: that is the one problem, not one per key.
    let keys = known_keys(root)
        .map_err(|problem| out.push(format!("the router's keys cannot be read: {problem}")))
        .ok();
    let mut seen = BTreeSet::new();
    let entries = catalogue.get("screens").and_then(Value::as_array);
    for entry in entries.into_iter().flatten() {
        let name = py_str(entry.get("id"));
        if !id_pattern().is_match(&name) {
            out.push(format!(
                "{name}: an id is lowercase words joined by hyphens"
            ));
        }
        if !seen.insert(name.clone()) {
            out.push(format!("{name}: appears twice"));
        }
        let steps = match entry.get("script") {
            Some(Value::Array(steps)) if !steps.is_empty() => steps,
            _ => {
                out.push(format!("{name}: a script with no steps reaches nothing"));
                continue;
            }
        };
        for (index, step) in steps.iter().enumerate() {
            match step {
                Value::Array(pressed) => {
                    let Some(keys) = &keys else { continue };
                    for key in pressed {
                        if !key.as_str().is_some_and(|k| keys.contains(k)) {
                            out.push(format!(
                                "{name}: step {index} presses {}, which the router does not name",
                                py_str(Some(key))
                            ));
                        }
                    }
                }
                Value::Object(line) => {
                    if let Some(hold) = line.get("hold") {
                        let only_hold = line.keys().all(|k| k == "hold" || k == "with");
                        if !only_hold || count(hold).is_none_or(|n| n < 1) {
                            out.push(format!(
                                "{name}: step {index}: hold is a count of at least 1, with at most a `with` line"
                            ));
                        }
                    }
                }
                _ => out.push(format!(
                    "{name}: step {index} is neither a list of keys nor a film line"
                )),
            }
        }
    }
    out
}

/// The film lines of one entry, one per frame. A step [`problems`] refuses gives none.
pub fn script_lines(entry: &Value) -> Vec<Value> {
    let mut lines = Vec::new();
    let steps = entry.get("script").and_then(Value::as_array);
    for step in steps.into_iter().flatten() {
        match step {
            Value::Array(keys) => lines.push(json!({"ui": keys})),
            Value::Object(line) => match line.get("hold") {
                Some(hold) => {
                    let held = match line.get("with") {
                        Some(Value::Object(with)) => Value::Object(with.clone()),
                        _ => json!({}),
                    };
                    let times = count(hold).unwrap_or(0).max(0);
                    for _ in 0..times {
                        lines.push(held.clone());
                    }
                }
                None => lines.push(Value::Object(line.clone())),
            },
            _ => {}
        }
    }
    lines
}

/// The input script `tools/film.gd` reads. Any `ui` line films the shell, so one is always there.
pub fn film_document(entry: &Value) -> Value {
    let mut lines = script_lines(entry);
    let shows_the_shell = lines.iter().any(|line| line.get("ui").is_some());
    if !shows_the_shell && let Some(Value::Object(first)) = lines.first_mut() {
        first.insert("ui".to_string(), json!([]));
    }
    json!({"version": 1, "inputs": lines})
}

/// TICKS for `film.gd`: in a shell film a tick is one script line.
pub fn film_ticks(entry: &Value) -> usize {
    script_lines(entry).len()
}

/// The manifest of the set in `out`, as written; none before there is one, or when it does not read
/// as an object. A file that exists but cannot be read, including a dangling symlink, is an error.
pub fn read_manifest(out: &Path) -> Result<Option<Map<String, Value>>, String> {
    let path = out.join(MANIFEST);
    let text = match read_text(&path) {
        Ok(text) => text,
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && matches!(std::fs::symlink_metadata(&path), Err(missing) if missing.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(os_error(&e, &path)),
    };
    Ok(match serde_json::from_str(&text) {
        Ok(Value::Object(manifest)) => Some(manifest),
        _ => None,
    })
}

const USAGE: &str = "usage: symphony screens [-h] [--root ROOT] --check";
const HELP: &str = "The screen catalogue: docs/data/screens.json, which `just screens` shoots.

options:
  -h, --help   show this help message and exit
  --root ROOT  project root holding docs/ and view/
  --check      validate the catalogue and shoot nothing";

/// Every line `--check` reports: the catalogue against its schema, then [`problems`].
fn check(root: &Path) -> Vec<String> {
    let catalogue = match load_catalogue(root) {
        Ok(catalogue) => catalogue,
        Err(problem) => return vec![problem],
    };
    let schema_path: PathBuf = root.join(SCHEMA);
    let schema = read_text(&schema_path)
        .map_err(|e| os_error(&e, &schema_path))
        .and_then(|text| {
            serde_json::from_str::<Value>(&text).map_err(|e| format!("{SCHEMA}: {e}"))
        });
    let name = Path::new(CATALOGUE)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut wrong =
        match schema.and_then(|schema| schema::validate(&catalogue, &schema).map_err(|e| e.0)) {
            Ok(errors) => errors.into_iter().map(|e| format!("{name} {e}")).collect(),
            Err(problem) => vec![problem],
        };
    wrong.extend(problems(&catalogue, root));
    wrong
}

/// `symphony screens --check [--root ROOT]`: the catalogue against its schema and [`problems`], each
/// wrong line on stderr; exit 1 when there is one.
pub fn check_main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let flags = [
        Flag {
            name: "--root",
            value: true,
        },
        Flag {
            name: "--check",
            value: false,
        },
    ];
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        Ok(parsed) if !parsed.positionals.is_empty() => {
            let problem = format!("unrecognized arguments: {}", parsed.positionals.join(" "));
            return refuse(err, USAGE, "screens", &problem);
        }
        Ok(parsed) if !parsed.flag("--check") => {
            let problem = "the following arguments are required: --check";
            return refuse(err, USAGE, "screens", problem);
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, USAGE, "screens", &problem),
    };
    let root = PathBuf::from(parsed.value("--root").unwrap_or("."));
    let wrong = check(&root);
    for line in &wrong {
        let _ = writeln!(err, "✖ screens: {line}");
    }
    i32::from(!wrong.is_empty())
}

#[cfg(test)]
mod after_manifest_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn an_existing_unreadable_manifest_is_an_error_including_a_dangling_symlink() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sf-manifest-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());
        std::fs::create_dir_all(&path).unwrap();
        std::os::unix::fs::symlink(path.join("missing"), path.join(MANIFEST)).unwrap();
        let error = read_manifest(&path).unwrap_err();
        assert!(error.contains("manifest.json"), "{error}");
        std::fs::remove_file(path.join(MANIFEST)).unwrap();
        std::fs::create_dir(path.join(MANIFEST)).unwrap();
        assert!(read_manifest(&path).unwrap_err().contains("manifest.json"));
    }
}
