//! The cards a branch posted and the owner's approve of them: how a card's lines split into the
//! owner's and the rest, what a commit adding the owner's lines may change of a document, and,
//! on scratch repositories, where the approve stands for the `owner` stamp and where it does not.

use super::*;
use std::fs;
use std::path::PathBuf;

const DOC: &str = "docs/DECISIONS.md";

fn raw(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| line.to_string()).collect()
}

#[test]
fn a_cards_lines_split_into_the_owners_entries_and_the_rest() {
    let (owner, rest) = split(&raw(&[
        "  [2026-11-20 04:00] agent — Posted for the task.",
        "      the agent's next line",
        "",
        "  [2026-11-20 05:00] owner · approved — wider",
        "      the owner's next line",
        "",
        "      a paragraph of the owner's",
        "  <!-- review:unseen-owner a b -->",
        "      still the owner's",
        "  [2026-11-20 05:10] owner — a message",
        "  ↳ an agent's undated answer",
        "      no dated line above: not a continuation",
    ]));
    assert_eq!(
        owner,
        [
            "  [2026-11-20 05:00] owner · approved — wider\n      the owner's next line\n      \
             a paragraph of the owner's\n      still the owner's",
            "  [2026-11-20 05:10] owner — a message",
        ]
    );
    assert_eq!(
        rest,
        [
            "  [2026-11-20 04:00] agent — Posted for the task.",
            "      the agent's next line",
            "  <!-- review:unseen-owner a b -->",
            "  ↳ an agent's undated answer",
            "      no dated line above: not a continuation",
        ]
    );
}

fn card(lines: &[&str]) -> Card {
    cards(&raw(lines)).remove(0)
}

#[test]
fn the_newest_verdict_is_the_latest_by_its_time_and_the_last_written_of_a_minute() {
    let newest = |lines: &[&str]| newest(&card(lines)).and_then(|said| said.entry.verdict);
    let head = "- [ ] **layout** — Wider?";
    assert_eq!(newest(&[head]), None);
    assert_eq!(
        newest(&[head, "  [2026-11-20 05:00] owner — a message, no verdict"]),
        None
    );
    assert_eq!(
        newest(&[
            head,
            "  [2026-11-20 05:00] agent · auto-approved — a: decided"
        ]),
        None,
        "an agent's verdict is no owner's"
    );
    assert_eq!(
        newest(&[
            head,
            "  [2026-11-20 06:00] owner · rejected — no",
            "  [2026-11-20 05:00] owner · approved — written after, given before",
        ])
        .as_deref(),
        Some("rejected")
    );
    assert_eq!(
        newest(&[
            head,
            "  [2026-11-20 05:00] owner · question — why?",
            "  [2026-11-20 05:00] owner · approved — then yes",
            "  [2026-11-20 05:01] owner — thanks, no verdict",
        ])
        .as_deref(),
        Some("approved")
    );
    assert_eq!(
        newest(&[head, "  [2026-11-20 05:00] ana · approved — yes"]),
        None,
        "a person's verdict a board named is no owner's"
    );
    assert_eq!(
        newest(&[
            head,
            "  [2026-11-20 05:00] owner · approved — yes",
            "  [2026-11-20 06:00] ana · rejected — no",
        ])
        .as_deref(),
        Some("approved")
    );
}

#[test]
fn a_card_is_decided_when_the_newest_verdict_of_every_persons_is_an_approve() {
    let decided = |lines: &[&str]| decided(&card(lines));
    let head = "- [ ] **layout** — Wider?";
    let owners = "  [2026-11-20 05:00] owner · approved — yes";
    assert!(!decided(&[head]));
    assert!(decided(&[head, owners]));
    assert!(decided(&[
        head,
        "  [2026-11-20 05:00] ana · approved — yes"
    ]));
    assert!(decided(&[
        head,
        owners,
        "  [2026-11-20 06:00] ana · approved — and yes"
    ]));
    for later in ["rejected", "question", "pending", "dismissed"] {
        let hers = format!("  [2026-11-20 06:00] ana · {later} — on second thought");
        assert!(!decided(&[head, owners, &hers]), "{later}");
    }
    assert!(
        decided(&[
            head,
            owners,
            "  [2026-11-20 06:00] agent · rejected — an agent's"
        ]),
        "an agent decides no card"
    );
}

