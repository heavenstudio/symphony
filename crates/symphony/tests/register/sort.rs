//! NewestFirstTest and SortRegisterTest: the register regrouped owner-first, nothing lost.

use symphony::register::sort_register;

use crate::lines;

const REGISTER: &str = "# Decision register

Intro text stays.

<!-- register:start -->
## Stations
### Sizes

- [x] **plain** — approved without a word.
- [ ] **new-card** — a proposal.

### Purpose

- [-] **gone** — rejected.
  [2026-09-24 10:00] owner · rejected — no
  [2026-09-24 11:00] agent — replaced by new-card
- [x] **noted** — approved with a note.
  [2026-09-24 10:00] owner · approved — tweak it
<!-- register:end -->

Trailing text stays.
";

/// The headings and card lines, in order.
fn order(text: &str) -> Vec<String> {
    lines(text)
        .into_iter()
        .filter(|l| l.starts_with("## ") || l.starts_with("### ") || l.starts_with("- ["))
        .collect()
}

fn sorted(text: &str) -> String {
    sort_register(text).unwrap()
}

// NewestFirstTest: Needs your decision reads newest first: topics by their newest card, cards newest first.

const NEWEST: &str = "<!-- register:start -->\n### Old\n\n- [ ] **old** — 1\n  [2026-09-20 10:00] agent — first\n\
    - [ ] **undated** — 2\n\n### New\n\n- [ ] **mid** — 3\n  [2026-09-22 10:00] agent — x\n\
    - [ ] **newest** — 4\n  [2026-09-25 09:00] agent — y\n\n### Settled\n\n- [x] **done** — 5\n\
    \x20 [2026-09-26 10:00] owner · approved\n<!-- register:end -->\n";

#[test]
fn pending_cards_come_newest_first_with_their_topics() {
    assert_eq!(
        order(&sorted(NEWEST)),
        vec![
            "## Needs your decision",
            "### New",
            "- [ ] **newest** — 4",
            "- [ ] **mid** — 3",
            "### Old",
            "- [ ] **old** — 1",
            "- [ ] **undated** — 2",
            "## Resolved",
            "### Settled",
            "- [x] **done** — 5",
        ]
    );
}

#[test]
fn newest_first_is_idempotent() {
    let once = sorted(NEWEST);
    assert_eq!(sorted(&once), once);
}

// SortRegisterTest

#[test]
fn groups_come_in_owner_first_order_with_topics_kept() {
    let out = sorted(REGISTER);
    assert!(
        out.starts_with("# Decision register\n\nIntro text stays.\n\n<!-- register:start -->\n"),
        "{out}"
    );
    assert!(
        out.trim_end()
            .ends_with("<!-- register:end -->\n\nTrailing text stays."),
        "{out}"
    );
    assert_eq!(
        order(&out),
        vec![
            "## Needs your decision",
            "### Sizes",
            "- [ ] **new-card** — a proposal.",
            "## Waiting on an agent",
            "### Purpose",
            "- [x] **noted** — approved with a note.",
            "## Rejected and replaced",
            "### Purpose",
            "- [-] **gone** — rejected.",
            "## Resolved",
            "### Sizes",
            "- [x] **plain** — approved without a word.",
        ]
    );
}

#[test]
fn history_travels_with_its_card() {
    let out = lines(&sorted(REGISTER));
    let at = out
        .iter()
        .position(|l| l == "- [-] **gone** — rejected.")
        .unwrap();
    assert_eq!(
        out[at + 1..at + 3].to_vec(),
        vec![
            "  [2026-09-24 10:00] owner · rejected — no",
            "  [2026-09-24 11:00] agent — replaced by new-card"
        ]
    );
}

#[test]
fn sorting_is_idempotent() {
    let once = sorted(REGISTER);
    assert_eq!(sorted(&once), once);
}

#[test]
fn empty_groups_are_left_out() {
    let out = sorted("<!-- register:start -->\n### T\n\n- [x] **a** — 1\n<!-- register:end -->\n");
    assert!(!out.contains("Needs your decision"), "{out}");
    assert!(out.contains("## Resolved\n"), "{out}");
}

#[test]
fn no_count_is_written_into_the_file() {
    // Counts go stale the moment the file is edited without a regroup, so the file carries none;
    // the page and the board count live.
    let out = sorted(REGISTER);
    assert!(
        !regex::Regex::new(r"(?m)^## .*\(\d+\)")
            .unwrap()
            .is_match(&out),
        "{out}"
    );
}

#[test]
fn old_counted_headings_are_accepted_and_dropped() {
    let text = "<!-- register:start -->\n## Resolved (7)\n### T\n\n- [x] **a** — 1\n<!-- register:end -->\n";
    assert!(sorted(text).contains("## Resolved\n"));
}

#[test]
fn card_without_topic_gets_general() {
    assert!(
        sorted("<!-- register:start -->\n- [ ] **a** — 1\n<!-- register:end -->\n")
            .contains("### General")
    );
}

#[test]
fn prose_inside_the_region_is_refused_rather_than_dropped() {
    assert!(
        sort_register(
            "<!-- register:start -->\n### T\nsome prose\n- [ ] **a** — 1\n<!-- register:end -->\n"
        )
        .is_err()
    );
}

#[test]
fn no_region_leaves_text_alone() {
    assert_eq!(sorted("# x\n- [ ] **a** — 1\n"), "# x\n- [ ] **a** — 1\n");
}

#[test]
fn duplicate_slug_is_refused() {
    assert!(
        sort_register(
            "<!-- register:start -->\n- [ ] **a** — 1\n- [x] **a** — 2\n<!-- register:end -->\n"
        )
        .is_err()
    );
}
