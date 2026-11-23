//! The Settings page's write ([`symphony_api::SETTING_WRITE_PATH`]): a preference kept in the
//! board's store, its entry removed, or its stored value made the project's. The page writes
//! nothing else: never `symphony.local.toml`, and no file but the project file by the commit
//! below.
//!
//! - **Store** keeps the value in the board's store ([`symphony_board::Store::store_setting`]),
//!   in force from the next read of the settings. The store judges it by the rules and lists the
//!   local file is judged by: a setting that holds a command or decides what lands, one the store
//!   does not keep, and a value of the wrong kind or out of range are refused with why, nothing
//!   written, whatever the request names.
//! - **Remove** removes the entry, its record kept: the files' value is in force again.
//! - **Commit** makes the stored value the project's. The default branch's project file is
//!   edited as the model command edits it (`crate::project::edit`), the value's span alone; the
//!   settings check runs on the result ([`crate::project::Project::checked`]), and one it refuses
//!   is answered with its file and line and nothing written. A green one is committed straight to
//!   the default branch by the owner's committer, one commit naming the key, the old value and
//!   the new ([`crate::commit::commit_setting`]), and the entry is removed once it stands; but
//!   where a checkout on the default branch kept its own edit of the line, which the commit does
//!   not merge into, the entry stays in force over that edit until the checkout takes the commit,
//!   and making the value the project's again then removes it, the branch and the checkout writing
//!   it already; before the checkout takes it, that is refused and the entry stays. Where
//!   the project delivers by pull request it is kept and delivered as a document save is
//!   (`crate::commit::pending`), answered as waiting, and its entry stays until the delivery is
//!   proved. Such an edit is made against the project file as the saves still waiting for their
//!   delivery leave it ([`crate::commit::pending::shown`]), as a document save's is: a later one
//!   waits for the earlier one's delivery and is then committed onto the default branch that
//!   holds it, which it would conflict with were it made against the branch alone.
//!
//! A write comes from the project's own page: one named for a worktree's scope is refused. Each
//! is made under the saves' lock and refused once they are closed ([`Saves::close`]), as a
//! document save is: a retired project's state still held by a request writes nothing.

use std::sync::MutexGuard;
use std::sync::atomic::Ordering;

use serde_json::Value;
use symphony_api::{SettingAction, SettingValue, SettingWrite, SettingWritten};
use symphony_board::lock::Recovered as _;
use symphony_board::settings::{Stored, kept, named, path, shown, written};

use super::{Failure, Saves, held_write_lock};
use crate::commit;
use crate::project::{FILE, Project};

/// A refusal of a write that conflicts with what the store or the project holds now.
fn conflict(error: String) -> Failure {
    Failure { status: 409, error }
}

/// The refusal of a write once the saves are closed.
fn closing() -> Failure {
    Failure {
        status: 503,
        error: "setting writes are closing".into(),
    }
}

/// Whether the checkout at `root` takes `value` for the key the tables `at` lead to: its own
/// project file, which its settings are read from, writes it; not where that file is none or no
/// TOML.
fn takes(root: &std::path::Path, at: &[&str], value: &toml::Value) -> bool {
    let Some((last, tables)) = at.split_last() else {
        return false;
    };
    let text = std::fs::read_to_string(root.join(FILE)).unwrap_or_default();
    let Ok(mut here) = toml::from_str::<toml::Table>(&text) else {
        return false;
    };
    for name in tables {
        match here.remove(*name) {
            Some(toml::Value::Table(inner)) => here = inner,
            _ => return false,
        }
    }
    here.get(*last) == Some(value)
}

/// The refusal of making `said` the project's where the default branch `default` writes it
/// already and the checkout served does not take it yet: its own edit of the line, which the
/// commit did not merge into, is still its value, and the board's entry stays in force over it.
fn untaken(said: &str, default: &str) -> Failure {
    conflict(format!(
        "not made the project's again: {FILE} on {default} writes {said} already, but this \
         checkout's own {FILE} does not take it yet: the board's entry stays, in force over the \
         checkout's own edit of the line, until the checkout takes the commit"
    ))
}

/// The value a write gives, as the project file writes it.
fn toml_of(value: &SettingValue) -> toml::Value {
    match value {
        SettingValue::Switch(on) => toml::Value::Boolean(*on),
        SettingValue::Number(n) => toml::Value::Integer(*n),
        SettingValue::Text(text) => toml::Value::String(text.clone()),
    }
}

