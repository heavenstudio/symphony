//! The tokens of a project's day files ([`symphony_api::reports::TokenSum`]): what the board's
//! `usage` table gained from one roll-up to the next, shared among the hours between the two.
//!
//! The table keeps a sum for each session, task, role, model, speed and round, and no day
//! ([`crate::usage`]), so a day's tokens are told by difference: each roll-up keeps the whole table
//! summed by role, model and speed in the day file it writes ([`ReportDay::usage`]), and the next
//! takes that from its own read. What was gained is shared among the hours from the earlier read
//! to the later by the time of each between them ([`gained`]), a count's remainder to the last, so
//! every token is counted once and a roll-up a few seconds into an hour gives the hour before it
//! nearly all. Hours no roll-up ran in (a server stopped) share what was gained across them
//! evenly. A sum that fell (a board put back from a copy) gained nothing. Nothing here reads the
//! board, a file or the clock.

use super::HOUR;
use crate::usage::{Prices, Spend, Tokens};
use std::cmp::Reverse;
use symphony_api::reports::{Landed, ReportDay, TokenSum};

/// How many of the dearest landed tasks a reader names.
pub const DEAREST: usize = 10;

/// `sum`'s tokens and what they cost at `prices`' price of its model and speed.
pub fn spend(sum: &TokenSum, prices: &Prices) -> Spend {
    let tokens = Tokens {
        input: sum.input,
        cache_write_5m: sum.cache_write_5m,
        cache_write_1h: sum.cache_write_1h,
        cache_read: sum.cache_read,
        output: sum.output,
        calls: sum.calls,
    };
    Spend {
        cost: prices.cost(&sum.model, &sum.speed, &tokens),
        tokens,
    }
}

/// `sums` by role at `prices`, in the order a task's tally lists its roles
/// ([`crate::usage::ROLE_ORDER`]), then by name; a role with no token is left out.
pub fn by_role<'a>(
    sums: impl IntoIterator<Item = &'a TokenSum>,
    prices: &Prices,
) -> Vec<(String, Spend)> {
    let rank = |role: &str| {
        (crate::usage::ROLE_ORDER.iter())
            .position(|known| *known == role)
            .unwrap_or(crate::usage::ROLE_ORDER.len())
    };
    // Kept in that order as it is made, which links no sort into the binaries.
    let mut roles: Vec<(String, Spend)> = Vec::new();
    for sum in sums {
        let spent = spend(sum, prices);
        if spent.tokens == Tokens::default() {
            continue;
        }
        let place = (rank(&sum.role), sum.role.as_str());
        match roles.binary_search_by(|(role, _)| (rank(role), role.as_str()).cmp(&place)) {
            Ok(at) => roles[at].1.add(&spent),
            Err(at) => roles.insert(at, (sum.role.clone(), spent)),
        }
    }
    roles
}

/// What the tasks landed in a window took, at the prices they were read at.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LandedCosts {
    /// Each landed task the board recorded tokens for, the dearest first, then by slug: its slug,
    /// when it landed and what it and its subtasks took.
    pub tasks: Vec<(String, f64, Spend)>,
    /// The landed tasks it recorded none for: in no figure.
    pub without: usize,
    /// The median estimated cost of those of `tasks` with a priced token, in picodollars: of an
    /// even count, the mean of the two in the middle. None with none.
    pub median: Option<u128>,
    /// What `tasks` took up to their first review, all told.
    pub first: Spend,
    /// What they took in every round after it: each review, and the fixes it asked for.
    pub later: Spend,
    /// How many of `tasks` took anything after their first round.
    pub again: usize,
}

