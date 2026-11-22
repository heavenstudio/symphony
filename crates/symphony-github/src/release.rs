//! `symphony-github release --request FILE`: what answers a release run that publishes a GitHub
//! Release for a tag it pushed (`[releases] github = ["symphony-github", "release"]`,
//! docs/workflow/RELEASES.md), one JSON object read and one printed. It serves only `enabled` settings
//! whose `releases` is `publish`, to the repository the state is bound to by its number.
//!
//! The request names the release's `tag`, the `commit` it tags, the `body` (the release's notes)
//! and whether it is the `latest`. The answer's `state` is one of:
//!
//! - `published`: the release was made now;
//! - `found`: GitHub holds a release of that tag already, and none was made;
//! - `refused`: nothing was made and running again as things are makes none (`why`);
//! - `unavailable`: GitHub gave no answer to go on by, and nothing was made (`why`);
//! - `unknown`: the release was asked for and its answer was lost: the next run finds it by its
//!   tag before it asks again.
//!
//! A release is looked for by its tag before it is made, so a second run and a run after a lost
//! answer make none. GitHub makes a tag of its own for a release whose tag it does not hold, so
//! the tag is read there first and must name the request's commit: with none there, or another
//! commit's, nothing is asked for. A release found is left as it is, whatever its text; one
//! published here that GitHub no longer shows is not made again. Nothing is uploaded, and no
//! release or tag is edited, moved or deleted: this module sends one kind of write, the release's
//! creation. The write is journaled before it is made, and no token is in an argument, a URL, the
//! journal, the state or a line said here.

use crate::config::{Config, Releases};
use crate::deliver::{self, Host, Lost, Serving, Stopped, refused, unavailable};
use crate::reconcile::{Context, Outcome};
use crate::state::{Entry, Event, Released, State, Store};
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;

/// The longest body sent, in characters: GitHub takes 125,000.
const BODY_CHARS: usize = 100_000;

/// `text` as a release's body: line ends as `\n`, no other control character, and no more than
/// [`BODY_CHARS`], a longer one cut with a line saying where the rest is read.
#[must_use]
pub fn body(text: &str) -> String {
    let cleaned: String = text
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().count() <= BODY_CHARS {
        return cleaned.into();
    }
    let cut: String = cleaned.chars().take(BODY_CHARS).collect();
    format!("{cut}\n\n[cut here: `symphony release-notes` prints the release's notes whole]")
}

/// A release as GitHub answered it: its number and page.
fn released(value: &Value, commit: &str) -> Option<Released> {
    Some(Released {
        id: value["id"].as_i64()?,
        url: value["html_url"].as_str().unwrap_or_default().into(),
        commit: commit.into(),
    })
}

/// One publication's run: the repository, the state and who asked.
struct Run<'a> {
    ctx: &'a Context<'a>,
    revision: String,
    store: Store,
    state: State,
    by: String,
}

