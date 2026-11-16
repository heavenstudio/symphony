//! The board searches by words alone. Word matches and ranking stay in SurrealDB, including
//! message-only matches. The exact slug branch retains identifiers containing punctuation even when
//! the analyzer splits them into words.
//!
//! The search by meaning the board had is deleted (DECISIONS: board-search-by-meaning). Its tables
//! (`task_chunk` with its `embedding` field and HNSW index, `task_embedding_source`,
//! `task_embedding_checkpoint`) keep their definitions in the schema, as removing one is a schema
//! move, and a board that holds their rows keeps them: nothing here reads or writes them.
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct SearchHit {
    pub slug: String,
    pub score: f64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchSource {
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
    // The full-text analyzer has no terms for punctuation alone; SurrealDB treats that
    // empty term set as matching every row. It must remain an empty search result.
    Ok(query.chars().any(char::is_alphanumeric))
}

impl Store {
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, Error> {
        if !validate_query(query)? {
            return Ok(Vec::new());
        }
        self.retrying(|| self.transact(SEARCH, json!({"query": query.trim().to_lowercase()})))
    }

    /// Fetch matching text for the displayed hits only. Prefer title, brief and thread context
    /// over repeating the slug. Use the database's analyzer rather than guessing matches in Rust.
    pub(crate) fn search_sources(
        &self,
        query: &str,
        slugs: &[&str],
    ) -> Result<std::collections::BTreeMap<String, SearchSource>, Error> {
        if slugs.is_empty() {
            return Ok(Default::default());
        }
        let groups: BTreeMap<String, Vec<ScoredSource>> = self.retrying(|| {
            self.transact(
                SOURCES,
                json!({"query": query.trim().to_lowercase(), "slugs": slugs}),
            )
        })?;
        // array::concat limits its output to 1 MiB. Keep each SELECT's rows in the same
        // snapshot, but concatenate/sort here so a large matching brief still has an excerpt.
        let mut rows: Vec<_> = groups.into_values().flatten().collect();
        rows.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then_with(|| b.score.total_cmp(&a.score))
                .then_with(|| a.source.slug.cmp(&b.source.slug))
                .then_with(|| a.source.text.cmp(&b.source.text))
        });
        let mut sources = std::collections::BTreeMap::new();
        for row in rows {
            let row = row.source;
            sources.entry(row.slug.clone()).or_insert(row);
        }
        Ok(sources)
    }
}

const SOURCES: &str = "
LET $titles = SELECT slug, search::score(0) ?? 0 AS score, 1 AS rank, 'title' AS source, title AS text
    FROM task WHERE slug IN $slugs AND title @0@ $query;
LET $briefs = SELECT slug, search::score(0) ?? 0 AS score, 2 AS rank, 'body' AS source, body AS text
    FROM task WHERE slug IN $slugs AND body @0@ $query;
LET $slugs_found = SELECT slug, search::score(0) ?? 0 AS score, 4 AS rank, 'slug' AS source, slug AS text
    FROM task WHERE slug IN $slugs AND slug @0@ $query;
LET $threads = SELECT slug, search::score(0) ?? 0 AS score, 3 AS rank, 'message' AS source, text
    FROM message WHERE slug IN $slugs AND text @0@ $query;
LET $exact = SELECT slug, 1000000 AS score, 4 AS rank, 'slug' AS source, slug AS text
    FROM task WHERE slug IN $slugs AND slug = $query;
RETURN { titles: $titles, briefs: $briefs, slugs: $slugs_found, threads: $threads, exact: $exact };
";

const SEARCH: &str = "
LET $titles = SELECT slug, (search::score(0) ?? 0) * 3 AS score FROM task WHERE title @0@ $query;
LET $briefs = SELECT slug, search::score(0) ?? 0 AS score FROM task WHERE body @0@ $query;
LET $slugs = SELECT slug, (search::score(0) ?? 0) * 4 AS score FROM task WHERE slug @0@ $query;
LET $threads = SELECT slug, search::score(0) ?? 0 AS score FROM message WHERE text @0@ $query;
LET $exact = SELECT slug, 1000000 AS score FROM task WHERE slug = $query;
RETURN SELECT slug, math::max(score) AS score
    FROM array::concat($titles, $briefs, $slugs, $threads, $exact)
    GROUP BY slug ORDER BY score DESC, slug ASC;
";

#[cfg(test)]
mod tests {
    use super::*;

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
