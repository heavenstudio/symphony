//! The Reports page's read ([`symphony_api::report_page`]): the project's day files for a window,
//! summed a period at a time. It reads three folders of `symphony/` in the project's git common dir
//! ([`Folders`]) and, for the marks of a change of the project's settings or the instructions its
//! agents work under, one `git log` of the default branch; never the board, so a board slow or down
//! leaves it as it is. Each source's part of a period is none when no file holds an hour of it:
//! missing, never a zero. The tokens the roll-up's files hold are priced here, at the table this
//! build embeds ([`symphony_board::usage::Prices`]): a period's by role, and what the tasks landed
//! in the window took ([`landed`]).
//!
//! A file is read whole only up to [`REPORTS_FILE_BYTES`] and the answer reads at most
//! [`REPORTS_READ_BYTES`], newest days first; a file past either, one that does not read, a link or
//! anything but a regular file is named in [`ReportsPage::unread`] and left out. An agents' day file
//! is read for its hour lines alone, by their `type` (a line of another type, or one cut short, is
//! passed over), and every line of an hour is taken together as docs/workflow/AGENT_ACTIVITY.md, *Reading an
//! hour*, says: seconds and counts added up, peaks the largest, means weighted by the seconds each
//! line watched (by its snapshots where none watched any), and the sessions started of the line
//! that ran latest.
//!
//! The answer of a window is kept between reads ([`kept`]) with what it was read from, so a read
//! that finds none of it moved opens no file and starts no git.

use std::fs::OpenOptions;
use std::io::Read as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use symphony_api::agent_activity::{AgentHour, Gauge, KindLevel, Level, RoleLevel};
use symphony_api::report_page::{
    BucketAgents, BucketFlow, BucketHealth, BucketLanded, BucketVerdicts, BucketWaits, Grain,
    KindRow, LandedFigures, LandedPage, LandedRow, REPORTS_FILE_BYTES, REPORTS_READ_BYTES,
    ReportBucket, ReportMark, ReportsPage, ReviewRow, RoleSpend, Spend,
};
use symphony_api::reports::{
    ByStatus, DecisionCounts, GitCounts, Landed, PlacedHours, ReportDay, Spent, Spread, TokenSum,
};
use symphony_api::{HealthHour, report_file, run_day};
use symphony_board::reports::spend;
use symphony_board::usage::Prices;

mod kept;
pub(crate) use kept::{Answer, Kept};

const HOUR: f64 = 3_600.0;
const DAY: f64 = 86_400.0;
const WEEK: f64 = 7.0 * DAY;

/// Where the project's day files are, each folder or why it has none.
#[derive(Clone, Debug)]
pub(crate) struct Folders {
    /// The hourly roll-up's, `symphony/reports/`.
    pub(crate) reports: Result<PathBuf, String>,
    /// The agents', `symphony/agents/`.
    pub(crate) agents: Result<PathBuf, String>,
    /// The board's health hours, `symphony/health/`.
    pub(crate) health: Result<PathBuf, String>,
    /// The recorder's runs, `symphony/runs/`, whose logs the run-log read serves
    /// ([`crate::api::run_log`]); none where a run keeps no line.
    pub(crate) runs: Option<PathBuf>,
}

impl Folders {
    /// The folders of the project at `root`, as the shared server's writers find them, so a private
    /// server reads what the shared one writes.
    pub(crate) fn of(root: &Path) -> Folders {
        let mut env = symphony::children::board_env();
        env.cwd = root.to_path_buf();
        Folders {
            reports: symphony_board::reports::folder(&env),
            agents: crate::live::agent_activity::folder(&env),
            health: symphony_board::board_health::hours::folder(&env),
            runs: symphony_board::runs::folder(&env).map(|runs| root.join(runs)),
        }
    }
}

/// The answer of a window of `days` by `grain` of the project `hub` serves, at the time its
/// records are read at ([`crate::live::Hub::recorded_now`]): its day
/// files, from the folders found at the first read ([`crate::live::Hub::report_folders`]), and
/// the marks from its default branch; the one the hub kept while none of them moved
/// ([`Kept::answer`]).
pub(crate) fn answer(hub: &crate::live::Hub, grain: Grain, days: u32) -> Answer {
    let folders = hub.report_folders.get_or_init(|| Folders::of(&hub.root));
    hub.reports_kept
        .answer(folders, &hub.root, grain, days, hub.recorded_now())
}

