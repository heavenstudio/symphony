//! A release's GitHub Release (docs/workflow/RELEASES.md, "A GitHub Release"): asked of the command
//! `[releases] github` names, the GitHub adapter's `symphony-github release`, and of nothing
//! while the project names none. No package here builds with the adapter: it is run as a program,
//! `COMMAND… --request FILE`, in the main checkout, and answers one line of JSON.
//!
//! A request names a release tagged here, its commit, whether it is the newest, and its body: the
//! release's own notes, as `symphony release-notes` prints them from the release before it to it
//! ([`body`]). The adapter looks for the release by its tag before it makes one, so asking twice
//! makes none twice; what each release was last answered is kept in [`LEDGER`] beside the runs'
//! records, where a release not published stays `pending` with why until a later ask publishes
//! it. A run asks for its own release and each one pending; `--github` asks by hand, for the
//! releases named or every release tagged here. Nothing here changes a run's result or its exit.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use symphony_board::sys::Command;

use super::group::{Bound, Failed, output_until};
use super::record::{age, local};
use super::tags::Release;

/// The file in the runs' folder that keeps what each release was last answered.
pub(crate) const LEDGER: &str = "github.json";
/// The lock of one publication at a time, beside it ([`Publishing`]).
const LOCK: &str = "github.lock";

/// The publication's lock in a runs' folder, taken: released when it goes. Its descriptor is
/// close-on-fork where the system has the flag, so no child another thread forks while it is held
/// copies it. Its drop unlocks it before it closes, which releases the lock for every copy of the
/// descriptor. A close alone releases it only with the last copy, so a child forked elsewhere,
/// between its fork and its exec, would hold it past this drop.
pub(crate) struct Publishing(File);

impl Publishing {
    /// The lock in `folder`, made when missing and taken without waiting: none while another
    /// publication holds it, or when it cannot be opened.
    pub(crate) fn take(folder: &Path) -> Option<Publishing> {
        fs::create_dir_all(folder).ok()?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).write(true);
        let file = process_cwd::close_on_fork(&mut options)
            .open(folder.join(LOCK))
            .ok()?;
        file.try_lock().ok()?;
        Some(Publishing(file))
    }
}

impl Drop for Publishing {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
/// The folder beside them that keeps, for each release, the last request made for it and what
/// the command answered (`NAME.request.json`, `NAME.answer.log`).
const LOGS: &str = "github";
/// The longest one ask may take: the adapter's few requests, each bounded by its own `curl`.
const ASK: Duration = Duration::from_secs(300);
/// What asks again for a release left pending, said with each.
const AGAIN: &str = "`just release --refresh`, the next release's run or `just release --github` \
                     asks again";

/// What a release was last answered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Asked {
    /// `published` (made by that ask), `found` (there already) or `pending`.
    pub(crate) state: String,
    /// The release's page, or why it is pending.
    pub(crate) said: String,
    /// When, in seconds since 1970, and by which run (`by hand` for `--github`).
    pub(crate) at: f64,
    pub(crate) by: String,
}

impl Asked {
    fn pending(&self) -> bool {
        self.state == "pending"
    }

    /// The line a run says of `release`.
    fn line(&self, release: &str) -> String {
        match self.state.as_str() {
            "published" => format!("published {release} as {}", self.said),
            "found" => format!("{release} is there already: {}", self.said),
            _ => format!("pending: {release}: {}", self.said),
        }
    }
}

/// The ledger: each release asked for, by its name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ledger {
    #[serde(default)]
    pub(crate) releases: BTreeMap<String, Asked>,
}

impl Ledger {
    /// The ledger kept in `folder`: empty with none, or with one that does not read, which the
    /// next ask writes over, as every release is looked for by its tag before it is made.
    pub(crate) fn read(folder: &Path) -> Ledger {
        fs::read_to_string(folder.join(LEDGER))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write(&self, folder: &Path) -> Result<(), String> {
        let path = folder.join(LEDGER);
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())? + "\n";
        symphony::text::write_whole(&path, &text).map_err(|e| symphony::text::os_error(&e, &path))
    }

    /// The releases left pending, oldest first.
    pub(crate) fn pending(&self) -> Vec<String> {
        let names = (self.releases.iter())
            .filter(|(_, asked)| asked.pending())
            .map(|(name, _)| name.as_str());
        oldest_first(names)
    }
}

/// `names` as releases, oldest first, each once; a name that is no release's is left out.
fn oldest_first<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut ordered = super::tags::newest_first(names);
    ordered.reverse();
    ordered.dedup();
    ordered.iter().map(Release::name).collect()
}

/// What `--status` says of the ledger in `folder` at `now`: nothing with none kept, else how many
/// releases are published and pending, each pending one with why, and the newest published.
pub(crate) fn status(folder: &Path, now: f64) -> String {
    let ledger = Ledger::read(folder);
    if ledger.releases.is_empty() {
        return String::new();
    }
    let pending = ledger.pending();
    let mut text = format!(
        "GitHub releases: {} published, {} pending\n",
        ledger.releases.len() - pending.len(),
        pending.len()
    );
    let when = |asked: &Asked| format!("{} ({})", local(asked.at), age(now, asked.at));
    for name in &pending {
        let asked = &ledger.releases[name];
        text.push_str(&format!(
            "  pending {name}: {}; asked by {}, {}; {AGAIN}\n",
            asked.said,
            asked.by,
            when(asked)
        ));
    }
    let published = (ledger.releases.keys())
        .filter(|name| !ledger.releases[*name].pending())
        .map(String::as_str);
    if let Some(newest) = oldest_first(published).last() {
        let asked = &ledger.releases[newest];
        text.push_str(&format!(
            "  newest published: {newest} as {}; asked by {}, {}\n",
            asked.said,
            asked.by,
            when(asked)
        ));
    }
    text
}

