use super::ways::*;
use super::{Kind, To};

/// Email addresses the rule takes.
fn plain_emails() -> Vec<String> {
    [
        "ana@example.com",
        "a@b",
        "first.last+tag@mail.example.co.uk",
        "!#$%&'*+-/=?^_`{|}~@example.com",
        "x@1-2.example",
    ]
    .map(String::from)
    .into_iter()
    .chain([
        format!("{}@example.com", "l".repeat(64)),
        format!("a@{}.example", "d".repeat(63)),
    ])
    .collect()
}

#[test]
fn a_plain_email_address_is_taken() {
    for taken in plain_emails() {
        assert!(email(&taken), "{taken}");
        assert_eq!(address(EMAIL, &taken), Ok(taken.clone()));
    }
}

#[test]
fn an_email_address_a_header_or_a_command_could_read_as_more_is_refused() {
    let long_local = format!("{}@example.com", "l".repeat(65));
    let long_label = format!("a@{}.example", "d".repeat(64));
    let long = format!("a@{}.example", vec!["d".repeat(60); 5].join("."));
    for refused in [
        "",
        "ana",
        "@example.com",
        "ana@",
        "ana@@example.com",
        "a@b@example.com",
        "\"ana\"@example.com",
        "ana smith@example.com",
        "Ana <ana@example.com>",
        "ana@example.com, bob@example.com",
        "ana@example.com;bob@example.com",
        "ana@example.com>",
        ".ana@example.com",
        "ana.@example.com",
        "an..a@example.com",
        "ana@example..com",
        "ana@.example.com",
        "ana@example.com.",
        "ana@-example.com",
        "ana@example-.com",
        "ana@exa_mple.com",
        "ana@[127.0.0.1]",
        "ana(comment)@example.com",
        "anä@example.com",
        "ana@exämple.com",
        " ana@example.com",
        "ana@example.com\n",
        &long_local,
        &long_label,
        &long,
    ] {
        assert!(!email(refused), "{refused:?}");
        assert_eq!(
            address(EMAIL, refused),
            Err(format!("the email address is no plain one: {EMAIL_RULE}")),
            "{refused:?}"
        );
    }
}

#[test]
fn a_slack_member_id_is_u_or_w_then_capitals_and_digits() {
    for taken in ["U024BE7LH", "W012A3CDE", "U1", "UABC"] {
        assert!(slack(taken), "{taken}");
        assert_eq!(address(SLACK, taken), Ok(taken.to_string()));
    }
    for refused in [
        "",
        "U",
        "W",
        "u024BE7LH",
        "U024be7lh",
        "U024 BE7LH",
        "U024-BE7LH",
        " U024BE7LH",
        "C024BE7LH",
        "G024BE7LH",
        "D024BE7LH",
        "@ana",
        "ana@example.com",
        "#general",
        "UÄBC",
    ] {
        assert!(!slack(refused), "{refused:?}");
        assert_eq!(
            address(SLACK, refused),
            Err(format!(
                "the Slack member id is none: {SLACK_RULE}, as U024BE7LH"
            )),
            "{refused:?}"
        );
    }
}

#[test]
fn a_whatsapp_number_is_its_plus_and_its_digits() {
    for (taken, held) in [
        ("+15551234567", "+15551234567"),
        ("+1 (555) 123-4567", "+15551234567"),
        ("+44 20 7946 0958", "+442079460958"),
        ("+6834002", "+6834002"),
        ("+123456789012345", "+123456789012345"),
    ] {
        assert_eq!(whatsapp(taken), Ok(held.to_string()), "{taken}");
        assert_eq!(address(WHATSAPP, taken), Ok(held.to_string()), "{taken}");
    }
}

#[test]
fn a_whatsapp_number_without_its_plus_or_of_another_shape_is_refused_by_its_rule() {
    let no_plus = "it does not start with `+` and its country code";
    let more = "it holds more than digits, spaces, dashes and parentheses";
    let count = "it has fewer than 7 digits or more than 15";
    for (refused, why) in [
        ("15551234567", no_plus),
        ("0015551234567", no_plus),
        ("(555) 123-4567", no_plus),
        (" +15551234567", no_plus),
        ("", no_plus),
        ("+1.555.123.4567", more),
        ("+1555123456x7", more),
        ("++15551234567", more),
        ("+1555/1234567", more),
        ("+15551234567\n", more),
        ("+", count),
        ("+ - ()", count),
        ("+683400", count),
        ("+1234567890123456", count),
    ] {
        assert_eq!(whatsapp(refused), Err(why), "{refused:?}");
        assert_eq!(
            address(WHATSAPP, refused),
            Err(format!("the WhatsApp number is none: {why}")),
            "{refused:?}"
        );
    }
}

#[test]
fn an_address_by_a_way_that_is_none_is_refused_without_saying_the_way() {
    for way in ["", "sms", "Email", "slack ", "U024BE7LH"] {
        let refused = address(way, "ana@example.com").unwrap_err();
        assert_eq!(
            refused,
            "an address is by one of email, slack, whatsapp, and that way is none of them"
        );
    }
}

#[test]
fn every_address_a_profile_holds_makes_a_notice() {
    let held = (plain_emails().into_iter().map(|text| (EMAIL, text)))
        .chain(["U024BE7LH", "W1"].map(|text| (SLACK, text.to_string())))
        .chain(["+1 (555) 123-4567", "+6834002"].map(|text| (WHATSAPP, text.to_string())));
    for (way, text) in held {
        let address = address(way, &text).unwrap();
        assert!(To::new(way, &address).is_ok(), "{way} {address}");
    }
}

#[test]
fn the_kinds_are_the_messages_kinds() {
    for kind in KINDS {
        assert_eq!(Kind::of(kind).word(), kind);
        assert_ne!(Kind::of(kind), Kind::Other, "{kind}");
    }
    // A kind added to `Kind` stops this match building until it is matched here, and so in KINDS
    // or not: a held message goes by the way chosen for questions, so a profile chooses it none.
    let made = |kind: Kind| match kind {
        Kind::Card | Kind::Question | Kind::Verdict => Some(kind.word()),
        Kind::Held | Kind::Other => None,
    };
    let made: Vec<&str> = [Kind::Card, Kind::Question, Kind::Verdict, Kind::Held, Kind::Other]
        .into_iter()
        .filter_map(made)
        .collect();
    assert_eq!(made, KINDS);
}
