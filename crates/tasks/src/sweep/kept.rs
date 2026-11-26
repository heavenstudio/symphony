//! What the main checkout keeps of removed worktrees, and its cap. The sweep sets a finished
//! worktree's artifacts aside in the main checkout's `FOLDER/swept/NAME/` ([`super::aside`]), its
//! build output never among them, and removes nothing from there, ever: past the cap
//! (`[board] swept_cap_gb` in the main checkout's `symphony.toml`, [`CAP_GB`] when it names none)
//! it still sets aside, and asks for a check by hand instead. Each sweep that may set aside
//! measures the folder and keeps the measure in its record (`symphony/tasks/swept.json` in the git
//! common dir, [`Record`]): where each folder there came from, by the tasks tied to its worktree
//! when it was set aside, and the largest with each task's stage as the sweep read it. Past the cap
//! the measure also says, for each of the largest, what still references it, so a check by hand
//! keeps it (an artifact something still actively references is kept until the space it uses
//! becomes a problem that requires removing it): a task tied to its worktree that is not done or
//! cancelled, a post on an open task's thread or a document of `docs/` on the default branch
//! naming a path under it ([`Measured::look_up`], read once for that measure). Those are marked in use; the rest, no reference found, are the ones
//! suggested, and none is removed. The sweep's own lines say the folder past its cap (a dry run
//! says it under it too), and `tasks status` warns of it from the record while it stays past it,
//! without a board read, saying that the warning stands until a sweep measures it again (a check by
//! hand clears it only then).

use super::size;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The record's file, in the sweep's folder of the git common dir ([`super::trigger::folder`]).
pub const RECORD: &str = "swept.json";
/// The `[board]` key of the project file naming the cap, in gigabytes.
pub const CAP_KEY: &str = "swept_cap_gb";
/// The cap when the project names none, in gigabytes.
pub const CAP_GB: f64 = 5.0;
/// A gigabyte, as the cap counts it.
const GB: f64 = 1e9;
/// How many of the largest folders a warning names.
pub const LARGEST: usize = 5;
/// How many of the places referencing a folder a warning names.
const PLACES: usize = 3;

/// The sweep's record of what the main checkout keeps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The tasks tied to the worktree each folder was set aside from, by its name in the kept
    /// folder.
    #[serde(default)]
    pub from: BTreeMap<String, Vec<String>>,
    /// The kept folder as a sweep last measured it.
    #[serde(default)]
    pub measured: Option<Measured>,
}

/// The kept folder, measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    pub folder: PathBuf,
    pub bytes: u64,
    /// The cap it was measured against, in gigabytes.
    pub cap_gb: f64,
    /// When, in seconds since the epoch.
    pub at: f64,
    /// Its largest folders, the largest first, at most [`LARGEST`].
    pub largest: Vec<Largest>,
    /// What could not be searched for references to them, and why: while any, none is suggested.
    #[serde(default)]
    pub unsearched: Vec<String>,
}

/// One folder of the kept folder: a worktree's artifacts, set aside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Largest {
    pub name: String,
    pub bytes: u64,
    /// The tasks tied to its worktree when it was set aside, each with its stage when measured
    /// (none: not on the board); none recorded for a folder the record does not know.
    pub tasks: Vec<(String, Option<String>)>,
    /// What still references it, past the cap ([`Measured::look_up`]): none, no reference found.
    #[serde(default)]
    pub uses: Vec<String>,
}

/// The record's path for the git common dir `common`.
pub fn path(common: &Path) -> PathBuf {
    super::trigger::folder(common).join(RECORD)
}

impl Record {
    /// The record at `path`: an empty one when there is none or it cannot be read, as the next
    /// sweep measures again.
    pub fn read(path: &Path) -> Record {
        std::fs::read_to_string(path)
            .ok()
            .and_then(crate::objects::parse)
            .unwrap_or_default()
    }

    /// Write it to `path` whole: to a file of this process's own, then renamed over it.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staged = path.with_extension(format!("json.{}", std::process::id()));
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&staged, text)?;
        std::fs::rename(&staged, path)
    }
}

