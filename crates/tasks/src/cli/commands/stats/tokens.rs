//! `stats`' tokens: what the board recorded per UTC day by role, what each landed task took,
//! what review rounds cost against first rounds and how many tasks were back after review, with their cost estimated at API list prices
//! ([`crate::usage::price`]). All of it is read from the hourly roll-up's day files
//! ([`crate::reports`]), the ones the Reports page sums, so the two say the same figures, and
//! never from the board's `usage` table, which keeps no day.

use crate::cli::Env;
use crate::json_object::object;
use crate::reports::spend::{self, DEAREST, LandedCosts};
use crate::reports::{DAY, day_start};
use crate::usage::{Prices, Spend, amount, price};
use serde_json::Value;
use std::path::Path;
use symphony_api::reports::{Landed, ReportDay, TokenSum};

/// A day's tokens by role, or none when no hour of it tells any.
pub(super) type DayRoles = Option<Vec<(String, Spend)>>;

/// What the day files of a window hold of tokens.
#[derive(Debug, Default)]
pub(super) struct Told {
    /// Each day of the window that has a file, oldest first, with its roles' tokens.
    pub days: Vec<(String, DayRoles)>,
    pub landed: LandedCosts,
    /// The day the prices were read.
    pub prices: String,
    /// Why no day file could be read, where none could.
    pub unread: Option<String>,
}

/// The tokens of the `days` UTC days up to the one `now` falls in, from the day files of the
/// board `env` reaches, at `prices`.
pub(super) fn told(env: &Env, now: f64, days: i64, prices: &Prices) -> Told {
    match crate::reports::folder(env) {
        Ok(folder) => told_in(&folder, now, days, prices),
        Err(why) => Told {
            prices: prices.read.clone(),
            unread: Some(why),
            ..Told::default()
        },
    }
}

/// [`told`], of the day files in `folder`. A file that does not read is passed over, as a day with
/// none.
pub(super) fn told_in(folder: &Path, now: f64, days: i64, prices: &Prices) -> Told {
    let mut told = Told {
        prices: prices.read.clone(),
        ..Told::default()
    };
    let mut landings: Vec<Landed> = Vec::new();
    let today = day_start(now);
    // Newest first, so a task landed again is its last landing's.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a count of days of at most ten years"
    )]
    for back in 0..days {
        let name = symphony_api::run_day(today - back as f64 * DAY);
        let Ok(Some(file)) = crate::reports::read(folder, &name) else {
            continue;
        };
        landings.extend(file.landed.iter().rev().cloned());
        told.days.push((name, day_tokens(&file, prices)));
    }
    told.days.reverse();
    told.landed = spend::landed_costs(&landings, prices);
    told
}

/// A day file's tokens by role: its hours' that tell any, summed; none when none does.
fn day_tokens(file: &ReportDay, prices: &Prices) -> Option<Vec<(String, Spend)>> {
    let mut sums: Option<Vec<TokenSum>> = None;
    for tokens in file.hours.iter().filter_map(|hour| hour.tokens.as_ref()) {
        let kept = sums.get_or_insert_default();
        for sum in tokens {
            spend::add(kept, sum);
        }
    }
    sums.map(|sums| spend::by_role(&sums, prices))
}

fn all(roles: &[(String, Spend)]) -> Spend {
    let mut all = Spend::default();
    for (_, spend) in roles {
        all.add(spend);
    }
    all
}

fn tokens(spend: &Spend) -> String {
    amount(spend.tokens.total())
}

