use super::*;
use std::time::Duration;
use symphony_notice::To;

/// 2026-11-24 06:28:00 UTC.
const NOVEMBER_24: u64 = 1_795_501_680;

fn notice(id: &str, kind: Kind) -> Notice {
    let to = To::new("email", "ana@example.com").unwrap();
    Notice::new(
        id,
        kind,
        "symphony",
        "https://mac.example.ts.net/symphony/cards#a",
        to,
    )
    .unwrap()
}

#[test]
fn a_mail_says_what_waits_in_which_project_and_the_link() {
    let now = UNIX_EPOCH + Duration::from_secs(NOVEMBER_24);
    let mail = Mail::of(
        &notice("0193f1c2-7a4e", Kind::Card),
        "symphony@mail.example.com",
        now,
    );
    assert_eq!(
        mail.headers,
        [
            "From: Symphony <symphony@mail.example.com>",
            "To: ana@example.com",
            "Subject: A card waits on you in symphony",
            "Date: Tue, 24 Nov 2026 06:28:00 +0000",
            "Message-ID: <0193f1c2-7a4e@mail.example.com>",
            "Auto-Submitted: auto-generated",
        ]
    );
    assert_eq!(
        mail.body,
        "A card waits on you in symphony:\r\n\r\nhttps://mac.example.ts.net/symphony/cards#a\r\n"
    );
    assert!(mail.body.is_ascii() && mail.headers.iter().all(|header| header.is_ascii()));
}

#[test]
fn each_kind_has_its_words_and_one_this_build_does_not_know_says_something_waits() {
    assert_eq!(sentence(Kind::Card, "p"), "A card waits on you in p");
    assert_eq!(
        sentence(Kind::Question, "p"),
        "A question waits on you in p"
    );
    assert_eq!(
        sentence(Kind::Verdict, "p"),
        "A reviewer gave a verdict in p"
    );
    assert_eq!(
        sentence(Kind::Held, "p"),
        "A person's words wait on you to pass on in p"
    );
    assert_eq!(sentence(Kind::Other, "p"), "Something waits on you in p");
    let read = Notice::read(
        r#"{"notice":1,"id":"n","kind":"poll","project":"p","link":"https://h/","to":{"way":"email","address":"a@b"}}"#,
    )
    .unwrap();
    let mail = Mail::of(&read, "s@b", UNIX_EPOCH);
    assert!(
        mail.headers
            .contains(&"Subject: Something waits on you in p".to_string())
    );
}

#[test]
fn a_message_id_is_a_dot_atom_whatever_dots_the_id_holds() {
    assert_eq!(message_id("abc", "example.com"), "<abc@example.com>");
    assert_eq!(message_id(".a..b.", "example.com"), "<=a==b=@example.com>");
    assert_ne!(message_id("a.b", "d"), message_id("a_b", "d"));
    assert_ne!(message_id("a.b", "d"), message_id("a-b", "d"));
}

#[test]
fn a_date_is_written_as_a_mail_writes_it() {
    assert_eq!(date(0), "Thu, 01 Jan 1970 00:00:00 +0000");
    assert_eq!(date(86_399), "Thu, 01 Jan 1970 23:59:59 +0000");
    assert_eq!(date(951_782_400), "Tue, 29 Feb 2000 00:00:00 +0000");
    assert_eq!(date(1_709_164_800), "Thu, 29 Feb 2024 00:00:00 +0000");
    assert_eq!(date(1_709_251_199), "Thu, 29 Feb 2024 23:59:59 +0000");
    assert_eq!(date(1_709_251_200), "Fri, 01 Mar 2024 00:00:00 +0000");
    assert_eq!(date(1_735_689_599), "Tue, 31 Dec 2024 23:59:59 +0000");
    assert_eq!(date(4_107_542_400), "Mon, 01 Mar 2100 00:00:00 +0000");
    assert_eq!(date(NOVEMBER_24), "Tue, 24 Nov 2026 06:28:00 +0000");
}

#[test]
fn a_clock_before_1970_dates_the_mail_1970() {
    let before = UNIX_EPOCH - Duration::from_secs(1);
    let mail = Mail::of(&notice("n", Kind::Verdict), "s@example.com", before);
    assert!(
        mail.headers
            .contains(&"Date: Thu, 01 Jan 1970 00:00:00 +0000".to_string())
    );
}