/// The Landed tasks tab's answer of a window of `days` by `grain` ([`landed_page`]), read from the
/// roll-up's day files on each ask: the route's validator spares the page the bytes of one that has
/// not moved.
pub(crate) fn landed_answer(hub: &crate::live::Hub, grain: Grain, days: u32) -> LandedPage {
    let folders = hub.report_folders.get_or_init(|| Folders::of(&hub.root));
    landed_page(&folders.reports, grain, days, hub.recorded_now())
}

/// The marks of `branch`, the default branch of the project at `root`, since `since`, oldest
/// first: each commit that changed its settings or the instructions its agents work under, from
/// one `git log` ([`symphony::reports::git::changes`]).
fn marks(root: &Path, branch: &str, since: f64) -> Result<Vec<ReportMark>, String> {
    let changes = symphony::reports::git::changes(root, branch, since)?;
    Ok(changes
        .into_iter()
        .rev()
        .map(|change| ReportMark {
            at: change.commit.at,
            commit: change.commit.hash.chars().take(12).collect(),
            subject: change.commit.subject,
            changed: change.changed,
        })
        .collect())
}

/// The grain and days a request asks for (`grain`, `days`), as a window takes them
/// ([`Grain::days`]); refused for a grain that is none of the three or days that are no number.
pub(crate) fn asked(grain: Option<&str>, days: Option<&str>) -> Result<(Grain, u32), String> {
    let grain = match grain.filter(|word| !word.is_empty()) {
        None => Grain::default(),
        Some(word) => {
            Grain::parse(word).ok_or_else(|| format!("grain is hour, day or week, not {word}"))?
        }
    };
    let days = match days.filter(|text| !text.is_empty()) {
        None => None,
        Some(text) => Some(
            text.parse::<u32>()
                .map_err(|_| format!("days is a whole number of days, not {text}"))?,
        ),
    };
    Ok((grain, grain.days(days)))
}

/// The start of the hour `at` falls in.
fn hour_of(at: f64) -> f64 {
    (at / HOUR).floor() * HOUR
}

/// The start of the UTC day `at` falls in.
fn day_of(at: f64) -> f64 {
    (at / DAY).floor() * DAY
}

/// The start of the week `at` falls in, from Monday 00:00 UTC (the epoch's first day was a
/// Thursday).
fn week_of(at: f64) -> f64 {
    let day = (at / DAY).floor();
    (day - (day + 3.0).rem_euclid(7.0)) * DAY
}

/// The periods of a window of `days` by `grain` at `now`, oldest first, each as its start and end:
/// whole hours, UTC days or weeks back from the one `now` is in, the last cut at the end of the
/// hour `now` is in. By the week, `days` holds as many whole weeks as fit in it, one at least.
pub(crate) fn periods(grain: Grain, days: u32, now: f64) -> Vec<(f64, f64)> {
    let to = hour_of(now) + HOUR;
    let (first, length, count) = match grain {
        Grain::Hour => {
            let count = days * 24;
            (to - f64::from(count) * HOUR, HOUR, count)
        }
        Grain::Day => (day_of(now) - f64::from(days - 1) * DAY, DAY, days),
        Grain::Week => {
            let count = (days / 7).max(1);
            (week_of(now) - f64::from(count - 1) * WEEK, WEEK, count)
        }
    };
    (0..count)
        .map(|n| {
            let start = first + f64::from(n) * length;
            (start, (start + length).min(to))
        })
        .collect()
}

/// The first second of each UTC day a window from `from` to `to` reads the files of, newest first.
fn days_read(from: f64, to: f64) -> impl Iterator<Item = f64> {
    let (first, last) = (day_of(from), day_of(to - 1.0));
    std::iter::successors(Some(last), |day| Some(day - DAY)).take_while(move |day| *day >= first)
}

/// The files of the day `name` in the folders there are: the roll-up's, the agents' and the
/// health's, in the order a page reads them.
fn day_files(folders: &Folders, name: &str) -> [Option<PathBuf>; 3] {
    [
        folders
            .reports
            .as_ref()
            .ok()
            .map(|folder| folder.join(report_file(name))),
        folders
            .agents
            .as_ref()
            .ok()
            .map(|folder| folder.join(format!("{name}.jsonl"))),
        folders
            .health
            .as_ref()
            .ok()
            .map(|folder| folder.join(format!("{name}.jsonl"))),
    ]
}

