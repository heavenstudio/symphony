//! The board searches by words alone. Word matches and ranking stay in the database, including
//! message-only matches: the analyzer's terms, which the schema's triggers keep, and BM25 over
//! them ([`super::postgres::search`]). The exact slug branch retains identifiers containing
//! punctuation even when the analyzer splits them into words.
//!
//! The search by meaning the board had is deleted
//! (docs/engineering/analysis/code-review-2026-11.md#board-search-by-meaning). Its tables (`task_chunk` with
//! its `embedding` field and HNSW index, `task_embedding_source`, `task_embedding_checkpoint`) keep
//! their definitions in the schema, as removing one is a schema move, and a board that holds their
//! rows keeps them: nothing here reads or writes them.
use super::postgres::{self, Pg};
use super::statement::Statement;
use super::*;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SearchHit {
    pub slug: String,
    pub score: f64,
}

/// Where a task matched a search: its slug, the field (`title`, `body`, `slug`, `message`) and
/// that field's text.
#[derive(Debug, PartialEq, Deserialize)]
pub struct SearchSource {
    pub slug: String,
    pub source: String,
    pub text: String,
}

/// Flattened, as the source holds text alone: a float goes beside it, among the record's own fields,
/// which serde reads outside the buffer that would refuse it as the board links `serde_json`
/// ([`Beside`]).
#[derive(Deserialize)]
struct ScoredSource {
    #[serde(flatten)]
    source: SearchSource,
    rank: u8,
    score: f64,
}

fn validate_query(query: &str) -> Result<bool, Error> {
    if query.len() > 1024 {
        return Err(Error::Refused(
            "search query must be at most 1024 UTF-8 bytes".into(),
        ));
    }
    // Bound the work before any match/scoring pass. The analyzer splits letter and number
    // runs, keeps punctuation as terms, and drops whitespace; count symbols separately too.
    let mut previous = 0;
    let mut terms = 0;
    // Count the lowercase form sent below, including Unicode character expansions.
    for ch in query.chars().flat_map(char::to_lowercase) {
        let class = if ch.is_alphabetic() {
            1
        } else if ch.is_numeric() {
            2
        } else {
            0
        };
        if !ch.is_whitespace() && (class == 0 || class != previous) {
            terms += 1;
            if terms > 32 {
                return Err(Error::Refused(
                    "search query must contain at most 32 terms".into(),
                ));
            }
        }
        previous = class;
    }
    // The full-text analyzer has no terms for punctuation alone, and every row holds every
    // term of an empty set. It must remain an empty search result.
    Ok(query.chars().any(char::is_alphanumeric))
}

impl Store {
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, Error> {
        if !validate_query(query)? {
            return Ok(Vec::new());
        }
        let search = || Statement::from(Pg::read(postgres::search::SEARCH.as_str()));
        let vars = json!({"query": query.trim().to_lowercase()});
        self.retrying(|| self.read_together(search(), vars.clone()))
    }

    /// Fetch matching text for the displayed hits only. Prefer title, brief and thread context
    /// over repeating the slug. Use the database's analyzer rather than guessing matches in Rust.
    pub fn search_sources(
        &self,
        query: &str,
        slugs: &[&str],
    ) -> Result<std::collections::BTreeMap<String, SearchSource>, Error> {
        if slugs.is_empty() {
            return Ok(Default::default());
        }
        let sources = || Statement::from(Pg::read(postgres::search::SOURCES.as_str()));
        let vars = json!({"query": query.trim().to_lowercase(), "slugs": slugs});
        let groups: BTreeMap<String, Vec<ScoredSource>> =
            self.retrying(|| self.read_together(sources(), vars.clone()))?;
        // array::concat limits its output to 1 MiB. Keep each SELECT's rows in the same
        // snapshot, then use the shared stable ordering to choose the best excerpt.
        Ok(best_sources(groups))
    }
}