/// The cap the project file at the main checkout `root` names (`[board] swept_cap_gb`), else
/// [`CAP_GB`]; refused, naming the file, when it is no number of gigabytes over 0 or the file does
/// not parse. Only that key is read; the rest of the file is the Symphony's.
pub fn cap(root: &Path) -> Result<f64, String> {
    let path = root.join(crate::project::FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(CAP_GB),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let table: toml::Table = text
        .parse()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let Some(value) = table.get("board").and_then(|board| board.get(CAP_KEY)) else {
        return Ok(CAP_GB);
    };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a cap of whole gigabytes is read as a number of them"
    )]
    let (gb, written) = match value {
        toml::Value::Integer(gb) => (*gb as f64, gb.to_string()),
        toml::Value::Float(gb) => (*gb, gb.to_string()),
        toml::Value::String(text) => (f64::NAN, format!("{text:?}")),
        other => (f64::NAN, format!("a {}", other.type_str())),
    };
    if gb > 0.0 && gb.is_finite() {
        Ok(gb)
    } else {
        Err(format!(
            "{}: [board] {CAP_KEY} is {written}, not a number of gigabytes over 0",
            path.display()
        ))
    }
}

/// The kept folder `folder` measured at `at` against `cap_gb`: what it holds, and its largest
/// folders with the tasks `from` records for each and their stage, as `stage` reads it.
pub fn measure(
    folder: &Path,
    cap_gb: f64,
    from: &BTreeMap<String, Vec<String>>,
    stage: &dyn Fn(&str) -> Option<String>,
    at: f64,
) -> Measured {
    let mut largest: Vec<Largest> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let tasks = from
                        .get(&name)
                        .map(|slugs| {
                            slugs
                                .iter()
                                .map(|slug| (slug.clone(), stage(slug)))
                                .collect()
                        })
                        .unwrap_or_default();
                    Largest {
                        bytes: size::bytes(&entry.path()),
                        name,
                        tasks,
                        uses: Vec::new(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let bytes = largest.iter().map(|l| l.bytes).sum();
    #[expect(
        clippy::disallowed_methods,
        reason = "a busy board path keeps std's in-place sort (crate::order): the sweep"
    )]
    largest.sort_unstable_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    largest.truncate(LARGEST);
    Measured {
        folder: folder.to_path_buf(),
        bytes,
        cap_gb,
        at,
        largest,
        unsearched: Vec::new(),
    }
}

/// The paths `text` names under the artifacts folder `folder`, each as written after `FOLDER/`:
/// `FOLDER` starting a path or a part of one, what follows up to the first character no path is
/// written with, less a sentence's closing stop.
fn named_under<'t>(text: &'t str, folder: &str) -> Vec<&'t str> {
    let path_char = |c: char| c.is_ascii_alphanumeric() || "._/@%+~-".contains(c);
    let needle = format!("{folder}/");
    text.match_indices(&needle)
        .filter(|(at, _)| {
            !text[..*at]
                .chars()
                .next_back()
                .is_some_and(|c| c != '/' && path_char(c))
        })
        .filter_map(|(at, _)| {
            let rest = &text[at + needle.len()..];
            let end = rest.find(|c: char| !path_char(c)).unwrap_or(rest.len());
            let path = rest[..end]
                .trim_end_matches(['.', '/'])
                .trim_start_matches('/');
            (!path.is_empty()).then_some(path)
        })
        .collect()
}

/// Whether `path`, as written under the artifacts folder, names the kept folder `name` at `kept`:
/// a path under `swept/NAME`, or one its worktree's artifacts folder held that it holds now.
fn names(path: &str, name: &str, kept: &Path) -> bool {
    let swept = path
        .strip_prefix(super::aside::SWEPT)
        .and_then(|rest| rest.strip_prefix('/'));
    if swept.is_some_and(|rest| rest == name || rest.starts_with(&format!("{name}/"))) {
        return true;
    }
    !path.split('/').any(|part| part == "..") && std::fs::symlink_metadata(kept.join(path)).is_ok()
}

impl Measured {
    /// Mark each of its largest folders with what still references it, so a check by hand keeps it:
    /// a task tied to its worktree that is not done or cancelled, and each place of `said` (a post
    /// on an open task's thread, a document on the default branch, with the text said there) naming
    /// a path under it as written under the artifacts folder `folder`. `unsearched` is what could
    /// not be read for it, and why.
    pub fn look_up(&mut self, folder: &str, said: &[(String, String)], unsearched: Vec<String>) {
        let named: Vec<(&str, &str)> = said
            .iter()
            .flat_map(|(place, text)| {
                named_under(text, folder)
                    .into_iter()
                    .map(move |path| (place.as_str(), path))
            })
            .collect();
        for largest in &mut self.largest {
            let mut uses: Vec<String> = largest
                .tasks
                .iter()
                .filter(|(_, stage)| {
                    stage
                        .as_deref()
                        .is_some_and(|stage| !crate::health::FINISHED.contains(&stage))
                })
                .map(|(slug, _)| format!("task {slug} is open"))
                .collect();
            let kept = self.folder.join(&largest.name);
            for (place, path) in &named {
                if !uses.iter().any(|used| used == place) && names(path, &largest.name, &kept) {
                    uses.push((*place).to_string());
                }
            }
            largest.uses = uses;
        }
        self.unsearched = unsearched;
    }