/// The lines `stats` prints of `told`.
pub(super) fn text(told: &Told) -> Vec<String> {
    if let Some(why) = &told.unread {
        return vec![format!("Tokens: no day file read ({why})")];
    }
    if told.days.is_empty() {
        return vec![
            "Tokens: no day file in these days (the machine's server writes one each hour)".into(),
        ];
    }
    let mut lines = vec![format!(
        "Tokens per day, UTC, from the hourly roll-up's day files; every dollar is an estimate at \
         API list prices read {}, never money spent. An hour's tokens are what the board's usage \
         gained between two roll-ups, shared among the hours between them by time; a day no two \
         roll-ups span says no record:",
        told.prices
    )];
    let mut unpriced = crate::usage::Cost::default();
    for (day, roles) in &told.days {
        lines.push(match roles {
            None => format!("  {day}  no record"),
            Some(roles) if roles.is_empty() => format!("  {day}  0"),
            Some(roles) => {
                let all = all(roles);
                unpriced.add(&all.cost);
                let by: Vec<String> = (roles.iter())
                    .map(|(role, spend)| format!("{role} {} {}", tokens(spend), spend.cost.said()))
                    .collect();
                format!(
                    "  {day}  {}  {}  ({})",
                    tokens(&all),
                    all.cost.said(),
                    by.join(" · ")
                )
            }
        });
    }
    if !unpriced.unpriced.is_empty() {
        lines.push(format!(
            "  no price for {}, in no figure",
            unpriced.unpriced_said(" · ")
        ));
    }
    let landed = &told.landed;
    if landed.tasks.is_empty() {
        lines.push(format!(
            "Cost per landed task: none landed with tokens recorded{}",
            without(landed)
        ));
        return lines;
    }
    lines.push(format!(
        "Cost per landed task: {} landed with tokens recorded{}, median {}; the dearest:",
        landed.tasks.len(),
        without(landed),
        landed.median.map_or("unpriced".into(), price::dollars)
    ));
    for task in landed.tasks.iter().take(DEAREST) {
        let spend = task.spend();
        lines.push(format!(
            "  {:<40} {}  {} tokens",
            task.slug,
            spend.cost.said(),
            tokens(&spend)
        ));
    }
    lines.push(format!(
        "First rounds against review rounds, of those tasks: first rounds {} ({} tokens) · review \
         rounds {} ({} tokens); back after review: {} of {} were reviewed twice or more",
        landed.first.cost.said(),
        tokens(&landed.first),
        landed.later.cost.said(),
        tokens(&landed.later),
        landed.back,
        landed.tasks.len()
    ));
    lines
}

fn without(landed: &LandedCosts) -> String {
    match landed.without {
        0 => String::new(),
        n => format!(" ({n} with none)"),
    }
}

fn spend_json(spend: &Spend, prices: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("tokens", spend.tokens.json()),
        ("cost", spend.cost.json(prices)),
    ]
}

