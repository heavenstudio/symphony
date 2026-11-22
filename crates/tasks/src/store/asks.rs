//! What the owner says that waits on an agent's answer: an ask (`ask:⟨sid⟩`), one for each thing the
//! owner waits on, and never a task: no stage, no claim, no review, in no task count. It is open,
//! held by a session or by nobody, until an answer or a close by hand ends it. A second line the
//! owner says about the same thing while it is open joins it, its age staying the first line's; a
//! line after it was answered opens a new one. Who holds it is the router's to say
//! ([`Store::hold_ask`]); the record keeps each earlier holder, from when, until when and why it moved
//! on. The owner's words and the answers stay where they were written (the card, the comment, the
//! ask's thread, the task's thread): the record says only what it is about, who has it and whether it
//! is answered.

use super::postgres::asks as pg;
use super::statement::both;
use super::surreal::asks as sql;
use super::{Error, REFUSED, Store, refused, repr, unknown_actor};
use crate::rows::AskRow;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// An ask as the board holds it.
pub type Ask = AskRow;

/// What the owner can be waiting on an agent for: a verdict with a note or a rejection, a message on
/// a card, a comment in a document, a chat, a message on a task's thread.
pub const ASK_KINDS: [&str; 5] = ["verdict", "message", "comment", "chat", "task"];

/// How long an open ask waits before it shows as late, in seconds: the design's ten minutes, the same
/// for every kind until the owner says otherwise (docs/analysis/owner-asks.md, Open questions).
pub const LATE_SECONDS: f64 = symphony_api::ASK_LATE_SECONDS as f64;

/// The most characters of the owner's words an ask keeps: enough to say what it is, never the post.
pub const WORDS: usize = 200;

/// The owner's line an ask opens on, or joins: what it is about and what was said.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAsk {
    /// One of [`ASK_KINDS`].
    pub kind: String,
    /// The document: for a verdict, a card's message or a comment, and a chat about a document.
    #[serde(default)]
    pub path: Option<String>,
    /// The card's slug, for a verdict or a card's message.
    #[serde(default)]
    pub card: Option<String>,
    /// The commented block, for a comment.
    #[serde(default)]
    pub block: Option<String>,
    /// The worktree's branch the owner said it on; none on the main checkout.
    #[serde(default)]
    pub tree: Option<String>,
    /// The task, for a message on its thread.
    #[serde(default)]
    pub task: Option<String>,
    /// The ask store's thread, for a chat.
    #[serde(default)]
    pub thread: Option<String>,
    /// What the owner said; the ask keeps it short ([`short`]).
    pub words: String,
    /// When the owner said it; the store's clock when none.
    #[serde(default)]
    pub at: Option<f64>,
}

impl NewAsk {
    /// The one key the ask is about, by its kind: a card (a verdict or a message on it), a commented
    /// block, a chat's thread or a task, each on its worktree. Refused when what the kind needs is
    /// missing, or the kind is none of [`ASK_KINDS`].
    pub fn about(&self) -> Result<String, Error> {
        fn given(field: &Option<String>) -> Option<&str> {
            field.as_deref().filter(|v| !v.is_empty())
        }
        let need = |field: &Option<String>, name: &str| -> Result<String, Error> {
            given(field).map(str::to_string).ok_or_else(|| {
                refused(format!(
                    "an ask of kind {} names its {name}",
                    repr(&self.kind)
                ))
            })
        };
        let tree = given(&self.tree).unwrap_or("");
        let key = match self.kind.as_str() {
            "verdict" | "message" => {
                json!([
                    "card",
                    tree,
                    need(&self.path, "document")?,
                    need(&self.card, "card")?
                ])
            }
            "comment" => json!([
                "comment",
                tree,
                need(&self.path, "document")?,
                need(&self.block, "block")?
            ]),
            "chat" => json!(["chat", need(&self.thread, "thread")?]),
            "task" => json!(["task", need(&self.task, "task")?]),
            other => {
                return Err(refused(format!(
                    "an ask's kind is one of {}, not {}",
                    ASK_KINDS.join(", "),
                    repr(other)
                )));
            }
        };
        Ok(key.to_string())
    }
}