    /// Whether it holds more than its cap.
    pub fn over(&self) -> bool {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a folder's bytes are compared in gigabytes"
        )]
        let gb = self.bytes as f64 / GB;
        gb > self.cap_gb
    }

    /// Its cap, as the lines say it.
    fn cap(&self) -> String {
        format!("{} GB cap ([board] {CAP_KEY})", self.cap_gb)
    }

    /// The sweep's line of it: past its cap, the check it asks for and its largest folders; under
    /// it (a dry run says that too), what it holds.
    pub fn line(&self) -> String {
        let head = format!(
            "set aside in {}: {}",
            self.folder.display(),
            size::shown(self.bytes)
        );
        if self.over() {
            format!("{head}, over its {}: {}", self.cap(), self.ask())
        } else {
            format!(
                "{head}, under its {}, the evidence of removed worktrees, which the sweep never removes",
                self.cap()
            )
        }
    }

    /// What `tasks status` warns of it, measured `ago` before, while it is past its cap: a warning
    /// that stands until a sweep measures it again, a check by hand since included, which it says.
    pub fn warning(&self, ago: &str) -> String {
        format!(
            "the artifacts set aside in {} hold {}, over their {}, as the sweep measured them {ago} ago, \
             said until a sweep measures them again (`just tasks sweep`, or the one a task's finish \
             starts; a dry run records nothing): {}",
            self.folder.display(),
            size::shown(self.bytes),
            self.cap(),
            self.ask()
        )
    }

    /// The check it asks for: its largest folders, each with its tasks and what still references
    /// it, and the ones suggested, which nothing found references.
    fn ask(&self) -> String {
        let largest: Vec<String> = self
            .largest
            .iter()
            .map(|folder| {
                let tasks = match folder.tasks.as_slice() {
                    [] => "no task recorded".to_string(),
                    [(slug, status)] => format!("task {slug}, {}", stage(status.as_deref())),
                    many => format!(
                        "tasks {}",
                        many.iter()
                            .map(|(slug, status)| format!("{slug}, {}", stage(status.as_deref())))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ),
                };
                let uses = match folder.uses.as_slice() {
                    [] => "no reference found".to_string(),
                    uses if uses.len() > PLACES => format!(
                        "in use: {} and {} more",
                        uses[..PLACES].join(", "),
                        uses.len() - PLACES
                    ),
                    uses => format!("in use: {}", uses.join(", ")),
                };
                format!(
                    "{} {} ({tasks}; {uses})",
                    folder.name,
                    size::shown(folder.bytes)
                )
            })
            .collect();
        let unused: Vec<&str> = self
            .largest
            .iter()
            .filter(|folder| folder.uses.is_empty())
            .map(|folder| folder.name.as_str())
            .collect();
        let suggested = if !self.unsearched.is_empty() {
            format!("none suggested, as {}", self.unsearched.join("; "))
        } else if unused.is_empty() {
            "none suggested, as each is in use".to_string()
        } else {
            format!(
                "suggested, as nothing found references them: {}",
                unused.join(", ")
            )
        };
        format!(
            "the sweep never removes any, so check by hand and remove the stale images and artifacts no longer in use; the largest: {}; {suggested}",
            largest.join(", ")
        )
    }
}

