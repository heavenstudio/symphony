//! What agents' model calls cost in tokens, as the heartbeat records it: the four kinds a price
//! tells apart (input read neither from nor into the cache, cache writes by their lifetime, cache
//! reads, output), by model and speed, on the board's `usage` table.
//!
//! `tasks beat` reads what the session's transcript gained since its last beat ([`transcript`]),
//! and the board adds it, in the beat's own one transaction, to the row of the session's handle,
//! the task and role it works then (an open executor's or reviewer's role first, as the agents view
//! picks one; with neither, no task and the session's kind), the model, the speed and the round: 1
//! and the moves into review the task's thread holds then, so a round is the work between two moves
//! to review ([`crate::Store::agent_beat_spending`]). `tasks show` sums a task's rows by role and
//! by round ([`Tally::of`]), the agents API a session's ([`crate::Store::spent_by`]), and a land
//! names them in its closing line ([`said`]).

pub mod transcript;

use crate::pyjson::Py;
use crate::rows::UsageRow;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The tokens of model calls on one model at one speed: what a beat hands the board.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Spent {
    /// The model that answered (`message.model`).
    pub model: String,
    /// Its speed (`usage.speed`: `standard`, or `fast`, which is priced apart).
    pub speed: String,
    pub tokens: Tokens,
}

/// Tokens of the four kinds, and the model calls they came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tokens {
    /// Input read neither from the cache nor into it (Claude Code's `input_tokens`).
    pub input: u64,
    /// Input written into the cache for five minutes (`ephemeral_5m_input_tokens`).
    pub cache_write_5m: u64,
    /// Input written into the cache for an hour (`ephemeral_1h_input_tokens`).
    pub cache_write_1h: u64,
    /// Input read from the cache (`cache_read_input_tokens`).
    pub cache_read: u64,
    /// Output, thinking included (`output_tokens`).
    pub output: u64,
    /// The model calls counted: one for each message id.
    pub calls: u64,
}

impl Tokens {
    /// Every token of the four kinds.
    pub fn total(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read + self.output
    }

    /// `other`'s added to these.
    pub fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
        self.cache_read += other.cache_read;
        self.output += other.output;
        self.calls += other.calls;
    }

    /// The JSON the agents API and `tasks show --json` give.
    pub fn json(&self) -> Py {
        let count = |n: u64| Py::Int(i64::try_from(n).unwrap_or(i64::MAX));
        Py::obj(vec![
            ("input", count(self.input)),
            ("cache_write_5m", count(self.cache_write_5m)),
            ("cache_write_1h", count(self.cache_write_1h)),
            ("cache_read", count(self.cache_read)),
            ("output", count(self.output)),
            ("calls", count(self.calls)),
        ])
    }
}

impl UsageRow {
    /// The row's tokens.
    pub fn tokens(&self) -> Tokens {
        Tokens {
            input: self.input,
            cache_write_5m: self.cache_write_5m,
            cache_write_1h: self.cache_write_1h,
            cache_read: self.cache_read,
            output: self.output,
            calls: self.calls,
        }
    }
}

/// `spent` merged by model and speed, in that order: what one beat hands the board.
pub fn merged(spent: impl IntoIterator<Item = Spent>) -> Vec<Spent> {
    let mut by: BTreeMap<(String, String), Tokens> = BTreeMap::new();
    for one in spent {
        by.entry((one.model, one.speed))
            .or_default()
            .add(&one.tokens);
    }
    by.into_iter()
        .filter(|(_, tokens)| tokens.calls > 0 || tokens.total() > 0)
        .map(|((model, speed), tokens)| Spent {
            model,
            speed,
            tokens,
        })
        .collect()
}

/// A task's rows summed: all told, by role and by round.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub total: Tokens,
    /// By role, in the order of [`ROLE_ORDER`], then by name.
    pub roles: Vec<(String, Tokens)>,
    /// By round, the first first.
    pub rounds: Vec<(i64, Tokens)>,
}

/// The roles a tally lists first, in this order.
const ROLE_ORDER: [&str; 4] = ["executor", "reviewer", "reserved", "orchestrator"];

