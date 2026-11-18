//! The analyzer on PostgreSQL 16, which has no builtin `pg_c_utf8` collation and an older
//! Unicode: PostgreSQL 18's own answers as data (`unicode16.tsv`), so `board_terms` gives the
//! same terms there, code point for code point.
//!
//! The data is PostgreSQL 18.7's (Unicode 16.0), read from a running server: `alpha`, the code
//! points `[[:alpha:]]` matches under `pg_c_utf8`, as ranges; `lower`, each code point `lower`
//! changes there and what to (one for one: its simple mapping); `nfkd`, each code point
//! `normalize(…, NFKD)` decomposes on 18 and not on 16, with its decomposition; `mark`, each
//! combining mark 18 orders by its class where 16 takes it for a starter, with a mark of the same
//! class 16 knows, which is no letter and no decomposition's part, so it stands in for it while
//! 16 orders the term and is put back after. Code points are hexadecimal. The hosted matrix
//! compares the terms of every code point on a running 16 with a running 18's
//! (`tests/hosted/analyzer.rs`), so a datum that no longer says what 18 does fails there.

use std::fmt::Write as _;

const DATA: &str = include_str!("../unicode16.tsv");

/// The text of each of `list`'s code points, a `'` doubled for a literal.
fn text(list: &str) -> String {
    let mut text = String::new();
    for point in list.split(',') {
        let c = u32::from_str_radix(point, 16)
            .ok()
            .and_then(char::from_u32)
            .expect("a code point");
        if c == '\'' {
            text.push(c);
        }
        text.push(c);
    }
    text
}

/// `list`'s ranges as a bracket expression's, each end `\x` and its code point.
fn class(list: &str) -> String {
    let mut class = String::with_capacity(list.len() * 2);
    for range in list.split(',') {
        let (from, to) = range
            .split_once('-')
            .map_or((range, None), |(from, to)| (from, Some(to)));
        let _ = write!(class, "\\x{from}");
        if let Some(to) = to {
            let _ = write!(class, "-\\x{to}");
        }
    }
    class
}

/// What opens the body of the analyzer's function, and what closes it.
const OPENS: &str = "LANGUAGE sql IMMUTABLE STRICT AS $f$\n";
const CLOSES: &str = "\n$f$;";
/// The analyzer's lowering and its letters, which 16 has neither of.
const LOWERED: &str = "lower(input COLLATE pg_c_utf8), '[[:alpha:]]+|";

