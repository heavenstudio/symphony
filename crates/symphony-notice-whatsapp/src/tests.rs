use super::*;
use serde_json::json;
use symphony_notice::{EMAIL, Kind, To};

const ID: &str = "0193f1c2-7a4e-7b3c-9d2e-5f6a7b8c9d0e";
const NUMBER: &str = "+15551234567";
const LINK: &str = "https://mac.example.ts.net/symphony/cards#notice-sender";
const TOKEN_TEXT: &str = "EAAG-test-token-not-a-real-one";

fn to(address: &str) -> Notice {
    Notice::new(
        ID,
        Kind::Card,
        "symphony",
        LINK,
        To::new(WHATSAPP, address).unwrap(),
    )
    .unwrap()
}

fn settings() -> Settings {
    Settings::read(
        "phone_number_id = \"1234567890\"\nlanguage = \"en_US\"\n\n[templates]\n\
         other = \"symphony_waits\"\ncard = \"symphony_card\"\n",
        Path::new("/m/notices"),
    )
    .unwrap()
}

/// The number's rule is `symphony_notice::ways::whatsapp`, whose own tests hold each case: these
/// hold that the program sends by it and says its refusal as its own.
#[test]
fn a_number_is_its_plus_and_its_digits() {
    for (address, written) in [(NUMBER, NUMBER), ("+1 (555) 123-4567", NUMBER)] {
        assert_eq!(number(&to(address)), Ok(written.to_string()), "{address}");
    }
}

#[test]
fn a_number_without_its_plus_or_of_another_shape_is_never_sent_and_never_said() {
    for (address, why) in [
        (
            "15551234567",
            "it does not start with `+` and its country code",
        ),
        (
            "+1.555.123.4567",
            "it holds more than digits, spaces, dashes and parentheses",
        ),
        ("+683400", "it has fewer than 7 digits or more than 15"),
    ] {
        let refused = number(&to(address));
        assert_eq!(
            refused,
            Err(Unsent::Never(format!(
                "the address is no WhatsApp number: {why}"
            ))),
            "{address}"
        );
    }
}

#[test]
fn a_notice_of_another_way_is_never_sent() {
    let by_mail = Notice::new(
        ID,
        Kind::Card,
        "symphony",
        LINK,
        To::new(EMAIL, "ana@example.com").unwrap(),
    )
    .unwrap();
    assert_eq!(
        number(&by_mail),
        Err(Unsent::Never(
            "the notice goes by `email`, and this program sends by `whatsapp`".into()
        ))
    );
}

#[test]
fn the_message_is_the_kinds_template_with_the_link_its_one_parameter() {
    assert_eq!(
        message(&to(NUMBER), &settings(), NUMBER),
        json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": NUMBER,
            "type": "template",
            "template": {
                "name": "symphony_card",
                "language": {"code": "en_US"},
                "components": [{
                    "type": "body",
                    "parameters": [{"type": "text", "text": LINK}],
                }],
            },
        })
    );
}

#[test]
fn a_kind_with_no_template_of_its_own_or_unknown_goes_by_others() {
    let line = |kind: &str| {
        format!(
            r#"{{"notice":1,"id":"{ID}","kind":"{kind}","project":"symphony","link":"{LINK}","to":{{"way":"whatsapp","address":"{NUMBER}"}}}}"#
        )
    };
    for kind in ["question", "verdict", "digest"] {
        let notice = Notice::read(line(kind)).unwrap();
        let sent = message(&notice, &settings(), NUMBER);
        assert_eq!(sent["template"]["name"], "symphony_waits", "{kind}");
        assert_eq!(
            sent["template"]["components"][0]["parameters"][0]["text"],
            LINK
        );
    }
}