impl Tally {
    /// `rows` summed.
    pub fn of(rows: &[UsageRow]) -> Tally {
        let mut total = Tokens::default();
        let mut roles: BTreeMap<(usize, &str), Tokens> = BTreeMap::new();
        let mut rounds: BTreeMap<i64, Tokens> = BTreeMap::new();
        for row in rows {
            let tokens = row.tokens();
            total.add(&tokens);
            let rank = ROLE_ORDER
                .iter()
                .position(|role| *role == row.role)
                .unwrap_or(ROLE_ORDER.len());
            roles.entry((rank, &row.role)).or_default().add(&tokens);
            rounds.entry(row.round).or_default().add(&tokens);
        }
        Tally {
            total,
            roles: (roles.into_iter())
                .map(|((_, role), tokens)| (role.to_string(), tokens))
                .collect(),
            rounds: rounds.into_iter().collect(),
        }
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.total == Tokens::default()
    }

    /// The JSON `tasks show --json` gives: the sums, and the rows themselves, which carry the model
    /// and speed a price is read by.
    pub fn json(&self, rows: &[UsageRow]) -> Py {
        let roles = (self.roles.iter())
            .map(|(role, tokens)| Py::obj(vec![("role", Py::str(role)), ("tokens", tokens.json())]))
            .collect();
        let rounds = (self.rounds.iter())
            .map(|(round, tokens)| {
                Py::obj(vec![("round", Py::Int(*round)), ("tokens", tokens.json())])
            })
            .collect();
        let rows = (rows.iter())
            .map(|row| {
                Py::obj(vec![
                    ("handle", Py::str(&row.handle)),
                    ("slug", Py::str(&row.slug)),
                    ("role", Py::str(&row.role)),
                    ("model", Py::str(&row.model)),
                    ("speed", Py::str(&row.speed)),
                    ("round", Py::Int(row.round)),
                    ("tokens", row.tokens().json()),
                    ("first", Py::Float(row.first)),
                    ("last", Py::Float(row.last)),
                ])
            })
            .collect();
        Py::obj(vec![
            ("total", self.total.json()),
            ("by_role", Py::List(roles)),
            ("by_round", Py::List(rounds)),
            ("rows", Py::List(rows)),
        ])
    }

    /// The lines `tasks show` prints: none when nothing was recorded.
    pub fn lines(&self) -> Vec<String> {
        if self.is_empty() {
            return Vec::new();
        }
        let t = &self.total;
        vec![
            format!(
                "Tokens: {} in {} (input {} · cache write {} · cache read {} · output {})",
                amount(t.total()),
                calls(t.calls),
                amount(t.input),
                amount(t.cache_write_5m + t.cache_write_1h),
                amount(t.cache_read),
                amount(t.output)
            ),
            format!("  by role: {}", self.by_role(" · ")),
            format!("  by round: {}", self.by_round(" · ")),
        ]
    }

    fn by_role(&self, between: &str) -> String {
        (self.roles.iter())
            .map(|(role, tokens)| format!("{role} {}", amount(tokens.total())))
            .collect::<Vec<_>>()
            .join(between)
    }

    fn by_round(&self, between: &str) -> String {
        (self.rounds.iter())
            .map(|(round, tokens)| match round {
                0 => format!("no task {}", amount(tokens.total())),
                n => format!("round {n} {}", amount(tokens.total())),
            })
            .collect::<Vec<_>>()
            .join(between)
    }
}

/// What a landing's closing line says of its task's tokens, after `; `: none when nothing was
/// recorded. Each amount carries its unit, so no number in it reads as a commit.
pub fn said(rows: &[UsageRow]) -> Option<String> {
    let tally = Tally::of(rows);
    (!tally.is_empty()).then(|| {
        format!(
            "{} tokens in {} ({}; {})",
            amount(tally.total.total()),
            calls(tally.total.calls),
            tally.by_role(", "),
            tally.by_round(", ")
        )
    })
}

fn calls(n: u64) -> String {
    format!("{n} call{}", if n == 1 { "" } else { "s" })
}

