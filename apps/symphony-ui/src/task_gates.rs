//! The task page's gates: every stamp or check the task needs to land, each met, missing, waived,
//! held or unknown, with who, when and why, drawn the same way for every project from
//! `GET /api/task-gates` ([`symphony_api::gates`], docs/TASKS.md#gates).
//!
//! The section reads its answer when it nears the screen, and the head, the brief and the message
//! box never wait on it: a project's answer may start its landing tool. While it reads it holds its
//! height. A task nothing gates shows no section, and a read that failed says so in its place while
//! the rest of the page works. On a phone a gate is three lines (its name and state, its detail,
//! who and when); wide, one line of columns. Its style comes with it ([`STYLE`]).

use crate::viewport::NearViewport as Near;
use dioxus::prelude::*;
use symphony_api::{Gate, GateState, TaskGates};

/// The section's style. Not a stylesheet asset: the shell names every kind's stylesheets, and so
/// would carry this one's name on every first visit, `/` and `/tasks` included.
pub const STYLE: &str = include_str!("../assets/task-gates.css");

/// The height the section holds before and while it reads, in px: a head and two gates.
const HOLD: u32 = 160;

/// What the section has of the task's gates.
#[derive(Clone, Debug, PartialEq)]
pub enum Gates {
    /// Not answered yet.
    Reading,
    /// The answer; no gates for a task nothing gates, or one the board does not have.
    Read(TaskGates),
    /// Why the read failed.
    Failed(String),
}

impl Gates {
    /// The section's state from a read: `None` is the server's 404, a task it has no gates for.
    #[cfg(any(test, target_arch = "wasm32"))]
    fn of(answer: Result<Option<TaskGates>, String>) -> Gates {
        match answer {
            Ok(answer) => Gates::Read(answer.unwrap_or_default()),
            Err(why) => Gates::Failed(why),
        }
    }
}

/// The gates of the task `slug`, read when the section nears the screen and again when the task
/// changes (`revision`, its `updated`), the last answer shown meanwhile.
#[component]
pub fn TaskGatesPanel(slug: String, revision: f64) -> Element {
    let mut gates = use_signal(|| Gates::Reading);
    let mut near = use_signal(|| false);
    let asked = use_signal(|| 0_u64);
    #[cfg(target_arch = "wasm32")]
    use_hook(style);
    let ask = slug.clone();
    use_effect(use_reactive!(|ask, revision| {
        let _ = revision;
        if near() {
            read(ask, gates, asked);
        }
    }));
    let again = slug.clone();
    rsx! { Near { id: "gates-{slug}", height: HOLD, onenter: move |_| near.set(true),
        GatesView { gates: gates(), now: now(), onretry: move |_| { gates.set(Gates::Reading); read(again.clone(), gates, asked); } }
    } }
}

/// Ask for the gates of `slug`; only the latest of the section's reads is shown.
fn read(slug: String, mut gates: Signal<Gates>, mut asked: Signal<u64>) {
    let ask = *asked.peek() + 1;
    asked.set(ask);
    #[cfg(target_arch = "wasm32")]
    spawn(async move {
        let path = format!(
            "{}?slug={}",
            symphony_api::TASK_GATES_PATH,
            crate::request::encode(&slug)
        );
        let answer = crate::request::get_found(&crate::task_live::url(&path)).await;
        if *asked.peek() == ask {
            gates.set(Gates::of(answer));
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (slug, &mut gates, ask);
}

/// Put [`STYLE`] in the page once.
#[cfg(target_arch = "wasm32")]
fn style() {
    const ID: &str = "task-gates-style";
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if document.get_element_by_id(ID).is_some() {
        return;
    }
    if let (Ok(style), Some(head)) = (document.create_element("style"), document.head()) {
        style.set_id(ID);
        style.set_text_content(Some(STYLE));
        let _ = head.append_child(&style);
    }
}

/// Now, in seconds since the epoch.
fn now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() / 1000.0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64())
    }
}

