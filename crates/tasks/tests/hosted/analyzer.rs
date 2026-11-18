//! The analyzer on PostgreSQL 16 (`store/postgres/schema/sixteen.rs`) against PostgreSQL 18's
//! own: every code point's terms, alone and inside a word, read from a board made on a running
//! server of each. The data 16's definition is built from is 18's answers, so a difference here
//! is a datum that no longer says what 18 does, or a rule of 16's the definition missed.

use crate::cluster::{self, Project};
use crate::store::{HOST, NOW};
use symphony_board::pg::Step;
use symphony_board::pg::hosted::Client;
use symphony_board::{Opening, Store};

/// How many code points one statement analyzes.
const BATCH: u32 = 8192;

/// A board made on `major`, and a client of it.
fn board(major: u32) -> Client {
    let project: Project = cluster::cluster(major).project();
    Store::opening_hosted(
        project.client("maintenance"),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &Opening::write().migrating(),
    )
    .unwrap();
    project.client("web")
}

/// The terms of `text`, each with where it is, as one line.
fn terms(client: &Client, text: &str) -> String {
    let param = serde_json::json!({ "t": text }).to_string();
    let answers = client
        .request(&[Step::with(
            "SELECT coalesce(string_agg(term || '@' || at, ' ' ORDER BY at, term), '') \
             FROM board_terms($1::jsonb ->> 't')",
            &param,
        )])
        .unwrap();
    answers[0].value().unwrap_or("").to_string()
}

/// The first difference between 16's terms and 18's among the code points from `start`, each
/// as a word of its own, between two letters, and after a letter with a combining mark, where
/// the order of marks shows; and how many were compared.
fn compared(sixteen: &Client, eighteen: &Client, start: u32) -> Result<usize, String> {
    let points: Vec<char> = (start..start + BATCH).filter_map(char::from_u32).collect();
    let mut compared = 0;
    for (before, after) in [("", ""), ("a", "b"), ("e\u{301}", "\u{323}")] {
        let text: String = points
            .iter()
            .map(|point| format!("{before}{point}{after} "))
            .collect();
        let (on_16, on_18) = std::thread::scope(|scope| {
            let on_16 = scope.spawn(|| terms(sixteen, &text));
            (terms(eighteen, &text), on_16.join().unwrap())
        });
        if on_16 != on_18 {
            // Named by the first code point that differs alone, so the datum is found.
            let differs = points.iter().find(|point| {
                let one = format!("{before}{point}{after}");
                terms(sixteen, &one) != terms(eighteen, &one)
            });
            return Err(format!(
                "the terms of U+{start:04X}.. between {before:?} and {after:?} differ on \
                 PostgreSQL 16, first at {:?}",
                differs.map(|point| format!("U+{:04X}", u32::from(*point)))
            ));
        }
        compared += points.len();
    }
    Ok(compared)
}

#[test]
fn every_code_points_terms_on_16_are_18s_alone_and_inside_a_word() {
    let (sixteen, eighteen) = (board(16), board(18));
    let next = std::sync::atomic::AtomicU32::new(1);
    // As many at once as a process's pool lends (`cluster::settings`' `web`).
    let compared: usize = std::thread::scope(|scope| {
        let working: Vec<_> = (0..3)
            .map(|_| {
                scope.spawn(|| {
                    let mut compared = 0;
                    loop {
                        let start = next.fetch_add(BATCH, std::sync::atomic::Ordering::SeqCst);
                        if start > 0x10_FFFF {
                            return Ok(compared);
                        }
                        compared += self::compared(&sixteen, &eighteen, start)?;
                    }
                })
            })
            .collect();
        working
            .into_iter()
            .map(|one| {
                one.join()
                    .unwrap()
                    .unwrap_or_else(|why: String| panic!("{why}"))
            })
            .sum()
    });
    // Every scalar value but U+0000, which no text holds, in each of the three places.
    assert_eq!(compared, 3 * (0x11_0000 - 0x800 - 1));
    // A text of ASCII alone takes a way of its own on 16: each of its characters alone, and
    // every pair of them.
    let ascii: Vec<char> = (1..0x80).filter_map(char::from_u32).collect();
    for first in &ascii {
        let text: String = ascii
            .iter()
            .map(|second| format!("{first}{second} "))
            .collect();
        assert_eq!(
            terms(&sixteen, &text),
            terms(&eighteen, &text),
            "after {first:?}"
        );
        let alone = first.to_string();
        assert_eq!(
            terms(&sixteen, &alone),
            terms(&eighteen, &alone),
            "{first:?} alone"
        );
    }
}

#[test]
fn a_boards_search_reads_the_same_words_on_16_and_18() {
    let texts = [
        "ÉCOLE naïve Straße İstanbul ﬁnally",
        "ǅungla ΌΣΟΣ ὈΔΥΣΣΕΎΣ résumé Ångström",
        "東京 кириллица Zoë's coöperative ½ ⅷ x² ℌello ㎏",
        "snake_case kebab-case CamelCase v1.2.3 a/b/c @handle #42",
        "e\u{301}\u{323} e\u{323}\u{301} \u{1E69} s\u{323}\u{307} s\u{307}\u{323}",
    ];
    let (sixteen, eighteen) = (board(16), board(18));
    for text in texts {
        let on_18 = terms(&eighteen, text);
        assert!(!on_18.is_empty(), "{text}");
        assert_eq!(terms(&sixteen, text), on_18, "{text}");
    }
}