/// What Meta answers of an error, with `code` and `message`.
fn refusal(code: u64, message: &str) -> String {
    json!({
        "error": {
            "message": message,
            "type": "OAuthException",
            "code": code,
            "error_data": {"messaging_product": "whatsapp", "details": "Details of it."},
            "fbtrace_id": "Az8or2yhqkZfEZ-_4Qn_Bam",
        }
    })
    .to_string()
}

#[test]
fn a_2xx_is_sent() {
    for status in [200, 201, 299] {
        assert_eq!(answered(status, "{\"messages\":[]}", &[]), Ok(()));
    }
    assert_eq!(answered(200, "not even JSON", &[]), Ok(()));
}

#[test]
fn a_refusal_meta_says_to_try_later_is_later() {
    for code in LATER {
        let said = answered(400, &refusal(code, "(#130429) Rate limit hit"), &[]);
        assert_eq!(
            said,
            Err(Unsent::Later(format!(
                "WhatsApp refused the notice: HTTP 400, code {code}: (#130429) Rate limit hit Details of it."
            )))
        );
    }
    for (status, body) in [
        (429, ""),
        (500, "<html>Bad gateway</html>"),
        (503, "{}"),
        (502, &refusal(131_026, "Message undeliverable")),
    ] {
        let Err(Unsent::Later(why)) = answered(status, body, &[]) else {
            panic!("{status} is later");
        };
        assert!(
            why.starts_with(&format!("WhatsApp refused the notice: HTTP {status}")),
            "{why}"
        );
    }
    assert_eq!(
        answered(503, "{}", &[]),
        Err(Unsent::Later(
            "WhatsApp refused the notice: HTTP 503".into()
        ))
    );
}

#[test]
fn any_other_refusal_is_never_with_metas_code_and_words() {
    for (status, code) in [
        (401, 190),
        (400, 100),
        (400, 131_026),
        (400, 131_042),
        (400, 131_047),
        (400, 131_048),
        (400, 132_001),
        (403, 200),
        (400, 1),
    ] {
        let said = answered(status, &refusal(code, "(#0) Its words"), &[]);
        assert_eq!(
            said,
            Err(Unsent::Never(format!(
                "WhatsApp refused the notice: HTTP {status}, code {code}: (#0) Its words Details of it."
            )))
        );
    }
    assert_eq!(
        answered(404, "<html>Not found</html>", &[]),
        Err(Unsent::Never(
            "WhatsApp refused the notice: HTTP 404".into()
        ))
    );
    let words_alone = json!({"error": {"message": "No code"}}).to_string();
    assert_eq!(
        answered(400, &words_alone, &[]),
        Err(Unsent::Never(
            "WhatsApp refused the notice: HTTP 400: No code".into()
        ))
    );
}

#[test]
fn what_meta_says_has_the_token_the_number_and_the_link_taken_out() {
    let hidden = [TOKEN_TEXT, "+1 (555) 123-4567", NUMBER, "15551234567", LINK];
    let words = format!(
        "(#131026) Undeliverable to {NUMBER} (wa_id 15551234567, written +1 (555) 123-4567) \
         at {LINK} with {TOKEN_TEXT}"
    );
    let Err(Unsent::Never(why)) = answered(400, &refusal(131_026, &words), &hidden) else {
        panic!("never");
    };
    assert_eq!(
        why,
        "WhatsApp refused the notice: HTTP 400, code 131026: (#131026) Undeliverable to [hidden] \
         (wa_id [hidden], written [hidden]) at [hidden] with [hidden] Details of it."
    );
}

#[test]
fn metas_words_are_one_line_of_at_most_300_characters() {
    assert_eq!(clean("one\ntwo\tthree\u{7}", &[]), "one two three ");
    assert_eq!(clean(&"é".repeat(300), &[]), "é".repeat(300));
    assert_eq!(clean(&"é".repeat(301), &[]), "é".repeat(300) + "…");
    assert_eq!(clean("a secret", &["", "secret"]), "a [hidden]");
    assert_eq!(clean("15551234567", &["5", "15551234567"]), "[hidden]");
}
