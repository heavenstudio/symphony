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

/// A landed task priced: what it and its subtasks took up to its first review and after it, how
/// many reviews it took, its kind and the lines its landing changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LandedTask {
    pub slug: String,
    /// When it landed.
    pub at: f64,
    /// The kind it was given ([`crate::kind`]); none for one given none.
    pub kind: Option<String>,
    /// What it took up to its first review: its round 1, with any sum told in no round.
    pub first: Spend,
    /// What it took in every round after it: each review, and the fixes it asked for.
    pub later: Spend,
    /// How many reviews it took: the last round it took tokens in, less the first; none for a task
    /// whose tokens are all its first round's.
    pub reviews: u32,
    /// The lines its landing changed, added and removed together; none where the roll-up read
    /// none ([`Landed::lines`]).
    pub lines: Option<u64>,
}

impl LandedTask {
    /// What it took, all told.
    pub fn spend(&self) -> Spend {
        let mut all = self.first.clone();
        all.add(&self.later);
        all
    }

    /// What its priced tokens cost, in picodollars.
    pub fn total(&self) -> u128 {
        self.first.cost.total() + self.later.cost.total()
    }

    /// Its estimated cost in picodollars; none where none of its tokens had a price, which gives
    /// it no figure to rank. A cost with a part unpriced is its priced part.
    pub fn priced(&self) -> Option<u128> {
        let unpriced = !self.first.cost.unpriced.is_empty() || !self.later.cost.unpriced.is_empty();
        let total = self.total();
        (total > 0 || !unpriced).then_some(total)
    }
}

/// What the tasks landed in a window took, at the prices they were read at.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LandedCosts {
    /// Each landed task the board recorded tokens for, the dearest first, then by slug.
    pub tasks: Vec<LandedTask>,
    /// The landed tasks it recorded none for: in no figure.
    pub without: usize,
    /// The median estimated cost of those of `tasks` with a priced token, in picodollars: of an
    /// even count, the mean of the two in the middle. None with none.
    pub median: Option<u128>,
    /// What `tasks` took up to their first review, all told.
    pub first: Spend,
    /// What they took in every round after it: each review, and the fixes it asked for.
    pub later: Spend,
    /// How many of `tasks` took anything after their first round: every task reviewed, as its
    /// first review's tokens are its second round's. Kept for `stats --json`'s readers.
    pub again: usize,
    /// How many of `tasks` were back after review: reviewed twice or more, a round past the second,
    /// so their first review sent them back.
    pub back: usize,
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
        let mut priced = LandedTask {
            slug: task.slug.clone(),
            at: task.at,
            kind: task.kind.clone(),
            lines: task.lines.map(|lines| lines.changed()),
            ..LandedTask::default()
        };
        let mut last = 1;
        for sum in &task.spent {
            let spent = spend(sum, prices);
            if spent.tokens == Tokens::default() {
                continue;
            }
            last = last.max(sum.round);
            if sum.round > 1 {
                priced.later.add(&spent);
            } else {
                priced.first.add(&spent);
            }
        }
        if priced.spend().tokens == Tokens::default() {
            costs.without += 1;
            continue;
        }
        priced.reviews = last - 1;
        costs.first.add(&priced.first);
        costs.later.add(&priced.later);
        costs.again += usize::from(last > 1);
        costs.back += usize::from(last > 2);
        costs.tasks.push(priced);
    }
    // The dearest first, then by slug, through the crate's one sort.
    crate::order::sort_by(&mut costs.tasks, &|a: &LandedTask, b: &LandedTask| {
        (Reverse(a.total()), &a.slug).cmp(&(Reverse(b.total()), &b.slug))
    });
    let priced: Vec<u128> = costs.tasks.iter().filter_map(LandedTask::priced).collect();
    costs.median = match priced.len() {
        0 => None,
        n if n % 2 == 1 => Some(priced[n / 2]),
        n => Some(u128::midpoint(priced[n / 2 - 1], priced[n / 2])),
    };
    costs
}

/// `pico` picodollars in dollars, as a figure on the wire says them.
#[expect(
    clippy::cast_precision_loss,
    reason = "a dollar figure: an f64 keeps fifteen significant digits of it"
)]
pub fn in_dollars(pico: u128) -> f64 {
    pico as f64 / 1e12
}