/// A task's stage as a line says it.
fn stage(status: Option<&str>) -> &str {
    status.unwrap_or("not on the board")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("symphony-sweep-kept-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, path: &str, bytes: usize) {
        let at = dir.join(path);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn the_cap_is_the_projects_else_five_gigabytes() {
        let root = scratch("cap");
        assert_eq!(cap(&root), Ok(CAP_GB), "no project file");
        for (text, gb) in [
            ("[project]\nid = \"x\"\n", CAP_GB),
            ("[board]\non_review = []\n", CAP_GB),
            ("[board]\nswept_cap_gb = 12\n", 12.0),
            ("[board]\nswept_cap_gb = 0.5\n", 0.5),
        ] {
            fs::write(root.join("symphony.toml"), text).unwrap();
            assert_eq!(cap(&root), Ok(gb), "{text}");
        }
        for (text, said) in [
            (
                "[board]\nswept_cap_gb = 0\n",
                "is 0, not a number of gigabytes over 0",
            ),
            ("[board]\nswept_cap_gb = -3\n", "is -3, not a number"),
            (
                "[board]\nswept_cap_gb = \"lots\"\n",
                "is \"lots\", not a number",
            ),
            ("[board\n", ""),
        ] {
            fs::write(root.join("symphony.toml"), text).unwrap();
            let why = cap(&root).unwrap_err();
            assert!(
                why.starts_with(&root.join("symphony.toml").display().to_string())
                    && why.contains(said),
                "{text}: {why}"
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_measure_names_the_largest_with_their_tasks_and_stages() {
        let folder = scratch("measure");
        write(&folder, "agent-a/tools-a/shot.png", 3000);
        write(&folder, "agent-b/shot.png", 2000);
        write(&folder, "by-hand/notes.txt", 1000);
        for n in 0..6 {
            write(&folder, &format!("small-{n}/x"), 10);
        }
        let from = BTreeMap::from([
            ("agent-a".to_string(), vec!["tools/a".to_string()]),
            (
                "agent-b".to_string(),
                vec!["tools/b".to_string(), "tools/gone".to_string()],
            ),
        ]);
        let stage = |slug: &str| (slug != "tools/gone").then(|| "done".to_string());
        let measured = measure(&folder, 0.000_005, &from, &stage, 1.8e9);
        assert_eq!(measured.bytes, size::bytes(&folder));
        assert_eq!(measured.largest.len(), LARGEST);
        assert_eq!(
            measured
                .largest
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>(),
            ["agent-a", "agent-b", "by-hand", "small-0", "small-1"]
        );
        assert!(measured.over());
        let line = measured.line();
        assert!(
            line.starts_with(&format!(
                "set aside in {}: {}, over its 0.000005 GB cap ([board] swept_cap_gb): the sweep never \
                 removes any, so check by hand and remove the stale images and artifacts no longer \
                 in use; the largest: agent-a ",
                folder.display(),
                size::shown(measured.bytes)
            )),
            "{line}"
        );
        assert!(
            line.contains("(task tools/a, done; no reference found), agent-b ")
                && line.contains(
                    "(tasks tools/b, done; tools/gone, not on the board; no reference found), \
                     by-hand "
                )
                && line.contains("(no task recorded; no reference found), small-0 ")
                && line.ends_with(
                    "; suggested, as nothing found references them: agent-a, agent-b, by-hand, \
                     small-0, small-1"
                ),
            "{line}"
        );
        assert!(
            measured.warning("5 min").contains(
                ", as the sweep measured them 5 min ago, said until a sweep measures them again \
                 (`just tasks sweep`, or the one a task's finish starts; a dry run records \
                 nothing): the sweep never removes any"
            ),
            "{}",
            measured.warning("5 min")
        );

        let under = measure(&folder, CAP_GB, &from, &stage, 1.8e9);
        assert!(!under.over());
        assert_eq!(
            under.line(),
            format!(
                "set aside in {}: {}, under its 5 GB cap ([board] swept_cap_gb), the evidence of \
                 removed worktrees, which the sweep never removes",
                folder.display(),
                size::shown(under.bytes)
            )
        );
        let none = measure(&folder.join("absent"), CAP_GB, &from, &stage, 1.8e9);
        assert_eq!((none.bytes, none.largest.len()), (0, 0));
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_path_named_under_the_artifacts_folder_is_read_as_written_after_it() {
        assert_eq!(
            named_under(
                "artifacts/a/b.png, and `artifacts/c` (artifacts/d/) [e](../artifacts/e/f.png).",
                "artifacts"
            ),
            ["a/b.png", "c", "d", "e/f.png"]
        );
        assert_eq!(
            named_under("/wt/agent-x/artifacts/swept/agent-y/g.png.", "artifacts"),
            ["swept/agent-y/g.png"]
        );
        for text in [
            "my-artifacts/g",
            "artifacts/ alone",
            "artifacts",
            "",
            "artifacts/.",
        ] {
            assert!(named_under(text, "artifacts").is_empty(), "{text}");
        }
        assert_eq!(named_under("out/evidence/h", "out/evidence"), ["h"]);
        assert!(named_under("about/evidence/h", "out/evidence").is_empty());
    }

    #[test]
    fn what_still_references_a_kept_folder_keeps_it_out_of_the_suggested() {
        let folder = scratch("uses");
        write(&folder, "agent-a/tools-a/shot.png", 4000);
        write(&folder, "agent-b/x.png", 3000);
        write(&folder, "agent-c/notes.txt", 2000);
        write(&folder, "agent-d/y", 1000);
        let from: BTreeMap<String, Vec<String>> = ["a", "b", "c", "d"]
            .map(|n| (format!("agent-{n}"), vec![format!("tools/{n}")]))
            .into();
        let stage = |slug: &str| {
            Some(match slug {
                "tools/a" => "review".to_string(),
                "tools/d" => "cancelled".to_string(),
                _ => "done".to_string(),
            })
        };
        let said = |place: &str, text: &str| (place.to_string(), text.to_string());
        let mentions = [
            said(
                "the thread of tools/x",
                "the picture: artifacts/swept/agent-b/x.png.",
            ),
            // What the worktree's own artifacts folder held, as its thread named it then.
            said("docs/DECISIONS.md", "![c](../artifacts/notes.txt)"),
            // Neither names the folder agent-d.
            said("docs/A.md", "my-artifacts/y and artifacts/../agent-d/y"),
            said("docs/B.md", "artifacts/swept/agent-dx/y"),
        ];
        let mut measured = measure(&folder, 0.000_001, &from, &stage, 1.8e9);
        measured.look_up("artifacts", &mentions, Vec::new());
        let uses: Vec<(&str, Vec<&str>)> = measured
            .largest
            .iter()
            .map(|l| (l.name.as_str(), l.uses.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(
            uses,
            [
                ("agent-a", vec!["task tools/a is open"]),
                ("agent-b", vec!["the thread of tools/x"]),
                ("agent-c", vec!["docs/DECISIONS.md"]),
                ("agent-d", vec![]),
            ]
        );
        let line = measured.line();
        let shown = |name: &str| size::shown(size::bytes(&folder.join(name)));
        assert!(
            line.ends_with(&format!(
                "; the largest: agent-a {} (task tools/a, review; in use: task tools/a is open), \
                 agent-b {} (task tools/b, done; in use: the thread of tools/x), agent-c {} (task \
                 tools/c, done; in use: docs/DECISIONS.md), agent-d {} (task tools/d, cancelled; no \
                 reference found); suggested, as nothing found references them: agent-d",
                shown("agent-a"),
                shown("agent-b"),
                shown("agent-c"),
                shown("agent-d"),
            )),
            "{line}"
        );

        // Many places name it: the first few are said, and how many more.
        let many: Vec<(String, String)> = (1..=5)
            .map(|n| said(&format!("the thread of t/{n}"), "artifacts/x.png"))
            .chain([said("the thread of t/1", "again artifacts/x.png")])
            .collect();
        let mut measured = measure(&folder, 0.000_001, &BTreeMap::new(), &stage, 1.8e9);
        measured.look_up("artifacts", &many, Vec::new());
        assert!(
            measured.line().contains(&format!(
                "agent-b {} (no task recorded; in use: the thread of t/1, the thread of t/2, the \
                 thread of t/3 and 2 more)",
                shown("agent-b")
            )),
            "{}",
            measured.line()
        );

        // What could not be read suggests none; nor does each folder in use.
        let mut measured = measure(&folder, 0.000_001, &from, &stage, 1.8e9);
        measured.look_up(
            "artifacts",
            &mentions,
            vec!["the open tasks' threads could not be read: down".into()],
        );
        assert!(
            measured
                .line()
                .ends_with("; none suggested, as the open tasks' threads could not be read: down"),
            "{}",
            measured.line()
        );
        let all: Vec<(String, String)> = ["a", "b", "c", "d"]
            .map(|n| said("docs/ALL.md", &format!("artifacts/swept/agent-{n}")))
            .into();
        let mut measured = measure(&folder, 0.000_001, &from, &stage, 1.8e9);
        measured.look_up("artifacts", &all, Vec::new());
        assert!(
            measured
                .line()
                .ends_with("; none suggested, as each is in use"),
            "{}",
            measured.line()
        );
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_record_is_written_whole_and_read_back_or_empty() {
        let dir = scratch("record");
        let at = dir.join("symphony/tasks").join(RECORD);
        assert_eq!(Record::read(&at), Record::default(), "none yet");
        let record = Record {
            from: BTreeMap::from([("agent-a".to_string(), vec!["tools/a".to_string()])]),
            measured: Some(measure(&dir, CAP_GB, &BTreeMap::new(), &|_| None, 1.8e9)),
        };
        record.write(&at).unwrap();
        assert_eq!(Record::read(&at), record);
        assert_eq!(
            fs::read_dir(at.parent().unwrap()).unwrap().count(),
            1,
            "nothing staged is left"
        );
        fs::write(&at, "{").unwrap();
        assert_eq!(Record::read(&at), Record::default(), "garbled");
        fs::remove_dir_all(&dir).unwrap();
    }
}
