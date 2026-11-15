//! The runs `synced` keeps: each one's measurements as one JSON record in the project's artifacts
//! folder (`synced/`, which no commit carries), and a run's figures printed beside an earlier
//! one's, in plain records with no threshold: a fix is read from the run before it and the run
//! after it, measured the same way, each with the load and memory it was taken under.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::machine::{Memory, load_said, mb};
use super::{Visit, median, ms, short};

#[cfg(test)]
#[path = "kept_tests.rs"]
mod tests;

/// The folder under the project's artifacts folder the records are kept in.
pub(super) const FOLDER: &str = "synced";

/// What a kept run says of itself, beside its pages.
pub(super) struct Run<'a> {
    pub(super) base: &'a str,
    pub(super) runs: usize,
    pub(super) tabs: usize,
    pub(super) note: Option<&'a str>,
    /// When it began: local `YYYY-MM-DD HH:MM:SS`, and seconds since the epoch.
    pub(super) at: String,
    pub(super) epoch: i64,
    /// The commit the checkout the run was made from stood at (not always the server's).
    pub(super) checkout: Option<String>,
    /// The load averages at its start and at its end.
    pub(super) load: [Option<[f64; 3]>; 2],
    /// The share of the machine's CPU busy over the whole run, in percent.
    pub(super) busy: Option<f64>,
}