/// One source per slug, using the existing rank/score/text order and keeping the first equal row.
fn best_sources(groups: BTreeMap<String, Vec<ScoredSource>>) -> BTreeMap<String, SearchSource> {
    let mut rows: Vec<_> = groups.into_values().flatten().collect();
    crate::order::sort_by(&mut rows, &|a, b| {
        a.rank
            .cmp(&b.rank)
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.source.slug.cmp(&b.source.slug))
            .then_with(|| a.source.text.cmp(&b.source.text))
    });
    let mut sources = BTreeMap::new();
    for row in rows {
        let row = row.source;
        sources.entry(row.slug.clone()).or_insert(row);
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    type Spec = (&'static str, &'static str, &'static str, u8, f64);

    fn grouped(specs: &[Spec], groups: usize) -> BTreeMap<String, Vec<ScoredSource>> {
        let mut rows: BTreeMap<String, Vec<ScoredSource>> = BTreeMap::new();
        for (at, &(slug, source, text, rank, score)) in specs.iter().enumerate() {
            rows.entry(format!("group-{}", at % groups))
                .or_default()
                .push(ScoredSource {
                    source: SearchSource {
                        slug: slug.into(),
                        source: source.into(),
                        text: text.into(),
                    },
                    rank,
                    score,
                });
        }
        rows
    }

    /// The previous selection: a stable sort of all matches, then the first source per slug.
    fn sorted_sources(
        groups: BTreeMap<String, Vec<ScoredSource>>,
    ) -> BTreeMap<String, SearchSource> {
        let mut rows: Vec<_> = groups.into_values().flatten().collect();
        rows.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then_with(|| b.score.total_cmp(&a.score))
                .then_with(|| a.source.slug.cmp(&b.source.slug))
                .then_with(|| a.source.text.cmp(&b.source.text))
        });
        let mut sources = BTreeMap::new();
        for row in rows {
            let row = row.source;
            sources.entry(row.slug.clone()).or_insert(row);
        }
        sources
    }

    #[test]
    fn snippet_selection_preserves_rank_score_text_order_and_first_equal_row() {
        let specs = [
            ("a/task", "body", "body", 2, 100.0),
            ("a/task", "title", "title", 1, 0.0),
            ("b/task", "low", "a", 3, 1.0),
            ("b/task", "high", "z", 3, 2.0),
            ("c/task", "first", "same", 3, 4.0),
            ("c/task", "second", "same", 3, 4.0),
            ("d/task", "message", "猫", 3, 4.0),
            ("d/task", "message", "a", 3, 4.0),
            ("e/task", "negative-zero", "zero", 3, -0.0),
            ("e/task", "positive-zero", "zero", 3, 0.0),
        ];
        let chosen = best_sources(grouped(&specs, 1));
        assert_eq!(chosen["a/task"].source, "title");
        assert_eq!(chosen["b/task"].source, "high");
        assert_eq!(chosen["c/task"].source, "first");
        assert_eq!(chosen["d/task"].text, "a");
        assert_eq!(chosen["e/task"].source, "positive-zero");
        assert_eq!(chosen, sorted_sources(grouped(&specs, 1)));
    }

    #[test]
    fn snippet_selection_matches_the_previous_sort_across_orders_groups_and_float_edges() {
        let specs = [
            ("a/task", "body", "body", 2, 100.0),
            ("a/task", "title", "title", 1, 0.0),
            ("b/task", "first", "same", 3, 4.0),
            ("b/task", "second", "same", 3, 4.0),
            ("c/task", "message", "猫", 3, 4.0),
            ("c/task", "message", "", 3, 4.0),
            ("d/task", "minus-zero", "0", 3, -0.0),
            ("d/task", "plus-zero", "0", 3, 0.0),
            ("e/task", "infinite", "x", 3, f64::INFINITY),
            ("e/task", "nan-low", "x", 3, f64::NAN),
            (
                "e/task",
                "nan-high",
                "x",
                3,
                f64::from_bits(0x7ff8_0000_0000_0001),
            ),
            (
                "e/task",
                "negative-nan",
                "x",
                3,
                f64::from_bits(0xfff8_0000_0000_0001),
            ),
            ("f/task", "first-rank", "x", 0, f64::NEG_INFINITY),
            ("f/task", "last-rank", "x", u8::MAX, f64::INFINITY),
        ];
        for groups in [1, 2, 4] {
            for reverse in [false, true] {
                for offset in 0..specs.len() {
                    let mut ordered = specs.to_vec();
                    ordered.rotate_left(offset);
                    if reverse {
                        ordered.reverse();
                    }
                    assert_eq!(
                        best_sources(grouped(&ordered, groups)),
                        sorted_sources(grouped(&ordered, groups)),
                        "groups={groups} reverse={reverse} offset={offset}"
                    );
                }
            }
        }
    }

    #[test]
    fn snippet_selection_handles_empty_groups_and_preserves_group_order_for_equal_rows() {
        let mut groups = BTreeMap::new();
        groups.insert("empty".into(), vec![]);
        assert!(best_sources(groups).is_empty());
        assert!(best_sources(BTreeMap::new()).is_empty());
        let mut groups = BTreeMap::new();
        groups.insert(
            "z".into(),
            grouped(&[("a/b", "later-group", "same", 3, 1.0)], 1)
                .into_values()
                .next()
                .unwrap(),
        );
        groups.insert(
            "a".into(),
            grouped(&[("a/b", "earlier-group", "same", 3, 1.0)], 1)
                .into_values()
                .next()
                .unwrap(),
        );
        assert_eq!(best_sources(groups)["a/b"].source, "earlier-group");
    }

    /// A source's rank and score are its own fields, read outside serde's buffer for the flattened
    /// source, so the score reads in every form a float is written, from text and from a value.
    #[test]
    fn a_scored_source_reads_its_score_however_it_is_written() {
        for (score, want) in [
            ("1.5", 1.5_f64),
            ("1.50", 1.5),
            ("3e2", 300.0),
            ("0", 0.0),
            ("1000000", 1e6),
            ("0.30000000000000004441", 0.300_000_000_000_000_04),
        ] {
            let text = format!(
                r#"{{"slug":"a/b","source":"title","text":"Jump","rank":1,"score":{score}}}"#
            );
            let value: Value = serde_json::from_str(&text).unwrap();
            for row in [
                serde_json::from_str::<ScoredSource>(&text).unwrap(),
                serde_json::from_value::<ScoredSource>(value).unwrap(),
            ] {
                assert_eq!(row.score.to_bits(), want.to_bits(), "{text}");
                assert_eq!(row.rank, 1);
                assert_eq!(
                    (
                        row.source.slug.as_str(),
                        row.source.source.as_str(),
                        row.source.text.as_str()
                    ),
                    ("a/b", "title", "Jump")
                );
            }
        }
    }
}