/// What `landed` took at `prices`: each task once, the first of a slug met standing for it (a
/// reader hands the newest day's first, so a task landed again is counted at its last landing).
pub fn landed_costs<'a>(
    landed: impl IntoIterator<Item = &'a Landed>,
    prices: &Prices,
) -> LandedCosts {
    let mut costs = LandedCosts::default();
    let mut seen: Vec<&str> = Vec::new();
    for task in landed {
        if seen.contains(&task.slug.as_str()) {
            continue;
        }
        seen.push(&task.slug);
        let mut all = Spend::default();
        let mut after = false;
        for sum in &task.spent {
            let spent = spend(sum, prices);
            if spent.tokens == Tokens::default() {
                continue;
            }
            all.add(&spent);
            if sum.round > 1 {
                costs.later.add(&spent);
                after = true;
            } else {
                costs.first.add(&spent);
            }
        }
        if all.tokens == Tokens::default() {
            costs.without += 1;
            continue;
        }
        costs.again += usize::from(after);
        // Put in its place, the dearest first, then by slug, which links no sort.
        let place = (Reverse(all.cost.total()), task.slug.as_str());
        let at = (costs.tasks.iter())
            .position(|(slug, _, kept)| (Reverse(kept.cost.total()), slug.as_str()) > place)
            .unwrap_or(costs.tasks.len());
        costs.tasks.insert(at, (task.slug.clone(), task.at, all));
    }
    // A task none of whose tokens had a price has no figure to rank.
    let priced: Vec<u128> = (costs.tasks.iter())
        .map(|(_, _, spend)| &spend.cost)
        .filter(|cost| cost.total() > 0 || cost.unpriced.is_empty())
        .map(crate::usage::Cost::total)
        .collect();
    costs.median = match priced.len() {
        0 => None,
        n if n % 2 == 1 => Some(priced[n / 2]),
        n => Some(u128::midpoint(priced[n / 2 - 1], priced[n / 2])),
    };
    costs
}

/// The counts of a sum, in the order the file writes them.
fn counts(sum: &mut TokenSum) -> [&mut u64; 6] {
    [
        &mut sum.input,
        &mut sum.cache_write_5m,
        &mut sum.cache_write_1h,
        &mut sum.cache_read,
        &mut sum.output,
        &mut sum.calls,
    ]
}

fn is_none(sum: &TokenSum) -> bool {
    let mut sum = sum.clone();
    counts(&mut sum).iter().all(|count| **count == 0)
}

fn same(a: &TokenSum, b: &TokenSum) -> bool {
    (&a.role, a.round, &a.model, &a.speed) == (&b.role, b.round, &b.model, &b.speed)
}

/// `sum` added to the one of `sums` it is of, or put there.
pub fn add(sums: &mut Vec<TokenSum>, sum: &TokenSum) {
    match sums.iter_mut().find(|kept| same(kept, sum)) {
        Some(kept) => {
            let mut sum = sum.clone();
            for (kept, more) in counts(kept).into_iter().zip(counts(&mut sum)) {
                *kept += *more;
            }
        }
        None => sums.push(sum.clone()),
    }
}

/// What the table gained from `base`, its sums at `from`, to `now`, its sums at `to`: each hour
/// from the one `from` falls in to the one `to` falls in, by its first second, with its share. An
/// hour that gained nothing is there with none, as it is known.
pub fn gained(
    base: &[TokenSum],
    from: f64,
    now: &[TokenSum],
    to: f64,
) -> Vec<(f64, Vec<TokenSum>)> {
    let from = from.min(to);
    let first = (from / HOUR).floor() * HOUR;
    // Each hour's part of the time between the reads, in milliseconds; one read in the same
    // instant as the other gives its hour everything.
    let mut hours: Vec<(f64, u128, Vec<TokenSum>)> = Vec::new();
    let mut start = first;
    while start < to || hours.is_empty() {
        let part = ((start + HOUR).min(to) - start.max(from)).max(0.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "at most an hour in milliseconds, never negative"
        )]
        hours.push((start, (part * 1000.0).round() as u128, Vec::new()));
        start += HOUR;
    }
    let mut whole: u128 = hours.iter().map(|hour| hour.1).sum();
    if whole == 0 {
        if let Some(last) = hours.last_mut() {
            last.1 = 1;
        }
        whole = 1;
    }
    let last = hours.len() - 1;
    for sum in now {
        let mut more = sum.clone();
        if let Some(had) = base.iter().find(|had| same(had, sum)) {
            let mut had = had.clone();
            for (more, had) in counts(&mut more).into_iter().zip(counts(&mut had)) {
                *more = more.saturating_sub(*had);
            }
        }
        if is_none(&more) {
            continue;
        }
        let mut left = more.clone();
        for (at, (_, part, sums)) in hours.iter_mut().enumerate() {
            let mut share = more.clone();
            let mut rest = left.clone();
            for ((share, all), left) in counts(&mut share)
                .into_iter()
                .zip(counts(&mut more))
                .zip(counts(&mut rest))
            {
                *share = if at == last {
                    *left
                } else {
                    u64::try_from(u128::from(*all) * *part / whole).unwrap_or(*left)
                };
                *left -= *share;
            }
            left = rest;
            if !is_none(&share) {
                sums.push(share);
            }
        }
    }
    (hours.into_iter())
        .map(|(start, _, sums)| (start, sums))
        .collect()
}

