use super::*;
use symphony_notice::{To, Unsent};

const LINK: &str = "https://mac.example.ts.net/symphony/cards#notice-sender";

fn notice(kind: Kind, link: &str) -> Notice {
    let to = To::new("slack", "U024BE7LH").unwrap();
    Notice::new("0193f1c2-7a4e", kind, "symphony", link, to).unwrap()
}

fn answer(status: u16, body: &str) -> Answer {
    Answer {
        status,
        body: body.into(),
    }
}

#[test]
fn a_member_id_is_u_or_w_then_capitals_and_digits() {
    for address in ["U024BE7LH", "W012A3CDE", "U1", "UABC"] {
        assert!(member(address), "{address}");
    }
    for address in [
        "",
        "U",
        "W",
        "u024BE7LH",
        "U024be7lh",
        "U024 BE7LH",
        "U024-BE7LH",
        "C024BE7LH",
        "G024BE7LH",
        "D024BE7LH",
        "@ana",
        "ana@example.com",
        "#general",
        "UÄBC",
    ] {
        assert!(!member(address), "{address:?}");
    }
}

#[test]
fn the_text_says_what_waits_in_which_project_and_links_it() {
    assert_eq!(
        text(&notice(Kind::Card, LINK)),
        format!("A card waits on you in *symphony*. <{LINK}|Open it>")
    );
    assert_eq!(
        text(&notice(Kind::Question, LINK)),
        format!("An agent's question waits on you in *symphony*. <{LINK}|Open it>")
    );
    assert_eq!(
        text(&notice(Kind::Verdict, LINK)),
        format!("A reviewer gave a verdict in *symphony*. <{LINK}|Open it>")
    );
}

#[test]
fn a_kind_this_build_does_not_know_is_sent_as_something_waiting() {
    let read = Notice::read(format!(
        r#"{{"notice":1,"id":"n-1","kind":"poll","project":"symphony","link":"{LINK}","to":{{"way":"slack","address":"U024BE7LH"}}}}"#
    ))
    .unwrap();
    assert_eq!(read.kind(), Kind::Other);
    assert_eq!(
        text(&read),
        format!("Something on the Symphony waits on you in *symphony*. <{LINK}|Open it>")
    );
}

#[test]
fn a_link_is_written_so_that_slacks_markup_neither_ends_nor_misreads_it() {
    let link = "https://h.example/p?a=1&b=<2>|c";
    assert_eq!(
        text(&notice(Kind::Card, link)),
        "A card waits on you in *symphony*. <https://h.example/p?a=1&amp;b=%3C2%3E%7Cc|Open it>"
    );
}

#[test]
fn the_body_posts_the_text_to_the_member_and_unfurls_nothing() {
    let body = body(&notice(Kind::Card, LINK));
    assert!(!body.contains('\n'), "{body}");
    assert!(body.starts_with('{'), "{body}");
    let read: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        read,
        json!({
            "channel": "U024BE7LH",
            "text": text(&notice(Kind::Card, LINK)),
            "unfurl_links": false,
            "unfurl_media": false,
        })
    );
}

#[test]
fn an_ok_answer_is_sent() {
    assert_eq!(
        answered(&answer(200, r#"{"ok":true,"channel":"D1","ts":"1.2"}"#)),
        Ok(())
    );
}

#[test]
fn a_limit_an_outage_or_a_slow_slack_is_worth_handing_again() {
    for status in [429, 500, 502, 503, 504] {
        let why = answered(&answer(status, "")).unwrap_err();
        assert!(matches!(why, Unsent::Later(_)), "{status}: {why:?}");
    }
    for word in LATER {
        let said = answered(&answer(200, &format!(r#"{{"ok":false,"error":"{word}"}}"#)));
        assert_eq!(
            said,
            Err(Unsent::Later(format!(
                "Slack did not send it this time: {word}"
            )))
        );
    }
}

#[test]
fn any_other_error_is_never_sent_and_named_by_its_word() {
    for word in [
        "channel_not_found",
        "user_not_found",
        "invalid_auth",
        "missing_scope",
        "not_allowed_token_type",
        "msg_too_long",
        "a_word_slack_adds_later",
    ] {
        let said = answered(&answer(200, &format!(r#"{{"ok":false,"error":"{word}"}}"#)));
        let Err(Unsent::Never(why)) = said else {
            panic!("{word}: {said:?}");
        };
        assert!(
            why.starts_with(&format!("Slack refused it: {word} ")),
            "{why}"
        );
        assert!(why.contains("docs/operations/NOTICE_SLACK.md"), "{why}");
    }
}

#[test]
fn an_error_that_is_no_plain_word_is_not_repeated() {
    let long = "a".repeat(65);
    for error in [
        r#""U024BE7LH is not here""#,
        r#""<https://evil.example>""#,
        r#""Channel_Not_Found""#,
        r#""""#,
        &format!(r#""{long}""#),
        "7",
        "null",
    ] {
        assert_eq!(
            answered(&answer(200, &format!(r#"{{"ok":false,"error":{error}}}"#))),
            Err(Unsent::Never("Slack refused it, naming no error".into())),
            "{error}"
        );
    }
    assert_eq!(
        answered(&answer(200, r#"{"ok":false}"#)),
        Err(Unsent::Never("Slack refused it, naming no error".into()))
    );
}

#[test]
fn another_status_is_never_sent() {
    for status in [301, 400, 401, 403, 404, 410] {
        assert_eq!(
            answered(&answer(status, r#"{"ok":true}"#)),
            Err(Unsent::Never(format!(
                "Slack refused the call: HTTP {status}"
            ))),
        );
    }
}

#[test]
fn a_success_that_says_neither_is_worth_handing_again() {
    for body in [
        "",
        "<html>a portal</html>",
        "[true]",
        r#"{"ok":"true"}"#,
        r#"{"sent":true}"#,
    ] {
        let said = answered(&answer(200, body));
        let Err(Unsent::Later(why)) = said else {
            panic!("{body:?}: {said:?}");
        };
        assert!(
            why.starts_with(
                "Slack's answer (HTTP 200) says neither that it sent nor that it did not: "
            ),
            "{why}"
        );
        assert!(!why.contains("portal"), "{why}");
    }
}