/// The body of the GitHub Release of `tag` at `commit` in the repository at `root`: its own
/// section of the notes, as `symphony release-notes` prints it from the release before.
pub(crate) fn body(root: &Path, tag: &str, commit: &str) -> Result<String, String> {
    let document = super::consumer::document_at(root, tag, commit)?;
    let before = super::consumer::releases(root, commit)?;
    let previous = (before.iter().map(|(name, _)| name.as_str()))
        .take_while(|name| *name != tag)
        .last();
    symphony::machine::release::notes_between(&document, previous, Some(tag))
}

/// What a publication asks with.
pub(crate) struct Asking<'a> {
    /// The checkout whose tags and history are read.
    pub(crate) root: &'a Path,
    /// The main checkout, which the command is run in.
    pub(crate) main: &'a Path,
    /// The runs' folder, which keeps the ledger.
    pub(crate) folder: &'a Path,
    /// `[releases] github`: the command.
    pub(crate) command: &'a [String],
    /// The run that asks, or `by hand`.
    pub(crate) by: &'a str,
    pub(crate) now: f64,
    pub(crate) until: Bound<'a>,
}

impl Asking<'_> {
    /// The answer of the command to a request for `tag`: its state and what it said, the
    /// release's page or why none was made.
    fn ask(&self, tag: &str, commit: &str, latest: bool) -> Result<(String, String), String> {
        let body = body(self.root, tag, commit).map_err(|why| format!("its notes: {why}"))?;
        let request = json!({
            "tag": tag, "commit": commit, "latest": latest, "body": body, "by": self.by,
        });
        let logs = self.folder.join(LOGS);
        fs::create_dir_all(&logs).map_err(|e| symphony::text::os_error(&e, &logs))?;
        let file = logs.join(format!("{tag}.request.json"));
        fs::write(&file, format!("{request}\n"))
            .map_err(|e| symphony::text::os_error(&e, &file))?;
        let said = format!("{} --request {}", self.command.join(" "), file.display());
        let mut command = Command::new(&self.command[0]);
        command
            .args(&self.command[1..])
            .arg("--request")
            .arg(&file)
            .current_dir(self.main);
        let done =
            output_until(&mut command, self.until.sooner(ASK)).map_err(|failed| match failed {
                Failed::Io(e) => format!("cannot run {said}: {e}"),
                Failed::Late(took) => format!(
                    "{said} gave no answer within {} s and was stopped",
                    took.as_secs()
                ),
                Failed::Interrupted(_) => {
                    format!("{said} was stopped: the run was told to stop")
                }
            })?;
        let (out, err) = (
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr),
        );
        let _ = fs::write(
            logs.join(format!("{tag}.answer.log")),
            format!("$ {said}\n{out}{err}({})\n", done.status),
        );
        let first = |text: &str| {
            (text.lines().map(str::trim))
                .find(|line| !line.is_empty())
                .map(str::to_string)
        };
        if !done.status.success() {
            return Err(format!(
                "{said}: {}",
                first(&err).unwrap_or_else(|| done.status.to_string())
            ));
        }
        let answer: Value = (first(&out).and_then(|line| serde_json::from_str(&line).ok()))
            .filter(Value::is_object)
            .ok_or_else(|| format!("{said}: its answer is not a JSON object"))?;
        let text = |field: &str| answer[field].as_str().unwrap_or_default().to_string();
        match text("state").as_str() {
            state @ ("published" | "found") => Ok((state.into(), text("url"))),
            state @ ("refused" | "unavailable" | "unknown") => {
                Err(format!("{state}: {}", text("why")))
            }
            other => Err(format!("{said}: it answered the state {other:?}")),
        }
    }

    /// Each of `wanted` asked for, oldest first, among `releases` (those tagged here, newest
    /// first, each with its commit), and the ledger kept after each: one line for each release,
    /// with whether it is published. One publication runs at a time: with another running,
    /// nothing is asked and each is left as the ledger has it.
    pub(crate) fn publish(
        &self,
        wanted: &[String],
        releases: &[(Release, String)],
    ) -> Vec<(bool, String)> {
        let Some(lock) = Publishing::take(self.folder) else {
            return vec![(
                false,
                format!(
                    "another publication holds {}: nothing is asked; {AGAIN}",
                    self.folder.join(LOCK).display()
                ),
            )];
        };
        let mut ledger = Ledger::read(self.folder);
        let mut lines = Vec::new();
        for tag in oldest_first(wanted.iter().map(String::as_str)) {
            let Some((_, commit)) = releases.iter().find(|(release, _)| release.name() == tag)
            else {
                lines.push((
                    false,
                    format!("{tag}: no release of that name is tagged here"),
                ));
                continue;
            };
            let latest = (releases.first()).is_some_and(|(newest, _)| newest.name() == tag);
            let asked = match self.ask(&tag, commit, latest) {
                Ok((state, said)) => Asked {
                    state,
                    said,
                    at: self.now,
                    by: self.by.into(),
                },
                Err(why) => Asked {
                    state: "pending".into(),
                    said: why,
                    at: self.now,
                    by: self.by.into(),
                },
            };
            lines.push(if asked.pending() {
                (false, format!("{}; {AGAIN}", asked.line(&tag)))
            } else {
                (true, asked.line(&tag))
            });
            ledger.releases.insert(tag, asked);
            if let Err(why) = ledger.write(self.folder) {
                lines.push((false, format!("the ledger was not kept: {why}")));
            }
        }
        drop::<Publishing>(lock);
        lines
    }
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