/// The page of a window of `days` by `grain`, read at `now` from `folders`; `marks` the settings
/// changes since a time, or why they could not be read.
pub(crate) fn page(
    folders: &Folders,
    grain: Grain,
    days: u32,
    now: f64,
    marks: impl FnOnce(f64) -> Result<Vec<ReportMark>, String>,
) -> ReportsPage {
    let spans = periods(grain, days, now);
    let from = spans.first().map_or(now, |span| span.0);
    let to = spans.last().map_or(now, |span| span.1);
    let mut sums: Vec<Sums> = spans.iter().map(|_| Sums::default()).collect();
    let mut unread = Vec::new();
    let mut budget = REPORTS_READ_BYTES;
    let mut reds: Vec<(String, f64, f64)> = Vec::new();
    // The tasks landed in the window, as [`report_day`] gathers them.
    let mut landings: Vec<Landed> = Vec::new();
    let place = |at: f64| {
        let after = spans.partition_point(|span| span.0 <= at);
        (after > 0 && at < spans[after - 1].1).then(|| after - 1)
    };
    for (name, folder) in [
        ("reports", &folders.reports),
        ("agents", &folders.agents),
        ("health", &folders.health),
    ] {
        if let Err(why) = folder {
            unread.push(format!("{name}: {why}"));
        }
    }
    // Newest day first, so the bytes left go to the days nearest now.
    for day in days_read(from, to) {
        let [report, agents, health] = day_files(folders, &run_day(day));
        if let Some(report) = report
            .and_then(|path| report_day(&path, (from, to), &mut budget, &mut unread, &mut landings))
        {
            for hour in &report.hours {
                if let Some(at) = place(hour.start) {
                    sums[at].hour(hour);
                }
            }
            if grain != Grain::Hour
                && let Some(at) = place(report.start)
            {
                sums[at].day(&report);
            }
            for red in &report.reds {
                if let (Some(closed), "done") = (red.closed, red.status.as_str())
                    && !reds.iter().any(|(slug, _, _)| *slug == red.slug)
                {
                    reds.push((red.slug.clone(), red.filed, closed));
                }
            }
        }
        if let Some(path) = agents {
            match read(&path, &mut budget) {
                Ok(None) => {}
                Ok(Some(text)) => {
                    for line in text
                        .lines()
                        .filter(|line| line.contains("\"type\":\"hour\""))
                    {
                        if let Ok(hour) = serde_json::from_slice::<AgentHour>(line.as_bytes())
                            && let Some(at) = place(hour.start)
                        {
                            sums[at].agents.push(hour);
                        }
                    }
                }
                Err(why) => unread.push(why),
            }
        }
        if let Some(path) = health {
            match read(&path, &mut budget) {
                Ok(None) => {}
                Ok(Some(text)) => {
                    for line in text.lines() {
                        if let Ok(hour) = serde_json::from_slice::<HealthHour>(line.as_bytes())
                            && let Some(at) = place(hour.start)
                        {
                            sums[at].health(&hour);
                        }
                    }
                }
                Err(why) => unread.push(why),
            }
        }
    }
    for (_, filed, closed) in &reds {
        if let Some(at) = place(*closed) {
            sums[at].red_fix.push((closed - filed).max(0.0));
        }
    }
    let marks = match marks(from) {
        Ok(marks) => marks
            .into_iter()
            .filter(|mark| from <= mark.at && mark.at < to)
            .collect(),
        Err(why) => {
            unread.push(format!("settings and instructions marks: {why}"));
            Vec::new()
        }
    };
    let prices = Prices::embedded();
    // Each task landed in the window once, at its last landing there, in the period of it.
    let mut seen: Vec<&str> = Vec::new();
    for task in &landings {
        if !seen.contains(&task.slug.as_str()) {
            seen.push(&task.slug);
            if let Some(at) = place(task.at) {
                sums[at].landings += 1;
            }
        }
    }
    for task in spend::landed_costs(&landings, prices).tasks {
        if let Some(at) = place(task.at) {
            sums[at].landed.push(task);
        }
    }
    ReportsPage {
        grain,
        days,
        from,
        to,
        prices: prices.read.clone(),
        buckets: spans
            .iter()
            .zip(sums)
            .map(|(&(start, end), sums)| sums.bucket(start, end, grain, prices))
            .collect(),
        first: folders
            .reports
            .as_ref()
            .ok()
            .and_then(|folder| first(folder)),
        marks,
        unread,
    }
}