/// A register with its cards under the group headings a save writes.
fn register(groups: &[(&str, &str)]) -> String {
    let mut text = String::from("# Decisions\n\nIntro.\n\n<!-- register:start -->\n");
    for (group, cards) in groups {
        text.push_str(&format!("## {group}\n\n### General\n\n{cards}\n"));
    }
    text.push_str("<!-- register:end -->\n\nAfter.\n");
    text
}

const LAYOUT: &str = "- [ ] **layout** — Wider or narrower?\n  \
                      [2026-11-20 04:00] agent — Posted for the task.\n";
const OLD: &str = "- [x] **old** — An old choice.\n  [2026-11-01 09:00] owner · approved\n";
const APPROVED: &str = "- [x] **layout** — Wider or narrower?\n  \
                        [2026-11-20 04:00] agent — Posted for the task.\n  \
                        [2026-11-20 05:00] owner · approved — wider it is\n";

fn posted(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn a_save_of_the_owners_approve_adds_the_owners_lines_alone_its_regroup_and_mark_included() {
    let before = register(&[("Needs your decision", LAYOUT), ("Resolved", OLD)]);
    let saved = register(&[
        ("Answered: approve again or comment", APPROVED),
        ("Resolved", OLD),
    ]);
    let layout = posted(&["layout"]);
    let doc = |text: &str| Doc::of(text, &layout);
    assert!(doc(&before).adds_owners(&doc(&saved)));
    assert!(doc(&saved).adds_owners(&doc(&saved)), "nothing changed");
    // Taking the owner's line out again is no line added.
    assert!(!doc(&saved).adds_owners(&doc(&before)));
    // On a card the branch did not post, the owner's line and the mark are a change.
    let unposted = posted(&[]);
    assert!(!Doc::of(&before, &unposted).adds_owners(&Doc::of(&saved, &unposted)));
    let old_answered = register(&[
        ("Needs your decision", LAYOUT),
        (
            "Resolved",
            &format!("{OLD}  [2026-11-20 05:00] owner — and another thing\n"),
        ),
    ]);
    assert!(!doc(&before).adds_owners(&doc(&old_answered)));
    // The card's text, an agent's line, another line of the document, or a card more: changes.
    for changed in [
        saved.replace("Wider or narrower?", "Wider, narrower or both?"),
        saved.replace("[2026-11-20 05:00] owner", "[2026-11-20 05:00] owner — x\n  [2026-11-20 05:01] agent — noted\n  [2026-11-20 05:00] owner"),
        saved.replace("Intro.", "A new intro."),
        saved.replace("After.", "After, and more."),
        saved.replace(
            "<!-- register:end -->",
            "- [ ] **colour** — Which colour?\n<!-- register:end -->",
        ),
        saved.replace("### General", "### Another topic"),
        saved.replace("<!-- register:end -->", "Prose in the register.\n<!-- register:end -->"),
    ] {
        assert!(!doc(&before).adds_owners(&doc(&changed)), "{changed}");
    }
    // Blank lines and the register's headings are what a save writes again.
    let respaced = saved.replace("\n\n### General\n\n", "\n### General\n");
    assert!(doc(&before).adds_owners(&doc(&respaced)));
}

#[test]
fn kept_lines_are_counted_each_time_they_are_written() {
    let lines = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
    assert!(kept(&lines(&[]), &lines(&["a"])));
    assert!(kept(&lines(&["a", "b"]), &lines(&["b", "a", "c"])));
    assert!(!kept(&lines(&["a", "a"]), &lines(&["a", "b"])));
    assert!(!kept(&lines(&["a"]), &lines(&[])));
}

/// A scratch repository, removed when dropped: main holds a register with the card `old`.
struct Repo(PathBuf);

impl Repo {
    /// Each in a folder of its own, numbered in this process: two tests that start together
    /// never share one, as a clock's reading could.
    fn new() -> Repo {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "symphony-posted-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let repo = Repo(dir);
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit(&[
            (DOC, &register(&[("Resolved", OLD)])),
            ("src/lib.rs", "pub fn one() {}\n"),
            ("docs/NOTES.md", "# Notes\n"),
        ]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let done = crate::sys::output(
            crate::sys::command("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
                .arg(&self.0)
                .args(args),
        )
        .unwrap();
        assert!(done.status.success(), "git {args:?}: {done:?}");
        String::from_utf8(done.stdout).unwrap().trim().to_string()
    }

    fn write(&self, files: &[(&str, &str)]) {
        for (path, text) in files {
            let file = self.0.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, text).unwrap();
        }
    }

    /// `files` written and committed on the branch checked out: the commit.
    fn commit(&self, files: &[(&str, &str)]) -> String {
        self.write(files);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", "a change"]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// The branch `task` from main, posting `cards` beside a change of code: its tip.
    fn posting(&self, cards: &str) -> String {
        self.git(&["checkout", "-qb", "task", "main"]);
        self.commit(&[
            (
                DOC,
                &register(&[("Needs your decision", cards), ("Resolved", OLD)]),
            ),
            ("src/lib.rs", "pub fn one() {}\npub fn two() {}\n"),
        ])
    }

    /// The owner's save of `cards` on the branch's register, committed alone: the commit.
    fn saved(&self, group: &str, cards: &str) -> String {
        self.commit(&[(DOC, &register(&[(group, cards), ("Resolved", OLD)]))])
    }

    /// Main moves on, `file` to `text`, and the branch merges it, git's own merge: the merge.
    fn merges_main(&self, file: &str, text: &str) -> String {
        self.git(&["checkout", "-q", "main"]);
        self.commit(&[(file, text)]);
        self.git(&["checkout", "-q", "task"]);
        self.git(&["merge", "-q", "--no-edit", "main"]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn read(&self, tip: &str) -> Posted {
        read(&self.0, tip)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn approved(cards: &[&str]) -> Posted {
    Posted::Approved {
        cards: cards.iter().map(|card| card.to_string()).collect(),
    }
}

fn changed(cards: &[&str], commit: &str) -> Posted {
    Posted::Changed {
        cards: cards.iter().map(|card| card.to_string()).collect(),
        commit: commit.to_string(),
    }
}

#[test]
fn the_owners_approve_of_the_one_card_posted_stands_with_its_line_and_a_merge_of_main_after_it() {
    let repo = Repo::new();
    let posted = repo.posting(LAYOUT);
    assert_eq!(repo.read(&posted), Posted::Nothing, "no verdict yet");
    let line = repo.saved("Answered: approve again or comment", APPROVED);
    assert_eq!(repo.read(&line), approved(&["layout"]));
    let merged = repo.merges_main("src/main.rs", "fn main() {}\n");
    assert_eq!(repo.read(&merged), approved(&["layout"]));
    // An owner's line more after it, on the posted card, is still the owner's alone.
    let thanks = format!("{APPROVED}  [2026-11-20 05:30] owner — thanks\n");
    let more = repo.saved("Answered: approve again or comment", &thanks);
    assert_eq!(repo.read(&more), approved(&["layout"]));
}

#[test]
fn a_default_branch_the_project_files_rule_refuses_names_no_branch_to_read_the_cards_on() {
    let repo = Repo::new();
    repo.posting(LAYOUT);
    let line = repo.saved("Answered: approve again or comment", APPROVED);
    let file = repo.0.join("symphony.toml");
    fs::write(&file, "[project]\ndefault_branch = \"main\"\n").unwrap();
    assert_eq!(repo.read(&line), approved(&["layout"]));
    fs::write(&file, "[project]\ndefault_branch = \" \"\n").unwrap();
    assert_eq!(repo.read(&line), Posted::Nothing);
}

#[test]
fn any_other_commit_after_the_approve_keeps_it_from_standing_naming_the_commit() {
    for after in [
        ("src/lib.rs", "pub fn three() {}\n"),
        ("docs/NOTES.md", "# Notes\n\nMore.\n"),
        // A document added, which no owner's line is.
        ("docs/NEW.md", "# New\n"),
    ] {
        let repo = Repo::new();
        repo.posting(LAYOUT);
        repo.saved("Resolved", APPROVED);
        let later = repo.commit(&[after]);
        assert_eq!(repo.read(&later), changed(&["layout"], &later), "{after:?}");
        // A merge of main on top changes nothing of that.
        let merged = repo.merges_main("src/main.rs", "fn main() {}\n");
        assert_eq!(repo.read(&merged), changed(&["layout"], &later));
    }
    // The card's own text, rewritten after the owner approved it.
    let repo = Repo::new();
    repo.posting(LAYOUT);
    repo.saved("Resolved", APPROVED);
    let rewritten = repo.saved(
        "Resolved",
        &APPROVED.replace("narrower?", "narrower, or both?"),
    );
    assert_eq!(repo.read(&rewritten), changed(&["layout"], &rewritten));
}

#[test]
fn the_owners_line_committed_with_other_work_is_no_line_alone() {
    let repo = Repo::new();
    repo.posting(LAYOUT);
    repo.write(&[("src/lib.rs", "pub fn three() {}\n")]);
    let both = repo.saved("Resolved", APPROVED);
    assert_eq!(repo.read(&both), changed(&["layout"], &both));
}

#[test]
fn a_merge_that_is_not_gits_own_merge_of_main_keeps_it_from_standing() {
    // A merge of main changed by hand.
    let repo = Repo::new();
    repo.posting(LAYOUT);
    repo.saved("Resolved", APPROVED);
    repo.git(&["checkout", "-q", "main"]);
    repo.commit(&[("src/main.rs", "fn main() {}\n")]);
    repo.git(&["checkout", "-q", "task"]);
    repo.git(&["merge", "-q", "--no-commit", "main"]);
    let by_hand = repo.commit(&[("src/lib.rs", "pub fn by_hand() {}\n")]);
    assert_eq!(repo.read(&by_hand), changed(&["layout"], &by_hand));
    // A merge of a branch the default branch does not hold.
    let repo = Repo::new();
    repo.posting(LAYOUT);
    repo.saved("Resolved", APPROVED);
    repo.git(&["checkout", "-qb", "other", "main"]);
    repo.commit(&[("src/other.rs", "\n")]);
    repo.git(&["checkout", "-q", "task"]);
    repo.git(&["merge", "-q", "--no-edit", "other"]);
    let other = repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(repo.read(&other), changed(&["layout"], &other));
    // A merge of more than two parents, though each the default branch holds: two heads,
    // neither holding the other (git reduces one the other holds to a merge of two).
    let repo = Repo::new();
    repo.posting(LAYOUT);
    repo.saved("Resolved", APPROVED);
    for (head, file) in [("first", "src/first.rs"), ("second", "src/second.rs")] {
        repo.git(&["checkout", "-qb", head, "main"]);
        repo.commit(&[(file, "\n")]);
    }
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-edit", "first", "second"]);
    repo.git(&["checkout", "-q", "task"]);
    repo.git(&["merge", "-q", "--no-edit", "first", "second"]);
    let octopus = repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(repo.read(&octopus), changed(&["layout"], &octopus));
}

#[test]
fn a_reject_a_question_pending_dismissed_or_no_verdict_is_no_approve() {
    for verdict in ["rejected", "question", "pending", "dismissed"] {
        let repo = Repo::new();
        repo.posting(LAYOUT);
        let card = APPROVED.replace("approved", verdict);
        let tip = repo.saved("Resolved", &card);
        assert_eq!(repo.read(&tip), Posted::Nothing, "{verdict}");
    }
    // A message with no verdict leaves the card with none.
    let repo = Repo::new();
    repo.posting(LAYOUT);
    let tip = repo.saved(
        "Needs your decision",
        &format!("{LAYOUT}  [2026-11-20 05:00] owner — why wider?\n"),
    );
    assert_eq!(repo.read(&tip), Posted::Nothing);
    // An approve, then a reject: the newest decides.
    let tip = repo.saved(
        "Resolved",
        &format!("{APPROVED}  [2026-11-20 05:30] owner · rejected — on second thought\n"),
    );
    assert_eq!(repo.read(&tip), Posted::Nothing);
}

/// A person a board named approves as the owner does, and her verdict reads as hers on every
/// other page, but the stamp is the owner's alone until the board gives her the decide power: her
/// approve of a posted card does not meet it, the owner's does, her approve after it, committed
/// alone, is a word on the card that leaves it standing, and her reject after it opens the card
/// again, and the stamp with it.
#[test]
fn a_persons_approve_of_a_posted_card_is_no_owners_and_her_reject_after_the_owners_stops_it() {
    const GROUP: &str = "Answered: approve again or comment";
    let repo = Repo::new();
    repo.posting(LAYOUT);
    let hers = format!("{LAYOUT}  [2026-11-20 05:00] ana · approved — wider it is\n");
    let tip = repo.saved(GROUP, &hers);
    assert_eq!(repo.read(&tip), Posted::Nothing, "her approve alone");
    let owners = format!("{hers}  [2026-11-20 05:10] owner · approved — wider\n");
    let tip = repo.saved(GROUP, &owners);
    assert_eq!(repo.read(&tip), approved(&["layout"]));
    let again = format!("{owners}  [2026-11-20 05:20] ana · approved — still wider\n");
    let tip = repo.saved(GROUP, &again);
    assert_eq!(repo.read(&tip), approved(&["layout"]));
    let rejected = format!("{again}  [2026-11-20 05:30] ana · rejected — on second thought\n");
    let tip = repo.saved(GROUP, &rejected);
    assert_eq!(repo.read(&tip), Posted::Nothing);
}

const COLOUR: &str = "- [ ] **colour** — Which colour?\n  \
                      [2026-11-20 04:10] agent — Posted for the task.\n";

#[test]
fn every_card_the_branch_posted_needs_the_approve_the_newest_deciding_where_it_stands() {
    let repo = Repo::new();
    repo.posting(&format!("{LAYOUT}{COLOUR}"));
    let one = repo.saved("Resolved", &format!("{APPROVED}{COLOUR}"));
    assert_eq!(repo.read(&one), Posted::Nothing, "colour has none");
    // Colour approved later: both stand from there.
    let colour = COLOUR.replace("[ ]", "[x]") + "  [2026-11-20 06:00] owner · approved — blue\n";
    let both = repo.saved("Resolved", &format!("{APPROVED}{colour}"));
    assert_eq!(repo.read(&both), approved(&["layout", "colour"]));
    // A change between the two approves: the newest came after it, so both stand.
    let repo = Repo::new();
    repo.posting(&format!("{LAYOUT}{COLOUR}"));
    repo.saved("Resolved", &format!("{APPROVED}{COLOUR}"));
    repo.commit(&[("src/lib.rs", "pub fn three() {}\n")]);
    let both = repo.saved("Resolved", &format!("{APPROVED}{colour}"));
    assert_eq!(repo.read(&both), approved(&["layout", "colour"]));
}

/// A document with no register: `card` inline under the paragraph that says the thing.
fn inline(card: &str, after: &str) -> String {
    format!("# Notes\n\n## Layout\n\nThe page is 900 wide.\n\n{card}\n{after}\n")
}

#[test]
fn an_inline_card_the_branch_posted_in_a_document_with_no_register_stands_as_a_registers_does() {
    const NOTES: &str = "docs/NOTES.md";
    let repo = Repo::new();
    repo.git(&["checkout", "-qb", "task", "main"]);
    let posted = repo.commit(&[
        (NOTES, &inline(LAYOUT, "Margins follow.")),
        ("src/lib.rs", "pub fn one() {}\npub fn two() {}\n"),
    ]);
    assert_eq!(repo.read(&posted), Posted::Nothing, "no verdict yet");
    // The owner's approve, saved from the card's page: its line and its mark, nothing else.
    let line = repo.commit(&[(NOTES, &inline(APPROVED, "Margins follow."))]);
    assert_eq!(repo.read(&line), approved(&["layout"]));
    let merged = repo.merges_main("src/main.rs", "fn main() {}\n");
    assert_eq!(repo.read(&merged), approved(&["layout"]));
    // Prose beside the card changed after the approve: the owner approved another page.
    let later = repo.commit(&[(NOTES, &inline(APPROVED, "Margins follow, and gutters."))]);
    assert_eq!(repo.read(&later), changed(&["layout"], &later));
}

#[test]
fn an_inline_card_main_held_is_none_the_branch_posted_where_it_stays_or_where_it_moves() {
    const NOTES: &str = "docs/NOTES.md";
    let repo = Repo::new();
    repo.commit(&[(NOTES, &inline(LAYOUT, "Margins follow."))]);
    repo.git(&["checkout", "-qb", "task", "main"]);
    // Answered where main held it.
    let answered = repo.commit(&[(NOTES, &inline(APPROVED, "Margins follow."))]);
    assert_eq!(repo.read(&answered), Posted::Nothing);
    // Moved, word for word, into another document with no register.
    let moved = repo.commit(&[
        (NOTES, "# Notes\n\nMargins follow.\n"),
        ("docs/LAYOUT.md", &inline(APPROVED, "")),
    ]);
    assert_eq!(repo.read(&moved), Posted::Nothing);
}

#[test]
fn an_approve_on_a_card_main_held_is_no_card_the_branch_posted() {
    let repo = Repo::new();
    repo.git(&["checkout", "-qb", "task", "main"]);
    repo.commit(&[("src/lib.rs", "pub fn two() {}\n")]);
    let old = format!("{OLD}  [2026-11-20 05:00] owner · approved — still\n");
    let tip = repo.commit(&[(DOC, &register(&[("Resolved", &old)]))]);
    assert_eq!(repo.read(&tip), Posted::Nothing);
}

#[test]
fn an_owners_line_no_commit_carries_counts_for_nothing() {
    let repo = Repo::new();
    let posted = repo.posting(LAYOUT);
    repo.write(&[(DOC, &register(&[("Resolved", APPROVED), ("Resolved", OLD)]))]);
    assert_eq!(repo.read(&posted), Posted::Nothing);
}

/// Once landed, a tip the default branch holds reads as it did before: the fork is taken from the
/// default branch as it was before the merge that brought it in, whatever main did since.
#[test]
fn a_tip_the_default_branch_merged_reads_the_cards_it_posted_as_before() {
    let repo = Repo::new();
    repo.posting(LAYOUT);
    let tip = repo.saved("Resolved", APPROVED);
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-ff", "--no-edit", "task"]);
    assert_eq!(repo.read(&tip), approved(&["layout"]));
    repo.commit(&[("src/main.rs", "fn main() {}\n")]);
    assert_eq!(repo.read(&tip), approved(&["layout"]));
    // A code commit after the approve reads the same once landed too.
    let repo = Repo::new();
    repo.posting(LAYOUT);
    repo.saved("Resolved", APPROVED);
    let later = repo.commit(&[("src/lib.rs", "pub fn three() {}\n")]);
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-ff", "--no-edit", "task"]);
    assert_eq!(repo.read(&later), changed(&["layout"], &later));
}

#[test]
fn a_tip_with_no_commit_of_its_own_or_no_repository_posted_nothing() {
    let repo = Repo::new();
    let main = repo.git(&["rev-parse", "main"]);
    assert_eq!(repo.read(&main), Posted::Nothing);
    assert_eq!(read(Path::new("/nowhere"), &main), Posted::Nothing);
}
