use super::*;

#[test]
fn a_run_is_titled_for_its_commit_as_the_workflow_names_it() {
    assert_eq!(title("abc"), "Release verification of abc");
    let workflow = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(PATH),
    )
    .unwrap();
    assert!(
        workflow.contains("run-name: Release verification of ${{ inputs.commit }}"),
        "the title a run is found and believed by is the workflow's run-name"
    );
    assert!(PATH.ends_with(&format!("/{WORKFLOW}")));
}

#[test]
fn the_seconds_between_two_of_githubs_times_are_none_where_one_does_not_read() {
    let (from, to) = (json!("2026-11-21T10:00:00Z"), json!("2026-11-21T10:01:30Z"));
    assert_eq!(between(&from, &to), Some(90));
    assert_eq!(between(&to, &from), Some(0), "never negative");
    assert_eq!(between(&Value::Null, &to), None);
    assert_eq!(between(&from, &json!("soon")), None);
}

#[test]
fn the_looks_for_a_started_run_take_two_minutes_in_all() {
    assert_eq!(SHOWS.iter().map(Duration::as_secs).sum::<u64>(), 120);
}