/// `spend` on the wire: its tokens and their estimated cost in dollars, as the board's own JSON
/// gives them (`tasks show --json`), at `prices`.
fn wire(spend: &symphony_board::usage::Spend, prices: &Prices) -> Spend {
    Spend {
        tokens: serde_json::from_value(spend.tokens.json()).unwrap_or_default(),
        cost: serde_json::from_value(spend.cost.json(&prices.read)).unwrap_or_default(),
    }
}

/// The Landed tasks tab's answer of a window of `days` by `grain` at `now`, from the roll-up's day
/// files in `folder` alone ([`LandedPage`]): every top-level task landed in the window, each once at
/// its last landing in it, priced at the table this build embeds, with the window's figures, by
/// kind and by reviews ([`spend`]). Its day files are read by [`page`]'s reader, newest day first,
/// within the same bounds; one it cannot read is named in [`LandedPage::unread`].
pub(crate) fn landed_page(
    folder: &Result<PathBuf, String>,
    grain: Grain,
    days: u32,
    now: f64,
) -> LandedPage {
    let spans = periods(grain, days, now);
    let from = spans.first().map_or(now, |span| span.0);
    let to = spans.last().map_or(now, |span| span.1);
    let mut unread = Vec::new();
    let mut landings: Vec<Landed> = Vec::new();
    match folder {
        Err(why) => unread.push(format!("reports: {why}")),
        Ok(folder) => {
            let mut budget = REPORTS_READ_BYTES;
            for day in days_read(from, to) {
                let path = folder.join(report_file(&run_day(day)));
                report_day(&path, (from, to), &mut budget, &mut unread, &mut landings);
            }
        }
    }
    let prices = Prices::embedded();
    let costs = spend::landed_costs(&landings, prices);
    let all = spend::summary(&costs.tasks);
    let dollars = |pico: u128| rounded(spend::in_dollars(pico));
    LandedPage {
        grain,
        days,
        from,
        to,
        tasks: (costs.tasks.iter())
            .map(|task| {
                let spent = task.spend();
                LandedRow {
                    slug: task.slug.clone(),
                    at: task.at,
                    kind: task.kind.clone(),
                    first: dollars(task.first.cost.total()),
                    later: dollars(task.later.cost.total()),
                    tokens: spent.tokens.total(),
                    unpriced: !spent.cost.unpriced.is_empty(),
                    reviews: task.reviews,
                    lines: task.lines,
                }
            })
            .collect(),
        without: count(costs.without),
        figures: LandedFigures {
            tasks: count(all.tasks),
            median: all.median.map(rounded),
            mean: all.mean.map(rounded),
            p90: all.p90.map(rounded),
            cost: rounded(all.cost),
            later: rounded(all.later),
            back: count(costs.back),
            lined: count(all.lined),
            lined_cost: rounded(all.lined_cost),
            lines: all.lines,
            unlined: count(all.unlined),
            unread: count(all.unread),
        },
        kinds: (spend::by_kind(&costs.tasks).into_iter())
            .map(|group| KindRow {
                kind: group.kind,
                tasks: count(group.summary.tasks),
                median: group.summary.median.map(rounded),
                cost: rounded(group.summary.cost),
                lined_cost: rounded(group.summary.lined_cost),
                lines: group.summary.lines,
            })
            .collect(),
        reviews: (spend::by_reviews(&costs.tasks).into_iter())
            .map(|group| ReviewRow {
                reviews: group.reviews,
                tasks: count(group.tasks),
                first: rounded(group.first),
                later: rounded(group.later),
                median: group.median.map(rounded),
            })
            .collect(),
        kinds_from: (landings.iter())
            .filter(|task| task.kind.is_some())
            .map(|task| task.at)
            .reduce(f64::min),
        prices: prices.read.clone(),
        unread,
    }
}

