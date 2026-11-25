//! Ranked task rows with an excerpt from the field the board's search actually matched.

use super::commands::{Context, Stop};
use super::shape;
use super::text::roles_text;
use crate::Store;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let limit = usize::try_from(p.int("limit", 20))
        .ok()
        .filter(|limit| *limit > 0)
        .ok_or_else(|| Stop::Refused("--limit must be a positive integer".into()))?;
    let query = p.or("query", "");
    let hits = store.search(query)?;
    let mut eligible: BTreeSet<_> = BTreeSet::new();
    eligible.extend(store.task_slugs(p.get("status"))?);
    // Filtering comes before the limit; never reorder the store's relevance ranking by priority.
    let selected: Vec<_> = hits
        .iter()
        .filter(|hit| eligible.contains(&hit.slug))
        .take(limit)
        .collect();
    let named: Vec<_> = selected.iter().map(|hit| hit.slug.clone()).collect();
    let mut tasks = BTreeMap::new();
    tasks.extend((store.tasks_by_slug(&named)?.into_iter()).map(|task| (task.slug.clone(), task)));
    let hits: Vec<_> = selected
        .into_iter()
        .filter_map(|hit| tasks.get(&hit.slug).map(|task| (hit, task)))
        .collect();
    let slugs: Vec<_> = hits.iter().map(|(hit, _)| hit.slug.as_str()).collect();
    let sources = store.search_sources(query, &slugs)?;
    // Rows as `list --json` lists them, when each task took its status too.
    let since = if p.flag("json") {
        let named: Vec<String> = hits.iter().map(|(hit, _)| hit.slug.clone()).collect();
        store.in_status_since(&named)?
    } else {
        BTreeMap::new()
    };
    let mut json = Vec::new();
    for (hit, task) in hits {
        let source = sources.get(&hit.slug).ok_or_else(|| {
            Stop::Board(format!(
                "search match for {} changed; retry the search",
                hit.slug
            ))
        })?;
        let (source_name, text) = if source.source == "slug" {
            ("title", task.title.as_str())
        } else {
            (source.source.as_str(), source.text.as_str())
        };
        let snippet = excerpt(text, query);
        if p.flag("json") {
            let Value::Object(mut fields) = shape::listed(task, since.get(&task.slug).copied())
            else {
                unreachable!("shape::listed is an object");
            };
            fields.insert("score".into(), json!(hit.score));
            fields.insert("snippet".into(), json!(&snippet));
            fields.insert("snippet_source".into(), json!(source_name));
            json.push(Value::Object(fields));
        } else {
            let roles = roles_text(task);
            let roles = if roles.is_empty() {
                String::new()
            } else {
                format!("  [{roles}]")
            };
            c.say(&format!(
                "p{} {:<11} {:<28} {}{roles}",
                task.priority, task.status, task.slug, task.title
            ));
            c.say(&format!("  {source_name}: {snippet}"));
        }
    }
    if p.flag("json") {
        c.say(
            &serde_json::to_string_pretty(&json!({
                "tasks": Value::Array(json),
            }))
            .expect("JSON values serialize"),
        );
    }
    Ok(())
}

fn excerpt(text: &str, query: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<_> = flat.chars().collect();
    // Keep original character positions across Unicode lowercase expansions, then cut on chars.
    let folded: Vec<_> = chars
        .iter()
        .enumerate()
        .flat_map(|(at, c)| c.to_lowercase().map(move |lower| (lower, at)))
        .collect();
    let lower: String = folded.iter().map(|(c, _)| c).collect();
    let terms = query.to_lowercase();
    let matched = terms
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .filter_map(|term| lower.find(term))
        .min()
        .map(|byte| folded[lower[..byte].chars().count()].1)
        .unwrap_or(0);
    let start = matched.saturating_sub(60);
    let end = (start + 200).min(chars.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        chars[start..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::excerpt;

    #[test]
    fn excerpts_flatten_whitespace_and_keep_unicode_intact() {
        for (text, query, expected) in [
            ("  Hello\n\tworld  ", "world", "Hello world"),
            ("İstanbul 🚀 TORPEDO", "torpedo", "İstanbul 🚀 TORPEDO"),
            ("Crème brûlée", "creme", "Crème brûlée"),
            ("", "needle", ""),
            ("   ", "...", ""),
        ] {
            assert_eq!(excerpt(text, query), expected);
        }
    }

    #[test]
    fn long_excerpts_show_the_match_and_mark_the_cut_ends() {
        let tail_match = excerpt(&format!("{} TORPEDO 🚀", "İ".repeat(260)), "torpedo");
        assert!(tail_match.starts_with('…'));
        assert!(tail_match.ends_with("TORPEDO 🚀"));
        assert!(tail_match.chars().count() <= 202);
        let head_match = excerpt(&format!("needle {}", "🚀".repeat(220)), "needle");
        assert!(head_match.starts_with("needle "));
        assert!(head_match.ends_with('…'));
        assert_eq!(head_match.chars().count(), 201);
    }
}
