//! Compare source blocks, not generated markup or ordinal keys. Inserting a paragraph must not
//! mark every following paragraph changed, and a live component must not invent a source edit.

use gix::diff::blob::{Algorithm, Diff, InternedInput};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use symphony_api::{Difference, Document, DocumentComparison, Section, SectionComparison};

use crate::render::Render;

fn baselines(root: &std::path::Path, rel: &str) -> Option<(String, String)> {
    let repo = gix::discover(root).ok()?;
    let project = crate::project::Project::of(root);
    let main_ref = format!("refs/heads/{}", project.default_branch);
    let file = project.file(rel);
    let main = repo.rev_parse_single(main_ref.as_str()).ok()?;
    let head = repo.head_id().ok()?;
    let base = repo.merge_base(main, head).ok()?;
    let text = |id: gix::Id<'_>| {
        let tree = id.object().ok()?.peel_to_tree().ok()?;
        let Some(id) = crate::versions::entry(&tree, &file) else {
            return Some(String::new());
        };
        String::from_utf8(repo.find_blob(id).ok()?.take_data()).ok()
    };
    Some((text(main)?, text(base)?))
}

fn diff(before: &[crate::render::Block], after: &[crate::render::Block]) -> Diff {
    let mut input = InternedInput::default();
    input.update_before(before.iter().map(|b| b.text.as_str()));
    input.update_after(after.iter().map(|b| b.text.as_str()));
    Diff::compute(Algorithm::Myers, &input)
}

fn slug(block: &crate::render::Block) -> Option<&str> {
    crate::register::card_line(block.text.lines().next().unwrap_or("")).map(|c| c.slug)
}

fn similarity(before: &str, after: &str) -> usize {
    if before.trim_start().starts_with('#') != after.trim_start().starts_with('#') {
        return 0;
    }
    let words = |text: &str| {
        let mut words = BTreeSet::new();
        words.extend(
            (text.split(|c: char| !c.is_alphanumeric()))
                .filter(|word| !word.is_empty())
                .map(str::to_lowercase),
        );
        words
    };
    let (before, after) = (words(before), words(after));
    before.intersection(&after).count() * 1000 / (before.len() + after.len()).max(1)
}

fn unique_best(scores: impl Iterator<Item = (usize, usize)>) -> Option<usize> {
    let (mut best, mut chosen, mut tied) = (0, None, false);
    for (score, index) in scores {
        if score > best {
            (best, chosen, tied) = (score, Some(index), false);
        } else if score == best {
            tied = true;
        }
    }
    (!tied).then_some(chosen).flatten()
}

/// Track replacements to their fork version. Prose needs a mutual unique content match;
/// an ambiguous insertion must never become another block's previous content.
fn origins(before: &[crate::render::Block], after: &[crate::render::Block]) -> Vec<Option<usize>> {
    let mut origins = vec![None; after.len()];
    let (mut old, mut new) = (0, 0);
    for hunk in diff(before, after).hunks() {
        for origin in origins.iter_mut().take(hunk.after.start as usize).skip(new) {
            *origin = Some(old);
            old += 1;
        }
        let mut available =
            (hunk.before.start as usize..hunk.before.end as usize).collect::<Vec<_>>();
        for index in hunk.after.start as usize..hunk.after.end as usize {
            let Some(id) = slug(&after[index]) else {
                continue;
            };
            origins[index] = available
                .iter()
                .position(|&old| slug(&before[old]) == Some(id))
                .map(|at| available.remove(at));
        }
        let prose_before: Vec<_> = available
            .iter()
            .copied()
            .filter(|&i| slug(&before[i]).is_none())
            .collect();
        let prose_after: Vec<_> = (hunk.after.start as usize..hunk.after.end as usize)
            .filter(|&i| origins[i].is_none() && slug(&after[i]).is_none())
            .collect();
        let scores: Vec<_> = prose_before
            .iter()
            .flat_map(|&old| {
                prose_after
                    .iter()
                    .map(move |&new| (similarity(&before[old].text, &after[new].text), old, new))
            })
            .collect();
        for &index in &prose_after {
            if let Some(old) = unique_best(
                scores
                    .iter()
                    .filter(|(_, _, new)| *new == index)
                    .map(|(score, old, _)| (*score, *old)),
            ) && unique_best(
                scores
                    .iter()
                    .filter(|(_, candidate, _)| *candidate == old)
                    .map(|(score, _, new)| (*score, *new)),
            ) == Some(index)
            {
                origins[index] = Some(old);
            }
        }
        // A lone replacement is unambiguous even when every word changed.
        if prose_before.len() == 1
            && prose_after.len() == 1
            && before[prose_before[0]].text.trim_start().starts_with('#')
                == after[prose_after[0]].text.trim_start().starts_with('#')
        {
            origins[prose_after[0]] = Some(prose_before[0]);
        }
        old = hunk.before.end as usize;
        new = hunk.after.end as usize;
    }
    for origin in origins.iter_mut().skip(new) {
        *origin = Some(old);
        old += 1;
    }
    origins
}