/// `words`, the analyzer's definition, as PostgreSQL 16 takes it: its lowering, its letters and
/// its decomposition from the data. A text of ASCII alone, which most of a board's is, needs
/// none of the data, so it is analyzed by a query of its own, ASCII's letters lowered and
/// matched; and the function is PL/pgSQL's there, which plans each query once a session, where
/// SQL's would read the data's thousands of literals again at every call.
pub(super) fn words(words: &str) -> String {
    let (mut alpha, mut lower, mut marks) = (String::new(), None, None);
    let mut decomposed = "LATIN".to_string();
    for line in DATA.lines() {
        let mut fields = line.split('\t');
        match (fields.next(), fields.next(), fields.next()) {
            (Some("alpha"), Some(ranges), None) => alpha = class(ranges),
            (Some("lower"), Some(from), Some(to)) => lower = Some((text(from), text(to))),
            (Some("mark"), Some(from), Some(to)) => marks = Some((text(from), text(to))),
            (Some("nfkd"), Some(point), Some(to)) => {
                decomposed = format!("replace({decomposed}, '{}', '{}')", text(point), text(to));
            }
            _ => panic!("a line of unicode16.tsv not in its shape: {line}"),
        }
    }
    let (upper, lowered) = lower.expect("the lower-case mapping");
    let (marks, standins) = marks.expect("the combining marks");
    let replaced = |words: &str, from: &str, to: &str| {
        assert!(words.contains(from), "the analyzer no longer says {from}");
        words.replace(from, to)
    };
    let lost = || panic!("the analyzer no longer says {OPENS}");
    let (head, rest) = words.split_once(OPENS).unwrap_or_else(lost);
    let (body, tail) = rest.split_once(CLOSES).unwrap_or_else(lost);
    let ascii = replaced(body, LOWERED, "lower(input COLLATE \"C\"), '[a-z]+|");
    let wide = replaced(
        &replaced(
            body,
            "normalize(LATIN, NFKD)",
            &format!(
                "translate(normalize(translate({decomposed}, '{marks}', '{standins}'), NFKD), \
                 '{standins}', '{marks}')"
            ),
        ),
        LOWERED,
        &format!("translate(input, '{upper}', '{lowered}') COLLATE \"C\", '[{alpha}]+|"),
    );
    format!(
        "{head}LANGUAGE plpgsql IMMUTABLE STRICT AS $f$\n#variable_conflict use_column\nBEGIN\n\
         IF octet_length(input) = char_length(input) THEN RETURN QUERY {ascii};\n\
         ELSE RETURN QUERY {wide};\nEND IF;\nEND{CLOSES}{tail}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_definition_for_16_names_no_builtin_collation_and_changes_the_analyzer_alone() {
        let (on_18, on_16) = (super::super::define(), super::super::define_on_16());
        assert!(on_18.contains("pg_c_utf8") && on_18.contains("[[:alpha:]]"));
        assert!(!on_16.contains("pg_c_utf8") && !on_16.contains("[[:alpha:]]"));
        assert!(!on_16.contains("LATIN") && !on_16.contains("NUMBERS"));
        // Everything before the analyzer and from the words' trigger on is the same text.
        let cut = "CREATE OR REPLACE FUNCTION board_terms";
        let (before_18, after_18) = on_18.split_once(cut).unwrap();
        let (before_16, after_16) = on_16.split_once(cut).unwrap();
        assert_eq!(before_18, before_16);
        let rest = "CREATE OR REPLACE FUNCTION board_words()";
        assert_eq!(
            after_18.split_once(rest).unwrap().1,
            after_16.split_once(rest).unwrap().1
        );
        assert_eq!(on_16.matches(cut).count(), 1);
    }

    #[test]
    fn the_data_reads_as_ranges_pairs_and_marks_of_equal_number() {
        assert_eq!(class("41-5a,aa,100-101"), "\\x41-\\x5a\\xaa\\x100-\\x101");
        assert_eq!(text("41,27,e9"), "A''é");
        let mut seen = std::collections::BTreeMap::new();
        for line in DATA.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            *seen.entry(fields[0]).or_insert(0) += 1;
            if matches!(fields[0], "lower" | "mark") {
                assert_eq!(
                    fields[1].split(',').count(),
                    fields[2].split(',').count(),
                    "{}",
                    fields[0]
                );
            }
        }
        assert_eq!(
            seen.into_iter().collect::<Vec<_>>(),
            [("alpha", 1), ("lower", 1), ("mark", 1), ("nfkd", 56)]
        );
        let defined = words(&format!(
            "before {OPENS}normalize(LATIN, NFKD) {LOWERED}'{CLOSES} after"
        ));
        assert!(defined.starts_with("before LANGUAGE plpgsql ") && defined.ends_with("$f$; after"));
        assert!(defined.contains(
            "THEN RETURN QUERY normalize(LATIN, NFKD) lower(input COLLATE \"C\"), '[a-z]+|';"
        ));
        assert!(defined.contains("COLLATE \"C\", '[\\x41-\\x5a\\x61-\\x7a\\xaa"));
        // ASCII's own capitals are in the mapping: nothing else lowers them here.
        assert!(defined.contains("translate(input, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
    }

    #[test]
    #[should_panic(expected = "the analyzer no longer says")]
    fn a_definition_that_lost_what_16_replaces_is_refused() {
        let _ = words("SELECT 1");
    }
}
