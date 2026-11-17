use symphony_api::{TASK_DIFF_FILE_PATH, TASK_DIFF_PATH, TaskDiff, TaskDiffPatch, TaskDiffSource};

#[test]
fn task_diff_routes_and_source_words_are_stable() {
    assert_eq!(TASK_DIFF_PATH, "/api/task-diff");
    assert_eq!(TASK_DIFF_FILE_PATH, "/api/task-diff-file");
    for (source, word) in [
        (TaskDiffSource::Branch, "branch"),
        (TaskDiffSource::Landing, "landing"),
        (TaskDiffSource::Unavailable, "unavailable"),
    ] {
        let json = format!("\"{word}\"");
        assert_eq!(serde_json::to_string(&source).unwrap(), json);
        assert_eq!(
            serde_json::from_str::<TaskDiffSource>(&json).unwrap(),
            source
        );
    }
}

#[test]
fn summaries_round_trip_branch_landing_and_unavailable_with_explicit_nulls() {
    for (name, source, truncated) in [
        ("task-diff-branch.json", TaskDiffSource::Branch, false),
        ("task-diff-landing.json", TaskDiffSource::Landing, true),
        (
            "task-diff-unavailable.json",
            TaskDiffSource::Unavailable,
            false,
        ),
    ] {
        let (written, fixture) = super::round_trip::<TaskDiff>(name);
        assert_eq!(written, fixture, "{name}");
        let read: TaskDiff = serde_json::from_value(written).unwrap();
        assert_eq!((read.source, read.truncated), (source, truncated));
        if source == TaskDiffSource::Unavailable {
            assert_eq!((read.base, read.head), (None, None));
            assert!(read.files.is_empty());
            assert_eq!(
                read.reason.as_deref(),
                Some("Task branch is gone without a landing")
            );
        } else {
            assert_eq!(
                read.base.as_deref(),
                Some("0123456789abcdef0123456789abcdef01234567")
            );
            assert_eq!(
                read.head.as_deref(),
                Some("89abcdef0123456789abcdef0123456789abcdef")
            );
            assert_eq!(read.reason.is_some(), truncated);
        }
    }
}

#[test]
fn file_counts_distinguish_text_zeros_and_binary_nulls_and_keep_paths_as_text() {
    let read: TaskDiff = serde_json::from_str(&super::fixture("task-diff-branch.json")).unwrap();
    assert_eq!(read.files.len(), 3);
    assert_eq!(
        (read.files[0].added, read.files[0].removed),
        (Some(12), Some(0))
    );
    assert_eq!(
        (read.files[1].added, read.files[1].removed),
        (Some(0), Some(4))
    );
    assert_eq!((read.files[2].added, read.files[2].removed), (None, None));
    assert_eq!(read.files[2].path, "pictures/猫 \"new\"\t.png");
}

#[test]
fn patches_round_trip_the_comparison_path_and_text_with_either_truncation_state() {
    let (written, fixture) = super::round_trip::<TaskDiffPatch>("task-diff-patch.json");
    assert_eq!(written, fixture);
    let mut patch: TaskDiffPatch = serde_json::from_value(written).unwrap();
    let summary: TaskDiff = serde_json::from_str(&super::fixture("task-diff-branch.json")).unwrap();
    assert_eq!(Some(&patch.base), summary.base.as_ref());
    assert_eq!(Some(&patch.head), summary.head.as_ref());
    assert_eq!(patch.path, summary.files[0].path);
    assert_eq!(patch.patch, "@@ -0,0 +1 @@\n+hello 猫\n");
    assert!(!patch.truncated);
    patch.truncated = true;
    patch.patch.clear();
    let written = serde_json::to_value(&patch).unwrap();
    assert_eq!(written["truncated"], true);
    assert_eq!(written["patch"], "");
    assert_eq!(
        serde_json::from_value::<TaskDiffPatch>(written).unwrap(),
        patch
    );
}
