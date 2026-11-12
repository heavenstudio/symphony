//! `asks`: what the owner waits on an agent for ([`crate::Ask`]), the open ones oldest first, each
//! with what it is about, its age, who holds it and since when; `--all` adds those answered in the
//! last day, the newest first; `--json` says them as the board holds them. `asks close ID WHY` closes
//! one by hand, as needing no answer.

use super::{Context, Stop};
use crate::cli::shape;
use crate::pyjson::{self, Py};
use crate::store::asks::age;
use crate::{Ask, Store};

/// How far back `--all` lists the answered asks.
const DAY: f64 = 86_400.0;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    if c.parsed.get("action") == Some("close") {
        return close(store, c);
    }
    let now = store.now();
    let asks = store.asks(c.parsed.flag("all").then_some(now - DAY))?;
    if c.parsed.flag("json") {
        let json = Py::obj(vec![(
            "asks",
            Py::List(asks.iter().map(|a| shape::ask(a, now)).collect()),
        )]);
        c.say(&pyjson::dumps(&json));
        return Ok(());
    }
    if !asks.iter().any(|a| a.open) {
        c.say("no open asks");
    }
    for a in &asks {
        c.say(&line(a, now));
        c.say(&format!("  \"{}\"", a.words));
    }
    Ok(())
}

fn close(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let actor = c.actor()?;
    let id = c.parsed.get("id").unwrap_or("");
    let id: i64 = id.trim().trim_start_matches('#').parse().map_err(|_| {
        Stop::Refused(format!(
            "name the ask by its number: `just tasks asks close ID \"WHY\"`, not {}",
            crate::pyjson::repr(id)
        ))
    })?;
    let a = store.close_ask(id, &actor, c.parsed.get("why").unwrap_or(""))?;
    c.say(&format!(
        "closed ask {}: {}",
        a.id,
        a.answer.as_deref().unwrap_or("")
    ));
    Ok(())
}

/// One ask on one line: its number, kind and what it is about, then its age and holder while it is
/// open, or who answered it and how soon.
pub(crate) fn line(a: &Ask, now: f64) -> String {
    let head = format!("ask {}  {}  {}", a.id, a.kind, about(a));
    if a.open {
        let (age, late) = age(a, now);
        let late = if late { ", late" } else { "" };
        let holder = match (&a.holder, a.held) {
            (Some(holder), Some(held)) => {
                format!("held by {holder} for {}", minutes(now - held))
            }
            (Some(holder), None) => format!("held by {holder}"),
            (None, _) => "held by nobody".into(),
        };
        let gone = match (&a.gone, a.gone_since) {
            (Some(why), Some(since)) => format!("  gone for {}: {why}", minutes(now - since)),
            (Some(why), None) => format!("  gone: {why}"),
            (None, _) => String::new(),
        };
        return format!("{head}  {}{late}  {holder}{gone}", minutes(age));
    }
    let after = minutes(a.answered.unwrap_or(a.asked) - a.asked);
    let how = if a.closed { "closed" } else { "answered" };
    let by = a
        .answered_by
        .as_deref()
        .map(|by| format!(" by {by}"))
        .unwrap_or_default();
    let answer = a
        .answer
        .as_deref()
        .filter(|answer| !answer.is_empty())
        .map(|answer| format!(": {answer}"))
        .unwrap_or_default();
    format!("{head}  {how}{by} after {after}{answer}")
}

/// What an ask is about, in words: the card on its document, the commented block, the chat's thread,
/// the task; and the worktree it was said on.
fn about(a: &Ask) -> String {
    let path = a.path.as_deref().unwrap_or("");
    let said = match a.kind.as_str() {
        "verdict" | "message" => format!("{} on {path}", a.card.as_deref().unwrap_or("")),
        "comment" => format!("{path} block {}", a.block.as_deref().unwrap_or("")),
        "chat" if path.is_empty() => format!("chat {}", a.thread.as_deref().unwrap_or("")),
        "chat" => format!("chat {} on {path}", a.thread.as_deref().unwrap_or("")),
        _ => a.task.as_deref().unwrap_or("").to_string(),
    };
    match a.tree.as_deref() {
        Some(tree) => format!("{said} @{tree}"),
        None => said,
    }
}

