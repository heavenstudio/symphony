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
//! names them in its closing line ([`said`]); each beside its cost, estimated at the API list
//! prices of the price table ([`price`]).

pub mod price;
pub mod transcript;

use crate::json_object::object;
use crate::rows::UsageRow;
pub use price::{Cost, Prices};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    pub fn json(&self) -> Value {
        let count = |n: u64| Value::from(i64::try_from(n).unwrap_or(i64::MAX));
        object(vec![
            ("input", count(self.input)),
            ("cache_write_5m", count(self.cache_write_5m)),
            ("cache_write_1h", count(self.cache_write_1h)),
            ("cache_read", count(self.cache_read)),
            ("output", count(self.output)),
            ("calls", count(self.calls)),
        ])
    }
}

/// Tokens, and what they cost at the price table's prices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spend {
    pub tokens: Tokens,
    pub cost: Cost,
}

impl Spend {
    /// `other`'s added to this.
    pub fn add(&mut self, other: &Spend) {
        self.tokens.add(&other.tokens);
        self.cost.add(&other.cost);
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

    /// The row's tokens and what they cost at `prices`' price of its model and speed.
    pub fn spend(&self, prices: &Prices) -> Spend {
        let tokens = self.tokens();
        Spend {
            cost: prices.cost(&self.model, &self.speed, &tokens),
            tokens,
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

/// A task's rows summed, each with its cost at the price table's prices: all told, by role and by
/// round.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub total: Spend,
    /// By role, in the order of [`ROLE_ORDER`], then by name.
    pub roles: Vec<(String, Spend)>,
    /// By round, the first first.
    pub rounds: Vec<(i64, Spend)>,
    /// The day the table's prices were read ([`Prices::read`]).
    pub prices: String,
}

/// The roles a tally lists first, in this order.
pub const ROLE_ORDER: [&str; 4] = ["executor", "reviewer", "reserved", "orchestrator"];

impl Tally {
    /// `rows` summed, at the prices every build embeds.
    pub fn of(rows: &[UsageRow]) -> Tally {
        Tally::at(rows, Prices::embedded())
    }

    /// `rows` summed, at `prices`.
    pub fn at(rows: &[UsageRow], prices: &Prices) -> Tally {
        let mut total = Spend::default();
        let mut roles: BTreeMap<(usize, &str), Spend> = BTreeMap::new();
        let mut rounds: BTreeMap<i64, Spend> = BTreeMap::new();
        for row in rows {
            let spend = row.spend(prices);
            total.add(&spend);
            let rank = ROLE_ORDER
                .iter()
                .position(|role| *role == row.role)
                .unwrap_or(ROLE_ORDER.len());
            roles.entry((rank, &row.role)).or_default().add(&spend);
            rounds.entry(row.round).or_default().add(&spend);
        }
        Tally {
            total,
            roles: (roles.into_iter())
                .map(|((_, role), spend)| (role.to_string(), spend))
                .collect(),
            rounds: rounds.into_iter().collect(),
            prices: prices.read.clone(),
        }
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.total.tokens == Tokens::default()
    }

    /// The JSON `tasks show --json` gives: the sums, and the rows themselves, which carry the model
    /// and speed a price is read by, each beside its cost.
    pub fn json(&self, rows: &[UsageRow], prices: &Prices) -> Value {
        let cost = |cost: &Cost| cost.json(&prices.read);
        let roles = (self.roles.iter())
            .map(|(role, spend)| {
                object(vec![
                    ("role", Value::from(role.as_str())),
                    ("tokens", spend.tokens.json()),
                    ("cost", cost(&spend.cost)),
                ])
            })
            .collect();
        let rounds = (self.rounds.iter())
            .map(|(round, spend)| {
                object(vec![
                    ("round", Value::from(*round)),
                    ("tokens", spend.tokens.json()),
                    ("cost", cost(&spend.cost)),
                ])
            })
            .collect();
        let rows = (rows.iter())
            .map(|row| {
                let spend = row.spend(prices);
                object(vec![
                    ("handle", Value::from(row.handle.as_str())),
                    ("slug", Value::from(row.slug.as_str())),
                    ("role", Value::from(row.role.as_str())),
                    ("model", Value::from(row.model.as_str())),
                    ("speed", Value::from(row.speed.as_str())),
                    ("round", Value::from(row.round)),
                    ("tokens", spend.tokens.json()),
                    ("cost", cost(&spend.cost)),
                    ("first", Value::from(row.first)),
                    ("last", Value::from(row.last)),
                ])
            })
            .collect();
        object(vec![
            ("total", self.total.tokens.json()),
            ("cost", cost(&self.total.cost)),
            ("by_role", Value::Array(roles)),
            ("by_round", Value::Array(rounds)),
            ("rows", Value::Array(rows)),
        ])
    }

    /// The lines `tasks show` prints: the tokens, then their estimated cost, each all told, by role
    /// and by round; none when nothing was recorded.
    pub fn lines(&self) -> Vec<String> {
        if self.is_empty() {
            return Vec::new();
        }
        let t = &self.total.tokens;
        let c = &self.total.cost;
        let cost = if c.total() == 0 && !c.unpriced.is_empty() {
            format!(
                "Cost: unpriced: the API list prices read {} have none for {}",
                self.prices,
                c.unpriced_said(" · ")
            )
        } else {
            let mut cost = format!(
                "Cost: {}, estimated at API list prices read {} (input {} · cache write {} · \
                 cache read {} · output {})",
                price::dollars(c.total()),
                self.prices,
                price::dollars(c.input),
                price::dollars(c.cache_write_5m + c.cache_write_1h),
                price::dollars(c.cache_read),
                price::dollars(c.output)
            );
            if !c.unpriced.is_empty() {
                cost.push_str(&format!(
                    "; none for {}, in no figure",
                    c.unpriced_said(" · ")
                ));
            }
            cost
        };
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
            format!(
                "  by role: {}",
                self.by_role(" · ", |s| amount(s.tokens.total()))
            ),
            format!(
                "  by round: {}",
                self.by_round(" · ", |s| amount(s.tokens.total()))
            ),
            cost,
            format!("  by role: {}", self.by_role(" · ", |s| s.cost.said())),
            format!("  by round: {}", self.by_round(" · ", |s| s.cost.said())),
        ]
    }

    fn by_role(&self, between: &str, said: impl Fn(&Spend) -> String) -> String {
        (self.roles.iter())
            .map(|(role, spend)| format!("{role} {}", said(spend)))
            .collect::<Vec<_>>()
            .join(between)
    }

    fn by_round(&self, between: &str, said: impl Fn(&Spend) -> String) -> String {
        (self.rounds.iter())
            .map(|(round, spend)| match round {
                0 => format!("no task {}", said(spend)),
                n => format!("round {n} {}", said(spend)),
            })
            .collect::<Vec<_>>()
            .join(between)
    }
}

/// What a landing's closing line says of its task's tokens and their estimated cost, after `; `:
/// none when nothing was recorded. Each amount carries its unit, so no number in it reads as a
/// commit.
pub fn said(rows: &[UsageRow]) -> Option<String> {
    said_at(rows, Prices::embedded())
}

/// [`said`], at `prices`.
pub fn said_at(rows: &[UsageRow], prices: &Prices) -> Option<String> {
    let tally = Tally::at(rows, prices);
    (!tally.is_empty()).then(|| {
        let c = &tally.total.cost;
        let cost = match (c.total(), c.unpriced.is_empty()) {
            (0, false) => format!(
                "no API list price read {} for {}",
                tally.prices,
                c.unpriced_said(", ")
            ),
            (pico, true) => format!(
                "{} estimated at API list prices read {}",
                price::dollars(pico),
                tally.prices
            ),
            (pico, false) => format!(
                "{} estimated at API list prices read {}, none for {}",
                price::dollars(pico),
                tally.prices,
                c.unpriced_said(", ")
            ),
        };
        format!(
            "{} tokens in {} ({}; {}), {cost}",
            amount(tally.total.tokens.total()),
            calls(tally.total.tokens.calls),
            tally.by_role(", ", |s| amount(s.tokens.total())),
            tally.by_round(", ", |s| amount(s.tokens.total())),
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
            // Tenths, rounded down, so 999.95k never reads as 1000.0k; none from 100 on.
            let tenths = n / (size / 10);
            return if tenths >= 1000 || tenths.is_multiple_of(10) {
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

    /// A table that prices `claude-opus-5-5` high enough for a few tokens to show in cents: a
    /// thousandth of a dollar an input token, two and three thousandths a cache write, a
    /// ten-thousandth a cache read and a cent an output token.
    fn prices() -> Prices {
        Prices::parse(
            "read = \"2026-01-02\"\npage = \"https://example.com/pricing\"\n[[price]]\n\
             model = \"claude-opus-5-5\"\ninput = 1000\ncache_write_5m = 2000\n\
             cache_write_1h = 3000\ncache_read = 100\noutput = 10000\n",
        )
        .unwrap()
    }

    fn row(role: &str, round: i64, t: Tokens) -> UsageRow {
        on("claude-opus-5-5", role, round, t)
    }

    fn on(model: &str, role: &str, round: i64, t: Tokens) -> UsageRow {
        UsageRow {
            handle: "amber-heron".into(),
            slug: "sim/jump".into(),
            role: role.into(),
            model: model.into(),
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
        let tally = Tally::at(&rows, &prices());
        assert_eq!(tally.total.tokens, tokens(6, 2_400, 24_500, 245, 7));
        let roles: Vec<(&str, u64, String)> = (tally.roles.iter())
            .map(|(role, s)| (role.as_str(), s.tokens.total(), s.cost.said()))
            .collect();
        assert_eq!(
            roles,
            [
                ("executor", 25_535, "$9.21".to_string()),
                ("reviewer", 1_111, "$0.40".to_string()),
                ("worker", 505, "$0.10".to_string())
            ]
        );
        let rounds: Vec<(i64, u64)> = (tally.rounds.iter())
            .map(|(round, s)| (*round, s.tokens.total()))
            .collect();
        assert_eq!(rounds, [(0, 505), (1, 23_313), (2, 3_333)]);
        // Input 6 at $0.001 = $0.006, cache writes 2,400 at $0.002 = $4.80, reads 24,500 at
        // $0.0001 = $2.45, output 245 at $0.01 = $2.45: $9.706 all told.
        assert_eq!(tally.total.cost.total(), 9_706_000_000_000);
        assert_eq!(
            tally.lines(),
            [
                "Tokens: 27.1k in 7 calls (input 6 · cache write 2.4k · cache read 24.5k · output 245)",
                "  by role: executor 25.5k · reviewer 1.1k · worker 505",
                "  by round: no task 505 · round 1 23.3k · round 2 3.3k",
                "Cost: $9.71, estimated at API list prices read 2026-01-02 (input $0.01 · cache \
                 write $4.80 · cache read $2.45 · output $2.45)",
                "  by role: executor $9.21 · reviewer $0.40 · worker $0.10",
                "  by round: no task $0.10 · round 1 $8.40 · round 2 $1.20",
            ]
        );
        assert_eq!(
            said_at(&rows, &prices()).unwrap(),
            "27.1k tokens in 7 calls (executor 25.5k, reviewer 1.1k, worker 505; no task 505, \
             round 1 23.3k, round 2 3.3k), $9.71 estimated at API list prices read 2026-01-02"
        );
    }

    #[test]
    fn a_model_with_no_price_is_named_beside_the_cost_and_in_no_figure() {
        let rows = [
            row("executor", 1, tokens(0, 0, 0, 100, 1)),
            on(
                "gpt-6.1-sol",
                "executor",
                1,
                tokens(1_000, 0, 50_000, 500, 4),
            ),
            on("gpt-6.1-sol", "reviewer", 2, tokens(10, 0, 0, 0, 1)),
        ];
        let tally = Tally::at(&rows, &prices());
        assert_eq!(
            tally.lines()[3..],
            [
                "Cost: $1.00, estimated at API list prices read 2026-01-02 (input $0.00 · cache \
                 write $0.00 · cache read $0.00 · output $1.00); none for gpt-6.1-sol 51.5k \
                 tokens, in no figure",
                "  by role: executor $1.00 + unpriced · reviewer unpriced",
                "  by round: round 1 $1.00 + unpriced · round 2 unpriced",
            ]
        );
        assert_eq!(
            said_at(&rows, &prices()).unwrap(),
            "51.6k tokens in 6 calls (executor 51.6k, reviewer 10; round 1 51.6k, round 2 10), \
             $1.00 estimated at API list prices read 2026-01-02, none for gpt-6.1-sol 51.5k tokens"
        );
        let none = &rows[1..];
        assert_eq!(
            Tally::at(none, &prices()).lines()[3],
            "Cost: unpriced: the API list prices read 2026-01-02 have none for gpt-6.1-sol 51.5k \
             tokens"
        );
        assert_eq!(
            said_at(none, &prices()).unwrap(),
            "51.5k tokens in 5 calls (executor 51.5k, reviewer 10; round 1 51.5k, round 2 10), no \
             API list price read 2026-01-02 for gpt-6.1-sol 51.5k tokens"
        );
    }

    #[test]
    fn a_tally_prices_each_row_by_its_own_model_and_speed() {
        let mut fast = row("executor", 1, tokens(0, 0, 0, 100, 1));
        fast.speed = "fast".into();
        let rows = [row("executor", 1, tokens(0, 0, 0, 100, 1)), fast];
        let tally = Tally::at(&rows, &prices());
        // The fixture has no fast price for Opus: its row is unpriced, never at the standard one.
        assert_eq!(tally.total.cost.said(), "$1.00 + unpriced");
        assert_eq!(
            tally.total.cost.unpriced_said(", "),
            "claude-opus-5-5 fast 100 tokens"
        );
        // The embedded table is what `of` and `said` price at.
        assert_eq!(Tally::of(&rows).prices, Prices::embedded().read);
        assert_eq!(said(&rows), said_at(&rows, Prices::embedded()));
    }

    #[test]
    fn the_json_carries_each_sums_cost_and_each_rows() {
        let rows = [
            row("executor", 1, tokens(0, 0, 0, 100, 1)),
            on("gpt-6.1-sol", "reviewer", 2, tokens(10, 0, 0, 0, 1)),
        ];
        let prices = prices();
        let json = serde_json::to_string(&Tally::at(&rows, &prices).json(&rows, &prices))
            .expect("JSON values serialize");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["cost"]["dollars"], 1.0);
        assert_eq!(value["cost"]["prices"], "2026-01-02");
        assert_eq!(value["cost"]["unpriced"][0]["model"], "gpt-6.1-sol");
        assert_eq!(value["by_role"][0]["role"], "executor");
        assert_eq!(value["by_role"][0]["cost"]["output"], 1.0);
        assert_eq!(value["by_role"][1]["cost"]["dollars"], 0.0);
        assert_eq!(
            value["by_round"][1]["cost"]["unpriced"][0]["tokens"]["input"],
            10
        );
        assert_eq!(value["rows"][0]["cost"]["dollars"], 1.0);
        assert_eq!(value["rows"][1]["cost"]["unpriced"][0]["speed"], "standard");
        let keys: Vec<&str> = (value["rows"][0].as_object().unwrap().keys())
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "handle", "slug", "role", "model", "speed", "round", "tokens", "cost", "first",
                "last"
            ]
        );
    }

    #[test]
    fn nothing_recorded_says_nothing() {
        assert!(Tally::of(&[]).lines().is_empty());
        assert_eq!(said(&[]), None);
        let json = serde_json::to_string_pretty(&Tally::of(&[]).json(&[], Prices::embedded()))
            .expect("JSON values serialize");
        assert!(json.contains("\"by_role\": []"), "{json}");
    }

    #[test]
    fn json_token_counts_keep_the_signed_limit() {
        let counts = Tokens {
            input: u64::MAX,
            output: i64::MAX as u64,
            ..Tokens::default()
        }
        .json();
        assert_eq!(counts["input"].as_i64(), Some(i64::MAX));
        assert_eq!(counts["output"].as_i64(), Some(i64::MAX));
        assert_eq!(counts["calls"].as_u64(), Some(0));
    }

    #[test]
    fn a_landing_line_naming_tokens_names_no_commit() {
        // `land::named` reads 7 to 40 hex digits as a commit; an amount always carries its unit.
        let rows = [row(
            "executor",
            1,
            tokens(0, 0, 9_876_543, 1_234_567, 1_000),
        )];
        let line = said(&rows).unwrap();
        assert_eq!(crate::land::named(&line), Vec::<&str>::new(), "{line}");
    }
}
