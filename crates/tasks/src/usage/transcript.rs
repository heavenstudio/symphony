//! A Claude Code transcript's token usage, read a beat at a time: what the file gained since the
//! last beat of the same key, never the whole file again (a long session's runs to tens of
//! megabytes, and the beat holds up a tool call).
//!
//! Each assistant message's usage is on every line Claude Code writes for it (one per content
//! block), the same on each: a message id counts once. Its kinds are kept apart as a price tells
//! them apart: `input_tokens` (the uncached remainder), `cache_creation_input_tokens` split by
//! lifetime (`cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens`; what they
//! leave, or all of it with no split, is the five-minute kind), `cache_read_input_tokens` and
//! `output_tokens`, by `message.model` and `usage.speed`. A `<synthetic>` message is no model call.
//!
//! What a key read is kept in its [`Ledger`], `KEY.json` in the usage folder (`symphony/tasks/usage/`
//! in the git common dir, beside the bound keys): the transcript, the offset after the last whole
//! line read, the last message ids counted, and what was read that the board has not taken (a
//! subagent's calls before a board command binds it), handed again at the next beat. A beat of the
//! key holds `KEY.lock` while it reads, hands and keeps: a second beat of the same key meanwhile
//! (two tool calls side by side) reads nothing, and the next beat after reads what it left.
//!
//! A fork (a subagent whose `agent-ID.meta.json` beside its transcript says `"isFork": true`)
//! begins its transcript with its parent's history, under the parent's message ids, which the
//! parent's own beats count: read from its start, a fork's transcript is passed over up to the
//! tool result that answers the parent's call that made it (the meta's `toolUseId`), the last line
//! it inherited, and counted from there.
//!
//! What cannot be read is never counted twice: a ledger that does not parse starts again at the
//! transcript's end, so what it held is lost, not repeated, and is never removed. A ledger is
//! removed, when a new one is made, only once nothing wrote to it for 7 days and the transcript it
//! names is gone: a session resumed after a week reads on from where it stopped.

use super::{Spent, Tokens, merged};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The usage folder's name, in `symphony/tasks/` of the git common dir.
pub const FOLDER: &str = "usage";

/// The most one beat reads, give or take a line: a transcript first read whole is read over as
/// many beats as it takes.
pub const READ_AT_MOST: u64 = 32 << 20;

/// How many of the last message ids counted a ledger keeps: a message whose lines a beat read
/// only some of has the rest read by the next, and counts once.
const SEEN: usize = 32;

/// How long a ledger nothing wrote to stays.
const PRUNE_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);

