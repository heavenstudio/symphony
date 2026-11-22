//! A task's file list and explicitly opened patch, in the real client on desktop and phone.
use super::{SHOWN, chrome, look_parity, site::Site};
use serde_json::{Value, json};
use symphony_api::{TASK_DIFF_FILE_PATH, TASK_DIFF_PATH};

const PAGE: &str = "/task/visual/in-progress";

fn site() -> Site {
    let site = crate::site();
    let content = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site.set_file(
        TASK_DIFF_PATH,
        include_bytes!("../../../../crates/symphony-api/fixtures/task-diff-branch.json").to_vec(),
        "application/json",
    );
    site.set_file(
        TASK_DIFF_FILE_PATH,
        include_bytes!("../../../../crates/symphony-api/fixtures/task-diff-patch.json").to_vec(),
        "application/json",
    );
    site
}

fn open(page: &mut super::Chrome, site: &Site) {
    page.open(&site.url(PAGE));
    assert!(page.wait_for(
        "document.getElementById('changes-visual/in-progress') !== null",
        SHOWN
    ));
    page.eval("document.getElementById('changes-visual/in-progress').scrollIntoView()");
    assert!(
        page.wait_for(
            "document.querySelectorAll('.task-diff details').length === 3",
            SHOWN
        ),
        "{:?}",
        page.eval("document.body.innerText")
    );
}

#[test]
fn files_load_before_hunks_and_only_the_opened_file_is_read_on_desktop_and_phone() {
    for width in [1280, 390] {
        let site = site();
        let Some(mut page) = chrome() else {
            panic!("Chrome is required for task diff acceptance")
        };
        page.viewport(width, 900, width < 400);
        open(&mut page, &site);

        assert!(
            site.requests()
                .iter()
                .all(|path| path != TASK_DIFF_FILE_PATH)
        );
        assert_eq!(
            page.eval("document.querySelector('.task-diff-patch') === null"),
            json!(true)
        );
        page.eval("document.querySelector('.task-diff summary').click()");
        assert!(page.wait_for(
            "document.querySelector('.task-diff-patch')?.textContent.includes('+hello 猫')",
            SHOWN
        ));
        let patches: Vec<String> = site
            .targets()
            .into_iter()
            .filter(|path| path.starts_with(TASK_DIFF_FILE_PATH))
            .collect();
        assert_eq!(patches.len(), 1);
        assert!(patches[0].contains("path=src%2Fnew.rs"));
        assert!(patches[0].contains("base=0123456789abcdef"));
        assert!(patches[0].contains("head=89abcdef01234567"));
        assert_eq!(
            page.eval("document.documentElement.scrollWidth <= innerWidth"),
            json!(true),
            "page overflow at {width}"
        );
        page.eval("document.querySelector('.task-diff summary').click(); document.querySelector('.task-diff summary').click()");
        assert!(page.wait_for("document.querySelector('.task-diff details')?.open", SHOWN));
        assert_eq!(
            site.requests()
                .iter()
                .filter(|path| *path == TASK_DIFF_FILE_PATH)
                .count(),
            1,
            "reopening keeps the immutable patch"
        );
    }
}

#[test]
fn a_failed_file_refresh_keeps_the_previous_list_and_says_when_it_is_retrying() {
    let site = site();
    let Some(mut page) = chrome() else {
        panic!("Chrome is required for task diff acceptance")
    };
    open(&mut page, &site);
    site.fail_route(TASK_DIFF_PATH, true);
    page.eval("document.querySelector('.task-diff > .row button').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-diff [role=status]')?.textContent.includes('Showing files from the last successful read')",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.querySelectorAll('.task-diff details').length"),
        json!(3)
    );
    site.hold_route(TASK_DIFF_PATH, true);
    site.fail_route(TASK_DIFF_PATH, false);
    page.eval("document.querySelector('.task-diff > .row button').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-diff > .row button')?.disabled && document.querySelector('.task-diff > .row button')?.textContent === 'Reading files…'",
        SHOWN
    ));
    let mut diff: Value = serde_json::from_str(include_str!(
        "../../../../crates/symphony-api/fixtures/task-diff-branch.json"
    ))
    .unwrap();
    diff["head"] = json!("c".repeat(40));
    site.set_json(TASK_DIFF_PATH, &diff);
    site.hold_route(TASK_DIFF_PATH, false);
    assert!(page.wait_for(
        "document.querySelector('.task-diff')?.textContent.includes('cccccccc') && document.querySelector('.task-diff [role=status]') === null && !document.querySelector('.task-diff > .row button').disabled",
        SHOWN
    ));
}

#[test]
fn patch_failure_retries_and_a_new_comparison_drops_the_previous_hunks() {
    let site = site();
    let Some(mut page) = chrome() else {
        panic!("Chrome is required for task diff acceptance")
    };
    site.fail_route(TASK_DIFF_FILE_PATH, true);
    open(&mut page, &site);
    page.eval("document.querySelector('.task-diff summary').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-diff details')?.textContent.includes('Retry patch')",
        SHOWN
    ));
    site.fail_route(TASK_DIFF_FILE_PATH, false);
    page.eval("document.querySelector('.task-diff details button').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-diff-patch')?.textContent.includes('+hello 猫')",
        SHOWN
    ));
    let mut diff: Value = serde_json::from_str(include_str!(
        "../../../../crates/symphony-api/fixtures/task-diff-branch.json"
    ))
    .unwrap();
    diff["source"] = json!("landing");
    diff["head"] = json!("c".repeat(40));
    diff["truncated"] = json!(true);
    diff["reason"] = json!("Showing at most 500 files and 256 KiB of file counts.");
    site.set_json(TASK_DIFF_PATH, &diff);
    page.eval("document.querySelector('.task-diff > .row button').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-diff')?.textContent.includes('Landing merge')",
        SHOWN
    ));
    assert_eq!(page.eval("document.querySelector('.task-diff-patch') === null && !document.querySelector('.task-diff details').open"), json!(true));
    assert!(
        page.eval("document.querySelector('.task-diff').textContent.includes('500 files')")
            .as_bool()
            .unwrap()
    );
    site.hold_route(TASK_DIFF_FILE_PATH, true);
    page.eval("document.querySelector('.task-diff summary').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-diff details')?.textContent.includes('Reading this file')",
        SHOWN
    ));
    diff["head"] = json!("d".repeat(40));
    site.set_json(TASK_DIFF_PATH, &diff);
    page.eval("document.querySelector('.task-diff > .row button').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-diff')?.textContent.includes('dddddddd')",
        SHOWN
    ));
    site.hold_route(TASK_DIFF_FILE_PATH, false);
    assert_eq!(page.eval("document.querySelector('.task-diff-patch') === null && !document.querySelector('.task-diff details').open"), json!(true), "an abandoned read cannot draw hunks in a newer comparison");
}
