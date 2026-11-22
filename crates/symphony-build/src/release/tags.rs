//! Release tags: `release-YYYY-MM-DD` for a day's first release and `release-YYYY-MM-DD.N` for its
//! Nth (N from 2), the shared interface the update command reads (docs/workflow/RELEASES.md). Their order is
//! the releases': by day, then by number, which a plain sort of the names gets wrong (`.10` before
//! `.2`, and the day's first after its second when sorted newest first).

/// Every release tag's name begins so.
pub(crate) const PREFIX: &str = "release-";

/// A release, by its tag's name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Release {
    /// Its day, `YYYY-MM-DD`, which sorts as the date does.
    day: String,
    /// Its number that day: 1 for the day's first, whose name has none.
    number: u32,
}

impl Release {
    /// The release `name` tags, or none for a name that is not a release tag's.
    pub(crate) fn parse(name: &str) -> Option<Release> {
        let rest = name.strip_prefix(PREFIX)?;
        let (day, number) = match rest.split_once('.') {
            None => (rest, 1),
            Some((day, number)) => (day, nth(number)?),
        };
        is_day(day).then(|| Release {
            day: day.to_string(),
            number,
        })
    }

    /// Its tag's name.
    pub(crate) fn name(&self) -> String {
        if self.number == 1 {
            format!("{PREFIX}{}", self.day)
        } else {
            format!("{PREFIX}{}.{}", self.day, self.number)
        }
    }
}

/// A day's release number past its first: digits with no leading zero, 2 or more.
fn nth(number: &str) -> Option<u32> {
    if number.starts_with('0') || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok().filter(|n| *n >= 2)
}

/// Whether `day` is a `YYYY-MM-DD` date: a month from 01 to 12 and a day from 01 to 31.
pub(crate) fn is_day(day: &str) -> bool {
    let parts: Vec<&str> = day.split('-').collect();
    let [year, month, date] = parts.as_slice() else {
        return false;
    };
    let digits =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    if !(digits(year, 4) && digits(month, 2) && digits(date, 2)) {
        return false;
    }
    let (month, date): (u32, u32) = (month.parse().unwrap_or(0), date.parse().unwrap_or(0));
    (1..=12).contains(&month) && (1..=31).contains(&date)
}

/// The releases `names` tag, newest first, each once; a name that is not a release tag's is left out.
pub(crate) fn newest_first<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<Release> {
    let mut releases: Vec<Release> = names.into_iter().filter_map(Release::parse).collect();
    releases.sort_by(|a, b| b.cmp(a));
    releases.dedup();
    releases
}

/// The name the next release on `day` takes, beside the releases `names` tag: the day's first when
/// it has none, else one past its newest.
pub(crate) fn next<'a>(day: &str, names: impl IntoIterator<Item = &'a str>) -> String {
    let number = newest_first(names)
        .into_iter()
        .find(|release| release.day == day)
        .map_or(1, |newest| newest.number + 1);
    Release {
        day: day.to_string(),
        number,
    }
    .name()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(releases: &[Release]) -> Vec<String> {
        releases.iter().map(Release::name).collect()
    }

    #[test]
    fn a_release_tag_is_a_day_and_its_number_from_two() {
        for (name, day, number) in [
            ("release-2026-11-12", "2026-11-12", 1),
            ("release-2026-11-12.2", "2026-11-12", 2),
            ("release-2026-11-12.10", "2026-11-12", 10),
            ("release-2026-01-01", "2026-01-01", 1),
            ("release-2026-12-31.3", "2026-12-31", 3),
        ] {
            let release = Release::parse(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(
                release,
                Release {
                    day: day.into(),
                    number
                },
                "{name}"
            );
            assert_eq!(release.name(), name, "the name reads back");
        }
    }

    #[test]
    fn a_name_that_is_not_a_release_tag_is_none() {
        for name in [
            "release-2026-11-12.1",
            "release-2026-11-12.0",
            "release-2026-11-12.02",
            "release-2026-11-12.",
            "release-2026-11-12.x",
            "release-2026-11-12.2.3",
            "release-2026-11-12-2",
            "release-2026-13-01",
            "release-2026-00-10",
            "release-2026-11-00",
            "release-2026-11-32",
            "release-26-11-12",
            "release-2026-1-12",
            "release-2026-11-12 ",
            "release-",
            "v1.0.0",
            "2026-11-12",
            "Release-2026-11-12",
            "release-2026-11-12.99999999999",
        ] {
            assert_eq!(Release::parse(name), None, "{name}");
        }
    }

    #[test]
    fn newest_first_orders_by_day_then_number_not_by_the_names_letters() {
        let order = newest_first([
            "release-2026-11-12.2",
            "release-2026-11-11",
            "release-2026-11-12.10",
            "release-2026-11-12",
            "release-2026-12-01",
            "release-2025-12-31.3",
            "release-2026-11-12.9",
        ]);
        assert_eq!(
            names(&order),
            [
                "release-2026-12-01",
                "release-2026-11-12.10",
                "release-2026-11-12.9",
                "release-2026-11-12.2",
                "release-2026-11-12",
                "release-2026-11-11",
                "release-2025-12-31.3",
            ]
        );
    }

    #[test]
    fn newest_first_names_each_release_once_and_leaves_out_other_tags() {
        let order = newest_first([
            "release-2026-11-12",
            "v2",
            "release-2026-11-12",
            "release-2026-11-12.1",
            "release-2026-11-11",
        ]);
        assert_eq!(names(&order), ["release-2026-11-12", "release-2026-11-11"]);
        assert!(newest_first([]).is_empty());
    }

    #[test]
    fn the_next_release_of_a_day_is_one_past_its_newest() {
        assert_eq!(next("2026-11-12", []), "release-2026-11-12");
        assert_eq!(
            next("2026-11-12", ["release-2026-11-11", "release-2026-11-11.2"]),
            "release-2026-11-12",
            "another day's releases count for nothing"
        );
        assert_eq!(
            next("2026-11-12", ["release-2026-11-12"]),
            "release-2026-11-12.2"
        );
        assert_eq!(
            next(
                "2026-11-12",
                [
                    "release-2026-11-12",
                    "release-2026-11-12.9",
                    "release-2026-11-12.10"
                ]
            ),
            "release-2026-11-12.11"
        );
        assert_eq!(
            next("2026-11-12", ["release-2026-11-12.3"]),
            "release-2026-11-12.4",
            "a gap is never filled: the next is past the newest"
        );
        assert_eq!(
            next("2026-11-12", ["release-2026-11-13"]),
            "release-2026-11-12",
            "a later day's release does not number an earlier day's"
        );
    }

    #[test]
    fn a_day_is_a_dashed_date_of_real_months_and_days() {
        for day in ["2026-11-12", "1999-01-31", "2026-12-01"] {
            assert!(is_day(day), "{day}");
        }
        for day in [
            "2026-11-1",
            "2026-1-12",
            "2026/11/12",
            "2026-11-12-",
            "",
            "abcd-ef-gh",
            "2026-00-01",
            "2026-11-32",
            "+026-11-12",
        ] {
            assert!(!is_day(day), "{day}");
        }
    }
}
