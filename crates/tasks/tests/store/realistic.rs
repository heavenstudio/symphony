//! A board at the live one's shape, for what is tried on a whole board: the export
//! `SYMPHONY_BOARD_EXPORT` names (`tasks-copy --board --out`), else one generated from one seed at
//! the counts and lengths of the live export of 2026-11-15 (720 tasks, 6,962 messages of 97
//! characters in the median and 4,530 at the 90th percentile, 1,315 roles, 719 sessions, briefs of
//! some 2,000 characters), with every table, kind, status and optional column set and unset, and
//! the live board's text: markup, paths, slugs, `…` and `→`, `µs`, accents, a line in another
//! script, a null inside a task's fields, times to the microsecond. Each table is in the order of
//! its key, as a dump reads it.

use symphony_board::rows::{
    AskRow, BarredKeyRow, DeliveryKeyRow, DeliveryRow, Dump, HeldRow, LinkRow, MessageRow, RoleRow,
    SessionRow, TaskRow, TaskSessionRow, UsageRow,
};

/// The board: the export `SYMPHONY_BOARD_EXPORT` names, or the generated one.
pub fn board() -> Dump {
    let Some(path) = std::env::var_os("SYMPHONY_BOARD_EXPORT") else {
        return generated();
    };
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("SYMPHONY_BOARD_EXPORT names {path:?}, unread: {e}"));
    Dump::from_json(&text).unwrap_or_else(|e| panic!("{path:?} is no board's export: {e}"))
}

/// SplitMix64 from one seed: the same board on every run.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a>(&mut self, of: &[&'a str]) -> &'a str {
        of[self.below(of.len())]
    }

    /// A time after `from`, to the microsecond, as the board writes one.
    fn after(&mut self, from: f64, most: u64) -> f64 {
        let micros = self.next() % (most * 1_000_000);
        from + micros as f64 / 1e6
    }
}

const WORDS: [&str; 64] = [
    "the",
    "board",
    "store",
    "PostgreSQL",
    "SurrealDB",
    "review",
    "approve",
    "slice",
    "marker",
    "fence",
    "copy",
    "dump",
    "search",
    "task",
    "thread",
    "line",
    "commit",
    "tip",
    "branch",
    "main",
    "land",
    "verdict",
    "changes",
    "test",
    "lint",
    "green",
    "red",
    "evidence",
    "owner",
    "card",
    "a",
    "of",
    "to",
    "and",
    "is",
    "in",
    "on",
    "for",
    "it",
    "with",
    "that",
    "not",
    "one",
    "each",
    "server",
    "socket",
    "request",
    "transaction",
    "schema",
    "migration",
    "writer",
    "session",
    "orchestrator",
    "executor",
    "reviewer",
    "assessor",
    "release",
    "client",
    "build",
    "folder",
    "Engine",
    "Nebula",
    "Quantum",
    "comet",
];

const PIECES: [&str; 36] = [
    "**bold**",
    "`just tasks show`",
    "crates/tasks/src/store/copy.rs:42",
    "#8705",
    "…",
    "→",
    "—",
    "12 µs",
    "Crème brûlée",
    "naïve façade",
    "İstanbul",
    "x² and ½",
    "مرحبا",
    "привет",
    "✓ ✅ ⚠",
    "🚀",
    "it's",
    "\"quoted\"",
    "a_b-c",
    "1.5e3",
    "0x1f",
    "https://example.com/a?b=c&d=e",
    "\n",
    "\n\n- ",
    "\n```rust\nfn x() -> u8 { 1 }\n```\n",
    "\t",
    "50%",
    "(see docs/workflow/TASKS.md#needed-stamps)",
    "[a link](docs/X.md)",
    "<b>markup</b>",
    "'single'",
    "\\back\\slash",
    "{\"json\": [1, null]}",
    "tools/board-pg-search-and-copy",
    "NONE",
    "'); DROP TABLE task; --",
];

/// Text of at least `chars` characters, from the board's words and pieces.
fn text(d: &mut Draw, chars: usize) -> String {
    let mut out = String::new();
    while out.chars().count() < chars.max(1) {
        if !out.is_empty() {
            out.push(' ');
        }
        if d.chance(12) {
            out.push_str(d.pick(&PIECES));
        } else {
            out.push_str(d.pick(&WORDS));
        }
    }
    out
}