/// `seconds` as whole minutes, and hours past the hour: `0 min`, `14 min`, `1 h 12 min`.
pub(crate) fn minutes(seconds: f64) -> String {
    let whole = (seconds.max(0.0) / 60.0).floor() as i64;
    match (whole / 60, whole % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::HeldRow;

    fn open() -> Ask {
        Ask {
            id: 3,
            kind: "verdict".into(),
            path: Some("docs/DECISIONS.md".into()),
            card: Some("owner-asks".into()),
            words: "why not B?".into(),
            asked: 1000.0,
            lines: 1,
            last: 1000.0,
            open: true,
            ..Ask::default()
        }
    }

    #[test]
    fn minutes_are_whole_and_hours_past_sixty() {
        assert_eq!(minutes(0.0), "0 min");
        assert_eq!(minutes(59.9), "0 min");
        assert_eq!(minutes(14.0 * 60.0 + 30.0), "14 min");
        assert_eq!(minutes(3600.0), "1 h");
        assert_eq!(minutes(72.0 * 60.0), "1 h 12 min");
        assert_eq!(minutes(-5.0), "0 min");
    }

    #[test]
    fn an_open_ask_says_its_age_and_holder_and_late_past_ten_minutes() {
        let mut a = open();
        assert_eq!(
            line(&a, 1000.0 + 4.0 * 60.0),
            "ask 3  verdict  owner-asks on docs/DECISIONS.md  4 min  held by nobody"
        );
        a.holder = Some("umber-stoat".into());
        a.held = Some(1000.0 + 120.0);
        a.tree = Some("tools/orphan-processes".into());
        assert_eq!(
            line(&a, 1000.0 + 14.0 * 60.0),
            "ask 3  verdict  owner-asks on docs/DECISIONS.md @tools/orphan-processes  14 min, late  held by umber-stoat for 12 min"
        );
    }

    #[test]
    fn an_open_ask_whose_card_is_gone_says_so_and_for_how_long() {
        let mut a = open();
        a.gone = Some("the worktree of tools/x is gone".into());
        a.gone_since = Some(1000.0 + 60.0);
        assert_eq!(
            line(&a, 1000.0 + 4.0 * 60.0),
            "ask 3  verdict  owner-asks on docs/DECISIONS.md  4 min  held by nobody  gone for 3 min: the worktree of tools/x is gone"
        );
        a.gone_since = None;
        assert!(line(&a, 1300.0).ends_with("  gone: the worktree of tools/x is gone"));
        // An answered ask says its answer, not where its card went.
        a.open = false;
        a.answered = Some(1300.0);
        assert!(!line(&a, 1300.0).contains("gone"), "{}", line(&a, 1300.0));
    }

    #[test]
    fn an_answered_or_closed_ask_says_who_and_how_soon() {
        let mut a = open();
        a.open = false;
        a.answered = Some(1000.0 + 6.0 * 60.0);
        a.answered_by = Some("brisk-lark".into());
        a.answer = Some("the card".into());
        a.before = vec![HeldRow {
            handle: "nimble-curlew".into(),
            since: 1000.0,
            until: 1600.0,
            why: "no answer in 10 min".into(),
        }];
        assert_eq!(
            line(&a, 99_999.0),
            "ask 3  verdict  owner-asks on docs/DECISIONS.md  answered by brisk-lark after 6 min: the card"
        );
        a.closed = true;
        a.answered_by = None;
        a.answer = None;
        assert_eq!(
            line(&a, 99_999.0),
            "ask 3  verdict  owner-asks on docs/DECISIONS.md  closed after 6 min"
        );
    }

    #[test]
    fn each_kind_says_what_it_is_about() {
        let comment = Ask {
            kind: "comment".into(),
            card: None,
            block: Some("12".into()),
            ..open()
        };
        assert_eq!(about(&comment), "docs/DECISIONS.md block 12");
        let chat = Ask {
            kind: "chat".into(),
            thread: Some("7".into()),
            ..open()
        };
        assert_eq!(about(&chat), "chat 7 on docs/DECISIONS.md");
        let bare_chat = Ask { path: None, ..chat };
        assert_eq!(about(&bare_chat), "chat 7");
        let task = Ask {
            kind: "task".into(),
            path: None,
            task: Some("sim/jump".into()),
            ..open()
        };
        assert_eq!(about(&task), "sim/jump");
    }
}
