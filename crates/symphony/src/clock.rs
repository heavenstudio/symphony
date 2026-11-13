//! The local time a stamp records, `YYYY-MM-DD HH:MM`.

use std::sync::LazyLock;

use regex::Regex;

use crate::text::S;

unsafe extern "C" {
    fn tzset();
}

/// Now, in this machine's time zone, as Python's `datetime.now().strftime("%Y-%m-%d %H:%M")`.
pub fn now_stamp() -> String {
    let mut now: libc::time_t = 0;
    // SAFETY: time writes only into the value given, which outlives the call.
    unsafe {
        libc::time(&mut now);
    }
    stamp_at(now)
}

/// The minute `seconds` since the epoch fell in, in this machine's time zone, as Python's
/// `datetime.fromtimestamp(seconds).strftime("%Y-%m-%d %H:%M")`.
pub fn stamp_at(seconds: libc::time_t) -> String {
    // SAFETY: an all-zero `tm` is a valid value of a plain C struct of integers (and, on some
    // platforms, a null zone name), which localtime_r overwrites.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: tzset reads TZ into libc's own state.
    unsafe {
        tzset();
    }
    // SAFETY: localtime_r reads the time given and writes only into `tm`, both outliving the call.
    unsafe {
        libc::localtime_r(&seconds, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        i64::from(tm.tm_year) + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

/// Python's `strptime` pattern for `%Y-%m-%d %H:%M`, the space any run of spaces.
static STAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(\d\d\d\d)-(1[0-2]|0[1-9]|[1-9])-(3[01]|[12]\d|0[1-9]|[1-9]| [1-9]){S}+(2[0-3]|[0-1]\d|\d):([0-5]\d|\d)$"
    ))
    .unwrap()
});

fn leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Minutes since 1970-01-01 00:00 of a `%Y-%m-%d %H:%M` stamp, as Python's `strptime` reads it;
/// `None` when it does not read.
pub fn stamp_minutes(stamp: &str) -> Option<i64> {
    let caps = STAMP.captures(stamp)?;
    let field = |i: usize| caps[i].trim().parse::<i64>().ok();
    let (year, month, day, hour, minute) = (field(1)?, field(2)?, field(3)?, field(4)?, field(5)?);
    if year < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 1440 + hour * 60 + minute)
}

/// The second since the epoch a card's stamp names, in this machine's time zone, as
/// [`stamp_at`] wrote it: `%Y-%m-%d %H:%M`, or a day alone (`%Y-%m-%d`), read as its first minute.
/// `None` when it does not read, or names a minute the clocks skipped.
pub fn stamp_seconds(stamp: &str) -> Option<f64> {
    let stamp = if stamp.len() == 10 {
        format!("{stamp} 00:00")
    } else {
        stamp.to_string()
    };
    stamp_minutes(&stamp)?;
    let caps = STAMP.captures(&stamp)?;
    let field = |i: usize| caps[i].trim().parse::<libc::c_int>().ok();
    // SAFETY: an all-zero `tm` is a valid value of a plain C struct of integers (and, on some
    // platforms, a null zone name), which mktime reads and normalises.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = field(1)? - 1900;
    tm.tm_mon = field(2)? - 1;
    tm.tm_mday = field(3)?;
    tm.tm_hour = field(4)?;
    tm.tm_min = field(5)?;
    // Whether summer time holds then is mktime's to work out.
    tm.tm_isdst = -1;
    // SAFETY: tzset reads TZ into libc's own state; mktime reads and writes only `tm`, which
    // outlives the call.
    let seconds = unsafe {
        tzset();
        libc::mktime(&mut tm)
    };
    #[allow(clippy::cast_precision_loss)]
    (seconds != -1).then_some(seconds as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_reads_back_to_the_minute_it_was_written_at() {
        let noon = 1_800_000_000 - 1_800_000_000 % 86_400 + 12 * 3600;
        for at in [noon, noon + 59, noon + 3_600 * 7 + 120, noon + 86_400 * 180] {
            let minute = at - at % 60;
            #[allow(clippy::cast_precision_loss)]
            let expected = minute as f64;
            assert_eq!(stamp_seconds(&stamp_at(at)), Some(expected), "{at}");
        }
        let day = stamp_at(noon);
        assert_eq!(
            stamp_seconds(&day[..10]),
            stamp_seconds(&format!("{} 00:00", &day[..10]))
        );
        for bad in ["2026-13-01 00:00", "x", "2026-01-01 24:00", ""] {
            assert_eq!(stamp_seconds(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_stamp_reads_to_minutes_and_a_bad_one_does_not() {
        assert_eq!(stamp_minutes("1970-01-01 00:00"), Some(0));
        assert_eq!(stamp_minutes("1970-01-02 01:05"), Some(24 * 60 + 65));
        assert_eq!(
            stamp_minutes("2026-10-06 09:05").unwrap() - stamp_minutes("2026-10-06 09:00").unwrap(),
            5
        );
        assert_eq!(
            stamp_minutes("2024-03-01 00:00").unwrap() - stamp_minutes("2024-02-28 00:00").unwrap(),
            2 * 1440
        );
        assert_eq!(
            stamp_minutes("2026-1-5 9:3"),
            stamp_minutes("2026-01-05 09:03")
        );
        for bad in [
            "2026-13-01 00:00",
            "2026-02-30 00:00",
            "2026-01-01 24:00",
            "2026-01-01",
            "x",
            "2026-01-01 00:00 ",
        ] {
            assert_eq!(stamp_minutes(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_time_reads_as_the_minute_it_fell_in() {
        // a mid-January day, far from any change of clocks
        let noon = 1_800_000_000 - 1_800_000_000 % 86_400 + 12 * 3600;
        let at = |t: libc::time_t| stamp_minutes(&stamp_at(t)).unwrap();
        assert_eq!(at(noon + 3600) - at(noon), 60);
        assert_eq!(at(noon + 59) - at(noon), 0, "the seconds are dropped");
        assert_eq!(at(noon + 86_400) - at(noon), 1440);
        let mut now: libc::time_t = 0;
        // SAFETY: time writes only into the value given, which outlives the call.
        unsafe {
            libc::time(&mut now);
        }
        assert!((at(now) - stamp_minutes(&now_stamp()).unwrap()).abs() <= 1);
    }

    #[test]
    fn now_is_a_stamp() {
        let now = now_stamp();
        assert_eq!(now.len(), 16, "{now}");
        assert!(stamp_minutes(&now).is_some(), "{now}");
    }
}