/// A line's length, as the live board's are spread: most short, a tenth long.
fn line_length(d: &mut Draw) -> usize {
    match d.below(100) {
        0..55 => 20 + d.below(180),
        55..90 => 200 + d.below(1_800),
        _ => 2_000 + d.below(10_000),
    }
}

const BASE: f64 = 1_794_405_039.644_481;
const STATUSES: [&str; 9] = [
    "done",
    "cancelled",
    "backlog",
    "ready",
    "in_progress",
    "review",
    "blocked",
    "concept",
    "design",
];
const AREAS: [&str; 6] = ["tools", "docs", "perf", "design", "tests", "client"];
const KINDS: [&str; 5] = ["worker", "orchestrator", "reviewer", "assessor", "chat"];
const ROLES: [&str; 4] = ["orchestrator", "executor", "reviewer", "reserved"];

fn handle(n: usize) -> String {
    const FIRST: [&str; 8] = [
        "amber", "brisk", "coral", "ivory", "misty", "silver", "slate", "quiet",
    ];
    const SECOND: [&str; 8] = [
        "heron", "finch", "hare", "marten", "otter", "sparrow", "egret", "plover",
    ];
    format!("{}-{}-{n}", FIRST[n % 8], SECOND[(n / 8) % 8])
}

/// The generated board.
pub fn generated() -> Dump {
    let d = &mut Draw(0x5eed_b0a2d);
    let sessions: Vec<SessionRow> = (0..719)
        .map(|n| {
            let connected = d.after(BASE, 2_000_000);
            SessionRow {
                handle: handle(n),
                kind: d.pick(&KINDS).into(),
                parent: d.chance(60).then(|| handle(d.below(719))),
                worktree: if d.chance(80) {
                    format!("/work/symphony/.claude/worktrees/agent-{n:x}")
                } else {
                    String::new()
                },
                branch: d.pick(&["main", "", "tools/board-pg-schema"]).into(),
                model: d.pick(&["", "claude-opus-5-5", "fable"]).into(),
                pid: d.chance(30).then(|| d.below(99_999) as i64),
                host: d.pick(&["", "this-host", "elsewhere"]).into(),
                affinity: d.pick(&["", "claude", "codex"]).into(),
                connected,
                last_seen: d.after(connected, 90_000),
                left_at: d.chance(70).then(|| d.after(connected, 100_000)),
                agent: d.chance(50).then(|| format!("a{:x}", d.next())),
                busy_until: d.chance(10).then(|| d.after(connected, 600)),
                busy_call: d.chance(10).then(|| format!("toolu_{:x}", d.next())),
                last_tool: d.chance(60).then(|| d.after(connected, 90_000)),
            }
        })
        .collect();
    let mut tasks: Vec<TaskRow> = Vec::new();
    for n in 0..720 {
        let parent = (n > 10 && d.chance(15)).then(|| tasks[d.below(tasks.len())].slug.clone());
        let slug = match &parent {
            Some(parent) => format!("{parent}/part-{n}"),
            None => format!("{}/{}-{n}", d.pick(&AREAS), d.pick(&WORDS).to_lowercase()),
        };
        let created = d.after(BASE, 2_500_000);
        let title_words = 3 + d.below(12);
        let mut fields = std::collections::BTreeMap::new();
        match d.below(100) {
            0..8 => {
                let why = text(d, 60);
                fields.insert(
                    "_board".to_string(),
                    [("fast_fix".into(), why.into())].into(),
                );
            }
            8..10 => {
                let hash = format!("{:016x}{:016x}", d.next(), d.next());
                fields.insert(
                    "_symphony".into(),
                    [("owner_save".into(), hash.into())].into(),
                );
            }
            // The stamps a task needs, as the board keeps them: keys whose order as text is not
            // their order by length.
            13..30 => {
                let needs = serde_json::json!({
                    "by": handle(d.below(719)),
                    "stamps": ["tests", "review"],
                    "tip": format!("{:016x}{:016x}", d.next(), d.next()),
                    "why": text(d, 80),
                });
                fields.insert("_board".into(), [("needs".to_string(), needs)].into());
            }
            10..12 => {
                let count = d.below(9);
                fields.insert(
                    "plugin-x".into(),
                    [
                        ("count".to_string(), count.into()),
                        ("label".to_string(), "x ✓".into()),
                    ]
                    .into(),
                );
            }
            12 => {
                let nested = serde_json::json!({"empty": null, "items": [null, {"v": 1.5}]});
                fields.insert("plugin-y".into(), [("settings".to_string(), nested)].into());
            }
            _ => {}
        }
        let brief = 300 + d.below(4_500) + if d.chance(5) { 12_000 } else { 0 };
        tasks.push(TaskRow {
            id: n as i64 + 1,
            slug,
            title: text(d, title_words * 6),
            body: if d.chance(3) {
                String::new()
            } else {
                text(d, brief)
            },
            status: match d.below(100) {
                0..73 => "done",
                73..91 => "cancelled",
                n => STATUSES[2 + n % 7],
            }
            .into(),
            priority: d.below(5) as i64,
            instruction: parent.is_some() && d.chance(40),
            parent,
            created_by: if d.chance(70) {
                "owner".into()
            } else {
                handle(d.below(719))
            },
            created,
            updated: d.after(created, 400_000),
            fields,
        });
    }
    let slug = |d: &mut Draw| tasks[d.below(tasks.len())].slug.clone();
    let mut task_links: Vec<LinkRow> = (0..328)
        .map(|_| {
            let from = slug(d);
            if d.chance(60) {
                LinkRow {
                    slug: from,
                    kind: "dep".into(),
                    target: slug(d),
                }
            } else {
                LinkRow {
                    slug: from,
                    kind: "doc".into(),
                    target: format!("docs/{}.md#{}", d.pick(&WORDS), d.below(40)),
                }
            }
        })
        .collect();
    let mut roles: Vec<RoleRow> = (0..1_315)
        .map(|_| {
            let role = d.pick(&ROLES);
            let since = d.after(BASE, 2_500_000);
            RoleRow {
                slug: slug(d),
                handle: handle(d.below(719)),
                role: role.into(),
                since,
                expires: (role == "reserved").then_some(since + 3_600.0),
            }
        })
        .collect();
    let mut ts = BASE;
    let messages: Vec<MessageRow> = (1..=6_962)
        .map(|id| {
            ts = d.after(ts, 360);
            let length = line_length(d);
            MessageRow {
                id,
                slug: slug(d),
                sender: match d.below(10) {
                    0..3 => "owner".into(),
                    3 => "board".into(),
                    _ => handle(d.below(719)),
                },
                recipient: d.chance(27).then(|| {
                    d.pick(&[
                        "@orchestrator",
                        "@executor",
                        "@reviewer",
                        "owner",
                        "amber-heron-0",
                    ])
                    .into()
                }),
                kind: d
                    .pick(&["system", "system", "msg", "msg", "verdict"])
                    .into(),
                text: text(d, length),
                ts,
            }
        })
        .collect();
    let mut deliveries: Vec<DeliveryRow> = (0..1_822)
        .map(|_| {
            let message = &messages[d.below(messages.len())];
            DeliveryRow {
                message: message.id,
                handle: handle(d.below(719)),
                acked: d.after(message.ts, 3_000),
            }
        })
        .collect();
    let mut barred_keys: Vec<BarredKeyRow> = (0..3)
        .map(|_| BarredKeyRow {
            key: format!("{:04x}", d.next() & 0xffff),
        })
        .collect();
    let mut task_sessions: Vec<TaskSessionRow> = (0..40)
        .map(|n| {
            let started = d.after(BASE, 2_000_000);
            TaskSessionRow {
                slug: slug(d),
                handle: handle(n),
                role: d.pick(&["executor", "reviewer"]).into(),
                claude_session: format!("{:x}", d.next()),
                worktree: format!("/work/symphony/.claude/worktrees/agent-{n:x}"),
                branch: slug(d),
                pane: format!("w{}:p{}", d.below(9), d.below(9)),
                model: "claude-opus-5-5".into(),
                started,
                closed: d.chance(70).then(|| d.after(started, 9_000)),
                closed_why: d.pick(&["", "done", "landed — closed"]).into(),
                restore: d.pick(&["", "claude --resume x"]).into(),
            }
        })
        .collect();
    let mut delivery_keys: Vec<DeliveryKeyRow> = (0..53)
        .map(|_| DeliveryKeyRow {
            slug: slug(d),
            key: format!("main-red:{:04x}", d.next() & 0xffff),
            message: d.below(6_962) as i64 + 1,
            ts: d.after(BASE, 2_500_000),
        })
        .collect();
    let asks: Vec<AskRow> = (1..=34)
        .map(|id| {
            let asked = d.after(BASE, 2_500_000);
            let answered = d.chance(60);
            AskRow {
                id,
                kind: d
                    .pick(&["verdict", "message", "comment", "chat", "task"])
                    .into(),
                about: format!("[\"card\",\"tools/x-{id}\",\"docs/DECISIONS.md\",\"c{id}\"]"),
                path: d.chance(50).then(|| "docs/DECISIONS.md".into()),
                card: d.chance(50).then(|| format!("card-{id}")),
                block: d.chance(30).then(|| d.below(40).to_string()),
                tree: d.chance(30).then(|| slug(d)),
                task: d.chance(40).then(|| slug(d)),
                thread: d.chance(30).then(|| id.to_string()),
                words: text(d, 40),
                asked,
                lines: 1 + d.below(4) as i64,
                last: d.after(asked, 9_000),
                holder: d.chance(40).then(|| handle(d.below(719))),
                held: d.chance(40).then(|| d.after(asked, 600)),
                before: (0..d.below(3))
                    .map(|n| HeldRow {
                        handle: handle(n),
                        since: asked,
                        until: asked + 600.0,
                        why: "no answer in 10 min".into(),
                    })
                    .collect(),
                open: !answered,
                answered: answered.then(|| d.after(asked, 90_000)),
                answered_by: answered.then(|| handle(d.below(719))),
                answer: answered.then(|| text(d, 30)),
                closed: d.chance(10),
                gone: d
                    .chance(10)
                    .then(|| "the card is gone from its document".into()),
                gone_since: d.chance(10).then(|| d.after(asked, 90_000)),
            }
        })
        .collect();
    let mut usages: Vec<UsageRow> = (0..120)
        .map(|n| {
            let first = d.after(BASE, 2_000_000);
            UsageRow {
                handle: handle(n % 60),
                slug: if d.chance(20) { String::new() } else { slug(d) },
                role: d.pick(&ROLES[..3]).into(),
                model: d.pick(&["claude-opus-5-5", "fable", "claude-haiku"]).into(),
                speed: d.pick(&["standard", "fast"]).into(),
                round: d.below(4) as i64,
                input: d.next() % 5_000,
                cache_write_5m: d.next() % 900_000,
                cache_write_1h: d.next() % 90_000,
                cache_read: d.next() % 90_000_000,
                output: d.next() % 900_000,
                calls: 1 + d.next() % 600,
                first,
                last: d.after(first, 90_000),
            }
        })
        .collect();
    let mut sessions = sessions;
    sessions.sort_by(|a, b| a.handle.cmp(&b.handle));
    keyed(&mut task_links, |l| {
        (l.slug.clone(), l.kind.clone(), l.target.clone())
    });
    keyed(&mut roles, |r| {
        (r.slug.clone(), r.handle.clone(), r.role.clone())
    });
    keyed(&mut deliveries, |r| (r.message, r.handle.clone()));
    keyed(&mut barred_keys, |r| r.key.clone());
    keyed(&mut task_sessions, |r| (r.slug.clone(), r.handle.clone()));
    keyed(&mut delivery_keys, |r| (r.slug.clone(), r.key.clone()));
    keyed(&mut usages, |r| {
        let key = (
            r.handle.clone(),
            r.slug.clone(),
            r.role.clone(),
            r.model.clone(),
        );
        (key, r.speed.clone(), r.round)
    });
    Dump {
        tasks,
        task_links,
        sessions,
        roles,
        messages,
        deliveries,
        barred_keys,
        task_sessions,
        delivery_keys,
        asks,
        usages,
        ..Dump::default()
    }
}

/// `rows` in the order of `key`, each key once: a table's rows as the board keys them.
fn keyed<T, K: Ord>(rows: &mut Vec<T>, key: impl Fn(&T) -> K) {
    rows.sort_by_key(|row| key(row));
    rows.dedup_by(|a, b| key(a) == key(b));
}
