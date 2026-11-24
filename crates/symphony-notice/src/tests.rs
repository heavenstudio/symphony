use super::*;
use serde_json::json;
use std::cell::Cell;

const ADDRESS: &str = "ana@example.com";
const LINK: &str = "https://mac.example.ts.net/symphony/cards#notice-sender";

/// A notice of each field's usual value.
fn notice() -> Notice {
    Notice::new(
        "card-notice-sender-ana-email",
        Kind::Card,
        "symphony",
        LINK,
        To::new(EMAIL, ADDRESS).unwrap(),
    )
    .unwrap()
}

/// [`notice`] as the message carries it, for a test to change before it is read.
fn wire() -> Value {
    json!({
        "notice": 1,
        "id": "card-notice-sender-ana-email",
        "kind": "card",
        "project": "symphony",
        "link": LINK,
        "to": {"way": "email", "address": ADDRESS},
    })
}

/// What reading `message` refuses with.
fn refused(message: &Value) -> String {
    Notice::read(message.to_string()).unwrap_err()
}

/// [`notice`] with its id `id`.
fn with_id(id: &str) -> Result<Notice, String> {
    let n = notice();
    Notice::new(id, n.kind, &n.project, &n.link, n.to)
}

/// [`notice`] in the project `project`.
fn in_project(project: &str) -> Result<Notice, String> {
    let n = notice();
    Notice::new(&n.id, n.kind, project, &n.link, n.to)
}

/// [`notice`] about the page at `link`.
fn linking(link: &str) -> Result<Notice, String> {
    let n = notice();
    Notice::new(&n.id, n.kind, &n.project, link, n.to)
}

#[test]
fn the_message_is_one_json_line_of_format_1_in_the_written_order() {
    assert_eq!(
        notice().line(),
        format!(
            "{{\"notice\":1,\"id\":\"card-notice-sender-ana-email\",\"kind\":\"card\",\
             \"project\":\"symphony\",\"link\":\"{LINK}\",\"to\":{{\"way\":\"email\",\
             \"address\":\"{ADDRESS}\"}}}}\n"
        )
    );
    assert_eq!(notice().line().matches('\n').count(), 1);
}

#[test]
fn a_notice_written_reads_back_the_same() {
    let written = notice();
    let read = Notice::read(written.line()).unwrap();
    assert_eq!(read, written);
    assert_eq!(read.id(), "card-notice-sender-ana-email");
    assert_eq!(read.kind(), Kind::Card);
    assert_eq!(read.project(), "symphony");
    assert_eq!(read.link(), LINK);
    assert_eq!(read.to().way(), EMAIL);
    assert_eq!(read.to().address(), ADDRESS);
}

#[test]
fn a_message_reads_whatever_the_order_of_its_members_and_the_whitespace_around_it() {
    let reversed = format!(
        "\n  {{\"to\":{{\"address\":\"{ADDRESS}\",\"way\":\"email\"}},\"link\":\"{LINK}\",\
         \"project\":\"symphony\",\"kind\":\"card\",\"id\":\"card-notice-sender-ana-email\",\
         \"notice\":1}}\r\n\n"
    );
    assert_eq!(Notice::read(reversed).unwrap(), notice());
    let pretty = serde_json::to_string_pretty(&wire()).unwrap();
    assert_eq!(Notice::read(pretty).unwrap(), notice());
}

#[test]
fn a_field_the_reader_does_not_know_is_ignored_wherever_it_stands() {
    let mut message = wire();
    message["title"] = json!("Words a later format may carry");
    message["at"] = json!({"utc": "2026-11-24T10:00:00Z"});
    message["to"]["name"] = json!("Ana");
    assert_eq!(Notice::read(message.to_string()).unwrap(), notice());
}

#[test]
fn each_kind_is_written_and_read_by_its_word() {
    for (kind, word) in [
        (Kind::Card, "card"),
        (Kind::Question, "question"),
        (Kind::Verdict, "verdict"),
    ] {
        assert_eq!(kind.word(), word);
        let n = notice();
        let made = Notice::new(&n.id, kind, &n.project, &n.link, n.to).unwrap();
        assert!(
            made.line().contains(&format!("\"kind\":\"{word}\"")),
            "{word}"
        );
        let mut message = wire();
        message["kind"] = json!(word);
        assert_eq!(Notice::read(message.to_string()).unwrap().kind(), kind);
    }
    assert_eq!(Kind::Other.word(), "other");
}

#[test]
fn a_kind_the_reader_does_not_know_reads_as_other_and_no_notice_is_made_of_other() {
    let mut message = wire();
    message["kind"] = json!("release");
    let read = Notice::read(message.to_string()).unwrap();
    assert_eq!(read.kind(), Kind::Other);
    assert_eq!(read.link(), LINK);

    let n = notice();
    let other = Notice::new(&n.id, Kind::Other, &n.project, &n.link, n.to).unwrap_err();
    assert!(other.contains("`other`"), "{other}");
}

