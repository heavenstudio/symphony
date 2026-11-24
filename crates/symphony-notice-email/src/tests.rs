use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use symphony_notice::{Kind, To};

fn notice(way: &str, address: &str) -> Notice {
    let to = To::new(way, address).unwrap();
    Notice::new(
        "n-1",
        Kind::Card,
        "symphony",
        "https://h.example/symphony/",
        to,
    )
    .unwrap()
}

/// A sender whose machine folder holds settings for a relay on a loopback port nothing listens
/// on, running `curl` as `program`.
fn sender(name: &str, program: &str) -> Sender {
    let folder = std::env::temp_dir().join(format!(
        "symphony-notice-email-lib-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&folder);
    let notices = folder.join(settings::FOLDER);
    fs::create_dir_all(&notices).unwrap();
    fs::set_permissions(&notices, fs::Permissions::from_mode(0o700)).unwrap();
    let file = notices.join(settings::SETTINGS);
    fs::write(
        &file,
        "server = \"smtp://127.0.0.1:9\"\nfrom = \"s@example.com\"\n",
    )
    .unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    Sender {
        folder,
        curl: program.into(),
    }
}

fn never(sent: Result<(), Unsent>) -> String {
    match sent {
        Err(Unsent::Never(why)) => why,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_notice_by_another_way_is_never_sent_and_its_address_never_said() {
    let sender = sender("way", "no-such-curl");
    let why = never(sender.send(&notice("slack", "U024BE7LH"), SystemTime::now()));
    assert_eq!(
        why,
        "this plugin sends a notice by `email`, and this one is by `slack`"
    );
}

#[test]
fn an_address_that_is_no_plain_email_address_is_never_sent() {
    let sender = sender("address", "no-such-curl");
    let why = never(sender.send(&notice("email", "Ana <ana@example.com>"), SystemTime::now()));
    assert!(
        why.starts_with("the notice's address is no plain email address"),
        "{why}"
    );
    assert!(!why.contains("ana@"), "{why}");
}

#[test]
fn settings_that_do_not_read_are_never_sent_by() {
    let mut sender = sender("unset", "no-such-curl");
    sender.folder = sender.folder.join("elsewhere");
    let why = never(sender.send(&notice("email", "ana@example.com"), SystemTime::now()));
    assert!(why.contains("the email plugin is not set up"), "{why}");
}

#[test]
fn no_curl_says_to_install_it() {
    let sender = sender("no-curl", "/nonexistent/curl");
    let why = never(sender.send(&notice("email", "ana@example.com"), SystemTime::now()));
    assert!(why.contains("/nonexistent/curl is not on PATH"), "{why}");
}

#[test]
fn a_mail_server_nobody_answers_at_is_tried_again() {
    let sender = sender("refused", "curl");
    match sender.send(&notice("email", "ana@example.com"), SystemTime::now()) {
        Err(Unsent::Later(why)) => {
            assert!(why.contains("not reached"), "{why}");
            assert!(!why.contains("ana@") && !why.contains("h.example"), "{why}");
        }
        other => panic!("{other:?}"),
    }
}