/// `n` tokens in three figures at most: `950`, `12.3k`, `456k`, `7.8M`, `1.2G`.
pub fn amount(n: u64) -> String {
    const UNITS: [(u64, &str); 3] = [(1_000_000_000, "G"), (1_000_000, "M"), (1_000, "k")];
    for (size, unit) in UNITS {
        if n >= size {
            // Tenths, rounded down, so 999.95k never reads as 1000.0k.
            let tenths = n / (size / 10);
            return if tenths >= 1000 {
                format!("{}{unit}", tenths / 10)
            } else if tenths % 10 == 0 {
                format!("{}{unit}", tenths / 10)
            } else {
                format!("{}.{}{unit}", tenths / 10, tenths % 10)
            };
        }
    }
    n.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: u64, write: u64, read: u64, output: u64, calls: u64) -> Tokens {
        Tokens {
            input,
            cache_write_5m: write,
            cache_write_1h: 0,
            cache_read: read,
            output,
            calls,
        }
    }

    fn row(role: &str, round: i64, t: Tokens) -> UsageRow {
        UsageRow {
            handle: "amber-heron".into(),
            slug: "sim/jump".into(),
            role: role.into(),
            model: "claude-opus-5-5".into(),
            speed: "standard".into(),
            round,
            input: t.input,
            cache_write_5m: t.cache_write_5m,
            cache_write_1h: t.cache_write_1h,
            cache_read: t.cache_read,
            output: t.output,
            calls: t.calls,
            first: 1.0,
            last: 2.0,
        }
    }

    #[test]
    fn amounts_keep_three_figures_and_never_round_up_past_their_unit() {
        for (n, said) in [
            (0, "0"),
            (999, "999"),
            (1_000, "1k"),
            (1_050, "1k"),
            (1_150, "1.1k"),
            (12_345, "12.3k"),
            (99_999, "99.9k"),
            (456_789, "456k"),
            (999_999, "999k"),
            (1_000_000, "1M"),
            (7_890_000, "7.8M"),
            (2_500_000_000, "2.5G"),
        ] {
            assert_eq!(amount(n), said, "{n}");
        }
    }

    #[test]
    fn merging_sums_one_model_and_speed_and_drops_what_holds_nothing() {
        let spent = |model: &str, speed: &str, t: Tokens| Spent {
            model: model.into(),
            speed: speed.into(),
            tokens: t,
        };
        let merged = merged([
            spent("b", "standard", tokens(1, 2, 3, 4, 1)),
            spent("a", "fast", tokens(1, 0, 0, 1, 1)),
            spent("b", "standard", tokens(10, 20, 30, 40, 2)),
            spent("c", "standard", Tokens::default()),
        ]);
        assert_eq!(
            merged,
            [
                spent("a", "fast", tokens(1, 0, 0, 1, 1)),
                spent("b", "standard", tokens(11, 22, 33, 44, 3)),
            ]
        );
    }

    #[test]
    fn a_tally_sums_by_role_in_the_boards_order_and_by_round() {
        let rows = [
            row("reviewer", 1, tokens(1, 100, 1_000, 10, 1)),
            row("executor", 1, tokens(2, 2_000, 20_000, 200, 3)),
            row("worker", 0, tokens(0, 0, 500, 5, 1)),
            row("executor", 2, tokens(3, 300, 3_000, 30, 2)),
        ];
        let tally = Tally::of(&rows);
        assert_eq!(tally.total, tokens(6, 2_400, 24_500, 245, 7));
        let roles: Vec<(&str, u64)> = (tally.roles.iter())
            .map(|(role, t)| (role.as_str(), t.total()))
            .collect();
        assert_eq!(
            roles,
            [("executor", 25_535), ("reviewer", 1_111), ("worker", 505)]
        );
        let rounds: Vec<(i64, u64)> = (tally.rounds.iter())
            .map(|(round, t)| (*round, t.total()))
            .collect();
        assert_eq!(rounds, [(0, 505), (1, 23_313), (2, 3_333)]);
        assert_eq!(
            tally.lines(),
            [
                "Tokens: 27.1k in 7 calls (input 6 · cache write 2.4k · cache read 24.5k · output 245)",
                "  by role: executor 25.5k · reviewer 1.1k · worker 505",
                "  by round: no task 505 · round 1 23.3k · round 2 3.3k",
            ]
        );
        assert_eq!(
            said(&rows).unwrap(),
            "27.1k tokens in 7 calls (executor 25.5k, reviewer 1.1k, worker 505; no task 505, \
             round 1 23.3k, round 2 3.3k)"
        );
    }

    #[test]
    fn nothing_recorded_says_nothing() {
        assert!(Tally::of(&[]).lines().is_empty());
        assert_eq!(said(&[]), None);
        let json = crate::pyjson::dumps(&Tally::of(&[]).json(&[]));
        assert!(json.contains("\"by_role\": []"), "{json}");
    }

    #[test]
    fn a_landing_line_naming_tokens_names_no_commit() {
        // `land::named` reads 7 to 40 hex digits as a commit; an amount always carries its unit.
        let rows = [row("executor", 1, tokens(0, 0, 9_876_543, 1_234_567, 1_000))];
        let line = said(&rows).unwrap();
        assert_eq!(crate::land::named(&line), Vec::<&str>::new(), "{line}");
    }
}
