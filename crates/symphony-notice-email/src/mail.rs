//! The mail a notice is sent as: its headers and its body, all ASCII.
//!
//! It says what waits on the person, in which project, and the link: a notice carries no title and
//! no words (docs/engineering/NOTICES.md, "A link alone"). It is from `Symphony` at the settings'
//! address, its `Message-ID` is made from the notice's id, so a mail server or a mailbox that keeps
//! the ids it took can drop a second hand of one notice, and it says it is `Auto-Submitted`, so a
//! vacation reply does not answer it.

use std::time::{SystemTime, UNIX_EPOCH};
use symphony_notice::{Kind, Notice};

/// What [`address`] takes, as a refusal says it.
pub(crate) const ADDRESS_RULE: &str = "ASCII `local@domain`, the local part letters, digits, dots \
     and `!#$%&'*+-/=?^_`{|}~`, the domain's labels letters, digits and inner dashes";

/// Whether `text` is a plain email address: `local@domain` in ASCII, the local part a dot-atom
/// (RFC 5322: no quoted string, no comment) of at most 64 characters, the domain a host name of
/// labels of letters, digits and inner dashes, at most 254 characters in all. What a header or an
/// SMTP command could read as more than one address (`<`, `>`, `,`, `;`, a space, a quote) is in
/// neither part.
pub(crate) fn address(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    let atext = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~".contains(c);
    let label = |label: &str| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    text.len() <= 254
        && (1..=64).contains(&local.len())
        && local
            .split('.')
            .all(|atom| !atom.is_empty() && atom.chars().all(atext))
        && domain.split('.').all(label)
}

/// A notice's mail: its headers, each one line, and its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mail {
    pub(crate) headers: Vec<String>,
    pub(crate) body: String,
}

impl Mail {
    /// The mail of `notice`, from the address `from`, dated `now`.
    pub(crate) fn of(notice: &Notice, from: &str, now: SystemTime) -> Mail {
        let said = sentence(notice.kind(), notice.project());
        let domain = from.rsplit_once('@').map_or(from, |(_, domain)| domain);
        let seconds = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        Mail {
            headers: vec![
                format!("From: Symphony <{from}>"),
                format!("To: {}", notice.to().address()),
                format!("Subject: {said}"),
                format!("Date: {}", date(seconds)),
                format!("Message-ID: {}", message_id(notice.id(), domain)),
                "Auto-Submitted: auto-generated".into(),
            ],
            body: format!("{said}:\r\n\r\n{}\r\n", notice.link()),
        }
    }
}

/// What the notice tells of, in `project`.
fn sentence(kind: Kind, project: &str) -> String {
    let what = match kind {
        Kind::Card => "A card waits on you",
        Kind::Question => "A question waits on you",
        Kind::Verdict => "A reviewer gave a verdict",
        // A kind a newer server gives (NOTICES.md, "Kinds").
        _ => "Something waits on you",
    };
    format!("{what} in {project}")
}

/// The `Message-ID` of the notice `id`, at `domain`: `<ID@DOMAIN>`, each dot of the id written
/// `=`. An id may hold dots anywhere, where a message id's left side is a dot-atom, with no dot at
/// either end or two together; `=` is in no id, so two ids keep two message ids.
fn message_id(id: &str, domain: &str) -> String {
    format!("<{}@{domain}>", id.replace('.', "="))
}

/// The date `seconds` after 1970 in UTC, as a mail's `Date` writes it (RFC 5322):
/// `Tue, 24 Nov 2026 06:28:00 +0000`.
fn date(seconds: u64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (days, second) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil(days);
    format!(
        "{}, {day:02} {} {year} {:02}:{:02}:{:02} +0000",
        DAYS[usize::try_from(days % 7).unwrap_or(0)],
        MONTHS[usize::from(month - 1)],
        second / 3600,
        second % 3600 / 60,
        second % 60
    )
}

/// The year, month (1 to 12) and day (1 to 31) of the day `days` after 1970-01-01, in the
/// proleptic Gregorian calendar (Howard Hinnant's `civil_from_days`, for days from 1970 on).
fn civil(days: u64) -> (u64, u8, u8) {
    // Counted from 0000-03-01, so a leap day ends its year: an era is 400 years of 146 097 days.
    let days = days + 719_468;
    let era = days / 146_097;
    let of_era = days % 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (
        year,
        u8::try_from(month).unwrap_or(1),
        u8::try_from(day).unwrap_or(1),
    )
}

#[cfg(test)]
#[path = "mail_tests.rs"]
mod tests;
