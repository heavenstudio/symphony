//! `symphony.toml`'s `[board]` table: the guide, what a review starts, the cap on an
//! orchestrator's open tasks, when the pipeline alarm raises and the cap on the evidence the sweep
//! sets aside, each key's default and what it refuses.

use super::manifest::{parsed, refused};

#[test]
fn the_board_table_names_its_guide_and_what_a_review_starts() {
    let project = parsed(
        "[board]\nguide = \"docs/TASKS.md\"\non_review = [\"tools/rs-bin.sh\", \"land\", \"review\"]\n",
    )
    .unwrap();
    assert_eq!(project.board.guide(), "docs/TASKS.md");
    assert_eq!(
        project.board.on_review,
        ["tools/rs-bin.sh", "land", "review"]
    );
    let plain = parsed("").unwrap();
    assert_eq!(plain.board.guide(), symphony::project::Board::GUIDE);
    assert!(plain.board.on_review.is_empty());
}

#[test]
fn the_board_table_names_the_open_tasks_an_orchestrator_may_hold_and_none_caps_nothing() {
    assert_eq!(
        parsed("[board]\norchestrator_slots = 8\n")
            .unwrap()
            .board
            .orchestrator_slots,
        Some(8)
    );
    assert_eq!(parsed("").unwrap().board.orchestrator_slots, None);
    assert!(
        refused("[board]\norchestrator_slots = 0\n")
            .contains("orchestrator_slots is 0; an orchestrator needs at least one slot")
    );
    for text in [
        "[board]\norchestrator_slots = -1\n",
        "[board]\norchestrator_slots = \"8\"\n",
    ] {
        assert!(refused(text).contains("orchestrator_slots"), "{text}");
    }
}

#[test]
fn the_board_table_names_when_the_pipeline_alarm_raises_and_none_takes_the_board_s_default() {
    let board = parsed(
        "[board]\nreview_alarm_ratio = 2\nreview_alarm_hours = 4\nreview_alarm_clear_hours = 1\n",
    )
    .unwrap()
    .board;
    assert_eq!(
        (
            board.review_alarm_ratio,
            board.review_alarm_hours,
            board.review_alarm_clear_hours
        ),
        (Some(2), Some(4), Some(1))
    );
    let plain = parsed("").unwrap().board;
    assert_eq!(
        (
            plain.review_alarm_ratio,
            plain.review_alarm_hours,
            plain.review_alarm_clear_hours
        ),
        (None, None, None)
    );
    for key in [
        "review_alarm_ratio",
        "review_alarm_hours",
        "review_alarm_clear_hours",
    ] {
        assert!(
            refused(&format!("[board]\n{key} = 0\n"))
                .contains(&format!("[board] {key} is 0; it is at least 1")),
            "{key}"
        );
        for value in ["-1", "\"2\"", "1.5"] {
            let text = format!("[board]\n{key} = {value}\n");
            assert!(refused(&text).contains(key), "{text}");
        }
    }
}

#[test]
fn the_board_table_takes_the_cap_on_what_the_sweep_sets_aside_as_the_board_reads_it() {
    // The board reads the cap itself; the project takes what it takes and refuses what it refuses.
    for (text, gb) in [
        ("", symphony_board::sweep::kept::CAP_GB),
        ("[board]\nswept_cap_gb = 12\n", 12.0),
        ("[board]\nswept_cap_gb = 0.5\n", 0.5),
    ] {
        assert!(parsed(text).is_ok(), "{text}");
        let root = std::env::temp_dir().join(format!(
            "symphony-swept-cap-{}-{}",
            std::process::id(),
            text.len()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("symphony.toml"), text).unwrap();
        assert_eq!(symphony_board::sweep::kept::cap(&root), Ok(gb), "{text}");
        std::fs::remove_dir_all(&root).unwrap();
    }
    for value in ["0", "-1", "\"5\""] {
        let text = format!("[board]\nswept_cap_gb = {value}\n");
        assert!(refused(&text).contains("swept_cap_gb"), "{text}");
    }
    assert!(
        refused("[board]\nswept_cap_gb = 0\n")
            .contains("[board] swept_cap_gb is 0; it is a number of gigabytes over 0")
    );
}