/// The record of a run.
pub(super) fn record(run: &Run<'_>, measured: &[(String, Vec<Visit>)]) -> Value {
    let last = measured
        .iter()
        .rev()
        .find_map(|(_, visits)| visits.last())
        .map(|visit| visit.memory.clone())
        .unwrap_or_default();
    json!({
        "record": 1,
        "at": run.at,
        "epoch": run.epoch,
        "base": run.base,
        "runs": run.runs,
        "tabs": run.tabs,
        "note": run.note,
        "checkout": run.checkout,
        "load": {"start": run.load[0], "end": run.load[1]},
        "cpu_busy": run.busy,
        "memory": memory_json(&last),
        "pages": measured.iter().map(|(page, visits)| {
            let first = visits.iter().find(|visit| visit.run == 0);
            json!({
                "page": page,
                "build": visits.iter().find_map(|visit| visit.probe["build"].as_str()),
                "first": first.map(moments),
                "median": median(visits).map(|visit| {
                    let mut said = moments(visit);
                    said["run"] = json!(visit.run);
                    said["heap"] = json!(visit.memory.heap);
                    said
                }),
                "visits": visits.iter().map(as_json).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    })
}

/// A visit's drawn, checked and synced.
fn moments(visit: &Visit) -> Value {
    json!({"drawn": visit.drawn(), "checked": visit.checked(), "synced": visit.synced()})
}

fn memory_json(memory: &Memory) -> Value {
    json!({
        "heap": memory.heap,
        "server": memory.server,
        "databases": memory.databases,
        "database_count": memory.database_count,
    })
}

fn memory_of(value: &Value) -> Memory {
    Memory {
        heap: value["heap"].as_f64(),
        server: value["server"].as_f64(),
        databases: value["databases"].as_f64(),
        database_count: value["database_count"]
            .as_u64()
            .and_then(|count| usize::try_from(count).ok())
            .unwrap_or(0),
    }
}

/// A visit as a record holds it.
pub(super) fn as_json(visit: &Visit) -> Value {
    json!({
        "run": visit.run,
        "load": visit.load,
        "cpu_busy": visit.busy,
        "memory": memory_json(&visit.memory),
        "spent_ms": visit.spent.iter().map(|(name, value)| (name.clone(), json!(value))).collect::<serde_json::Map<_, _>>(),
        "error": visit.error,
        "probe": visit.probe,
    })
}

/// The file a run that began `at` (`YYYY-MM-DD HH:MM:SS`) is kept in: `YYYY-MM-DD-HHMMSS.json`.
pub(super) fn file_name(at: &str) -> String {
    format!("{}.json", at.replace(' ', "-").replace(':', ""))
}

/// The newest run of `base` kept in `folder`, by when it began; none when none is.
pub(super) fn earlier(folder: &Path, base: &str) -> Option<(PathBuf, Value)> {
    let entries = std::fs::read_dir(folder).ok()?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| {
            let record = read(&path).ok()?;
            (record["base"] == base).then_some((path, record))
        })
        .max_by_key(|(_, record)| record["epoch"].as_i64().unwrap_or(i64::MIN))
}

/// The kept run `name` names: a file, else one in `folder` by its name with or without `.json`.
pub(super) fn named(folder: &Path, name: &str) -> Result<(PathBuf, Value), String> {
    let given = Path::new(name);
    let path = [
        given.to_path_buf(),
        folder.join(name),
        folder.join(format!("{name}.json")),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .ok_or_else(|| {
        format!(
            "no kept run {name:?} (the runs are kept in {})",
            folder.display()
        )
    })?;
    let record = read(&path)?;
    Ok((path, record))
}

/// A kept run's record, read; refused when it is none.
fn read(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("reading {}: {error}", path.display()))?;
    let record: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is no kept run: {error}", path.display()))?;
    if record["record"].as_u64().is_none() {
        return Err(format!("{} is no kept run of synced", path.display()));
    }
    Ok(record)
}

/// The run `now` beside the kept run `then` (kept as `name`): the machine each was measured on,
/// then each page `now` measured, its median return's moments and its first visit's synced, then
/// and now, with how many times faster or slower its synced is, and its tab's heap.
pub(super) fn compare(name: &str, then: &Value, now: &Value) -> String {
    let mut said = String::new();
    let note = |record: &Value| {
        record["note"]
            .as_str()
            .map(|note| format!(" ({note})"))
            .unwrap_or_default()
    };
    let _ = writeln!(
        said,
        "\nbeside the run kept as {name}, made {}{}; this one {}{}",
        then["at"].as_str().unwrap_or("-"),
        note(then),
        now["at"].as_str().unwrap_or("-"),
        note(now)
    );
    let side = |label: &str, said_of: &dyn Fn(&Value) -> String| {
        format!(
            "  {label:<24} then {:<32} now {}\n",
            said_of(then),
            said_of(now)
        )
    };
    let load = |at: &'static str| {
        move |record: &Value| {
            let averages: Option<Vec<f64>> = record["load"][at]
                .as_array()
                .map(|all| all.iter().filter_map(Value::as_f64).collect());
            load_said(averages.and_then(|all| <[f64; 3]>::try_from(all).ok()))
        }
    };
    said.push_str(&side("at the start", &load("start")));
    said.push_str(&side("at the end", &load("end")));
    said.push_str(&side("CPU busy", &|record| {
        record["cpu_busy"]
            .as_f64()
            .map_or_else(|| "-".into(), |busy| format!("{busy:.0}%"))
    }));
    said.push_str(&side("memory at the end", &|record| {
        memory_of(&record["memory"]).machine_said()
    }));
    said.push_str(&side("other tabs", &|record| {
        record["tabs"]
            .as_u64()
            .map_or_else(|| "-".into(), |tabs| tabs.to_string())
    }));
    said.push_str(&side("checkout", &|record| {
        record["checkout"]
            .as_str()
            .map_or_else(|| "-".into(), |commit| commit.chars().take(10).collect())
    }));
    said.push_str(&side("client build", &|record| {
        record["pages"][0]["build"]
            .as_str()
            .map_or_else(|| "-".into(), short)
    }));
    let _ = writeln!(
        said,
        "  {:<32} {:<38} {:<14} {:<16} tab heap",
        "page", "median return drawn/checked/synced", "synced", "first visit"
    );
    for page in now["pages"].as_array().into_iter().flatten() {
        let named = page["page"].as_str().unwrap_or_default();
        let Some(before) = then["pages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|earlier| earlier["page"] == named)
        else {
            let _ = writeln!(said, "  {named:<32} not measured then");
            continue;
        };
        let three = |median: &Value| {
            format!(
                "{}/{}/{}",
                ms(median["drawn"].as_f64()),
                ms(median["checked"].as_f64()),
                ms(median["synced"].as_f64())
            )
        };
        let first = |page: &Value| ms(page["first"]["synced"].as_f64());
        let _ = writeln!(
            said,
            "  {named:<32} {:<38} {:<14} {:<16} {} → {} MB",
            format!("{} → {}", three(&before["median"]), three(&page["median"])),
            times(
                before["median"]["synced"].as_f64(),
                page["median"]["synced"].as_f64()
            ),
            format!("{} → {}", first(before), first(page)),
            mb(before["median"]["heap"].as_f64()),
            mb(page["median"]["heap"].as_f64()),
        );
    }
    said
}

/// How a time moved from `then` to `now`: `3.5x faster`, `1.2x slower`, `the same`, or `-` when
/// either is unknown.
pub(super) fn times(then: Option<f64>, now: Option<f64>) -> String {
    match (then, now) {
        (Some(then), Some(now)) if then > 0.0 && now > 0.0 => {
            if (then - now).abs() < f64::EPSILON {
                "the same".into()
            } else if then > now {
                format!("{:.1}x faster", then / now)
            } else {
                format!("{:.1}x slower", now / then)
            }
        }
        _ => "-".into(),
    }
}