/// `told` as `stats --json` carries it, under `tokens`: each `tokens` and `cost` as `show --json`
/// writes them.
pub(super) fn json(told: &Told) -> Value {
    let count = |n: usize| Value::from(i64::try_from(n).unwrap_or(i64::MAX));
    let prices = told.prices.as_str();
    let days = (told.days.iter())
        .map(|(day, roles)| {
            let by_role = roles.as_ref().map_or(Value::Null, |roles| {
                Value::Array(
                    (roles.iter())
                        .map(|(role, spend)| {
                            let mut fields = vec![("role", Value::from(role.as_str()))];
                            fields.extend(spend_json(spend, prices));
                            object(fields)
                        })
                        .collect(),
                )
            });
            object(vec![
                ("day", Value::from(day.as_str())),
                ("by_role", by_role),
            ])
        })
        .collect();
    let landed = &told.landed;
    let dearest = (landed.tasks.iter().take(DEAREST))
        .map(|task| {
            let mut fields = vec![
                ("slug", Value::from(task.slug.as_str())),
                ("at", Value::from(task.at)),
            ];
            fields.extend(spend_json(&task.spend(), prices));
            object(fields)
        })
        .collect();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a dollar figure in JSON: an f64 keeps fifteen significant digits of it"
    )]
    let median = landed.median.map(|pico| pico as f64 / 1e12);
    object(vec![
        ("prices", Value::from(prices)),
        ("per_day", Value::Array(days)),
        (
            "landed",
            object(vec![
                ("tasks", count(landed.tasks.len())),
                ("without", count(landed.without)),
                ("median_dollars", Value::from(median)),
                ("dearest", Value::Array(dearest)),
                ("first_round", object(spend_json(&landed.first, prices))),
                ("later_rounds", object(spend_json(&landed.later, prices))),
                // `again` as it always was, every task reviewed, for a reader of it; the count
                // that says something is `back_after_review`, the tasks reviewed twice or more.
                ("again", count(landed.again)),
                ("back_after_review", count(landed.back)),
            ]),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn told(days: Vec<(&str, DayRoles)>, landed: LandedCosts) -> Told {
        Told {
            days: (days.into_iter())
                .map(|(day, roles)| (day.to_string(), roles))
                .collect(),
            landed,
            prices: "2026-11-16".into(),
            unread: None,
        }
    }

    #[test]
    fn a_day_no_two_roll_ups_span_says_no_record_and_one_of_none_says_zero() {
        let lines = text(&told(
            vec![("2026-11-18", None), ("2026-11-19", Some(Vec::new()))],
            LandedCosts {
                without: 2,
                ..LandedCosts::default()
            },
        ));
        assert!(lines[0].contains("API list prices read 2026-11-16, never money spent"));
        assert_eq!(
            lines[1..],
            [
                "  2026-11-18  no record",
                "  2026-11-19  0",
                "Cost per landed task: none landed with tokens recorded (2 with none)",
            ]
        );
        let json = json(&told(vec![("2026-11-18", None)], LandedCosts::default()));
        assert_eq!(json["per_day"][0]["by_role"], Value::Null);
        assert_eq!(json["landed"]["median_dollars"], Value::Null);
        assert_eq!(json["landed"]["dearest"], Value::Array(Vec::new()));
    }

    #[test]
    fn back_after_review_counts_the_tasks_reviewed_twice_beside_again() {
        let task = |slug: &str| spend::LandedTask {
            slug: slug.into(),
            ..spend::LandedTask::default()
        };
        let landed = LandedCosts {
            tasks: vec![task("a/one"), task("a/two"), task("a/three")],
            again: 3,
            back: 1,
            ..LandedCosts::default()
        };
        let told = told(vec![("2026-11-18", Some(Vec::new()))], landed);
        let lines = text(&told);
        assert!(
            lines
                .last()
                .unwrap()
                .ends_with("back after review: 1 of 3 were reviewed twice or more"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.starts_with("  a/one ")),
            "{lines:?}"
        );
        let json = json(&told);
        assert_eq!(
            (
                &json["landed"]["again"],
                &json["landed"]["back_after_review"]
            ),
            (&Value::from(3), &Value::from(1))
        );
    }

    #[test]
    fn a_window_with_no_file_and_a_board_whose_files_are_not_kept_say_so() {
        let empty = text(&told(Vec::new(), LandedCosts::default()));
        assert_eq!(
            empty,
            ["Tokens: no day file in these days (the machine's server writes one each hour)"]
        );
        let unread = Told {
            unread: Some("no folder".into()),
            ..Told::default()
        };
        assert_eq!(text(&unread), ["Tokens: no day file read (no folder)"]);
    }

    #[test]
    fn a_file_that_does_not_read_is_passed_over_as_a_day_with_none() {
        let folder =
            std::env::temp_dir().join(format!("symphony-stats-tokens-unit-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let now = 1_800_000_000.0;
        let name = symphony_api::run_day(day_start(now));
        std::fs::write(folder.join(format!("{name}.json")), "{not json").unwrap();
        let told = told_in(&folder, now, 2, Prices::embedded());
        assert!(told.days.is_empty(), "{:?}", told.days);
        assert_eq!(told.prices, Prices::embedded().read);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