/// `words` on one line, at most [`WORDS`] characters, cut with `…`.
pub fn short(words: &str) -> String {
    let line = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= WORDS {
        return line;
    }
    let mut cut: String = line.chars().take(WORDS - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// The variables [`sql::ON_THREAD`] reads beside a post's own: what the ask about task `slug` is about,
/// and the post's words as an ask keeps them.
pub(super) fn on_thread_vars(slug: &str, text: &str) -> (String, String) {
    let about = NewAsk {
        kind: "task".into(),
        task: Some(slug.into()),
        ..NewAsk::default()
    }
    .about()
    .expect("a task's ask names its task");
    (about, short(text))
}

#[derive(Deserialize)]
struct Listed {
    open: Vec<Ask>,
    answered: Vec<Ask>,
}

/// A refusal's text as a write throws it.
fn thrown(text: String) -> Value {
    Value::String(format!("{REFUSED}{text}"))
}

fn no_ask(sid: i64) -> Value {
    thrown(format!("no ask {sid}"))
}

fn answered(sid: i64) -> Value {
    thrown(format!("ask {sid} is answered already"))
}

impl Store {
    /// Open an ask on the owner's line `new`, or join the line to the ask about the same thing while
    /// that one is open, which keeps its words and its age and counts the line. Refused when the kind
    /// is none of [`ASK_KINDS`], what it is about is not named, or a task's ask names no task on the
    /// board.
    pub fn open_ask(&self, new: &NewAsk) -> Result<Ask, Error> {
        let vars = self.ask_vars(new)?;
        self.retrying(|| self.transact(both(sql::OPEN.as_str(), pg::open()), vars.clone()))
    }

    /// `JOIN_OR_OPEN`'s variables for the owner's line `new`, checked as [`Store::open_ask`] checks
    /// it.
    pub(super) fn ask_vars(&self, new: &NewAsk) -> Result<Value, Error> {
        let about = new.about()?;
        let words = short(&new.words);
        if words.is_empty() {
            return Err(refused("an ask holds the owner's words".into()));
        }
        let named = |field: &Option<String>| field.clone().filter(|v| !v.is_empty());
        let task = named(&new.task);
        Ok(json!({
            "about": about, "kind": new.kind, "path": named(&new.path), "card": named(&new.card),
            "block": named(&new.block), "tree": named(&new.tree), "task": task,
            "thread": named(&new.thread), "words": words,
            "at": new.at.unwrap_or_else(|| self.now()),
            "no_task": thrown(super::work::no_task(task.as_deref().unwrap_or("")).to_string()),
        }))
    }

    /// Ask `sid`, open or answered.
    pub fn ask(&self, sid: i64) -> Result<Ask, Error> {
        let found: Option<Ask> =
            self.retrying(|| self.read(both(sql::ASK, pg::ask()), json!({"sid": sid})))?;
        found.ok_or_else(|| refused(format!("no ask {sid}")))
    }

    /// The open asks, oldest first; then, with `answered_since`, the asks answered since then, the
    /// newest first.
    pub fn asks(&self, answered_since: Option<f64>) -> Result<Vec<Ask>, Error> {
        let listed: Listed = self.retrying(|| {
            self.read(
                both(sql::ASKS, pg::asks()),
                json!({"since": answered_since}),
            )
        })?;
        Ok(listed.open.into_iter().chain(listed.answered).collect())
    }

    /// Ask `sid` held by `holder` from now (none: by nobody), the holder before it kept with `why` it
    /// moved on. Holding it as it is held changes nothing. Refused for an answered ask or a session
    /// the board does not know.
    pub fn hold_ask(&self, sid: i64, holder: Option<&str>, why: &str) -> Result<Ask, Error> {
        let vars = json!({
            "sid": sid, "holder": holder, "why": why, "now": self.now(),
            "no_ask": no_ask(sid), "answered": answered(sid),
            "unknown": thrown(unknown_actor(&self.board_command(), holder.unwrap_or(""))),
        });
        self.retrying(|| self.transact(both(sql::HOLD, pg::hold()), vars.clone()))
    }

    /// Ask `sid` answered now, by `by` when the answer names its session, the answer being where it
    /// is (`answer`). The first answer closes it, whoever holds it; a later one changes nothing.
    pub fn answer_ask(&self, sid: i64, by: Option<&str>, answer: &str) -> Result<Ask, Error> {
        let vars = json!({
            "sid": sid, "by": by, "answer": answer, "now": self.now(), "no_ask": no_ask(sid),
        });
        self.retrying(|| self.transact(both(sql::ANSWER, pg::answer()), vars.clone()))
    }

    /// The open ask about what `about` names ([`NewAsk::about`]) answered now, as [`Store::answer_ask`]
    /// answers one: the ask it answered, or none when none was open.
    pub fn answer_about(
        &self,
        about: &NewAsk,
        by: Option<&str>,
        answer: &str,
    ) -> Result<Option<Ask>, Error> {
        let vars = json!({
            "about": about.about()?, "by": by, "answer": answer, "now": self.now(),
        });
        self.retrying(|| self.transact(both(sql::ANSWER_ABOUT, pg::answer_about()), vars.clone()))
    }

    /// Ask `sid`, while open, marked as what it is about read gone (`why`), or no longer (none): the
    /// Symphony says so on an ask whose card, comment or thread it cannot find, which stays open,
    /// since that is no answer. Marking it as it is marked changes nothing, nor does a mark on an
    /// answered ask.
    pub fn mark_gone(&self, sid: i64, why: Option<&str>) -> Result<Ask, Error> {
        let vars = json!({"sid": sid, "why": why, "now": self.now(), "no_ask": no_ask(sid)});
        self.retrying(|| self.transact(both(sql::GONE, pg::gone()), vars.clone()))
    }

    /// Ask `sid` closed by hand by the session `by`, as needing no answer, saying `why`. Refused for an
    /// answered ask, a session the board does not know, or no reason.
    pub fn close_ask(&self, sid: i64, by: &str, why: &str) -> Result<Ask, Error> {
        if why.trim().is_empty() {
            return Err(refused(format!(
                "say why ask {sid} needs no answer: `just tasks asks close {sid} \"WHY\"`"
            )));
        }
        let vars = json!({
            "sid": sid, "by": by, "why": why.trim(), "now": self.now(),
            "no_ask": no_ask(sid), "answered": answered(sid),
            "unknown": thrown(unknown_actor(&self.board_command(), by)),
        });
        self.retrying(|| self.transact(both(sql::CLOSE, pg::close()), vars.clone()))
    }
}

/// An ask's age from the owner's first line: until `now` while it is open, until its answer once
/// answered; and whether that makes it late, which only an open ask is.
pub fn age(ask: &Ask, now: f64) -> (f64, bool) {
    let until = match (ask.open, ask.answered) {
        (false, Some(answered)) => answered,
        _ => now,
    };
    let age = (until - ask.asked).max(0.0);
    (age, ask.open && age >= LATE_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> NewAsk {
        NewAsk {
            kind: "verdict".into(),
            path: Some("docs/DECISIONS.md".into()),
            card: Some("owner-asks".into()),
            words: "why not B?".into(),
            ..NewAsk::default()
        }
    }

    #[test]
    fn a_verdict_and_a_message_on_one_card_are_about_the_same_thing() {
        let message = NewAsk {
            kind: "message".into(),
            ..card()
        };
        assert_eq!(card().about().unwrap(), message.about().unwrap());
        assert_eq!(
            card().about().unwrap(),
            r#"["card","","docs/DECISIONS.md","owner-asks"]"#
        );
    }

    #[test]
    fn the_same_card_on_a_worktree_is_another_thing() {
        let branch = NewAsk {
            tree: Some("tools/x".into()),
            ..card()
        };
        let empty_tree = NewAsk {
            tree: Some(String::new()),
            ..card()
        };
        assert_ne!(card().about().unwrap(), branch.about().unwrap());
        assert_eq!(card().about().unwrap(), empty_tree.about().unwrap());
    }

    #[test]
    fn each_kind_is_about_what_it_names() {
        let comment = NewAsk {
            kind: "comment".into(),
            path: Some("docs/a.md".into()),
            block: Some("12".into()),
            ..NewAsk::default()
        };
        let chat = NewAsk {
            kind: "chat".into(),
            thread: Some("7".into()),
            ..NewAsk::default()
        };
        let task = NewAsk {
            kind: "task".into(),
            task: Some("sim/jump".into()),
            ..NewAsk::default()
        };
        assert_eq!(
            comment.about().unwrap(),
            r#"["comment","","docs/a.md","12"]"#
        );
        assert_eq!(chat.about().unwrap(), r#"["chat","7"]"#);
        assert_eq!(task.about().unwrap(), r#"["task","sim/jump"]"#);
    }

    #[test]
    fn a_kind_missing_what_it_is_about_or_unknown_is_refused() {
        let no_card = NewAsk {
            card: None,
            ..card()
        };
        assert_eq!(
            no_card.about(),
            Err(Error::Refused(
                "an ask of kind 'verdict' names its card".into()
            ))
        );
        let blank_path = NewAsk {
            path: Some(String::new()),
            ..card()
        };
        assert_eq!(
            blank_path.about(),
            Err(Error::Refused(
                "an ask of kind 'verdict' names its document".into()
            ))
        );
        let chat = NewAsk {
            kind: "chat".into(),
            ..NewAsk::default()
        };
        assert_eq!(
            chat.about(),
            Err(Error::Refused(
                "an ask of kind 'chat' names its thread".into()
            ))
        );
        let other = NewAsk {
            kind: "approval".into(),
            ..card()
        };
        assert_eq!(
            other.about(),
            Err(Error::Refused(
                "an ask's kind is one of verdict, message, comment, chat, task, not 'approval'"
                    .into()
            ))
        );
    }

    #[test]
    fn the_words_are_kept_on_one_line_and_short() {
        assert_eq!(short("  why\n\nnot   B? "), "why not B?");
        assert_eq!(short(""), "");
        // 199 characters are 39 words and their spaces, then a word: the cut ends on it.
        let kept = short(&"word ".repeat(100));
        assert_eq!(kept, format!("{}word…", "word ".repeat(39)));
        assert_eq!(kept.chars().count(), WORDS);
        // A cut that would end on a space ends on the word before it.
        let spaced = short(&format!("{} {}", "a".repeat(WORDS - 2), "b".repeat(10)));
        assert_eq!(spaced, format!("{}…", "a".repeat(WORDS - 2)));
        let exact = "a".repeat(WORDS);
        assert_eq!(short(&exact), exact);
        let over = "é".repeat(WORDS + 1);
        assert_eq!(short(&over), format!("{}…", "é".repeat(WORDS - 1)));
    }

    #[test]
    fn an_open_ask_is_late_from_ten_minutes_and_an_answered_one_never() {
        let mut ask = Ask {
            asked: 1000.0,
            open: true,
            ..Ask::default()
        };
        assert_eq!(age(&ask, 1000.0 + LATE_SECONDS - 1.0), (599.0, false));
        assert_eq!(age(&ask, 1000.0 + LATE_SECONDS), (600.0, true));
        assert_eq!(age(&ask, 900.0), (0.0, false));
        // Answered after 20 minutes: its age stops at the answer, and it is not late.
        ask.open = false;
        ask.answered = Some(1000.0 + 1200.0);
        assert_eq!(age(&ask, 5000.0), (1200.0, false));
        // A closed row with no answer time (none the board writes) ages to now.
        ask.answered = None;
        assert_eq!(age(&ask, 5000.0), (4000.0, false));
    }
}
