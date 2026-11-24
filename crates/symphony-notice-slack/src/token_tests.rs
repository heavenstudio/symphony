use super::*;
use crate::stand_in::{TOKEN, folder, keep};

#[test]
fn a_secret_is_one_token_of_printable_characters() {
    assert_eq!(Secret::new("  xoxb-1-2\n").unwrap().expose(), "xoxb-1-2");
    for (text, why) in [
        ("", "it is empty"),
        (" \n", "it is empty"),
        (
            "xoxb-1 xoxb-2",
            "it holds more than one token of printable characters",
        ),
        (
            "xoxb-\"1",
            "it holds more than one token of printable characters",
        ),
        (
            "xoxb-\\1",
            "it holds more than one token of printable characters",
        ),
        (
            "xoxb-é",
            "it holds more than one token of printable characters",
        ),
    ] {
        assert_eq!(Secret::new(text).unwrap_err(), why, "{text:?}");
    }
    assert!(Secret::new(&"x".repeat(LONGEST)).is_ok());
    assert!(Secret::new(&"x".repeat(LONGEST + 1)).is_err());
}

#[test]
fn a_secret_is_never_printed() {
    let secret = Secret::new(TOKEN).unwrap();
    assert_eq!(format!("{secret:?}"), "Secret(redacted)");
    assert_eq!(
        secret.redact(&format!("a {TOKEN} and {TOKEN}.")),
        "a [redacted] and [redacted]."
    );
}

#[test]
fn the_kept_token_is_read() {
    let machine = folder("kept");
    keep(&machine, &format!("{TOKEN}\n"));
    assert_eq!(read(&machine).unwrap().expose(), TOKEN);
}

#[test]
fn no_kept_token_says_where_it_is_kept() {
    let machine = folder("none");
    let why = read(&machine).unwrap_err();
    let notices = machine.join("notices");
    assert!(
        why.starts_with(&format!("no token was read: {}: ", notices.display())),
        "{why}"
    );
    assert!(
        why.contains("notices/slack.token of the machine folder"),
        "{why}"
    );
    assert!(why.contains("docs/operations/NOTICE_SLACK.md"), "{why}");

    fs::create_dir(&notices).unwrap();
    fs::set_permissions(&notices, fs::Permissions::from_mode(0o700)).unwrap();
    let why = read(&machine).unwrap_err();
    assert!(
        why.starts_with(&format!(
            "no token was read: {}: ",
            notices.join("slack.token").display()
        )),
        "{why}"
    );
}

#[test]
fn a_folder_or_a_file_open_to_others_is_refused_unread() {
    let machine = folder("open");
    keep(&machine, TOKEN);
    let notices = machine.join("notices");
    let file = notices.join("slack.token");
    for mode in [0o750, 0o705, 0o755, 0o777] {
        fs::set_permissions(&notices, fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            read(&machine).unwrap_err(),
            format!(
                "no token was read: {} is open to others; chmod 700 it",
                notices.display()
            ),
            "{mode:o}"
        );
    }
    fs::set_permissions(&notices, fs::Permissions::from_mode(0o700)).unwrap();
    for mode in [0o640, 0o604, 0o644, 0o666] {
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
        let why = read(&machine).unwrap_err();
        assert_eq!(
            why,
            format!(
                "no token was read: {} is open to others; chmod 600 it",
                file.display()
            ),
            "{mode:o}"
        );
    }
    fs::set_permissions(&file, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(read(&machine).is_ok());
}

#[test]
fn a_folder_where_the_file_is_kept_or_a_file_where_the_folder_is_is_refused() {
    let machine = folder("kinds");
    fs::write(machine.join("notices"), TOKEN).unwrap();
    fs::set_permissions(machine.join("notices"), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        read(&machine).unwrap_err(),
        format!(
            "no token was read: {} is not a folder",
            machine.join("notices").display()
        )
    );

    let machine = folder("kinds-file");
    let file = machine.join("notices/slack.token");
    fs::create_dir_all(&file).unwrap();
    fs::set_permissions(machine.join("notices"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        read(&machine).unwrap_err(),
        format!("no token was read: {} is not a file", file.display())
    );
}

#[test]
fn a_file_its_owner_cannot_read_says_so() {
    let machine = folder("unreadable");
    keep(&machine, TOKEN);
    let file = machine.join("notices/slack.token");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        read(&machine).unwrap_err(),
        format!("no token was read: {}: permission denied", file.display())
    );
}

#[test]
fn a_file_that_is_no_token_is_refused_without_its_content() {
    let machine = folder("garbled");
    keep(&machine, &format!("{TOKEN} {TOKEN}"));
    let why = read(&machine).unwrap_err();
    assert_eq!(
        why,
        format!(
            "no token was read from {}: it holds more than one token of printable characters",
            machine.join("notices/slack.token").display()
        )
    );
    assert!(!why.contains(TOKEN), "{why}");

    keep(&machine, "\n");
    assert!(read(&machine).unwrap_err().ends_with(": it is empty"));
}

#[test]
fn a_token_written_as_settings_is_refused_without_its_content() {
    let machine = folder("as-settings");
    for written in [
        format!("{TOKEN} = 1\n"),
        format!("token = \"{TOKEN}\"\n"),
        format!("{TOKEN}\n{TOKEN}\n"),
    ] {
        keep(&machine, &written);
        let why = read(&machine).unwrap_err();
        assert!(
            why.ends_with(": it holds more than one token of printable characters"),
            "{why}"
        );
        assert!(!why.contains(TOKEN), "{written:?}: {why}");
    }
}

#[test]
fn a_link_in_the_place_of_the_folder_or_the_file_is_refused_unread() {
    // What the link leads to is a folder and a token kept as they should be: the link alone is
    // refused.
    let elsewhere = folder("link-target");
    keep(&elsewhere, TOKEN);
    let kept = elsewhere.join("notices");

    let machine = folder("linked-folder");
    std::os::unix::fs::symlink(&kept, machine.join("notices")).unwrap();
    let why = read(&machine).unwrap_err();
    assert_eq!(
        why,
        format!(
            "no token was read: {} is a link; keep the folder itself there",
            machine.join("notices").display()
        )
    );

    let machine = folder("linked-file");
    keep(&machine, "");
    let file = machine.join("notices/slack.token");
    fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(kept.join("slack.token"), &file).unwrap();
    let why = read(&machine).unwrap_err();
    assert_eq!(
        why,
        format!(
            "no token was read: {} is a link; keep the file itself there",
            file.display()
        )
    );
    assert!(!why.contains(TOKEN), "{why}");
    assert_eq!(read(&elsewhere).unwrap().expose(), TOKEN);
}