/// `day`'s tokens: `usage`, the table's sums at its read, kept for the next roll-up; and each of
/// its hours' share of `gained` ([`gained`]) added to what `before`, the file this one takes the
/// place of, kept for that hour. An hour neither names stays unknown.
pub fn fill(
    day: &mut ReportDay,
    usage: &[TokenSum],
    before: Option<&ReportDay>,
    gained: &[(f64, Vec<TokenSum>)],
) {
    day.usage = Some(usage.to_vec());
    for hour in &mut day.hours {
        let kept = before
            .and_then(|file| file.hours.iter().find(|kept| kept.start == hour.start))
            .and_then(|kept| kept.tokens.clone());
        let more = gained.iter().find(|(start, _)| *start == hour.start);
        hour.tokens = match (kept, more) {
            (None, None) => None,
            (kept, more) => {
                let mut sums = kept.unwrap_or_default();
                for sum in more.map(|(_, sums)| sums).into_iter().flatten() {
                    add(&mut sums, sum);
                }
                Some(sums)
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_api::reports::ReportHour;

    fn sum(role: &str, model: &str, n: u64) -> TokenSum {
        TokenSum {
            role: role.into(),
            round: 0,
            model: model.into(),
            speed: "standard".into(),
            input: n,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: n * 10,
            output: n,
            calls: 1,
        }
    }

    /// Sums as a test says them: each one's role, input tokens and calls.
    type Said<'a> = Vec<(&'a str, u64, u64)>;

    fn inputs(hours: &[(f64, Vec<TokenSum>)]) -> Vec<(f64, Said<'_>)> {
        (hours.iter())
            .map(|(start, sums)| {
                let sums = sums.iter().map(|s| (s.role.as_str(), s.input, s.calls));
                (*start / HOUR, sums.collect())
            })
            .collect()
    }

    fn prices() -> Prices {
        Prices::parse(
            "read = \"2026-01-02\"\npage = \"https://example.com/pricing\"\n[[price]]\n\
             model = \"m\"\ninput = 1000000\ncache_write_5m = 0\ncache_write_1h = 0\n\
             cache_read = 0\noutput = 0\n",
        )
        .unwrap()
    }

    fn landed(slug: &str, at: f64, spent: &[(&str, u32, &str, u64)]) -> Landed {
        Landed {
            slug: slug.into(),
            at,
            spent: (spent.iter())
                .map(|(role, round, model, n)| TokenSum {
                    round: *round,
                    ..sum(role, model, *n)
                })
                .collect(),
        }
    }

    #[test]
    fn roles_are_summed_in_the_tallys_order_and_an_unpriced_model_is_in_no_dollar() {
        let sums = [
            sum("worker", "m", 1),
            sum("reviewer", "m", 2),
            sum("executor", "other", 5),
            sum("executor", "m", 3),
            sum("reviewer", "m", 0),
        ];
        let roles = by_role(&sums, &prices());
        let told: Vec<(&str, u64, String)> = (roles.iter())
            .map(|(role, spend)| (role.as_str(), spend.tokens.input, spend.cost.said()))
            .collect();
        // A dollar an input token.
        assert_eq!(
            told,
            [
                ("executor", 8, "$3.00 + unpriced".to_string()),
                ("reviewer", 2, "$2.00".to_string()),
                ("worker", 1, "$1.00".to_string()),
            ]
        );
        let mut empty = sum("worker", "m", 0);
        empty.calls = 0;
        assert!(by_role(&[empty], &prices()).is_empty());
        // Roles the tally names none of come after its own, by name, whatever order they came in.
        let odd = [
            sum("zed", "m", 1),
            sum("orchestrator", "m", 1),
            sum("able", "m", 1),
            sum("executor", "m", 1),
            sum("zed", "m", 1),
        ];
        let named: Vec<(String, u64)> = (by_role(&odd, &prices()).into_iter())
            .map(|(role, spend)| (role, spend.tokens.input))
            .collect();
        assert_eq!(
            named,
            [
                ("executor".to_string(), 1),
                ("orchestrator".to_string(), 1),
                ("able".to_string(), 1),
                ("zed".to_string(), 2),
            ]
        );
    }

    #[test]
    fn landed_tasks_are_ranked_by_cost_with_their_median_and_their_rounds_told_apart() {
        let files = [
            landed(
                "a/dear",
                30.0,
                &[("executor", 1, "m", 6), ("reviewer", 2, "m", 4)],
            ),
            landed("a/none", 31.0, &[]),
            landed("a/cheap", 32.0, &[("executor", 1, "m", 1)]),
            landed(
                "a/mid",
                33.0,
                &[("executor", 1, "m", 2), ("executor", 3, "m", 1)],
            ),
            // An older day's file names a task landed again: its newest landing stands.
            landed("a/dear", 5.0, &[("executor", 1, "m", 100)]),
            landed("a/unpriced", 34.0, &[("executor", 1, "other", 9)]),
        ];
        let costs = landed_costs(&files, &prices());
        let ranked: Vec<(&str, f64, String)> = (costs.tasks.iter())
            .map(|(slug, at, spend)| (slug.as_str(), *at, spend.cost.said()))
            .collect();
        assert_eq!(
            ranked,
            [
                ("a/dear", 30.0, "$10.00".to_string()),
                ("a/mid", 33.0, "$3.00".to_string()),
                ("a/cheap", 32.0, "$1.00".to_string()),
                ("a/unpriced", 34.0, "unpriced".to_string()),
            ]
        );
        assert_eq!(costs.without, 1);
        // Of the three with a price; the unpriced one has no figure to rank.
        assert_eq!(costs.median, Some(3_000_000_000_000));
        assert_eq!(costs.first.cost.said(), "$9.00 + unpriced");
        assert_eq!(costs.later.cost.said(), "$5.00");
        assert_eq!(costs.again, 2);
        // Of two: the mean of $10 and $1.
        let even = landed_costs(&files[..3], &prices());
        assert_eq!(even.median, Some(5_500_000_000_000));
        let none = landed_costs(&files[1..2], &prices());
        assert_eq!((none.median, none.without, none.tasks.len()), (None, 1, 0));
        // Tasks of one cost go by slug, whatever order the files name them in.
        let tied = [
            landed("t/b", 1.0, &[("executor", 1, "m", 2)]),
            landed("t/c", 2.0, &[("executor", 1, "m", 5)]),
            landed("t/a", 3.0, &[("executor", 1, "m", 2)]),
            landed("t/d", 4.0, &[("executor", 1, "m", 2)]),
        ];
        let order: Vec<String> = (landed_costs(&tied, &prices()).tasks.into_iter())
            .map(|(slug, _, _)| slug)
            .collect();
        assert_eq!(order, ["t/c", "t/a", "t/b", "t/d"]);
    }

    #[test]
    fn a_roll_up_just_into_an_hour_gives_the_hour_before_it_nearly_everything() {
        let base = [sum("executor", "m", 100)];
        let now = [sum("executor", "m", 3_700), sum("reviewer", "m", 7)];
        // From five seconds into hour 10 to five seconds into hour 11: 3,595 s and 5 s.
        let hours = gained(&base, 10.0 * HOUR + 5.0, &now, 11.0 * HOUR + 5.0);
        assert_eq!(
            inputs(&hours),
            [
                (10.0, vec![("executor", 3_595, 0), ("reviewer", 6, 0)]),
                // The remainder, and the one call neither hour's share holds whole.
                (11.0, vec![("executor", 5, 0), ("reviewer", 1, 1)]),
            ]
        );
        let all: u64 = hours
            .iter()
            .flat_map(|(_, sums)| sums)
            .map(|s| s.cache_read)
            .sum();
        assert_eq!(all, 36_000 + 70, "every token once");
    }

    #[test]
    fn hours_with_no_roll_up_share_evenly_and_a_quiet_one_is_known_as_none() {
        let hours = gained(&[], 2.0 * HOUR, &[sum("worker", "m", 300)], 5.0 * HOUR);
        assert_eq!(
            inputs(&hours),
            [
                (2.0, vec![("worker", 100, 0)]),
                (3.0, vec![("worker", 100, 0)]),
                (4.0, vec![("worker", 100, 1)]),
            ]
        );
        let same = [sum("worker", "m", 300)];
        assert_eq!(
            inputs(&gained(&same, 2.0 * HOUR, &same, 3.5 * HOUR)),
            [(2.0, vec![]), (3.0, vec![])],
            "nothing gained is known, not missing"
        );
    }

    #[test]
    fn a_sum_that_fell_gains_nothing_and_reads_in_one_instant_give_their_hour_all() {
        let fell = gained(
            &[sum("worker", "m", 300)],
            HOUR,
            &[sum("worker", "m", 200)],
            1.5 * HOUR,
        );
        assert_eq!(inputs(&fell), [(1.0, vec![])]);
        let at_once = gained(&[], 7.25 * HOUR, &[sum("worker", "m", 9)], 7.25 * HOUR);
        assert_eq!(inputs(&at_once), [(7.0, vec![("worker", 9, 1)])]);
        // A clock that went back: the later read's hour takes it.
        let back = gained(&[], 9.0 * HOUR, &[sum("worker", "m", 9)], 7.25 * HOUR);
        assert_eq!(inputs(&back), [(7.0, vec![("worker", 9, 1)])]);
        // A fast speed is a sum of its own.
        let mut fast = sum("worker", "m", 4);
        fast.speed = "fast".into();
        let both = gained(
            &[sum("worker", "m", 1)],
            HOUR,
            &[sum("worker", "m", 1), fast],
            HOUR,
        );
        assert_eq!(both[0].1.len(), 1);
        assert_eq!(both[0].1[0].speed, "fast");
    }

    #[test]
    fn a_day_keeps_the_sums_and_adds_each_hours_share_to_what_the_file_before_kept() {
        let hour = |start: f64, tokens: Option<Vec<TokenSum>>| ReportHour {
            start: start * HOUR,
            tokens,
            ..ReportHour::default()
        };
        let before = ReportDay {
            hours: vec![
                hour(0.0, None),
                hour(1.0, Some(vec![sum("executor", "m", 5)])),
            ],
            ..ReportDay::default()
        };
        let mut day = ReportDay {
            hours: vec![
                hour(0.0, None),
                hour(1.0, None),
                hour(2.0, None),
                hour(3.0, None),
            ],
            ..ReportDay::default()
        };
        let usage = [sum("executor", "m", 50)];
        let gained = [
            (HOUR, vec![sum("executor", "m", 2), sum("reviewer", "m", 1)]),
            (2.0 * HOUR, vec![]),
        ];
        fill(&mut day, &usage, Some(&before), &gained);
        assert_eq!(day.usage.as_deref(), Some(&usage[..]));
        let told: Vec<Option<Said<'_>>> = (day.hours.iter())
            .map(|hour| {
                (hour.tokens.as_ref()).map(|sums| {
                    sums.iter()
                        .map(|s| (s.role.as_str(), s.input, s.calls))
                        .collect()
                })
            })
            .collect();
        assert_eq!(
            told,
            [
                None,
                Some(vec![("executor", 7, 2), ("reviewer", 1, 1)]),
                Some(vec![]),
                None,
            ],
            "an hour no roll-up spans stays unknown"
        );
        let mut first = ReportDay {
            hours: vec![hour(0.0, None)],
            ..ReportDay::default()
        };
        fill(&mut first, &usage, None, &[]);
        assert_eq!(
            (first.usage.is_some(), &first.hours[0].tokens),
            (true, &None)
        );
    }
}