/// The roll-up's day file at `path`, read within `budget` ([`read`]), with the tasks it landed in
/// the window from `from` to `to` added to `landings`, its latest landing first; none for a day with
/// no file, or one that does not read, which `unread` then names. The one reader of the landed
/// tasks of [`page`] and [`landed_page`]: each reads its days newest first, so a task landed again
/// is met first at its last landing in the window, and the two count the same landings.
fn report_day(
    path: &Path,
    (from, to): (f64, f64),
    budget: &mut u64,
    unread: &mut Vec<String>,
    landings: &mut Vec<Landed>,
) -> Option<ReportDay> {
    let text = match read(path, budget) {
        Ok(text) => text?,
        Err(why) => {
            unread.push(why);
            return None;
        }
    };
    let report = match serde_json::from_slice::<ReportDay>(text.as_bytes()) {
        Ok(report) => report,
        Err(e) => {
            unread.push(format!("{}: {e}", path.display()));
            return None;
        }
    };
    landings.extend(
        (report.landed.iter().rev())
            .filter(|task| from <= task.at && task.at < to)
            .cloned(),
    );
    Some(report)
}

/// The text of the day file at `path`, its bytes taken from `budget`; none when there is none. A
/// link, a FIFO or anything but a regular file, one larger than [`REPORTS_FILE_BYTES`] or than what
/// is left of `budget`, or one that cannot be read, is refused in a sentence naming it.
fn read(path: &Path, budget: &mut u64) -> Result<Option<String>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let size = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !size.is_file() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    let size = size.len();
    if size > REPORTS_FILE_BYTES {
        return Err(format!(
            "{}: {size} bytes, over the {REPORTS_FILE_BYTES} a day file is read to",
            path.display()
        ));
    }
    if size > *budget {
        return Err(format!(
            "{}: not read, as the {REPORTS_READ_BYTES} bytes one answer reads were read",
            path.display()
        ));
    }
    *budget -= size;
    let mut text = String::new();
    file.take(REPORTS_FILE_BYTES)
        .read_to_string(&mut text)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(text))
}

/// The first second of the oldest day file in `folder`, by its name; none when it holds none.
fn first(folder: &Path) -> Option<f64> {
    let mut oldest: Option<f64> = None;
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let name = entry.file_name();
        let Some(start) = name
            .to_str()
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(day_start)
        else {
            continue;
        };
        if entry.file_type().is_ok_and(|kind| kind.is_file()) {
            oldest = Some(oldest.map_or(start, |kept| kept.min(start)));
        }
    }
    oldest
}

/// The first second of the UTC day `name` names (`YYYY-MM-DD`); none for a name that is no day.
fn day_start(name: &str) -> Option<f64> {
    let digits = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = name.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    if name.len() != 10 || name.as_bytes()[4] != b'-' || name.as_bytes()[7] != b'-' {
        return None;
    }
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm), as `run_day` reads back.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a day count fits an f64 exactly"
    )]
    let start = days as f64 * DAY;
    (run_day(start) == name).then_some(start)
}

/// What a period gathers from its files.
#[derive(Default)]
struct Sums {
    /// The roll-up's hours in it.
    rolled: u32,
    /// Whether an hour of it did not count its tasks done ([`BucketFlow::done`]): a file written
    /// before they were counted. The period then has none, its count being part of the period's.
    done_uncounted: bool,
    flow: Option<(f64, BucketFlow)>,
    decisions: Option<(f64, DecisionCounts)>,
    git: Option<GitCounts>,
    waits: [Vec<f64>; 4],
    leads: Vec<Spread>,
    verdicts: Option<BucketVerdicts>,
    red_fix: Vec<f64>,
    agents: Vec<AgentHour>,
    health: Option<BucketHealth>,
    /// The tokens of its hours that tell them, by role, model and speed; none when none does.
    tokens: Option<Vec<TokenSum>>,
    /// Where a custom workflow's top-level tasks spent its hours that record it.
    workflow: Option<PlacedHours>,
    /// The top-level tasks landed in it, each once at its last landing in the window, with tokens
    /// recorded or not, and of them those the board recorded tokens for, priced.
    landings: u32,
    landed: Vec<spend::LandedTask>,
}