/// The transcript of the session making the tool call `event` is of: for a subagent's call (an
/// `agent_id`), its own, `agent-AGENT_ID.jsonl` (the event's `agent_transcript_path`, else
/// `SESSION/subagents/` beside the session's `transcript_path`), never its parent's; for a full
/// session's, `transcript_path`. None when the event names none, or a path that is not absolute.
pub fn of_event(event: &Map<String, Value>) -> Option<PathBuf> {
    let named = |field: &str| {
        event
            .get(field)
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let agent = event
        .get("agent_id")
        .and_then(Value::as_str)
        .filter(|agent| !agent.is_empty());
    let Some(agent) = agent else {
        return named("transcript_path");
    };
    if !safe(agent) {
        return None;
    }
    let own = format!("agent-{agent}.jsonl");
    let is_own = |path: &PathBuf| path.file_name().is_some_and(|name| *name == *own);
    if let Some(path) = named("agent_transcript_path").filter(is_own) {
        return Some(path);
    }
    let session = named("transcript_path")?;
    if is_own(&session) {
        return Some(session);
    }
    let stem = session.file_stem()?.to_owned();
    Some(session.with_file_name(stem).join("subagents").join(own))
}

/// Whether `key` can name a file: letters, digits, `_` and `-`, as every agent and session id is.
pub fn safe(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// What a key's beats have read of its transcript ([`Ledger`]'s file).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Kept {
    transcript: String,
    offset: u64,
    seen: Vec<String>,
    pending: Vec<Spent>,
    /// A fork's: the parent's tool call whose result ends what the fork inherited, while the read
    /// has not passed it.
    inherited: Option<String>,
}

/// One beat's hold on a key's ledger: what it read, to hand the board, and where the next read
/// starts once it is closed.
#[derive(Debug)]
pub struct Ledger {
    _lock: File,
    file: PathBuf,
    kept: Kept,
    spent: Vec<Spent>,
}

impl Ledger {
    /// `key`'s ledger in `folder`, holding what `transcript` gained since the last beat and what
    /// the board did not take then; none when another beat of the key holds it, or the key or the
    /// folder cannot hold one.
    pub fn open(folder: &Path, key: &str, transcript: &Path) -> Option<Ledger> {
        if !safe(key) {
            return None;
        }
        fs::create_dir_all(folder).ok()?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(folder.join(format!("{key}.lock")))
            .ok()?;
        lock.try_lock().ok()?;
        let file = folder.join(format!("{key}.json"));
        let path = transcript.to_string_lossy().into_owned();
        let mut kept = match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| Kept {
                // Unreadable: start at the end, so nothing it held is counted again.
                transcript: path.clone(),
                offset: fs::metadata(transcript).map_or(0, |m| m.len()),
                ..Kept::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                prune(folder, SystemTime::now());
                Kept::default()
            }
            // There, but not to be read now: read nothing rather than all of it again.
            Err(_) => return None,
        };
        if kept.transcript != path {
            kept.transcript = path;
            kept.offset = 0;
            kept.seen.clear();
        }
        let read = read(transcript, &mut kept).unwrap_or_default();
        let spent = merged(kept.pending.iter().cloned().chain(read));
        Some(Ledger {
            _lock: lock,
            file,
            kept,
            spent,
        })
    }

    /// What to hand the board: what the board did not take at the last beat, and what the
    /// transcript gained since, by model and speed.
    pub fn spent(&self) -> &[Spent] {
        &self.spent
    }

    /// Keep where the next read starts, and, unless the board took it (`recorded`), what was
    /// read; then let the key go. The ledger is replaced whole (written beside it, then renamed),
    /// so a beat killed meanwhile leaves the one before; one that cannot be written leaves it too,
    /// and the next beat reads again what this one read.
    pub fn close(mut self, recorded: bool) {
        self.kept.pending = if recorded {
            Vec::new()
        } else {
            std::mem::take(&mut self.spent)
        };
        let Ok(text) = serde_json::to_vec(&self.kept) else {
            return;
        };
        let staged = self
            .file
            .with_extension(format!("json.{}", std::process::id()));
        if fs::write(&staged, text).is_err() || fs::rename(&staged, &self.file).is_err() {
            let _ = fs::remove_file(&staged);
        }
    }
}

/// The usage of each message `transcript` gained after `kept`'s offset, reading whole lines up to
/// about [`READ_AT_MOST`] bytes; `kept` moves on past them. A transcript shorter than the offset
/// is another file: it is read from its start. Read from its start, a fork's passes over what it
/// inherited ([`inherited_until`]).
fn read(transcript: &Path, kept: &mut Kept) -> std::io::Result<Vec<Spent>> {
    let mut file = File::open(transcript)?;
    let len = file.metadata()?.len();
    if len < kept.offset {
        kept.offset = 0;
    }
    if kept.offset == 0 {
        kept.inherited = inherited_until(transcript);
    }
    file.seek(SeekFrom::Start(kept.offset))?;
    let mut lines = BufReader::new(file);
    let mut seen: HashSet<String> = kept.seen.iter().cloned().collect();
    let mut spent = Vec::new();
    let mut line = Vec::new();
    let mut read = 0;
    while read < READ_AT_MOST {
        line.clear();
        let took = lines.read_until(b'\n', &mut line)?;
        if took == 0 || line.last() != Some(&b'\n') {
            break; // the end, or a line still being written: the next beat reads it whole
        }
        let took = u64::try_from(took).unwrap_or(u64::MAX);
        read += took;
        kept.offset += took;
        if let Some(call) = &kept.inherited {
            if answers(&line, call) {
                kept.inherited = None;
            }
            continue;
        }
        let Some((id, one)) = message(&line) else {
            continue;
        };
        if seen.insert(id.clone()) {
            kept.seen.push(id);
            spent.push(one);
        }
    }
    let over = kept.seen.len().saturating_sub(SEEN);
    kept.seen.drain(..over);
    Ok(spent)
}

