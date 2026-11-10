//! Actual private shell helper controls; never enters source_hash/the launcher.
use super::tests::private_fixture;
use super::*;
fn shell_capture_fixture(c: &Capture, body: &str) -> std::process::Output {
    shell_capture_fixture_control(c, true, body)
}

fn shell_capture_fixture_control(c: &Capture, enabled: bool, body: &str) -> std::process::Output {
    // Execute only the actual private helper bodies, never source_hash/the launcher.
    let source = fs::read_to_string(super::super::root().join("tools/rs-bin.sh")).unwrap();
    let start = source
        .find("sf_input_dir=\"${SF_TASKS_INPUT_RECEIPT-}\"")
        .unwrap();
    let end = source[start..].find("\nhere=").unwrap() + start;
    let helpers = &source[start..end];
    let script = format!("set -euo pipefail\n{helpers}\n{body}");
    let mut command = super::super::process(Path::new("/bin/bash"));
    command.args(["-c", &script]).env_remove(CONTROL);
    if enabled {
        command.env(CONTROL, &c.dir);
    }
    symphony_board::sys::output(&mut command).unwrap()
}

const SHELL_PREP: &str = r#"
name=tasks
link_only=""
where_only=""
build_only=1
project_workspace=/project
sf_input_begin
sources=/project/a.rs
printf -v blobs '%040d' 0
hash="$blobs"
test -z "${SF_TASKS_INPUT_RECEIPT+x}"
/bin/sh -c 'test -z "${SF_TASKS_INPUT_RECEIPT+x}"'
"#;

#[test]
fn owned_shell_stages_only_memory_then_publishes_after_the_release_guard() {
    let c = private_fixture("input-shell-stage", Caller::Warm);
    let body = format!(
        r#"{SHELL_PREP}
sf_input_locked=1
sf_input_return_hash 0
test "$sf_pending_valid" = 1
test ! -e "$sf_input_dir/build-0"
test ! -e "$sf_input_dir/invalid"
sf_input_locked=""
sf_input_publish
test -f "$sf_input_dir/build-0"
test -z "$sf_pending_valid"
printf '%s' 'owned stdout'
printf '%s' 'owned stderr' >&2
"#
    );
    let output = shell_capture_fixture(&c, &body);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"owned stdout");
    assert_eq!(output.stderr, b"owned stderr");
    // No final selector record was supplied: a staged/published H alone is still Unknown.
    assert!(c.validate(None).is_err());
}

#[test]
fn owned_shell_overflow_or_unpublished_early_exit_never_makes_prior_data_authoritative() {
    let c = private_fixture("input-shell-overflow", Caller::Warm);
    let body = format!(
        r#"{SHELL_PREP}
sf_input_locked=1
printf -v sources '%*s' 131073 ''
sf_input_return_hash 1
test "$sf_input_invalid" = 1
test ! -e "$sf_input_dir/invalid"
test ! -e "$sf_input_dir/build-1"
sf_input_locked=""
sf_input_publish
test -f "$sf_input_dir/invalid"
"#
    );
    let output = shell_capture_fixture(&c, &body);
    assert!(output.status.success(), "{output:?}");
    assert!(c.validate(None).is_err());
    let early = private_fixture("input-shell-unpublished", Caller::Warm);
    let output = shell_capture_fixture(
        &early,
        &format!("{SHELL_PREP}\nsf_input_locked=1\nsf_input_return_hash 0\nexit 0\n"),
    );
    assert!(output.status.success(), "{output:?}");
    assert!(!early.dir.join("build-0").exists());
    assert!(early.validate(None).is_err());
}

#[test]
fn owned_shell_without_control_keeps_diagnostics_inactive_and_preserves_failure_status() {
    let c = private_fixture("input-shell-disabled", Caller::Warm);
    let body = format!(
        r#"{SHELL_PREP}
sf_input_locked=1
sf_input_stage 0
sf_input_publish
sf_input_final
printf '%s' 'owned stdout'
printf '%s' 'owned stderr' >&2
exit 7
"#
    );
    let output = shell_capture_fixture_control(&c, false, &body);
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"owned stdout");
    assert_eq!(output.stderr, b"owned stderr");
    assert_eq!(
        fs::read_dir(&c.dir).unwrap().count(),
        1,
        "only its owned marker"
    );
    assert!(c.validate(None).is_err());
}

#[test]
fn owned_shell_private_return_keeps_values_in_memory_and_legacy_key_stdout_unchanged() {
    let c = private_fixture("input-shell-return", Caller::Warm);
    let body = format!(
        r#"{SHELL_PREP}
sf_input_locked=1
sf_input_return_hash 0
test "$sf_input_return" = 0000000000000000
test "$sf_pending_sources" = "$sources"
test "$sf_pending_blobs" = "$blobs"
test "$sf_pending_full" = "$hash"
test ! -e "$sf_input_dir/build-0"
sf_input_active=""
legacy="$(sf_input_return_hash)"
test "$legacy" = 0000000000000000
"#
    );
    let output = shell_capture_fixture(&c, &body);
    assert!(output.status.success(), "{output:?}");
    assert_eq!((output.stdout, output.stderr), (vec![], vec![]));
}