#[test]
fn a_message_of_another_format_is_refused_naming_both_formats() {
    let mut message = wire();
    message["notice"] = json!(2);
    message["kind"] = json!({"a": "shape format 1 cannot read"});
    let why = refused(&message);
    assert!(why.contains("format 2"), "{why}");
    assert!(why.contains("reads format 1"), "{why}");
}

#[test]
fn a_message_that_names_no_format_or_names_one_by_no_whole_number_is_refused() {
    let mut message = wire();
    message.as_object_mut().unwrap().remove("notice");
    let why = refused(&message);
    assert!(why.contains("names no format (`notice`)"), "{why}");

    for format in [json!("1"), json!(1.5), json!(-1), json!(null)] {
        let mut message = wire();
        message["notice"] = format.clone();
        let why = refused(&message);
        assert!(why.contains("where a whole number is"), "{format}: {why}");
    }
}

#[test]
fn what_is_no_json_object_is_refused() {
    for message in ["", "not json", "{\"notice\":1", "[1]", "\"notice\"", "1"] {
        let why = Notice::read(message).unwrap_err();
        assert!(
            why.starts_with("the message is no JSON"),
            "{message:?}: {why}"
        );
    }
    let mut message = wire().to_string();
    message.push_str("{}");
    assert!(Notice::read(message).is_err(), "two objects are no message");
    assert!(Notice::read(b"\xff\xfe").is_err(), "a message is UTF-8");
}

#[test]
fn a_field_missing_or_of_another_type_is_refused_by_its_name() {
    for field in ["id", "kind", "project", "link", "to"] {
        let mut message = wire();
        message.as_object_mut().unwrap().remove(field);
        let why = refused(&message);
        assert!(
            why.contains(&format!("missing field `{field}`")),
            "{field}: {why}"
        );
    }
    for field in ["way", "address"] {
        let mut message = wire();
        message["to"].as_object_mut().unwrap().remove(field);
        let why = refused(&message);
        assert!(
            why.contains(&format!("missing field `{field}`")),
            "{field}: {why}"
        );
    }
    let mut message = wire();
    message["id"] = json!(7);
    assert!(refused(&message).contains("refused"));
}

#[test]
fn an_id_is_1_to_128_ascii_letters_digits_dots_dashes_and_underscores() {
    for id in ["n", "N-1.a_b", &"x".repeat(ID_MOST), "0123456789"] {
        assert_eq!(with_id(id).unwrap().id(), id);
    }
    for id in [
        "",
        &"x".repeat(ID_MOST + 1),
        "a b",
        "a/b",
        "a:b",
        "é",
        "a\nb",
    ] {
        let why = with_id(id).unwrap_err();
        assert!(why.contains("the id"), "{id:?}: {why}");
    }
    let mut message = wire();
    message["id"] = json!("two words");
    assert!(refused(&message).contains("the id \"two words\""));
}

#[test]
fn a_project_is_a_project_id() {
    for project in ["symphony", "my-project-2", "a", "2"] {
        assert_eq!(in_project(project).unwrap().project(), project);
    }
    for project in ["", "Symphony", "-a", "a-", "a--b", "a_b", "a b", "a/b", "é"] {
        let why = in_project(project).unwrap_err();
        assert!(why.contains("no project id"), "{project:?}: {why}");
    }
}

#[test]
fn a_link_is_an_absolute_http_or_https_url_with_a_host_and_is_never_repeated() {
    for link in [
        "https://mac.example.ts.net/symphony/tasks/app/notice-message-format",
        "http://mac.example.ts.net:8765/symphony/cards",
        "https://h",
        "https://board.example.com/x?y=%20z#w",
    ] {
        assert_eq!(linking(link).unwrap().link(), link);
    }
    for link in [
        "",
        "mac.example.ts.net/symphony",
        "/symphony/cards",
        "ftp://mac.example.ts.net/x",
        "HTTPS://mac.example.ts.net/x",
        "https://",
        "https:///symphony",
        "https://?x",
        "https://#x",
        "https://mac.example.ts.net/a b",
        "https://mac.example.ts.net/\u{e9}",
        "https://mac.example.ts.net/x\n",
    ] {
        let why = linking(link).unwrap_err();
        assert!(why.contains("the link is no absolute"), "{link:?}: {why}");
        assert!(link.is_empty() || !why.contains(link), "{link:?}: {why}");
    }
}

