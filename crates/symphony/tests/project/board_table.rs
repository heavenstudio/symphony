//! `symphony.toml`'s `[board]` table: the guide, what a review starts, the cap on an
//! orchestrator's open tasks, when the pipeline alarm raises, the cap on the evidence the sweep
//! sets aside and the folder of the machine's PostgreSQL, each key's default and what it refuses.

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

#[test]
fn the_board_table_takes_the_hours_a_build_no_longer_run_holds_a_move_as_the_writer_rule_reads_them()
 {
    // The writer rule reads the bound itself; the project takes what it takes and refuses what it
    // refuses, the rule reading anything it refuses as its default.
    let tree = super::Tree::new();
    let root = tree.root.clone();
    for (text, hours) in [
        ("", symphony::machine::writers::UNUSED_HOURS),
        ("[board]\nstale_writer_hours = 48\n", 48),
        ("[board]\nstale_writer_hours = 1\n", 1),
    ] {
        assert!(parsed(text).is_ok(), "{text}");
        std::fs::write(root.join("symphony.toml"), text).unwrap();
        assert_eq!(
            symphony::machine::writers::Runs::of(&root).hours,
            hours,
            "{text}"
        );
    }
    for value in ["0", "-1", "1.5", "\"24\""] {
        let text = format!("[board]\nstale_writer_hours = {value}\n");
        assert!(refused(&text).contains("stale_writer_hours"), "{text}");
    }
    assert!(
        refused("[board]\nstale_writer_hours = 0\n")
            .contains("[board] stale_writer_hours is 0; it is a whole number of hours over 0")
    );
}

#[test]
fn the_board_table_takes_the_folder_of_the_machines_postgresql_as_the_board_reads_it() {
    // The board reads the folder itself; the project takes what it takes and refuses what it
    // refuses: a folder of the machine's, so absolute, never one inside the project.
    let named = "[board]\npostgresql = \"/opt/homebrew/opt/postgresql@18/bin\"\n";
    for (text, folder) in [
        ("", None),
        (named, Some("/opt/homebrew/opt/postgresql@18/bin")),
    ] {
        assert!(parsed(text).is_ok(), "{text}");
        let root = std::env::temp_dir().join(format!(
            "symphony-postgresql-{}-{}",
            std::process::id(),
            text.len()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("symphony.toml"), text).unwrap();
        assert_eq!(
            symphony_board::server::postgres::programs(&root),
            Ok(folder.map(std::path::PathBuf::from)),
            "{text}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
    assert!(refused("[board]\npostgresql = \"pg/bin\"\n").contains(
        "[board] postgresql is \"pg/bin\"; it is the absolute folder of the machine's \
             PostgreSQL programs"
    ));
    assert!(refused("[board]\npostgresql = 18\n").contains("postgresql"));
}

#[test]
fn the_board_table_takes_a_hosted_boards_postgresql_as_the_board_reads_it() {
    // The board reads the table itself; the project takes what it takes and refuses what it
    // refuses: references to secrets and a budget within the role's cap, never a credential.
    let written = "[board.external_postgresql]\nurl_env = \"BOARD_URL\"\nca_file_env = \"BOARD_CA\"\n\
        class_env = \"BOARD_CLASS\"\n[board.external_postgresql.budget]\nrole_connection_cap = 8\n\
        maintenance = 1\nheadroom = 2\n[board.external_postgresql.budget.classes]\n\
        web = { processes = 2, pool_max = 2 }\n";
    assert!(parsed(written).is_ok());
    let root = std::env::temp_dir().join(format!("symphony-external-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("symphony.toml"), written).unwrap();
    let read = symphony_board::pg::external::of(&root).unwrap().unwrap();
    assert_eq!(read.url_env.as_deref(), Some("BOARD_URL"));
    std::fs::remove_dir_all(&root).unwrap();
    for (broken, why) in [
        (written.replace("cap = 8", "cap = 6"), "role_connection_cap"),
        (written.replace("url_env = \"BOARD_URL\"\n", ""), "url_env"),
        (written.replace("url_env", "url"), "url"),
        (
            written.replace("headroom = 2\n", "headroom = 2\npassword = \"x\"\n"),
            "password",
        ),
    ] {
        assert!(
            refused(&broken).contains(why),
            "{broken}: {}",
            refused(&broken)
        );
    }
    let unbudgeted = written
        .split("[board.external_postgresql.budget]")
        .next()
        .unwrap();
    assert!(
        refused(unbudgeted).contains("budget"),
        "{}",
        refused(unbudgeted)
    );
}
