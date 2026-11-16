//! What agents' tokens cost, estimated at the providers' API list prices: the price table
//! (`prices.toml` beside this file, embedded at build, so every build of `tasks` and of the
//! Symphony has it and none reads it from disk), and the cost of a model's tokens of the four kinds
//! at its price.
//!
//! A row is found by the exact model id and speed the transcript named; a model the table lacks is
//! not priced: its tokens are kept apart in [`Cost::unpriced`], named, and left out of every dollar
//! total, never priced at zero. Money is counted in picodollars (10^-12 US dollars), in which a token
//! at a price per million tokens to the millionth of a dollar is a whole number, so a sum is exact
//! and rounds to the cent only where it is said.

use super::Tokens;
use crate::pyjson::Py;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The table every build embeds.
const TABLE: &str = include_str!("prices.toml");

/// The highest price a row may give, in dollars per million tokens: far above any list price, and
/// low enough that every price in picodollars per token fits a `u64`.
const MAX_PRICE: f64 = 1_000_000.0;

/// Picodollars in a cent.
const PICO_PER_CENT: u128 = 10_000_000_000;

/// The price table: the day its prices were read, the page they were read from, and a price for
/// each model and speed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prices {
    /// The day the prices were read (`YYYY-MM-DD`).
    pub read: String,
    /// The page they were read from.
    pub page: String,
    by: BTreeMap<(String, String), Price>,
}

/// One model's price at one speed, of each kind, in picodollars per token (which is microdollars
/// per million tokens).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Price {
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
}

/// The table's file, as it is written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    read: String,
    page: String,
    #[serde(default)]
    price: Vec<Row>,
}

/// A row of the file, in dollars per million tokens.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    model: String,
    #[serde(default = "standard")]
    speed: String,
    input: f64,
    cache_write_5m: f64,
    cache_write_1h: f64,
    cache_read: f64,
    output: f64,
}

fn standard() -> String {
    "standard".into()
}

impl Prices {
    /// The table every build embeds. One that does not read (a hand edit the price tests would
    /// have refused) prices nothing, so every model reads unpriced rather than at a wrong price.
    pub fn embedded() -> &'static Prices {
        static EMBEDDED: LazyLock<Prices> =
            LazyLock::new(|| Prices::parse(TABLE).unwrap_or_default());
        &EMBEDDED
    }

    /// The table `text` writes, or what is wrong with it: a key it does not know, a row twice for
    /// one model and speed, a price that is no number from 0 to [`MAX_PRICE`], or a `read` that is
    /// no date.
    pub fn parse(text: &str) -> Result<Prices, String> {
        let file: File =
            toml::from_str(text).map_err(|e| format!("the price table does not read: {e}"))?;
        if !is_date(&file.read) {
            return Err(format!(
                "the price table's read = {:?} is no date (YYYY-MM-DD)",
                file.read
            ));
        }
        let mut by = BTreeMap::new();
        for row in file.price {
            let named = format!("{} at {} speed", row.model, row.speed);
            if row.model.is_empty() || row.speed.is_empty() {
                return Err(format!(
                    "the price table has a row with no model or no speed ({named})"
                ));
            }
            let price = Price {
                input: micro(row.input, "input", &named)?,
                cache_write_5m: micro(row.cache_write_5m, "cache_write_5m", &named)?,
                cache_write_1h: micro(row.cache_write_1h, "cache_write_1h", &named)?,
                cache_read: micro(row.cache_read, "cache_read", &named)?,
                output: micro(row.output, "output", &named)?,
            };
            if by.insert((row.model, row.speed), price).is_some() {
                return Err(format!("the price table prices {named} twice"));
            }
        }
        Ok(Prices {
            read: file.read,
            page: file.page,
            by,
        })
    }

    /// `model`'s price at `speed`, by its exact id; none when the table has no row for it.
    pub fn of(&self, model: &str, speed: &str) -> Option<Price> {
        self.by
            .get(&(model.to_string(), speed.to_string()))
            .copied()
    }

    /// What `tokens` of `model` at `speed` cost; unpriced, and named, when the table has no price
    /// for it.
    pub fn cost(&self, model: &str, speed: &str, tokens: &Tokens) -> Cost {
        let Some(price) = self.of(model, speed) else {
            let mut cost = Cost::default();
            if *tokens != Tokens::default() {
                cost.unpriced
                    .insert((model.to_string(), speed.to_string()), *tokens);
            }
            return cost;
        };
        let at = |n: u64, per: u64| u128::from(n) * u128::from(per);
        Cost {
            input: at(tokens.input, price.input),
            cache_write_5m: at(tokens.cache_write_5m, price.cache_write_5m),
            cache_write_1h: at(tokens.cache_write_1h, price.cache_write_1h),
            cache_read: at(tokens.cache_read, price.cache_read),
            output: at(tokens.output, price.output),
            unpriced: BTreeMap::new(),
        }
    }
}