impl Run<'_> {
    fn journal(&self, event: Event, tag: &str, digest: &str, result: Value) {
        // A journal that cannot be written stops nothing already looked for: the next run looks
        // by the tag before it makes anything.
        let _ = self.store.journal(&Entry {
            at: self.ctx.now,
            event,
            preview: String::new(),
            op: format!("release:{tag}"),
            kind: "release".into(),
            target: tag.into(),
            digest: digest.into(),
            base: Value::Null,
            config: self.revision.clone(),
            by: self.by.clone(),
            result,
        });
    }

    /// The release of `tag` GitHub shows now, if any.
    fn find(host: &Host<'_>, tag: &str) -> Result<Option<Value>, Stopped> {
        match host.get(&format!("/releases/tags/{tag}"))? {
            (200, value) => Ok(Some(value)),
            (404, _) => Ok(None),
            (status, _) => Err(unavailable(format!(
                "looking for the release of {tag}: GitHub answered {status}"
            ))),
        }
    }

    /// The release `value` kept as `tag`'s, and the answer `state` naming it.
    fn keep(
        &mut self,
        tag: &str,
        commit: &str,
        value: &Value,
        state: &str,
    ) -> Result<Value, Stopped> {
        let kept = released(value, commit)
            .ok_or_else(|| unavailable(format!("GitHub's release of {tag} has no id")))?;
        let answer = json!({"state": state, "release": tag, "id": kept.id, "url": kept.url});
        if self.state.releases.get(tag) != Some(&kept) {
            self.state.releases.insert(tag.into(), kept);
            self.store.save(&self.state).map_err(unavailable)?;
        }
        Ok(answer)
    }

    /// The commit the tag `tag` names at GitHub, which must be `commit`: read through the tag's
    /// own object where it is annotated.
    fn tagged(host: &Host<'_>, tag: &str, commit: &str) -> Result<(), Stopped> {
        let name = host.repository.full_name();
        let object = match host.get(&format!("/git/ref/tags/{tag}"))? {
            (200, value) if value["ref"] == json!(format!("refs/tags/{tag}")) => {
                value["object"].clone()
            }
            (200 | 404, _) => {
                return Err(refused(format!(
                    "{name} holds no tag {tag}: GitHub would make one for the release, so none \
                     is asked for until the tag is pushed there"
                )));
            }
            (status, _) => {
                return Err(unavailable(format!(
                    "reading the tag {tag}: GitHub answered {status}"
                )));
            }
        };
        let sha = object["sha"].as_str().unwrap_or_default();
        let names = match object["type"].as_str() {
            Some("commit") => sha.to_string(),
            Some("tag") if deliver::commit(sha) => match host.get(&format!("/git/tags/{sha}"))? {
                (200, value) if value["object"]["type"] == json!("commit") => {
                    value["object"]["sha"].as_str().unwrap_or_default().into()
                }
                (200, _) => String::new(),
                (status, _) => {
                    return Err(unavailable(format!(
                        "reading the tag {tag}: GitHub answered {status}"
                    )));
                }
            },
            _ => String::new(),
        };
        if names == commit {
            Ok(())
        } else {
            Err(refused(format!(
                "the tag {tag} at {name} names {}, and the release is of {commit}: nothing is \
                 published for another commit's tag",
                if names.is_empty() {
                    "no commit"
                } else {
                    &names
                }
            )))
        }
    }

    fn publish(&mut self, host: &Host<'_>, request: &Value) -> Result<Value, Stopped> {
        let text = |field: &str| request[field].as_str().unwrap_or_default();
        let (tag, commit) = (text("tag"), text("commit"));
        if !deliver::plain(tag, "-._") {
            return Err(refused(format!("no release is named {tag:?}")));
        }
        if !deliver::commit(commit) {
            return Err(refused(format!(
                "a release's commit is its whole name, 40 hexadecimal digits: {commit:?}"
            )));
        }
        let body = body(text("body"));
        deliver::bound(host, &self.store, &mut self.state)?;
        if let Some(found) = Self::find(host, tag)? {
            return self.keep(tag, commit, &found, "found");
        }
        if let Some(kept) = self.state.releases.get(tag) {
            return Err(refused(format!(
                "{tag} was published as {} and GitHub shows no release of that tag now: it is \
                 not made again",
                kept.url
            )));
        }
        Self::tagged(host, tag, commit)?;
        let digest = crate::digest(&[tag, commit, &body]);
        let sent = json!({
            "tag_name": tag, "target_commitish": commit, "name": tag, "body": body,
            "draft": false, "prerelease": false,
            "make_latest": if request["latest"] == json!(true) { "true" } else { "false" },
        });
        self.journal(Event::Intent, tag, &digest, Value::Null);
        let stopped = match host.send("POST", host.path("/releases"), Some(&sent)) {
            Ok(Ok((201, value))) => {
                let answer = self.keep(tag, commit, &value, "published")?;
                self.journal(Event::Ack, tag, &digest, answer.clone());
                return Ok(answer);
            }
            // Refused as there already: another run made it since this one looked.
            Ok(Ok((422, value))) => match Self::find(host, tag)? {
                Some(found) => {
                    let answer = self.keep(tag, commit, &found, "found")?;
                    self.journal(Event::Ack, tag, &digest, answer.clone());
                    return Ok(answer);
                }
                None => refused(format!(
                    "GitHub refused the release of {tag} (422): {}",
                    crate::github::clean_title(value["message"].as_str().unwrap_or_default())
                )),
            },
            Ok(Ok((status @ (403 | 404), _))) => refused(format!(
                "GitHub refused the release of {tag} ({status}): token_file's token needs \
                 contents read and write on the repository"
            )),
            Ok(Ok((status, _))) => unavailable(format!(
                "GitHub answered {status} to the release of {tag}: none was made"
            )),
            Ok(Err(Lost(why))) => {
                self.journal(Event::Unknown, tag, &digest, json!(why));
                // Looked for at once: found, it was made; not found is not proof it was not.
                return match Self::find(host, tag) {
                    Ok(Some(found)) => {
                        let answer = self.keep(tag, commit, &found, "published")?;
                        self.journal(Event::Ack, tag, &digest, answer.clone());
                        Ok(answer)
                    }
                    _ => Err(json!({"state": "unknown", "release": tag, "why": format!(
                        "the release of {tag} was asked for and its answer was lost ({why}): \
                         the next run looks for it by its tag before it asks again"
                    )})),
                };
            }
            Err(stopped) => stopped,
        };
        self.journal(Event::Failed, tag, &digest, stopped["why"].clone());
        Err(stopped)
    }
}

/// The answer to `request`.
fn answer(ctx: &Context<'_>, request: &Value) -> Value {
    let publishes = |config: &Config| config.releases == Releases::Publish;
    let serving = match deliver::served(ctx, "releases = \"publish\"", publishes) {
        Ok(serving) => serving,
        Err(why) => return refused(why),
    };
    let Serving {
        config,
        revision,
        repository,
    } = serving;
    let (store, _lock, state) = match deliver::kept(ctx) {
        Ok(kept) => kept,
        Err(why) => return unavailable(why),
    };
    let transport = match deliver::connect(ctx, &config) {
        Ok(transport) => transport,
        Err(stopped) => return stopped,
    };
    let host = Host {
        transport: transport.as_ref(),
        repository,
    };
    let mut run = Run {
        ctx,
        revision,
        store,
        state,
        by: crate::github::clean_title(request["by"].as_str().unwrap_or_default()),
    };
    run.publish(&host, request)
        .unwrap_or_else(|stopped| stopped)
}

/// `release --request FILE`: the answer on `out`, one line of JSON.
///
/// # Errors
///
/// The request does not read: nothing was asked of anyone.
pub fn run(ctx: &Context<'_>, file: &Path, out: &mut dyn Write) -> Outcome {
    let said = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let request: Value =
        serde_json::from_str(&said).map_err(|e| format!("{}: {e}", file.display()))?;
    if !request.is_object() {
        return Err(format!("{}: a request is one JSON object", file.display()));
    }
    let _ = writeln!(out, "{}", answer(ctx, &request));
    Ok(0)
}

#[cfg(test)]
#[path = "release_tests.rs"]
mod tests;