/// `from`'s task-hours added to `into`'s, each under its ID and label, a new one after those kept:
/// the hours come ordered by ID and label, so a period's stay so but for a label first met later.
fn add_spent(into: &mut Vec<Spent>, from: &[Spent]) {
    for spent in from {
        match into
            .iter_mut()
            .find(|kept| kept.id == spent.id && kept.label == spent.label)
        {
            Some(kept) => kept.hours += spent.hours,
            None => into.push(spent.clone()),
        }
    }
}

/// `from`'s figures added to `into`'s, status by status.
fn add<T: Copy + std::ops::AddAssign>(into: &mut ByStatus<T>, from: &ByStatus<T>) {
    for (status, figure) in from.each() {
        if let Some(kept) = into.get_mut(status) {
            *kept += *figure;
        }
    }
}

impl Sums {
    /// An hour of the roll-up.
    fn hour(&mut self, hour: &symphony_api::ReportHour) {
        self.rolled += 1;
        let (last, flow) = self.flow.get_or_insert((f64::MIN, BucketFlow::default()));
        if hour.start >= *last {
            *last = hour.start;
            flow.held = hour.top.held;
        }
        add(&mut flow.entered, &hour.top.entered);
        match hour.top.done {
            Some(done) => *flow.done.get_or_insert(0) += done,
            None => self.done_uncounted = true,
        }
        add(&mut flow.task_hours, &hour.top.task_hours);
        flow.unclaimed_hours += hour.top.unclaimed_hours;
        flow.bounces += hour.top.bounces;
        flow.landed += hour.landed;
        flow.reds_filed += hour.reds_filed;
        flow.reds_closed += hour.reds_closed;
        if let Some(cards) = hour.decisions {
            let (last, kept) = self
                .decisions
                .get_or_insert((f64::MIN, DecisionCounts::default()));
            if hour.start >= *last {
                *last = hour.start;
                kept.open = cards.open;
            }
            kept.answered += cards.answered;
            kept.auto_approved += cards.auto_approved;
            kept.withdrawn += cards.withdrawn;
        }
        if let Some(git) = hour.git {
            let kept = self.git.get_or_insert_default();
            kept.commits += git.commits;
            kept.merges += git.merges;
            kept.added += git.added;
            kept.removed += git.removed;
            kept.reverts += git.reverts;
        }
        if let Some(tokens) = &hour.tokens {
            let kept = self.tokens.get_or_insert_default();
            for sum in tokens {
                spend::add(kept, sum);
            }
        }
        if let Some(placed) = &hour.workflow {
            let kept = self.workflow.get_or_insert_default();
            add_spent(&mut kept.phases, &placed.top.phases);
            add_spent(&mut kept.states, &placed.top.states);
        }
        let waits = &hour.waits;
        for (kept, ended) in self.waits.iter_mut().zip([
            &waits.review_to_reviewer,
            &waits.changes_to_resumed,
            &waits.changes_to_review,
            &waits.approved_to_landed,
        ]) {
            kept.extend(ended.iter().map(|wait| wait.seconds));
        }
    }

    /// What the roll-up keeps of a whole day: its lead time and its review verdicts.
    fn day(&mut self, day: &ReportDay) {
        self.leads.extend(day.top.lead);
        let verdicts = self.verdicts.get_or_insert_default();
        for round in &day.rounds {
            verdicts.changes += round.changes;
            verdicts.approved += round.approved;
        }
    }

    /// A line of the board's health.
    fn health(&mut self, hour: &HealthHour) {
        let kept = self.health.get_or_insert_default();
        kept.covered_s += (hour.to - hour.from).max(0.0);
        kept.dispatch.ok += hour.dispatch.ok;
        kept.dispatch.failed += hour.dispatch.failed;
        kept.sweep.ok += hour.sweep.ok;
        kept.sweep.failed += hour.sweep.failed;
        kept.requests += hour.requests.count;
        kept.slow += hour.requests.slow;
        kept.unanswered += hour.requests.unanswered;
        kept.writes += hour.writes.count;
        if let Some(slowest) = &hour.requests.slowest {
            kept.slowest_ms = Some(kept.slowest_ms.map_or(slowest.ms, |ms| ms.max(slowest.ms)));
        }
        if let Some(p90) = hour.writes.p90_ms {
            kept.write_p90_ms = Some(kept.write_p90_ms.map_or(p90, |ms| ms.max(p90)));
        }
    }