/// The project file the default branch `branch` of the checkout `root` commits: empty where it
/// commits none.
fn committed(root: &std::path::Path, branch: &str) -> Result<String, Failure> {
    let at = format!("refs/heads/{branch}:{FILE}");
    let done = symphony_board::sys::output(
        crate::children::git()
            .arg("-C")
            .arg(root)
            .args(["show", &at]),
    )
    .map_err(|e| e.to_string())?;
    if done.status.success() {
        return String::from_utf8(done.stdout).map_err(|e| Failure::from(e.to_string()));
    }
    // No such file on the branch is an empty one; any other failure is said.
    let exists = symphony_board::sys::output(crate::children::git().arg("-C").arg(root).args([
        "cat-file",
        "-e",
        &format!("refs/heads/{branch}"),
    ]))
    .is_ok_and(|done| done.status.success());
    if exists {
        Ok(String::new())
    } else {
        Err(conflict(format!(
            "{branch} has no commit here, so nothing can be made the project's"
        )))
    }
}

impl Saves {
    /// The Settings page's write, `payload` a [`SettingWrite`], recorded as made by `by`, the short
    /// name of who asked: what it did, as a [`SettingWritten`], or why it is refused with nothing
    /// changed.
    pub(super) fn setting(&self, scope: &str, payload: &Value, by: &str) -> Result<Value, Failure> {
        if !scope.is_empty() {
            return Err(Failure {
                status: 404,
                error: "a setting is written from the project's own Settings page, never a \
                        worktree's"
                    .into(),
            });
        }
        let write: SettingWrite = serde_json::from_value(payload.clone())
            .map_err(|e| format!("not a setting's write: {e}"))?;
        // A key that names no setting is refused before the board is asked anything.
        path(&write.key)?;
        let done = match write.action {
            SettingAction::Store => {
                let value = write
                    .value
                    .as_ref()
                    .ok_or("a store write gives the value it keeps".to_string())?;
                self.keep(&write.key, &toml_of(value), by)?
            }
            SettingAction::Remove => self.remove(&write.key, by)?,
            SettingAction::Commit => self.make_the_projects(&write.key, by)?,
        };
        Ok(serde_json::to_value(done).expect("a setting's answer is JSON"))
    }