/// The parent's tool call whose result ends what `transcript` inherited, when it is a fork's: its
/// `agent-ID.meta.json` says `"isFork": true` and names the call (`toolUseId`). None for any other
/// transcript, or a meta that cannot be read, which is read whole.
fn inherited_until(transcript: &Path) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Meta {
        is_fork: Option<bool>,
        tool_use_id: Option<String>,
    }
    let name = transcript.file_name()?.to_str()?.strip_suffix(".jsonl")?;
    let meta = fs::read(transcript.with_file_name(format!("{name}.meta.json"))).ok()?;
    let meta: Meta = serde_json::from_slice(&meta).ok()?;
    meta.is_fork
        .filter(|fork| *fork)
        .and(meta.tool_use_id)
        .filter(|call| !call.is_empty())
}

/// Whether `line` is the tool result that answers `call`: the user line ending a fork's inheritance.
fn answers(line: &[u8], call: &str) -> bool {
    let Ok(text) = std::str::from_utf8(line) else {
        return false;
    };
    if !text.contains(call) || !text.contains("\"tool_result\"") {
        return false;
    }
    let Ok(line) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let content = &line["message"]["content"];
    line["type"] == "user"
        && content.as_array().is_some_and(|blocks| {
            blocks
                .iter()
                .any(|block| block["type"] == "tool_result" && block["tool_use_id"] == call)
        })
}

/// One transcript line: an assistant message's id and usage, when it is one. A line that names
/// neither is passed over unparsed, by the standard library's search, which is optimised in every
/// build: most of a transcript is tool results, and the hook's build is a debug one.
fn message(line: &[u8]) -> Option<(String, Spent)> {
    let line = std::str::from_utf8(line).ok()?;
    if !line.contains("\"assistant\"") || !line.contains("\"usage\"") {
        return None;
    }
    let line: Line = serde_json::from_str(line).ok()?;
    if line.kind.as_deref() != Some("assistant") {
        return None;
    }
    let message = line.message?;
    let (id, usage) = (message.id?, message.usage?);
    let model = message.model.unwrap_or_default();
    if id.is_empty() || model == "<synthetic>" {
        return None;
    }
    let n = |count: Option<u64>| count.unwrap_or(0);
    let written = n(usage.cache_creation_input_tokens);
    let (five, hour) = usage.cache_creation.map_or((written, 0), |split| {
        let (five, hour) = (
            n(split.ephemeral_5m_input_tokens),
            n(split.ephemeral_1h_input_tokens),
        );
        (five + written.saturating_sub(five + hour), hour)
    });
    let speed = usage
        .speed
        .filter(|speed| !speed.is_empty())
        .unwrap_or_else(|| "standard".into());
    Some((
        id,
        Spent {
            model,
            speed,
            tokens: Tokens {
                input: n(usage.input_tokens),
                cache_write_5m: five,
                cache_write_1h: hour,
                cache_read: n(usage.cache_read_input_tokens),
                output: n(usage.output_tokens),
                calls: 1,
            },
        },
    ))
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_creation: Option<Creation>,
    speed: Option<String>,
}

#[derive(Deserialize)]
struct Creation {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

/// Remove from `folder` each ledger, and its lock, nothing wrote to for [`PRUNE_AFTER`] by `now`
/// whose transcript is gone, so no beat reads it again; a lock with no ledger by its own age. A
/// ledger whose transcript is still there is kept however old (a session resumed reads on from it),
/// and so is one that does not parse, which stands for the transcript's end.
fn prune(folder: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let old = |path: &Path| {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age >= PRUNE_AFTER))
    };
    // The ledger parses and its transcript is not there: no beat will read it again.
    let gone = |ledger: &Path| {
        let kept = fs::read(ledger)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Kept>(&bytes).ok());
        kept.is_some_and(|kept| {
            !kept.transcript.is_empty()
                && fs::symlink_metadata(&kept.transcript)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        })
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(key) = name.strip_suffix(".json") {
            if old(&path) && gone(&path) {
                let _ = fs::remove_file(&path);
                let _ = fs::remove_file(folder.join(format!("{key}.lock")));
            }
        } else if let Some(key) = name.strip_suffix(".lock")
            && !folder.join(format!("{key}.json")).exists()
            && old(&path)
        {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