/// What a set of landed tasks cost, as a figure says it: every amount in dollars, of the tasks
/// with a priced cost ([`LandedTask::priced`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    /// The tasks with a priced cost.
    pub tasks: usize,
    pub median: Option<f64>,
    pub mean: Option<f64>,
    pub p90: Option<f64>,
    /// What they cost, all told.
    pub cost: f64,
    /// What their rounds after the first cost: the review rounds.
    pub later: f64,
    /// Of them, those whose landing changed a counted line, what those cost and the lines.
    pub lined: usize,
    pub lined_cost: f64,
    pub lines: u64,
    /// Of them, those whose landing changed no counted line (the regenerator's, a landing that
    /// changed nothing): in no figure per line.
    pub unlined: usize,
    /// Of them, those whose landing's lines were not read (one landed before they were kept): in
    /// no figure per line.
    pub unread: usize,
}

impl Summary {
    /// What 100 of the lines cost, in dollars; none with no line.
    pub fn per_100_lines(&self) -> Option<f64> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a count of lines, far under 2^52"
        )]
        (self.lines > 0).then(|| self.lined_cost / self.lines as f64 * 100.0)
    }
}

/// The [`Summary`] of `tasks`.
pub fn summary<'a>(tasks: impl IntoIterator<Item = &'a LandedTask>) -> Summary {
    let mut summary = Summary::default();
    let mut costs = Vec::new();
    for task in tasks {
        let Some(pico) = task.priced() else {
            continue;
        };
        let cost = in_dollars(pico);
        costs.push(cost);
        summary.cost += cost;
        summary.later += in_dollars(task.later.cost.total());
        match task.lines {
            Some(0) => summary.unlined += 1,
            Some(lines) => {
                summary.lined += 1;
                summary.lined_cost += cost;
                summary.lines += lines;
            }
            None => summary.unread += 1,
        }
    }
    summary.tasks = costs.len();
    #[expect(clippy::cast_precision_loss, reason = "a count of tasks")]
    let mean = (!costs.is_empty()).then(|| summary.cost / costs.len() as f64);
    summary.mean = mean;
    if let Some(spread) = crate::stages::stats::spread(costs) {
        summary.median = Some(spread.median);
        summary.p90 = Some(spread.p90);
    }
    summary
}

/// The landed tasks of one kind, or of none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KindGroup {
    /// The kind; none for the tasks given none.
    pub kind: Option<String>,
    pub summary: Summary,
}

/// `tasks` by their kind, those that cost the most in all first, then by kind, and the tasks of no
/// kind last; a kind no task is of is left out, as is a task with no priced cost.
pub fn by_kind(tasks: &[LandedTask]) -> Vec<KindGroup> {
    let mut groups: Vec<KindGroup> = Vec::new();
    let kinds = tasks
        .iter()
        .filter(|task| task.priced().is_some())
        .map(|task| &task.kind);
    for kind in kinds {
        if !groups.iter().any(|group| group.kind == *kind) {
            groups.push(KindGroup {
                kind: kind.clone(),
                summary: summary(tasks.iter().filter(|task| task.kind == *kind)),
            });
        }
    }
    crate::order::sort_by(&mut groups, &|a: &KindGroup, b: &KindGroup| {
        let last = |group: &KindGroup| group.kind.is_none();
        (last(a).cmp(&last(b)))
            .then(b.summary.cost.total_cmp(&a.summary.cost))
            .then(a.kind.cmp(&b.kind))
    });
    groups
}

/// How many reviews the last group of [`by_reviews`] holds, with every task of more.
pub const REVIEWS_GROUPED: u32 = 4;

/// The landed tasks that took one number of reviews ([`LandedTask::reviews`]), or
/// [`REVIEWS_GROUPED`] or more.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReviewGroup {
    /// The reviews: [`REVIEWS_GROUPED`] for that many or more.
    pub reviews: u32,
    /// The tasks with a priced cost.
    pub tasks: usize,
    /// The mean cost of one's first round and of its review rounds, in dollars.
    pub first: f64,
    pub later: f64,
    /// The median cost of one, in dollars.
    pub median: Option<f64>,
}