/// The section for `gates`, its times told against `now`: nothing for a task nothing gates.
#[component]
pub fn GatesView(gates: Gates, now: f64, onretry: EventHandler<()>) -> Element {
    let head = rsx! { h2 { class: "eyebrow", id: "task-gates-title", "Gates" } };
    match gates {
        Gates::Reading => rsx! {
            section { class: "task-gates card", "data-gates": "reading", aria_labelledby: "task-gates-title", aria_busy: "true", style: "min-height:{HOLD}px;box-sizing:border-box",
                div { class: "gates-head", {head} }
                p { class: "gates-note", role: "status", "Reading the gates…" }
            }
        },
        Gates::Failed(why) => rsx! {
            section { class: "task-gates card", "data-gates": "failed", aria_labelledby: "task-gates-title",
                div { class: "gates-head", {head} }
                p { class: "gates-note", role: "status", "{failed(&why)}" }
                crate::components::button::Button { r#type: "button", onclick: move |_| onretry.call(()), "Retry" }
            }
        },
        Gates::Read(answer) if answer.gates.is_empty() => rsx! {},
        Gates::Read(answer) => {
            let today = local_minutes(now);
            let waits = waits_on(&answer);
            rsx! {
                section { class: "task-gates card", "data-gates": "read", aria_labelledby: "task-gates-title",
                    div { class: "gates-head",
                        {head}
                        if let Some((state, text)) = waits { span { class: "pill gate-{state.as_str()} gates-waits", "{text}" } }
                        span { class: "gates-counts", "{counts(&answer)}" }
                    }
                    ul { class: "gates",
                        for gate in answer.gates {
                            GateRow { key: "{gate.name}", byline: byline(&gate, today), gate }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn GateRow(gate: Gate, byline: String) -> Element {
    let state = gate.state.as_str();
    let detail = rsx! { for (code, part) in code_spans(&gate.detail) { if code { code { "{part}" } } else { "{part}" } } };
    let why = gate.why.filter(|why| !why.is_empty());
    rsx! {
        li { class: "gate gate-{state}", "data-gate": "{gate.name}", "data-state": "{state}",
            span { class: "gate-name", "{gate.name}" }
            span { class: "pill gate-state", "{state}" }
            span { class: "gate-detail",
                if let Some(link) = gate.link.filter(|_| !gate.detail.is_empty()) { a { href: "{link}", {detail} } } else { {detail} }
                if let Some(why) = why { span { class: "gate-why", if !gate.detail.is_empty() { ": " } "{why}" } }
            }
            span { class: "gate-by", "{byline}" }
        }
    }
}

/// What a read that failed says, from the reason the transport gave.
fn failed(why: &str) -> String {
    let why = why.trim().trim_end_matches('.');
    if why.is_empty() {
        "The gates could not be read. The rest of the page still works.".into()
    } else {
        format!("The gates could not be read: {why}. The rest of the page still works.")
    }
}

/// The gates the task waits on, named, and the state of the first: `waits on certificate`,
/// `waits on certificate and owner`, `waits on a, b and c`; `None` when it waits on none.
pub fn waits_on(answer: &TaskGates) -> Option<(GateState, String)> {
    let state = answer.waiting().next()?.state;
    let names: Vec<&str> = answer.waiting().map(|gate| gate.name.as_str()).collect();
    let (last, rest) = names.split_last()?;
    let named = if rest.is_empty() {
        (*last).to_string()
    } else {
        format!("{} and {last}", rest.join(", "))
    };
    Some((state, format!("waits on {named}")))
}

/// How many gates are in each state, what no longer stands in the way first: `2 met · 1 waived ·
/// 1 missing`. States with none are left out.
pub fn counts(answer: &TaskGates) -> String {
    [
        GateState::Met,
        GateState::Waived,
        GateState::Unknown,
        GateState::Held,
        GateState::Missing,
    ]
    .into_iter()
    .filter_map(|state| match answer.count(state) {
        0 => None,
        count => Some(format!("{count} {}", state.as_str())),
    })
    .collect::<Vec<_>>()
    .join(" · ")
}

/// Who and when, and the commit when the gate's words do not already name it:
/// `lucid-heron · 14:05`, `studio · 11-10 02:14 · 4be91c2`. `today` is now in
/// [`local_minutes`].
pub fn byline(gate: &Gate, today: i64) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(by) = gate.by.as_deref().filter(|by| !by.is_empty()) {
        parts.push(by.to_string());
    }
    if let Some(at) = gate.at {
        parts.push(when(local_minutes(at), today));
    }
    if let Some(commit) = gate.commit.as_deref().filter(|commit| !commit.is_empty()) {
        let short: String = commit.chars().take(7).collect();
        let said = |text: &str| text.contains(&short);
        if !said(&gate.detail) && !gate.why.as_deref().is_some_and(said) {
            parts.push(short);
        }
    }
    parts.join(" · ")
}

/// The minute `seconds` since the epoch falls in on the reader's clock, counted from 1970-01-01
/// 00:00 there: the browser's time zone at that moment, UTC on the host.
pub fn local_minutes(seconds: f64) -> i64 {
    let utc = (seconds / 60.0).floor() as i64;
    #[cfg(target_arch = "wasm32")]
    {
        let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(seconds * 1000.0));
        utc - date.get_timezone_offset() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        utc
    }
}

/// A time as short as `now` lets it be told, both in [`local_minutes`]: `14:05` on the same day,
/// `11-10 02:14` in the same year, `2025-11-10` before.
pub fn when(at: i64, now: i64) -> String {
    let (day, minute) = (at.div_euclid(1440), at.rem_euclid(1440));
    let clock = format!("{:02}:{:02}", minute / 60, minute % 60);
    if day == now.div_euclid(1440) {
        return clock;
    }
    let (year, month, date) = civil(day);
    if year == civil(now.div_euclid(1440)).0 {
        format!("{month:02}-{date:02} {clock}")
    } else {
        format!("{year:04}-{month:02}-{date:02}")
    }
}

/// The date of the proleptic Gregorian calendar `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let date = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, date)
}

/// `text` cut at its backticks: `(true, code)` for what a pair of them encloses, `(false, words)`
/// for the rest. A backtick with no partner is kept as written.
pub fn code_spans(text: &str) -> Vec<(bool, &str)> {
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let Some(close) = rest[open + 1..].find('`') else {
            break;
        };
        if open > 0 {
            spans.push((false, &rest[..open]));
        }
        spans.push((true, &rest[open + 1..open + 1 + close]));
        rest = &rest[open + 1 + close + 1..];
    }
    if !rest.is_empty() {
        spans.push((false, rest));
    }
    spans
}

#[cfg(test)]
#[path = "task_gates/tests.rs"]
mod tests;