    /// The period from `start` to `end` of a window by `grain`, as its files summed it.
    fn bucket(self, start: f64, end: f64, grain: Grain, prices: &Prices) -> ReportBucket {
        let [to_reviewer, to_resumed, to_review, to_landed] = self.waits;
        let any_wait = [&to_reviewer, &to_resumed, &to_review, &to_landed]
            .iter()
            .any(|waits| !waits.is_empty());
        let waits = any_wait.then(|| BucketWaits {
            review_to_reviewer: spread(to_reviewer),
            changes_to_resumed: spread(to_resumed),
            changes_to_review: spread(to_review),
            approved_to_landed: spread(to_landed),
        });
        ReportBucket {
            start,
            end,
            rolled: self.rolled,
            flow: self.flow.map(|(_, flow)| BucketFlow {
                done: flow.done.filter(|_| !self.done_uncounted),
                ..flow
            }),
            decisions: self.decisions.map(|(_, cards)| cards),
            git: self.git,
            waits,
            lead: lead(&self.leads, grain),
            verdicts: self.verdicts,
            red_fix: spread(self.red_fix),
            agents: agents(&self.agents),
            health: self.health,
            spend: self.tokens.map(|sums| {
                (spend::by_role(&sums, prices).iter())
                    .map(|(role, spent)| {
                        let Spend { tokens, cost } = wire(spent, prices);
                        RoleSpend {
                            role: role.clone(),
                            tokens,
                            cost,
                        }
                    })
                    .collect()
            }),
            workflow: self.workflow,
            landed: (self.landings > 0).then(|| {
                let summary = spend::summary(&self.landed);
                BucketLanded {
                    tasks: self.landings,
                    median: summary.median.map(rounded),
                    p90: summary.p90.map(rounded),
                    lined_cost: rounded(summary.lined_cost),
                    lines: summary.lines,
                    unread: count(summary.unread),
                }
            }),
        }
    }
}

/// A count on the wire.
fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The spread of `values`, as the roll-up spreads its own; none of none.
fn spread(values: Vec<f64>) -> Option<Spread> {
    symphony_board::stages::stats::spread(values).map(|s| Spread {
        n: u32::try_from(s.n).unwrap_or(u32::MAX),
        median: rounded(s.median),
        p90: rounded(s.p90),
    })
}

/// The lead time of a period out of its days' (`leads`): a day's as it is; a week's median the mean
/// of its days' weighted by their closes, its p90 the largest of theirs. None by the hour.
fn lead(leads: &[Spread], grain: Grain) -> Option<Spread> {
    if grain == Grain::Hour {
        return None;
    }
    let n: u32 = leads.iter().map(|lead| lead.n).sum();
    if n == 0 {
        return None;
    }
    let weighted: f64 = leads
        .iter()
        .map(|lead| lead.median * f64::from(lead.n))
        .sum();
    Some(Spread {
        n,
        median: rounded(weighted / f64::from(n)),
        p90: rounded(leads.iter().map(|lead| lead.p90).fold(0.0, f64::max)),
    })
}

/// `value` to the thousandth, as the day files write their figures.
fn rounded(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// A level being summed: its peak, and its mean times each line's weight.
#[derive(Clone, Copy, Default)]
struct Sum {
    peak: f64,
    weighted: f64,
}

impl Sum {
    fn take(&mut self, peak: f64, mean: f64, weight: f64) {
        self.peak = self.peak.max(peak);
        self.weighted += mean * weight;
    }

    fn gauge(self, weights: f64) -> Gauge {
        Gauge {
            peak: rounded(self.peak),
            mean: if weights > 0.0 {
                rounded(self.weighted / weights)
            } else {
                0.0
            },
        }
    }

    fn level(self, weights: f64) -> Level {
        let gauge = self.gauge(weights);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a peak is a count of sessions, read from a whole number"
        )]
        let peak = gauge.peak as u64;
        Level {
            peak,
            mean: gauge.mean,
        }
    }
}

