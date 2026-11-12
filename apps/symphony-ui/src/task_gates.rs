//! The task page's gates: every stamp or check the task needs to land, each met, missing, waived,
//! held or unknown, with who, when and why, drawn the same way for every project from
//! `GET /api/task-gates` ([`symphony_api::gates`], docs/TASKS.md#gates).
//!
//! The section reads its answer when it nears the screen, and the head, the brief and the message
//! box never wait on it: a project's answer may start its landing tool. Until it has an answer it
//! is its own outline, the card with a shadow line for each gate it expects ([`Room`]): as many as
//! the server last answered for the task, else as this browser last drew, else four, and none
//! when it knows there are none; so the page below does not move when the gates arrive. It reads
//! them again when the task changes, one of its runs starts or ends, its branch moves, a verdict
//! lands on its thread or a card in a document it links comes or goes ([`concerns`]), the last
//! answer shown meanwhile, and only the latest read is drawn: a newer one drops the one before.
//! A task nothing gates shows no section, and a read that failed says so in its place while the
//! rest of the page works.
//!
//! Wide, a gate is one line of bounded columns, each part cut short with its whole text kept in
//! its title; on a phone, three lines (its name and state, its detail, who and when). A link is
//! drawn only for a path of the app or a web address ([`symphony_api::linkable`]), on the words of
//! the detail that are its evidence ([`evidence`]). Plugin text keeps its own direction. Its style
//! comes with it ([`STYLE`]).

use dioxus::core::Task;
use dioxus::prelude::*;
use std::ops::Range;
use symphony_api::{Gate, GateState, GatesSeen, MachineWork, TaskGates};

use crate::tasks::Nudge;

/// The section's style. Not a stylesheet asset: the shell names every kind's stylesheets, and so
/// would carry this one's name on every first visit, `/` and `/tasks` included.
pub const STYLE: &str = include_str!("../assets/task-gates.css");

/// What the section has of the task's gates.
#[derive(Clone, Debug, PartialEq)]
pub enum Gates {
    /// Not answered yet.
    Reading,
    /// The answer; no gates for a task nothing gates, or one the board does not have.
    Read(TaskGates),
    /// The server did not answer in time.
    Slow,
    /// Why the read failed.
    Failed(String),
}

impl Gates {
    /// The section's state from a read: `None` is the server's 404, a task it has no gates for.
    #[cfg(any(test, target_arch = "wasm32"))]
    fn of(answer: Result<Option<TaskGates>, String>) -> Gates {
        match answer {
            Ok(answer) => Gates::Read(answer.unwrap_or_default()),
            Err(why) if why == crate::task_save::NO_ANSWER => Gates::Slow,
            Err(why) => Gates::Failed(why),
        }
    }
}

/// The room the section holds before its gates are read: how many it expects and how many of
/// them the task waits on (a phone puts the waits-on pill on a line of its own), and the height
/// this browser drew it at for a window this wide, `(width, height)` in px.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Room {
    pub count: usize,
    pub waiting: usize,
    pub drawn: Option<(u32, u32)>,
}

/// The room for a task nothing says anything of yet: four gates, one waited on.
pub const DEFAULT_ROOM: Room = Room {
    count: 4,
    waiting: 1,
    drawn: None,
};

impl Room {
    /// The room from what the server last answered for the task (`seen`, with the page) and what
    /// this browser kept (`kept`): the server's count, with the browser's height while the two
    /// agree; else the browser's; else [`DEFAULT_ROOM`].
    pub fn of(seen: Option<GatesSeen>, kept: Option<Room>) -> Room {
        match (seen, kept) {
            (Some(seen), Some(kept))
                if (kept.count, kept.waiting) == (seen.count, seen.waiting) =>
            {
                kept
            }
            (Some(seen), _) => Room {
                count: seen.count,
                waiting: seen.waiting,
                drawn: None,
            },
            (None, Some(kept)) => kept,
            (None, None) => DEFAULT_ROOM,
        }
    }

    /// The height to hold in a window `width` px wide: the one drawn at that width, if any.
    pub fn held(&self, width: Option<u32>) -> Option<u32> {
        self.drawn
            .filter(|(drawn, _)| Some(*drawn) == width)
            .map(|(_, height)| height)
    }
}

/// Where this browser keeps the room each task's gates took, newest first, as lines of
/// `COUNT WAITING WIDTH HEIGHT SLUG` (a width of 0 drew nothing).
const KEPT: &str = "gates-room";
/// How many tasks' rooms a browser keeps.
const KEPT_MAX: usize = 64;