/// `YYYY-MM-DD`, by its shape.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(at, b)| match at {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// `dollars` per million tokens in picodollars per token, or why it is no price.
fn micro(dollars: f64, kind: &str, named: &str) -> Result<u64, String> {
    if !(0.0..=MAX_PRICE).contains(&dollars) {
        return Err(format!(
            "the price table's {kind} for {named} is {dollars}, not a price from 0 to {MAX_PRICE}"
        ));
    }
    Ok(whole(dollars * 1_000_000.0))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a price checked from 0 to MAX_PRICE, in millionths, rounded, fits a u64"
)]
fn whole(millionths: f64) -> u64 {
    millionths.round() as u64
}

/// What tokens cost at the table's prices, each kind in picodollars, and the tokens of the models
/// it has no price for, counted apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cost {
    pub input: u128,
    pub cache_write_5m: u128,
    pub cache_write_1h: u128,
    pub cache_read: u128,
    pub output: u128,
    /// The tokens of each model and speed no price was found for: in no dollar figure.
    pub unpriced: BTreeMap<(String, String), Tokens>,
}

impl Cost {
    /// What the tokens of the priced models cost, every kind told, in picodollars.
    pub fn total(&self) -> u128 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read + self.output
    }

    /// `other`'s added to this.
    pub fn add(&mut self, other: &Cost) {
        self.input += other.input;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
        self.cache_read += other.cache_read;
        self.output += other.output;
        for (key, tokens) in &other.unpriced {
            self.unpriced.entry(key.clone()).or_default().add(tokens);
        }
    }

    /// The JSON the agents API and `tasks show --json` give: US dollars of each kind and all told,
    /// the tokens no price was found for, and `read`, the day the table's prices were read, which
    /// says it is an estimate at them.
    pub fn json(&self, read: &str) -> Py {
        let unpriced = (self.unpriced.iter())
            .map(|((model, speed), tokens)| {
                Py::obj(vec![
                    ("model", Py::str(model)),
                    ("speed", Py::str(speed)),
                    ("tokens", tokens.json()),
                ])
            })
            .collect();
        Py::obj(vec![
            ("dollars", Py::Float(in_dollars(self.total()))),
            ("input", Py::Float(in_dollars(self.input))),
            ("cache_write_5m", Py::Float(in_dollars(self.cache_write_5m))),
            ("cache_write_1h", Py::Float(in_dollars(self.cache_write_1h))),
            ("cache_read", Py::Float(in_dollars(self.cache_read))),
            ("output", Py::Float(in_dollars(self.output))),
            ("unpriced", Py::List(unpriced)),
            ("prices", Py::str(read)),
        ])
    }

    /// The figure a line says: the dollars, `+ unpriced` after them where some tokens had no price,
    /// and `unpriced` alone where none had one.
    pub fn said(&self) -> String {
        match (self.total(), self.unpriced.is_empty()) {
            (0, false) => "unpriced".into(),
            (pico, true) => dollars(pico),
            (pico, false) => format!("{} + unpriced", dollars(pico)),
        }
    }

    /// The models no price was found for, each with its tokens (`gpt-x 1.2M tokens`, a speed
    /// other than `standard` after the model), joined by `between`.
    pub fn unpriced_said(&self, between: &str) -> String {
        (self.unpriced.iter())
            .map(|((model, speed), tokens)| {
                let speed = if speed == "standard" {
                    String::new()
                } else {
                    format!(" {speed}")
                };
                format!("{model}{speed} {} tokens", super::amount(tokens.total()))
            })
            .collect::<Vec<_>>()
            .join(between)
    }
}

/// `pico` picodollars to the cent, half a cent up: `$0.00`, `$7.27`, `$1234.50`.
pub fn dollars(pico: u128) -> String {
    let cents = (pico + PICO_PER_CENT / 2) / PICO_PER_CENT;
    format!("${}.{:02}", cents / 100, cents % 100)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a dollar figure in JSON: an f64 keeps fifteen significant digits of it"
)]
fn in_dollars(pico: u128) -> f64 {
    pico as f64 / 1e12
}

#[cfg(test)]
#[path = "price_tests.rs"]
mod tests;
