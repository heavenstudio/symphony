//! The statements of `store/search.rs`.

/// Of the tasks `$slugs`, the titles, briefs, slugs and thread lines `$query`'s words match, each
/// with its score and the rank of where it matched, and the slug that is `$query` itself.
pub(in crate::store) const SOURCES: &str = "
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

/// The tasks `$query`'s words match in a title, a brief, a slug or a thread, or whose slug it is,
/// each with its best score, best first.
pub(in crate::store) const SEARCH: &str = "
LET $titles = SELECT slug, (search::score(0) ?? 0) * 3 AS score FROM task WHERE title @0@ $query;
LET $briefs = SELECT slug, search::score(0) ?? 0 AS score FROM task WHERE body @0@ $query;
LET $slugs = SELECT slug, (search::score(0) ?? 0) * 4 AS score FROM task WHERE slug @0@ $query;
LET $threads = SELECT slug, search::score(0) ?? 0 AS score FROM message WHERE text @0@ $query;
LET $exact = SELECT slug, 1000000 AS score FROM task WHERE slug = $query;
RETURN SELECT slug, math::max(score) AS score
    FROM array::concat($titles, $briefs, $slugs, $threads, $exact)
    GROUP BY slug ORDER BY score DESC, slug ASC;
";
