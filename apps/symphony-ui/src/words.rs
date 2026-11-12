//! The board's words and times.

/// A wall-clock time without a zone, as the registers write it: minutes since 1970-01-01 00:00.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Moment(i64);

impl Moment {
    /// `YYYY-MM-DD HH:MM` or `YYYY-MM-DD`; `None` for anything else.
    pub fn parse(text: &str) -> Option<Moment> {
        let (date, time) = match text.split_once(' ') {
            Some((date, time)) => (date, Some(time)),
            None => (text, None),
        };
        let mut parts = date.split('-');
        let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return None;
        }
        let (hour, minute) = match time {
            None => (0, 0),
            Some(time) => {
                let (hour, minute) = time.split_once(':')?;
                if hour.len() != 2 || minute.len() != 2 {
                    return None;
                }
                (hour.parse().ok()?, minute.parse().ok()?)
            }
        };
        Moment::from_parts(
            year.parse().ok()?,
            month.parse().ok()?,
            day.parse().ok()?,
            hour,
            minute,
        )
    }

    /// A date and time of day; `None` when one is out of range.
    pub fn from_parts(year: i64, month: u32, day: u32, hour: u32, minute: u32) -> Option<Moment> {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
            return None;
        }
        Some(Moment(
            days_from_civil(year, month, day) * 1440 + i64::from(hour * 60 + minute),
        ))
    }

    pub fn minutes(self) -> i64 {
        self.0
    }

    fn hour(self) -> i64 {
        self.0.rem_euclid(1440) / 60
    }

    fn day(self) -> i64 {
        self.0.div_euclid(1440)
    }

    /// Its time of day, `14:32`.
    fn clock(self) -> String {
        let minute = self.0.rem_euclid(1440);
        format!("{:02}:{:02}", minute / 60, minute % 60)
    }
}

/// The date of a day counted from 1970-01-01: year, month and day (Howard Hinnant's).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * month_from_march + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    })
    .unwrap_or(1);
    let year = year_of_era + era * 400;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// When a copy kept on this device was saved, as a page says it at `now`: `14:32` the same day,
/// `yesterday 14:32` the day before, else `3 Nov 14:32`, with the year when it is not `now`'s.
pub fn saved_at(saved: Moment, now: Moment) -> String {
    let clock = saved.clock();
    match now.day() - saved.day() {
        0 => clock,
        1 => format!("yesterday {clock}"),
        _ => {
            let (year, month, day) = civil_from_days(saved.day());
            let month = MONTHS[(month as usize).saturating_sub(1) % 12];
            if year == civil_from_days(now.day()).0 {
                format!("{day} {month} {clock}")
            } else {
                format!("{day} {month} {year} {clock}")
            }
        }
    }
}

/// What a page drawn from what this device kept says until the server has answered every read it
/// drew: that it is checking, or, while a read fails and its feed is not open, that the server is
/// not reachable.
/// `saved` is when the oldest of them was kept ([`saved_at`]).
pub fn device_notice(saved: &str, unreachable: bool) -> String {
    if unreachable {
        format!("Not reachable: shown from this device, saved {saved}")
    } else {
        format!("Shown from this device, saved {saved}. Checking for changes…")
    }
}

/// What an action's button says while the page may show what is out of date, and it waits.
pub const CHECKING: &str = "Checking…";

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar (Howard Hinnant's).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = i64::from((month + 9) % 12);
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// "Good morning", "Good afternoon" or "Good evening".
pub fn greeting(now: Moment) -> &'static str {
    match now.hour() {
        0..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

/// How long something has waited, short: "new", "3 h", "7 d"; "" without a time.
pub fn age(posted: Option<&str>, now: Moment) -> String {
    let Some(then) = posted.and_then(Moment::parse) else {
        return String::new();
    };
    let minutes = (now.0 - then.0).max(0);
    if minutes >= 1440 {
        format!("{} d", minutes / 1440)
    } else if minutes >= 60 {
        format!("{} h", minutes / 60)
    } else {
        "new".to_string()
    }
}

/// The line under the greeting.
pub fn lead(waiting: u32, docs: u32) -> String {
    if waiting == 0 {
        return "Your queue is empty.".to_string();
    }
    format!(
        "{waiting} {} on you across {docs} {}. Start with the one below.",
        if waiting == 1 {
            "card waits"
        } else {
            "cards wait"
        },
        plural(docs, "document")
    )
}

/// The feed's `tasks-status`: whether the task board answers (`None` when the event does not say),
/// and of one that does not, whether the server reached it and its answer came late.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatus {
    pub available: Option<bool>,
    pub slow: bool,
}

impl BoardStatus {
    /// What a task page says of the board: nothing unless it is unavailable; that it is slow when
    /// it is, else that it is unavailable.
    pub fn notice(self) -> &'static str {
        match self {
            Self {
                available: Some(false),
                slow: true,
            } => "The task board is slow to answer. Retrying…",
            Self {
                available: Some(false),
                slow: false,
            } => "The task board is unavailable. Reconnecting…",
            _ => "",
        }
    }
}

/// `noun`, with an s unless there is one.
pub fn plural(count: u32, noun: &str) -> String {
    if count == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}