/// The room `kept` (as [`KEPT`] holds it) gives the task `slug`.
pub fn recall(kept: &str, slug: &str) -> Option<Room> {
    kept.lines().find_map(|line| {
        let mut parts = line.splitn(5, ' ');
        let mut number = || parts.next()?.parse::<u32>().ok();
        let (count, waiting, width, height) = (number()?, number()?, number()?, number()?);
        (parts.next()? == slug).then_some(Room {
            count: count as usize,
            waiting: waiting as usize,
            drawn: (width > 0).then_some((width, height)),
        })
    })
}

/// `kept` with `room` as the task `slug`'s, first, its line before and the oldest past
/// [`KEPT_MAX`] dropped.
pub fn keep(kept: &str, slug: &str, room: Room) -> String {
    let (width, height) = room.drawn.unwrap_or_default();
    let line = format!("{} {} {width} {height} {slug}", room.count, room.waiting);
    std::iter::once(line.as_str())
        .chain(
            kept.lines()
                .filter(|old| old.splitn(5, ' ').nth(4) != Some(slug)),
        )
        .take(KEPT_MAX)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The gates of the task `slug`, read when the section nears the screen and again when the task
/// changes (`revision`, its `updated`), when its machine work does (`work`, [`work_of`]) and on a
/// board event that concerns it (`nudges`, [`concerns`] with its `links`), the last answer shown
/// meanwhile. `seen` is what the server last answered for it, the room it holds until then.
#[component]
pub fn TaskGatesPanel(
    slug: String,
    revision: f64,
    #[props(default)] seen: Option<GatesSeen>,
    #[props(default)] work: String,
    #[props(default)] links: Vec<String>,
    #[props(default)] nudges: Vec<(u64, Nudge)>,
) -> Element {
    let mut gates = use_signal(|| Gates::Reading);
    // How many reasons to read there have been, and how many the last read answered.
    let mut wanted = use_signal(|| 0_u64);
    let mut answered = use_signal(|| 0_u64);
    let mut heard = use_signal(|| nudges.last().map_or(0, |(number, _)| *number));
    let running: Signal<Option<Task>> = use_signal(|| None);
    let room = use_hook(|| Room::of(seen, kept(&slug)));
    let (near, reveal) = crate::viewport::use_visibility(false);
    #[cfg(target_arch = "wasm32")]
    use_hook(style);
    use_effect(use_reactive!(|revision, work| {
        let _ = (revision, work);
        *wanted.write() += 1;
    }));
    use_effect(use_reactive!(|nudges, links, slug| {
        let last = nudges.last().map_or(0, |(number, _)| *number);
        let from = *heard.peek();
        if last > from {
            heard.set(last);
            let branch = match &*gates.peek() {
                Gates::Read(answer) if !answer.branch.is_empty() => answer.branch.clone(),
                _ => slug.clone(),
            };
            if nudges
                .iter()
                .any(|(number, nudge)| *number > from && concerns(nudge, &slug, &branch, &links))
            {
                *wanted.write() += 1;
            }
        }
    }));
    let asked = slug.clone();
    use_effect(move || {
        let want = wanted();
        if near() && want != *answered.peek() {
            answered.set(want);
            read(asked.clone(), gates, running);
        }
    });
    // Once an answer is drawn, the room it took in this window, for the next visit.
    #[cfg(target_arch = "wasm32")]
    {
        let drawn = slug.clone();
        use_effect(move || {
            if let Gates::Read(answer) = &*gates.read() {
                remember(&drawn, answer.seen());
            }
        });
    }
    rsx! {
        div { id: "gates-{slug}", class: "task-gates-slot",
            if !near() { div { class: "viewport-sentinel", "aria-hidden": "true", ..reveal } }
            GatesView { gates: gates(), now: now(), room, width: window_width(),
                onretry: move |_| { gates.set(Gates::Reading); *wanted.write() += 1; } }
        }
    }
}

/// Ask for the gates of `slug`, dropping the read still running: only the latest is drawn, and
/// the one dropped is aborted (`request::send`), so nothing waits on its answer.
fn read(slug: String, mut gates: Signal<Gates>, mut running: Signal<Option<Task>>) {
    if let Some(task) = running.write().take() {
        task.cancel();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let task = spawn(async move {
            let path = format!(
                "{}?slug={}",
                symphony_api::TASK_GATES_PATH,
                crate::request::encode(&slug)
            );
            let answer = Gates::of(crate::request::get_found(&crate::task_live::url(&path)).await);
            running.set(None);
            gates.set(answer);
        });
        running.set(Some(task));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (slug, &mut gates);
}

/// The room this browser kept for the task `slug`.
fn kept(slug: &str) -> Option<Room> {
    recall(&crate::platform::current().load(KEPT)?, slug)
}

/// Keep the room the task `slug`'s gates now take: `seen`, at the height its section was drawn.
#[cfg(target_arch = "wasm32")]
fn remember(slug: &str, seen: GatesSeen) {
    let drawn = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(&format!("gates-{slug}")))
        .and_then(|slot| slot.query_selector("section.task-gates").ok().flatten())
        .map(|section| section.get_bounding_client_rect().height().round() as u32)
        .zip(window_width())
        .map(|(height, width)| (width, height));
    let room = Room {
        count: seen.count,
        waiting: seen.waiting,
        drawn,
    };
    let platform = crate::platform::current();
    let kept = platform.load(KEPT).unwrap_or_default();
    platform.save(KEPT, &keep(&kept, slug, room));
}

/// The window's width in px; none on the host.
fn window_width() -> Option<u32> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?
            .inner_width()
            .ok()?
            .as_f64()
            .map(|width| width.round() as u32)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
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

/// What of `work` the task `slug` runs, as one text: it changes when one of the task's builds or
/// tests starts, moves on or ends (its check may change with it), and not with each tick of the
/// machines' observation.
pub fn work_of(work: &MachineWork, slug: &str) -> String {
    let mut runs: Vec<String> = work
        .machines
        .iter()
        .flat_map(|machine| &machine.operations)
        .chain(&work.unassigned)
        .filter(|operation| operation.task.as_deref() == Some(slug))
        .map(|operation| {
            format!(
                "{} {} {} {:?}",
                operation.operation_id.as_deref().unwrap_or_default(),
                operation.checkout,
                operation.recipe,
                operation.state
            )
        })
        .collect();
    runs.sort();
    runs.join("\n")
}

/// Whether `nudge` may change the gates of the task `slug`, answered for `branch`, that links the
/// documents `links` (`docs/X.md#part`): a verdict on its thread, its branch's new tip, or a card
/// in a document it links, the owner gate's cards.
pub fn concerns(nudge: &Nudge, slug: &str, branch: &str, links: &[String]) -> bool {
    match nudge {
        Nudge::Verdict(task) => task == slug,
        Nudge::Branch(moved) => moved == branch,
        Nudge::Card(path) => links
            .iter()
            .any(|link| link.split('#').next() == Some(path.as_str())),
    }
}

/// The section for `gates`, its times told against `now`: nothing for a task nothing gates. Until
/// it has an answer it holds `room`, at the height drawn before in a window `width` px wide.
#[component]
pub fn GatesView(
    gates: Gates,
    now: f64,
    room: Room,
    width: Option<u32>,
    onretry: EventHandler<()>,
) -> Element {
    let head = rsx! { h2 { class: "eyebrow", id: "task-gates-title", "Gates" } };
    let note = |said: String| {
        rsx! {
            section { class: "task-gates card", "data-gates": "failed", aria_labelledby: "task-gates-title",
                div { class: "gates-head", {head.clone()} }
                p { class: "gates-note", role: "status", "{said}" }
                crate::components::button::Button { r#type: "button", onclick: move |_| onretry.call(()), "Retry" }
            }
        }
    };
    match gates {
        Gates::Reading if room.count == 0 => rsx! {},
        Gates::Reading => {
            let hold = room
                .held(width)
                .map(|height| format!("min-height:{height}px"))
                .unwrap_or_default();
            rsx! {
                section { class: "task-gates card", "data-gates": "reading", aria_labelledby: "task-gates-title", aria_busy: "true", style: "{hold}",
                    div { class: "gates-head",
                        {head}
                        if room.waiting > 0 { span { class: "pill gates-waits gate-shadow", span { class: "gate-bar", style: "--w:9em" } } }
                        span { class: "gates-counts", span { class: "gate-bar", style: "--w:11em" } }
                    }
                    p { class: "gates-said", role: "status", "Reading the gates…" }
                    ul { class: "gates", aria_hidden: "true",
                        for row in 0..room.count {
                            li { key: "{row}", class: "gate gate-shadow",
                                span { class: "gate-name", span { class: "gate-label", span { class: "gate-bar", style: "--w:{6 + row % 3}em" } } }
                                span { class: "pill gate-state", "\u{a0}" }
                                span { class: "gate-detail", span { class: "gate-bar", style: "--w:{55 + (row * 17) % 35}%" } }
                                span { class: "gate-by", span { class: "gate-bar", style: "--w:7em" } }
                            }
                        }
                    }
                }
            }
        }
        Gates::Slow => note(
            "The gates took too long to answer and were given up on. The rest of the page still works."
                .into(),
        ),
        Gates::Failed(why) => note(failed(&why)),
        Gates::Read(answer) if answer.gates.is_empty() => rsx! {},
        Gates::Read(answer) => {
            let today = local_minutes(now);
            let waits = waits_on(&answer);
            rsx! {
                section { class: "task-gates card", "data-gates": "read", aria_labelledby: "task-gates-title",
                    div { class: "gates-head",
                        {head}
                        if let Some((state, names)) = waits {
                            span { class: "pill gate-{state.as_str()} gates-waits",
                                "waits on "
                                for (i, name) in names.iter().enumerate() {
                                    if i > 0 { if i + 1 == names.len() { " and " } else { ", " } }
                                    span { dir: "auto", "{name}" }
                                }
                            }
                        }
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
fn GateRow(gate: Gate, byline: Vec<String>) -> Element {
    let state = gate.state.as_str();
    let link = gate
        .link
        .clone()
        .filter(|link| symphony_api::linkable(link));
    let why = gate.why.clone().filter(|why| !why.is_empty());
    let said = match &why {
        Some(why) if !gate.detail.is_empty() => format!("{}: {why}", gate.detail),
        Some(why) => why.clone(),
        None => gate.detail.clone(),
    };
    // With no words to put it on, a link goes on the gate's name.
    let (named, link) = match link {
        Some(link) if gate.detail.is_empty() => (Some(link), None),
        link => (None, link),
    };
    let [before, linked, after] = pieces(
        &gate.detail,
        link.as_ref()
            .map(|link| evidence(&gate.detail, link, gate.commit.as_deref())),
    );
    let words = |parts: Vec<(bool, &str)>| {
        rsx! { for (code, part) in parts { if code { code { "{part}" } } else { "{part}" } } }
    };
    rsx! {
        li { class: "gate gate-{state}", "data-gate": "{gate.name}", "data-state": "{state}",
            span { class: "gate-name",
                span { class: "gate-label", dir: "auto", title: "{gate.name}",
                    if let Some(href) = named { a { href: "{href}", "{gate.name}" } } else { "{gate.name}" }
                }
            }
            span { class: "pill gate-state", "{state}" }
            span { class: "gate-detail", title: "{said}",
                if !gate.detail.is_empty() {
                    span { dir: "auto",
                        {words(before)}
                        if let Some(href) = &link { a { href: "{href}", {words(linked)} } }
                        {words(after)}
                    }
                }
                if let Some(why) = why {
                    span { class: "gate-why", if !gate.detail.is_empty() { ": " } span { dir: "auto", "{why}" } }
                }
            }
            span { class: "gate-by", title: "{byline.join(\" · \")}",
                for (i, part) in byline.iter().enumerate() {
                    if i > 0 { " · " }
                    span { dir: "auto", "{part}" }
                }
            }
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

/// The gates the task waits on, by name, and the state of the first; `None` when it waits on none.
/// The pill says `waits on certificate`, `waits on certificate and owner`, `waits on a, b and c`.
pub fn waits_on(answer: &TaskGates) -> Option<(GateState, Vec<String>)> {
    let state = answer.waiting().next()?.state;
    Some((
        state,
        answer.waiting().map(|gate| gate.name.clone()).collect(),
    ))
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

/// Who, when, and the commit when the gate's words do not already name it, each a part of its
/// own: `[lucid-heron, 14:05]`, `[studio, 11-10 02:14, 4be91c2]`. `today` is now in
/// [`local_minutes`].
pub fn byline(gate: &Gate, today: i64) -> Vec<String> {
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
    parts
}

/// The words of `detail` a link to `link` stands on, as option A draws them: the longest run of
/// the link's own path the detail says, a name rather than a route's word (`tools/slot-pool-by-tree`
/// for `/task/tools/slot-pool-by-tree`, never `task`), else the gate's `commit` as the detail says
/// it, else the review round it names (`round 2 of 2`), else all of it.
pub fn evidence(detail: &str, link: &str, commit: Option<&str>) -> Range<usize> {
    let path = if link.starts_with('/') {
        link
    } else {
        let rest = link.split_once("://").map_or("", |(_, rest)| rest);
        rest.find('/').map_or("", |at| &rest[at..])
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut runs: Vec<String> = Vec::new();
    for start in 0..segments.len() {
        for end in start + 1..=segments.len() {
            runs.push(segments[start..end].join("/"));
        }
    }
    runs.sort_by_key(|run| std::cmp::Reverse(run.len()));
    // A name has a digit or a joiner in it; a plain word (`task`, `docs`, `runs`) is the route's.
    let named = |run: &&String| {
        run.chars().count() >= 4
            && run
                .chars()
                .any(|c| c.is_ascii_digit() || "-_./".contains(c))
    };
    let found = runs
        .iter()
        .filter(named)
        .find_map(|run| word(detail, run, |_| false))
        .or_else(|| {
            let short: String = commit?.chars().take(7).collect();
            (short.len() >= 4)
                .then(|| word(detail, &short, |c| c.is_ascii_hexdigit()))
                .flatten()
        })
        .or_else(|| round(detail));
    found.unwrap_or(0..detail.len())
}

/// Where `detail` says `words` as a word of its own, running on over what `more` takes.
fn word(detail: &str, words: &str, more: impl Fn(char) -> bool) -> Option<Range<usize>> {
    let edge = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || "-_/".contains(c)));
    detail.match_indices(words).find_map(|(at, _)| {
        let end = at
            + words.len()
            + detail[at + words.len()..]
                .chars()
                .take_while(|c| more(*c))
                .map(char::len_utf8)
                .sum::<usize>();
        (edge(detail[..at].chars().next_back()) && edge(detail[end..].chars().next()))
            .then_some(at..end)
    })
}

/// Where `detail` names a review round: `round 2` or `round 2 of 2`, in any case.
fn round(detail: &str) -> Option<Range<usize>> {
    let number = |text: &str| text.chars().take_while(char::is_ascii_digit).count();
    // ASCII case leaves every byte where it was, so a place in one is a place in the other.
    let lower = detail.to_ascii_lowercase();
    lower.match_indices("round ").find_map(|(at, said)| {
        let digits = number(&detail[at + said.len()..]);
        if digits == 0
            || detail[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        {
            return None;
        }
        let mut end = at + said.len() + digits;
        if let Some(rest) = detail[end..].strip_prefix(" of ") {
            let total = number(rest);
            if total > 0 {
                end += " of ".len() + total;
            }
        }
        Some(at..end)
    })
}

/// `text` cut at its backticks as [`code_spans`] does, in three: before `linked`, inside it and
/// after it (all before it when there is none).
pub fn pieces(text: &str, linked: Option<Range<usize>>) -> [Vec<(bool, &str)>; 3] {
    let linked = linked.unwrap_or(text.len()..text.len());
    let mut parts: [Vec<(bool, &str)>; 3] = Default::default();
    for (code, span) in spans(text) {
        for (part, range) in [
            0..linked.start.min(span.end),
            linked.start.max(span.start)..linked.end.min(span.end),
            linked.end.max(span.start)..span.end,
        ]
        .into_iter()
        .enumerate()
        {
            let range = range.start.max(span.start)..range.end;
            if range.start < range.end {
                parts[part].push((code, &text[range]));
            }
        }
    }
    parts
}

/// The byte ranges of `text` cut at its backticks: `(true, code)` for what a pair of them encloses,
/// `(false, words)` for the rest; a backtick with no partner is kept as written.
fn spans(text: &str) -> Vec<(bool, Range<usize>)> {
    let mut spans = Vec::new();
    let mut at = 0;
    while let Some(open) = text[at..].find('`').map(|open| at + open) {
        let Some(close) = text[open + 1..].find('`').map(|close| open + 1 + close) else {
            break;
        };
        if open > at {
            spans.push((false, at..open));
        }
        spans.push((true, open + 1..close));
        at = close + 1;
    }
    if at < text.len() {
        spans.push((false, at..text.len()));
    }
    spans
}

/// `text` cut at its backticks: `(true, code)` for what a pair of them encloses, `(false, words)`
/// for the rest. A backtick with no partner is kept as written.
pub fn code_spans(text: &str) -> Vec<(bool, &str)> {
    spans(text)
        .into_iter()
        .map(|(code, range)| (code, &text[range]))
        .collect()
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

#[cfg(test)]
#[path = "task_gates/tests.rs"]
mod tests;
