//! The statements of `store/search.rs` on PostgreSQL: the full-text match and score over the terms
//! `words` holds ([`super::schema`]).
//!
//! A row matches when it holds every term of the query (AND), and scores by BM25, each value in
//! this type and order: each term the query holds once, in the order it first comes,
//! `idf · (k1 + 1) · tf′ / (tf′ + k1 · (1 − b + b · length / average))`, where
//! `idf = max(0, ln((N − df + 0.5) / (df + 0.5)))` and `tf′ = 1 + ln(tf)`, summed as `f64` and
//! given as `f32`, `k1` 1.2 and `b` 0.75 as `f32` are; `N` is every row of the field's index, an
//! empty text among them, `df` those holding the term.

use std::sync::LazyLock;

/// The rows `words` matches the query `v ->> 'query'` in, narrowed by `which`, each with its score.
fn scored(which: &str) -> String {
    format!(
        "WITH q AS (SELECT term, min(at) AS at FROM board_terms(v ->> 'query') GROUP BY term),
w AS (SELECT id, source, slug, terms, length FROM words WHERE terms ?& ARRAY(SELECT term FROM q) AND EXISTS (SELECT FROM q){which}),
s AS (SELECT source, count(*)::float8 AS n, sum(length)::float8 AS total, 1.2::real::float8 AS k1
    FROM words WHERE source IN (SELECT source FROM w) GROUP BY source),
d AS (SELECT source, term, count(*)::float8 AS df FROM words, q
    WHERE terms ? term AND source IN (SELECT source FROM w) GROUP BY source, term),
scored AS (SELECT w.id, w.source, w.slug,
    sum((greatest(ln((n - df + 0.5) / (df + 0.5)), 0) * (k1 + 1) * tf)
        / (tf + k1 * (0.25 + 0.75 / (total / n) * length)) ORDER BY at)::real::float8 AS score
    FROM w JOIN s USING (source) CROSS JOIN q JOIN d USING (source, term),
        LATERAL (SELECT 1 + ln((terms ->> term)::float8) AS tf) f
    GROUP BY w.id, w.source, w.slug)"
    )
}

/// The tasks the query matches in a title, a brief, a slug or a thread, or whose slug it is, each
/// with its best score, best first: a title's score three times its own and a slug's four.
pub(in crate::store) static SEARCH: LazyLock<String> = LazyLock::new(|| {
    format!(
        "({},
hit AS (SELECT slug, score * CASE source WHEN 'title' THEN 3 WHEN 'slug' THEN 4 ELSE 1 END AS score
    FROM scored UNION ALL SELECT slug, 1000000 FROM task WHERE slug = v ->> 'query')
SELECT coalesce(jsonb_agg(jsonb_build_object('slug', slug, 'score', score)
    ORDER BY score DESC, slug COLLATE \"C\"), '[]')
FROM (SELECT slug, max(score) AS score FROM hit GROUP BY slug) h)",
        scored("")
    )
});

/// Of the tasks `v -> 'slugs'`, the titles, briefs, slugs and thread lines the query matches, each
/// with its score and the rank of where it matched, and the slug that is the query itself.
pub(in crate::store) static SOURCES: LazyLock<String> = LazyLock::new(|| {
    let slugs = "slug IN (SELECT jsonb_array_elements_text(v -> 'slugs'))";
    format!(
        "({},
found AS (SELECT s.slug, score,
    CASE source WHEN 'title' THEN 1 WHEN 'body' THEN 2 WHEN 'text' THEN 3 ELSE 4 END AS rank,
    CASE source WHEN 'text' THEN 'message' ELSE source END AS source,
    CASE source WHEN 'title' THEN t.title WHEN 'body' THEN t.body WHEN 'slug' THEN t.slug
        ELSE m.text END AS text
    FROM scored s LEFT JOIN task t ON t.slug = s.slug
        LEFT JOIN message m ON source = 'text' AND m.id = s.id -> 1
    UNION ALL SELECT slug, 1000000, 4, 'slug', slug FROM task WHERE slug = v ->> 'query' AND {slugs})
SELECT jsonb_build_object('found', coalesce(jsonb_agg(found), '[]')) FROM found)",
        scored(&format!(" AND {slugs}"))
    )
});