/// `tasks` by how many reviews each took, the fewest first; a number no task took is left out, as
/// is a task with no priced cost.
pub fn by_reviews(tasks: &[LandedTask]) -> Vec<ReviewGroup> {
    let grouped = |task: &LandedTask| task.reviews.min(REVIEWS_GROUPED);
    (0..=REVIEWS_GROUPED)
        .filter_map(|reviews| {
            let of: Vec<&LandedTask> = (tasks.iter())
                .filter(|task| grouped(task) == reviews && task.priced().is_some())
                .collect();
            let all = summary(of.iter().copied());
            #[expect(clippy::cast_precision_loss, reason = "a count of tasks")]
            let mean = |pick: fn(&LandedTask) -> u128| {
                let sum: u128 = of.iter().map(|task| pick(task)).sum();
                in_dollars(sum) / of.len() as f64
            };
            (!of.is_empty()).then(|| ReviewGroup {
                reviews,
                tasks: of.len(),
                first: mean(|task| task.first.cost.total()),
                later: mean(|task| task.later.cost.total()),
                median: all.median,
            })
        })
        .collect()
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
            ..Landed::default()
        }
    }

    /// `landed` given `kind` and `lines` changed, added alone.
    fn of(mut landed: Landed, kind: Option<&str>, lines: Option<u64>) -> Landed {
        landed.kind = kind.map(str::to_string);
        landed.lines = lines.map(|added| symphony_api::reports::Lines { added, removed: 0 });
        landed
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
            .map(|task| (task.slug.as_str(), task.at, task.spend().cost.said()))
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
        // Only a/mid took a round past the second: its first review sent it back.
        assert_eq!(costs.back, 1);
        let reviews: Vec<(&str, u32)> = (costs.tasks.iter())
            .map(|task| (task.slug.as_str(), task.reviews))
            .collect();
        assert_eq!(
            reviews,
            [
                ("a/dear", 1),
                ("a/mid", 2),
                ("a/cheap", 0),
                ("a/unpriced", 0)
            ]
        );
        let dear = &costs.tasks[0];
        assert_eq!(
            (dear.first.cost.said(), dear.later.cost.said()),
            ("$6.00".to_string(), "$4.00".to_string())
        );
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
            .map(|task| task.slug)
            .collect();
        assert_eq!(order, ["t/c", "t/a", "t/b", "t/d"]);
    }

    #[test]
    fn a_landed_task_carries_its_kind_and_lines_and_a_round_told_in_none_is_its_first() {
        let files = [
            of(
                landed(
                    "a/kinded",
                    1.0,
                    &[("executor", 0, "m", 2), ("reviewer", 2, "m", 1)],
                ),
                Some("fix"),
                Some(40),
            ),
            of(
                landed("a/plain", 2.0, &[("executor", 1, "m", 1)]),
                None,
                None,
            ),
        ];
        let costs = landed_costs(&files, &prices());
        let kinded = &costs.tasks[0];
        assert_eq!(
            (kinded.slug.as_str(), kinded.kind.as_deref(), kinded.lines),
            ("a/kinded", Some("fix"), Some(40))
        );
        assert_eq!(
            kinded.first.cost.said(),
            "$2.00",
            "a sum of round 0 is the first round's"
        );
        assert_eq!(
            (kinded.reviews, kinded.priced()),
            (1, Some(3_000_000_000_000))
        );
        let plain = &costs.tasks[1];
        assert_eq!(
            (plain.kind.as_ref(), plain.lines, plain.reviews),
            (None, None, 0)
        );
    }

    #[test]
    fn a_summary_spreads_the_priced_tasks_and_counts_lines_only_where_some_were_changed() {
        let files = [
            of(
                landed("s/a", 1.0, &[("executor", 1, "m", 1)]),
                None,
                Some(100),
            ),
            of(
                landed(
                    "s/b",
                    2.0,
                    &[("executor", 1, "m", 2), ("reviewer", 2, "m", 2)],
                ),
                None,
                Some(300),
            ),
            of(
                landed("s/c", 3.0, &[("executor", 1, "m", 3)]),
                None,
                Some(0),
            ),
            of(landed("s/d", 4.0, &[("executor", 1, "m", 10)]), None, None),
            // No price: in no figure.
            of(
                landed("s/e", 5.0, &[("executor", 1, "other", 9)]),
                None,
                Some(50),
            ),
        ];
        let costs = landed_costs(&files, &prices());
        let all = summary(&costs.tasks);
        assert_eq!(all.tasks, 4);
        assert_eq!((all.cost, all.later), (18.0, 2.0));
        assert_eq!(all.mean, Some(4.5));
        // $1, $3, $4, $10: interpolated as the lead times are.
        assert_eq!(all.median, Some(3.5));
        // 0.9 of the way along the four: 0.7 of the way from $4 to $10.
        assert_eq!(all.p90, Some(4.0 + (10.0 - 4.0) * (0.9 * 3.0 - 2.0)));
        assert_eq!((all.lined, all.lined_cost, all.lines), (2, 5.0, 400));
        assert_eq!((all.unlined, all.unread), (1, 1));
        assert_eq!(all.per_100_lines(), Some(1.25));
        let none = summary(&[]);
        assert_eq!(
            (
                none.tasks,
                none.median,
                none.mean,
                none.p90,
                none.per_100_lines()
            ),
            (0, None, None, None, None)
        );
    }

    #[test]
    fn kinds_go_dearest_in_all_first_and_the_tasks_of_none_last() {
        let files = [
            of(
                landed("k/a", 1.0, &[("executor", 1, "m", 1)]),
                Some("docs"),
                Some(10),
            ),
            of(
                landed("k/b", 2.0, &[("executor", 1, "m", 50)]),
                None,
                Some(10),
            ),
            of(
                landed("k/c", 3.0, &[("executor", 1, "m", 4)]),
                Some("tests"),
                Some(10),
            ),
            of(
                landed("k/d", 4.0, &[("executor", 1, "m", 3)]),
                Some("docs"),
                None,
            ),
            of(
                landed("k/e", 5.0, &[("executor", 1, "m", 4)]),
                Some("fix"),
                Some(0),
            ),
            of(
                landed("k/f", 6.0, &[("executor", 1, "other", 9)]),
                Some("red"),
                Some(5),
            ),
        ];
        let costs = landed_costs(&files, &prices());
        let kinds = by_kind(&costs.tasks);
        let groups: Vec<(Option<&str>, usize, f64, Option<f64>)> = (kinds.iter())
            .map(|g| {
                (
                    g.kind.as_deref(),
                    g.summary.tasks,
                    g.summary.cost,
                    g.summary.median,
                )
            })
            .collect();
        assert_eq!(
            groups,
            [
                // docs and tests tie at $4 and fix too: by kind's name.
                (Some("docs"), 2, 4.0, Some(2.0)),
                (Some("fix"), 1, 4.0, Some(4.0)),
                (Some("tests"), 1, 4.0, Some(4.0)),
                (None, 1, 50.0, Some(50.0)),
            ],
            "the red task has no priced cost, so red has no group"
        );
        assert!(by_kind(&[]).is_empty());
    }

    #[test]
    fn reviews_group_by_their_number_and_four_or_more_together() {
        let rounds = |slug: &str, last: u32| {
            let mut spent = vec![("executor", 1, "m", 2)];
            spent.extend((2..=last).map(|round| ("reviewer", round, "m", 1)));
            landed(slug, 1.0, &spent)
        };
        let files = [
            rounds("r/none", 1),
            rounds("r/once", 2),
            rounds("r/once-too", 2),
            rounds("r/thrice", 4),
            rounds("r/five", 6),
            rounds("r/six", 7),
        ];
        let costs = landed_costs(&files, &prices());
        let reviewed = by_reviews(&costs.tasks);
        let groups: Vec<(u32, usize, f64, f64, Option<f64>)> = (reviewed.iter())
            .map(|g| (g.reviews, g.tasks, g.first, g.later, g.median))
            .collect();
        assert_eq!(
            groups,
            [
                (0, 1, 2.0, 0.0, Some(2.0)),
                (1, 2, 2.0, 1.0, Some(3.0)),
                (3, 1, 2.0, 3.0, Some(5.0)),
                // Five and six reviews: $7 and $8.
                (REVIEWS_GROUPED, 2, 2.0, 5.5, Some(7.5)),
            ],
            "no task took two reviews, so two has no group"
        );
        assert_eq!(costs.back, 3);
        assert!(by_reviews(&[]).is_empty());
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