#[test]
fn a_way_is_a_word_any_word() {
    for way in [EMAIL, SLACK, WHATSAPP, "ms-teams", "sms2"] {
        assert_eq!(To::new(way, ADDRESS).unwrap().way(), way);
    }
    for way in ["", "Email", "e mail", "e_mail", "-sms", "sms-"] {
        let why = To::new(way, ADDRESS).unwrap_err();
        assert!(why.contains("is no word"), "{way:?}: {why}");
    }
}

#[test]
fn an_address_is_kept_as_written_and_a_refusal_never_repeats_it() {
    for address in [
        ADDRESS,
        "U024BE7LH",
        "+15551234567",
        "Ana Lima <ana@example.com>",
    ] {
        assert_eq!(To::new(EMAIL, address).unwrap().address(), address);
    }
    for (address, said) in [
        ("", "is empty"),
        ("   ", "is empty"),
        (
            "ana@example.com\r\nBcc: eve@example.com",
            "control character",
        ),
        ("ana\t@example.com", "control character"),
        (" ana@example.com", "a space at an end"),
        ("ana@example.com ", "a space at an end"),
    ] {
        let why = To::new(EMAIL, address).unwrap_err();
        assert!(why.contains(said), "{address:?}: {why}");
        assert!(!why.contains("ana"), "{address:?}: {why}");
    }
    let mut message = wire();
    message["to"]["address"] = json!("ana@example.com\nBcc: eve@example.com");
    assert!(refused(&message).contains("control character"));
}

#[test]
fn a_notice_printed_shows_neither_its_link_nor_its_address() {
    let printed = format!("{:?}", notice());
    assert!(!printed.contains(ADDRESS), "{printed}");
    assert!(!printed.contains("mac.example"), "{printed}");
    assert!(
        printed.contains("card-notice-sender-ana-email"),
        "{printed}"
    );
    assert!(
        printed.contains("Card") && printed.contains("symphony"),
        "{printed}"
    );
    assert!(
        printed.contains("\"email\"") && printed.contains("<hidden>"),
        "{printed}"
    );
}

#[test]
fn the_exit_statuses_are_the_contracts() {
    assert_eq!((SENT, LATER, NEVER), (0, 75, 1));
    assert_eq!(FORMAT, 1);
}

/// What [`take_from`] answers for `message` and the `send` it is given, with what it said on
/// stderr and whether `send` was called.
fn take_message(
    message: &[u8],
    send: impl FnOnce(&Notice) -> Result<(), Unsent>,
) -> (u8, String, bool) {
    let called = Cell::new(false);
    let mut errors = Vec::new();
    let status = take_from(message, &mut errors, |n: &Notice| {
        called.set(true);
        send(n)
    });
    (status, String::from_utf8(errors).unwrap(), called.get())
}

#[test]
fn take_hands_the_notice_on_stdin_to_send_and_a_sent_one_exits_0_saying_nothing() {
    let line = notice().line();
    let (status, said, called) = take_message(line.as_bytes(), |n| {
        assert_eq!(n, &notice());
        Ok(())
    });
    assert_eq!((status, said.as_str(), called), (SENT, "", true));
}

#[test]
fn take_exits_75_for_a_notice_worth_handing_again_and_1_for_one_that_never_will_be_sent() {
    let line = notice().line();
    let (status, said, _) = take_message(line.as_bytes(), |_| {
        Err(Unsent::Later(
            "Slack answered 429: try again in 30 s".into(),
        ))
    });
    assert_eq!(
        (status, said.as_str()),
        (LATER, "Slack answered 429: try again in 30 s\n")
    );

    let (status, said, _) = take_message(line.as_bytes(), |_| {
        Err(Unsent::Never("Slack knows no such user".into()))
    });
    assert_eq!(
        (status, said.as_str()),
        (NEVER, "Slack knows no such user\n")
    );
}

#[test]
fn take_never_sends_a_message_it_cannot_read_and_says_why() {
    let mut message = wire();
    message["notice"] = json!(2);
    let (status, said, called) = take_message(message.to_string().as_bytes(), |_| Ok(()));
    assert_eq!((status, called), (NEVER, false));
    assert!(said.contains("format 2"), "{said}");

    let (status, said, called) = take_message(b"", |_| Ok(()));
    assert_eq!((status, called), (NEVER, false));
    assert!(said.starts_with("the message is no JSON"), "{said}");
}

/// A stdin whose read fails.
struct Broken;

impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("the pipe broke"))
    }
}

#[test]
fn take_exits_75_when_stdin_cannot_be_read_as_nothing_was_sent() {
    let mut errors = Vec::new();
    let status = take_from(Broken, &mut errors, |_| panic!("nothing was read to send"));
    assert_eq!(status, LATER);
    let said = String::from_utf8(errors).unwrap();
    assert!(
        said.contains("could not be read from stdin: the pipe broke"),
        "{said}"
    );
}
