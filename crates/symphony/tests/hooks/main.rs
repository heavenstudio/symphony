//! The shipped heartbeat, memory guard and project installer, through their real CLI and temporary
//! projects.
mod beat_fixture;
#[path = "../../../tasks/tests/store/common.rs"]
#[allow(dead_code)]
mod board;
mod board_beat;
mod card_homes;
mod common;
#[path = "../../../../tools/test-support/script_board.rs"]
mod fixture_script;
mod installed_skills;
mod installer;
mod lifetime;
mod named_recipes;
mod own_names;
mod owner_words;
mod recipe_variables;
mod recipes;
#[path = "../common/mod.rs"]
mod support;
mod tasks_beat;

fn scratch(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "symphony-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
