use super::*;
use crate::stand_in::{Slack, TOKEN, folder, keep, unanswered};
use serde_json::{Value, json};
use symphony_notice::{Kind, To};

const ADDRESS: &str = "U024BE7LH";
const LINK: &str = "https://mac.example.ts.net/symphony/cards#notice-sender";

fn notice(way: &str, address: &str) -> Notice {
    let to = To::new(way, address).unwrap();
    Notice::new("0193f1c2-7a4e", Kind::Card, "symphony", LINK, to).unwrap()
}

/// A machine folder with the token kept.
fn kept(name: &str) -> PathBuf {
    let machine = folder(name);
    keep(&machine, TOKEN);
    machine
}

/// What a test says of an unsent notice, which never names the address, the link or the token.
fn why(unsent: &Unsent) -> &str {
    let (Unsent::Later(why) | Unsent::Never(why)) = unsent;
    for secret in [ADDRESS, LINK, TOKEN, "mac.example.ts.net"] {
        assert!(!why.contains(secret), "{why} names {secret}");
    }
    why
}

#[test]
fn a_notice_is_sent_to_its_member_as_the_kept_tokens_app() {
    let slack = Slack::answering(200, r#"{"ok":true,"channel":"D024BE91L","ts":"1.2"}"#);
    let sent = send(&notice("slack", ADDRESS), Ok(kept("sent")), &slack.api);
    assert_eq!(sent, Ok(()));
    let heard = slack.heard();
    assert_eq!(heard.head[0], "POST /api/chat.postMessage HTTP/1.1");
    assert_eq!(
        heard.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    let body: Value = serde_json::from_str(&heard.body).unwrap();
    assert_eq!(
        body,
        json!({
            "channel": ADDRESS,
            "text": format!("A card waits on you in *symphony*. <{LINK}|Open it>"),
            "unfurl_links": false,
            "unfurl_media": false,
        })
    );
}

#[test]
fn what_slack_refuses_is_never_sent() {
    let slack = Slack::answering(200, r#"{"ok":false,"error":"user_not_found"}"#);
    let unsent = send(&notice("slack", ADDRESS), Ok(kept("refused")), &slack.api).unwrap_err();
    assert!(matches!(unsent, Unsent::Never(_)), "{unsent:?}");
    assert!(why(&unsent).starts_with("Slack refused it: user_not_found "));
    slack.heard();
}

#[test]
fn a_limited_or_unreached_slack_is_worth_handing_again() {
    let slack = Slack::answering(429, r#"{"ok":false,"error":"ratelimited"}"#);
    let unsent = send(&notice("slack", ADDRESS), Ok(kept("limited")), &slack.api).unwrap_err();
    assert!(matches!(unsent, Unsent::Later(_)), "{unsent:?}");
    why(&unsent);
    slack.heard();

    let unsent = send(
        &notice("slack", ADDRESS),
        Ok(kept("unreached")),
        &unanswered(),
    )
    .unwrap_err();
    assert!(matches!(unsent, Unsent::Later(_)), "{unsent:?}");
    why(&unsent);
}

#[test]
fn a_notice_of_another_way_is_refused_before_the_token_is_read() {
    let unsent = send(
        &notice("email", "ana@example.com"),
        Err("no folder".into()),
        &unanswered(),
    )
    .unwrap_err();
    assert_eq!(
        unsent,
        Unsent::Never("this program sends by slack, and the notice goes by email".into())
    );
    assert!(!why(&unsent).contains("ana@example.com"));
}

#[test]
fn an_address_that_is_no_member_id_is_refused_before_the_token_is_read() {
    for address in ["C024BE7LH", "@ana", "u024be7lh", "ana@example.com"] {
        let unsent = send(
            &notice("slack", address),
            Err("no folder".into()),
            &unanswered(),
        )
        .unwrap_err();
        assert_eq!(
            unsent,
            Unsent::Never(
                "the address is no Slack member id (U or W, then capital letters and digits)"
                    .into()
            ),
            "{address}"
        );
    }
}

#[test]
fn no_token_is_never_sent() {
    let unsent = send(
        &notice("slack", ADDRESS),
        Err("the machine folder needs HOME, an absolute folder".into()),
        &unanswered(),
    )
    .unwrap_err();
    assert_eq!(
        unsent,
        Unsent::Never(
            "no token was read: the machine folder needs HOME, an absolute folder".into()
        )
    );

    let unsent = send(
        &notice("slack", ADDRESS),
        Ok(folder("tokenless")),
        &unanswered(),
    )
    .unwrap_err();
    assert!(matches!(unsent, Unsent::Never(_)), "{unsent:?}");
    assert!(why(&unsent).starts_with("no token was read: "));
}