    /// The saves' lock, held by the write in flight, which a close waits for; refused once they
    /// are closed, the flag being set under the lock.
    fn open(&self) -> Result<MutexGuard<'_, ()>, Failure> {
        let held = self.writes.lock().recover();
        if self.closed.load(Ordering::Acquire) {
            return Err(closing());
        }
        Ok(held)
    }

    fn keep(&self, key: &str, value: &toml::Value, by: &str) -> Result<SettingWritten, Failure> {
        let _held = self.open()?;
        // A switch is judged against the launch default the main checkout's project file names.
        let project_file = symphony_board::builds::main_of(&self.root).join(FILE);
        let (entry, changed) = self.store()?.store_setting(key, value, by, &project_file)?;
        // A switch is said as the page draws it: on or off.
        let held = match &entry.value {
            Some(Value::Bool(on)) => (if *on { "on" } else { "off" }).to_string(),
            value => value.as_ref().map(shown).unwrap_or_default(),
        };
        Ok(SettingWritten {
            said: if changed {
                format!(
                    "{} is {held} in the board's store, in force at once unless {} writes it",
                    named(key),
                    crate::project::LOCAL_FILE
                )
            } else {
                format!(
                    "{} is {held} in the board's store already: nothing written",
                    named(key)
                )
            },
            ..SettingWritten::default()
        })
    }

    fn remove(&self, key: &str, by: &str) -> Result<SettingWritten, Failure> {
        let _held = self.open()?;
        self.store()?.remove_setting(key, by)?;
        Ok(SettingWritten {
            said: format!(
                "{} has no entry in the board's store now: the files' value is in force",
                named(key)
            ),
            ..SettingWritten::default()
        })
    }

    /// The stored value of `key` made the project's, as [the module](self) says.
    fn make_the_projects(&self, key: &str, by: &str) -> Result<SettingWritten, Failure> {
        let root = &self.root;
        // Held through a delivery's submit, as a document save holds them; a commit straight to
        // the default branch takes them again under the commits' lock, which comes first
        // (`commit_setting`), and sees the close there too.
        let saving = self.open()?;
        let file = held_write_lock(root)?;
        let store = self.store()?;
        let entry = (store.stored_entries()?.into_iter())
            .find(|entry| entry.key == key && entry.value.is_some())
            .ok_or_else(|| {
                conflict(format!(
                    "{} has no entry in the board's store: nothing to make the project's",
                    named(key)
                ))
            })?;
        let held = entry.value.clone().expect("an entry holding a value");
        // An entry the store does not read is committed nowhere.
        let alone = Stored::of(std::slice::from_ref(&entry));
        if let Some(why) = alone.problems.first() {
            return Err(conflict(format!("not committed: {why}")));
        }
        let value = written(&held).expect("an entry the store reads is of a setting's kind");
        let said = format!("{} = {}", named(key), shown(&held));
        let project = Project::of(root);
        let default = project.default_branch.clone();
        if !commit::on_main(root) {
            return Err(conflict(format!(
                "not committed: the checkout this server serves is not on {default}, so nothing \
                 is committed from it"
            )));
        }
        if let Some(save) = (commit::pending::saves(root).into_iter())
            .find(|save| save.keys.iter().any(|waiting| waiting == key))
        {
            return Err(conflict(format!(
                "not committed again: making {} the project's waits for its delivery already, on \
                 {} ({})",
                named(key),
                save.branch,
                save.state.said(&default)
            )));
        }
        let delivers = commit::delivers(root);
        let base = committed(root, &default)?;
        let base = if delivers {
            commit::pending::shown(root, FILE, base)
        } else {
            base
        };
        let (tables, last) = path(key)?;
        let mut new = base.clone();
        let at: Vec<&str> = tables.iter().copied().chain([last]).collect();
        crate::project::edit::set(&mut new, &at, &value).map_err(|why| Failure {
            status: 422,
            error: format!("not committed: {why}; nothing was written, and the stored value holds"),
        })?;
        if new == base {
            if !takes(root, &at, &value) {
                return Err(untaken(&said, &default));
            }
            store.remove_setting_holding(key, &kept(&value), by)?;
            return Ok(SettingWritten {
                said: format!(
                    "{FILE} on {default} writes {said} already: the board's entry is removed, and \
                     the file's value is in force"
                ),
                ..SettingWritten::default()
            });
        }
        if let Err(problem) = Project::checked(root, &new) {
            let line = symphony_board::limits::line(&new, &tables, last)
                .map_or_else(String::new, |line| format!(" on line {line}"));
            return Err(Failure {
                status: 422,
                error: format!(
                    "not committed: the settings check refuses {FILE} as it would be, with {said}{line}: \
                     {problem}; nothing was written, and the stored value holds"
                ),
            });
        }
        let edit = commit::Edit {
            rel: FILE.into(),
            base,
            new,
        };
        let changes = commit::settings::changes(&edit)
            .expect("an edit of the project file")
            .map_err(|why| Failure {
                status: 422,
                error: format!("not committed: {why}"),
            })?;
        let message = commit::settings::message(&changes);
        if delivers {
            self.committer
                .get_or_init(|| commit::Committer::start(root, self.writes.clone()))
                .submit(edit)?;
            drop(file);
            drop(saving);
            return Ok(SettingWritten {
                said: format!(
                    "{said} is kept for delivery by pull request: this project delivers by pull \
                     request, so it is committed on a branch of its own and delivered as the \
                     owner's task; the board's entry stays in force until the delivery is proved"
                ),
                waiting: true,
                ..SettingWritten::default()
            });
        }
        drop(file);
        drop(saving);
        let outcome = commit::commit_setting(root, &edit, &message, &self.writes, &self.closed)?
            .ok_or_else(closing)?;
        if !outcome.committed {
            if !takes(root, &at, &value) {
                return Err(untaken(&said, &default));
            }
            store.remove_setting_holding(key, &held, by)?;
            return Ok(SettingWritten {
                said: format!(
                    "{default} holds {said} already: the board's entry is removed, and the \
                     file's value is in force"
                ),
                ..SettingWritten::default()
            });
        }
        let short: String = outcome.commit.chars().take(12).collect();
        let mut said = format!("committed to {default} as {short}, \"{message}\"");
        // A note is a checkout on the default branch that kept its own edit of the file, which
        // the commit does not merge into: there the file still writes that edit's value, so the
        // entry stays in force over it until the checkout takes the commit.
        let taken = outcome.notes.is_empty();
        for note in outcome.notes {
            said.push_str(&format!("; {note}"));
        }
        if taken {
            match store.remove_setting_holding(key, &held, by) {
                Ok(_) => said
                    .push_str("; the board's entry is removed, and the file's value is in force"),
                Err(why) => said.push_str(&format!(
                    "; the board's entry could not be removed, and still holds: {why}"
                )),
            }
        } else {
            said.push_str(&format!(
                "; the board's entry stays, in force over that edit, until the checkout takes the \
                 commit: make {} the project's again then, which removes it",
                named(key)
            ));
        }
        Ok(SettingWritten {
            said,
            commit: Some(outcome.commit),
            waiting: false,
        })
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