fn previous(render: &Render, rel: &str, scope: &str, text: &str) -> Vec<Section> {
    let history = render.typed_history(text, rel);
    render
        .typed_sections(text, rel)
        .into_iter()
        .map(|section| {
            let mut section: Section = super::wire(section);
            let slug = section
                .html
                .split("data-slug=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .map(str::to_string);
            if let Some(slug) = &slug {
                section.key = format!("card:{slug}");
            }
            super::prepare_fragment(
                &mut section.html,
                &mut section.media,
                render.root(),
                rel,
                scope,
            );
            // The comparison is read-only. Its headings and cards cannot steal the live anchors.
            section.html = section.html.replace(" id=\"", " id=\"main-before-");
            section.blocks = crate::blocks::parse(
                &section.html,
                &section.media,
                &super::scoped_path(rel, scope),
            );
            if let Some(thread) = slug.as_ref().and_then(|s| history.get(s)) {
                let mut thread: symphony_api::Fragment = super::wire(thread);
                super::prepare_fragment(
                    &mut thread.html,
                    &mut thread.media,
                    render.root(),
                    rel,
                    scope,
                );
                section.blocks.extend(crate::blocks::parse(
                    &thread.html,
                    &thread.media,
                    &super::scoped_path(rel, scope),
                ));
            }
            section
        })
        .collect()
}

pub(super) fn apply(render: &Render, rel: &str, scope: &str, text: &str, doc: &mut Document) {
    if scope.is_empty() {
        return;
    }
    let Some((before, base)) = baselines(render.root(), rel) else {
        return;
    };
    let (before, _) = crate::register::supersede(&before, &crate::clock::now_stamp());
    let (base, _) = crate::register::supersede(&base, &crate::clock::now_stamp());
    let base_blocks = crate::render::split_blocks(&base);
    let before_blocks = crate::render::split_blocks(&before);
    let after_blocks = crate::render::split_blocks(text);
    let fork_diff = diff(&base_blocks, &after_blocks);
    let mut changed: BTreeSet<_> = BTreeSet::new();
    changed.extend(fork_diff.hunks().flat_map(|h| h.after).map(|i| i as usize));
    let mut affected: BTreeSet<_> = BTreeSet::new();
    affected.extend(fork_diff.hunks().flat_map(|h| h.before).map(|i| i as usize));
    let main_origins = origins(&base_blocks, &before_blocks);
    let branch_origins = origins(&base_blocks, &after_blocks);
    let authored = |index: usize| main_origins[index].is_some_and(|base| affected.contains(&base));
    let hunks = diff(&before_blocks, &after_blocks)
        .hunks()
        .collect::<Vec<_>>();
    if hunks.is_empty() {
        return;
    }
    let old = previous(render, rel, scope, &before);
    let mut removed = Vec::new();
    for hunk in hunks {
        let before = hunk.before.start as usize..hunk.before.end as usize;
        let after = hunk.after.start as usize..hunk.after.end as usize;
        let mut available = before.clone().collect::<Vec<_>>();
        let mut pairs = Vec::new();
        for index in after.clone() {
            if !changed.contains(&index) {
                continue; // main moved; this block still says exactly what the branch forked from
            }
            // An inserted card beside an edited card must not consume the edited card's history.
            let key = &doc.sections[index].key;
            let matched = if key.starts_with("card:") {
                available
                    .iter()
                    .position(|&old_index| old[old_index].key == *key)
            } else {
                available.iter().position(|&old_index| {
                    branch_origins[index].is_some()
                        && main_origins[old_index] == branch_origins[index]
                        && !old[old_index].key.starts_with("card:")
                })
            }
            .map(|at| available.remove(at));
            if let Some(old_index) = matched {
                pairs.push((old_index, index));
            }
            doc.sections[index].comparison = Some(SectionComparison {
                kind: if matched.is_some() {
                    Difference::Changed
                } else {
                    Difference::Added
                },
                before: matched.map_or_else(Vec::new, |old_index| old[old_index].blocks.clone()),
            });
        }
        for index in available.into_iter().filter(|&index| authored(index)) {
            let mut section = old[index].clone();
            section.key = format!("removed:{}", section.key);
            section.start = 0;
            section.end = 0;
            section.part = doc
                .sections
                .get(after.start)
                .or_else(|| doc.sections.last())
                .and_then(|s| s.part);
            section.comparison = Some(SectionComparison {
                kind: Difference::Removed,
                before: Vec::new(),
            });
            let position = pairs
                .iter()
                .filter(|(old_index, _)| *old_index > index)
                .min_by_key(|(old_index, _)| *old_index)
                .map_or(after.end, |(_, new_index)| *new_index);
            removed.push((position, section));
        }
    }
    // Reverse insertion preserves both the branch's order and adjacent removed sections' order.
    symphony_board::order::sort_by_key(&mut removed, &|(index, _)| *index);
    for (index, section) in removed.into_iter().rev() {
        doc.sections.insert(index, section);
    }
    if doc.sections.iter().all(|s| s.comparison.is_none()) {
        return;
    }
    for section in doc.sections.iter_mut().filter(|s| s.comparison.is_some()) {
        let mut digest = Sha256::new();
        digest.update(section.hash.as_bytes());
        digest.update(serde_json::to_vec(&section.comparison).expect("comparison serializes"));
        digest.update(serde_json::to_vec(&section.blocks).expect("blocks serialize"));
        section.hash = digest
            .finalize()
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect();
    }
    let branch = scope.replace('~', "/");
    let task = crate::branches::task_names(render.root())
        .remove(&branch)
        .unwrap_or(branch);
    doc.comparison = Some(DocumentComparison { task });
}