/// `(peak, mean)` of `name` taken into the named sums `sums` at `weight`, a sum made for a name it
/// has none for. A list rather than a map: a period holds a few roles and kinds, and a map costs the
/// server's binary its own tree code (docs/workflow/CHECKS.md, size budgets).
fn take(sums: &mut Vec<(String, Sum)>, name: &str, peak: f64, mean: f64, weight: f64) {
    match sums.iter_mut().find(|(kept, _)| kept == name) {
        Some((_, sum)) => sum.take(peak, mean, weight),
        None => {
            let mut sum = Sum::default();
            sum.take(peak, mean, weight);
            sums.push((name.to_string(), sum));
        }
    }
}

/// The agents at work in a period out of its hour lines; none of none.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count of sessions or snapshots is far below 2^52"
)]
fn agents(lines: &[AgentHour]) -> Option<BucketAgents> {
    if lines.is_empty() {
        return None;
    }
    let observed: f64 = lines.iter().map(|line| line.observed_s).sum();
    let samples: u64 = lines.iter().map(|line| line.samples).sum();
    // Each line's weight: the seconds it watched, or its snapshots where no line watched any.
    let weight = |line: &AgentHour| {
        if observed > 0.0 {
            line.observed_s
        } else {
            line.samples as f64
        }
    };
    let weights = if observed > 0.0 {
        observed
    } else {
        samples as f64
    };
    let mut running = Vec::new();
    let mut holding = Vec::new();
    let mut live = Vec::new();
    let (mut running_agents, mut holding_agents) = (Sum::default(), Sum::default());
    let (mut load_1m, mut load_15m) = (None::<(Sum, f64)>, None::<(Sum, f64)>);
    let mut out = BucketAgents {
        observed_s: rounded(observed),
        samples,
        ..BucketAgents::default()
    };
    for line in lines {
        let w = weight(line);
        for role in &line.running_roles {
            take(&mut running, &role.role, role.peak as f64, role.mean, w);
        }
        for role in &line.holding_roles {
            take(&mut holding, &role.role, role.peak as f64, role.mean, w);
        }
        for kind in &line.live {
            take(&mut live, &kind.kind, kind.peak as f64, kind.mean, w);
        }
        running_agents.take(line.running_agents.peak as f64, line.running_agents.mean, w);
        holding_agents.take(line.holding_agents.peak as f64, line.holding_agents.mean, w);
        for (kept, read) in [(&mut load_1m, line.load_1m), (&mut load_15m, line.load_15m)] {
            if let Some(read) = read {
                let (sum, weights) = kept.get_or_insert_default();
                sum.take(read.peak, read.mean, w);
                *weights += w;
            }
        }
        for sessions in &line.sessions {
            out.went_stale += sessions.went_stale;
            out.went_lost += sessions.went_lost;
            out.stale_s += sessions.stale_s;
            out.lost_s += sessions.lost_s;
        }
        // The sessions an hour started are its line's that ran latest: each line counts them from
        // the hour's start to its own end.
        let latest =
            lines
                .iter()
                .filter(|other| other.start == line.start)
                .fold(
                    line,
                    |best, other| if other.to > best.to { other } else { best },
                );
        if std::ptr::eq(latest, line) {
            out.started += line.sessions.iter().map(|s| s.started).sum::<u64>();
        }
    }
    out.stale_s = rounded(out.stale_s);
    out.lost_s = rounded(out.lost_s);
    out.running = running
        .into_iter()
        .map(|(role, sum)| {
            let level = sum.level(weights);
            RoleLevel {
                role,
                peak: level.peak,
                mean: level.mean,
            }
        })
        .collect();
    out.holding = holding
        .into_iter()
        .map(|(role, sum)| {
            let level = sum.level(weights);
            RoleLevel {
                role,
                peak: level.peak,
                mean: level.mean,
            }
        })
        .collect();
    out.live = live
        .into_iter()
        .map(|(kind, sum)| {
            let level = sum.level(weights);
            KindLevel {
                kind,
                peak: level.peak,
                mean: level.mean,
            }
        })
        .collect();
    out.running_agents = running_agents.level(weights);
    out.holding_agents = holding_agents.level(weights);
    out.load_1m = load_1m.map(|(sum, weights)| sum.gauge(weights));
    out.load_15m = load_15m.map(|(sum, weights)| sum.gauge(weights));
    Some(out)
}

#[cfg(test)]
#[path = "reports/tests.rs"]
mod tests;
