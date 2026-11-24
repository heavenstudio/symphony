use super::*;
use crate::scratch::{Scratch, mode};

/// A token as a test writes one: no real token is in the tree.
const TOKEN_TEXT: &str = "EAAG-test-token-not-a-real-one";

/// Settings that name every key.
const FULL: &str = r#"
phone_number_id = "1234567890"
language = "en_US"
token_file = "/srv/keys/whatsapp.token"
api = "https://graph.example.com/v26.0/"

[templates]
other = "symphony_waits"
card = "symphony_card"
question = "symphony_question"
verdict = "symphony_verdict"
"#;

/// Settings that name only the keys with no default.
const LEAST: &str = r#"
phone_number_id = "1234567890"
language = "pt_BR"

[templates]
other = "symphony_waits"
"#;

fn read(text: &str) -> Result<Settings, String> {
    Settings::read(text, Path::new("/m/notices"))
}

/// [`LEAST`] with the key `key` (`templates.NAME` one of its templates) given `value` in the place
/// of what it holds, or added.
fn with(key: &str, value: &str) -> Result<Settings, String> {
    let name = key.strip_prefix("templates.").unwrap_or(key);
    let mut lines: Vec<String> = LEAST.lines().map(str::to_string).collect();
    let line = format!("{name} = {value}");
    match lines
        .iter()
        .position(|l| l.starts_with(&format!("{name} =")))
    {
        Some(at) => lines[at] = line,
        None if key.starts_with("templates.") => lines.push(line),
        None => lines.insert(1, line),
    }
    read(&lines.join("\n"))
}

#[test]
fn the_folder_is_its_owners_alone() {
    let scratch = Scratch::new();
    let missing = folder(scratch.path()).unwrap_err();
    assert!(
        missing.starts_with(&scratch.path().join("notices").display().to_string())
            && missing.contains("NOTICE_WHATSAPP.md"),
        "{missing}"
    );

    fs::write(scratch.path().join("notices"), "").unwrap();
    assert!(
        folder(scratch.path())
            .unwrap_err()
            .ends_with("is not a folder")
    );
    fs::remove_file(scratch.path().join("notices")).unwrap();

    let notices = scratch.notices(&[]);
    assert_eq!(folder(scratch.path()).unwrap(), notices);
    for open in [0o750, 0o705, 0o770] {
        mode(&notices, open);
        let refused = folder(scratch.path()).unwrap_err();
        assert!(refused.ends_with("chmod 700 it"), "{open:o}: {refused}");
    }
}

#[test]
fn settings_are_read_from_their_file_its_owners_alone() {
    let scratch = Scratch::new();
    let notices = scratch.notices(&[]);
    let missing = Settings::load(&notices).unwrap_err();
    assert!(
        missing.starts_with(&notices.join("whatsapp.toml").display().to_string()),
        "{missing}"
    );

    let notices = scratch.notices(&[("whatsapp.toml", LEAST)]);
    let settings = Settings::load(&notices).unwrap();
    assert_eq!(settings.phone_number_id(), "1234567890");
    assert_eq!(settings.token_file, notices.join("whatsapp.token"));

    mode(&notices.join("whatsapp.toml"), 0o640);
    assert!(
        Settings::load(&notices)
            .unwrap_err()
            .ends_with("chmod 600 it")
    );

    fs::write(notices.join("whatsapp.toml"), "phone_number_id = 5").unwrap();
    mode(&notices.join("whatsapp.toml"), 0o600);
    let refused = Settings::load(&notices).unwrap_err();
    assert!(
        refused.starts_with(&format!(
            "{}: line 1: ",
            notices.join("whatsapp.toml").display()
        )),
        "{refused}"
    );
}

#[test]
fn every_key_is_read_and_the_api_loses_a_slash_at_its_end() {
    let settings = read(FULL).unwrap();
    assert_eq!(settings.phone_number_id(), "1234567890");
    assert_eq!(settings.language(), "en_US");
    assert_eq!(settings.api(), "https://graph.example.com/v26.0");
    assert_eq!(settings.token_file, Path::new("/srv/keys/whatsapp.token"));
    assert_eq!(settings.template(Kind::Card), "symphony_card");
    assert_eq!(settings.template(Kind::Question), "symphony_question");
    assert_eq!(settings.template(Kind::Verdict), "symphony_verdict");
    assert_eq!(settings.template(Kind::Other), "symphony_waits");
}

#[test]
fn the_api_and_the_token_file_have_defaults_and_a_kind_with_no_template_takes_others() {
    let settings = read(LEAST).unwrap();
    assert_eq!(settings.api(), "https://graph.facebook.com/v27.0");
    assert_eq!(settings.token_file, Path::new("/m/notices/whatsapp.token"));
    for kind in [Kind::Card, Kind::Question, Kind::Verdict, Kind::of("news")] {
        assert_eq!(settings.template(kind), "symphony_waits", "{kind:?}");
    }
    let card = with("templates.card", r#""symphony_card""#).unwrap();
    assert_eq!(card.template(Kind::Card), "symphony_card");
    assert_eq!(card.template(Kind::Verdict), "symphony_waits");
}

#[test]
fn a_key_it_does_not_know_or_a_file_that_does_not_read_is_refused_without_its_value() {
    let pasted = format!("{LEAST}\ntoken = \"{TOKEN_TEXT}\"\n");
    assert_eq!(
        read(&pasted).unwrap_err(),
        "line 8: a key this program does not know"
    );
    for broken in [
        format!("phone_number_id = \"{TOKEN_TEXT}\n"),
        format!("phone_number_id = {TOKEN_TEXT}\n"),
        format!("{TOKEN_TEXT}\n"),
        format!("phone_number_id = \"1\" \"{TOKEN_TEXT}\"\n"),
        format!("language = '''{TOKEN_TEXT}"),
        pasted,
    ] {
        let refused = read(&broken).unwrap_err();
        assert!(!refused.contains(TOKEN_TEXT), "{broken:?}: {refused}");
    }
    let unknown = with("templates.digest", r#""symphony_digest""#).unwrap_err();
    assert_eq!(unknown, "line 7: a key this program does not know");
}

#[test]
fn a_token_written_as_a_key_is_refused_by_its_line_and_never_said() {
    let head = "phone_number_id = \"1234567890\"\nlanguage = \"pt_BR\"\n";
    let other = "a key this program does not know";
    for (text, line) in [
        (format!("{TOKEN_TEXT} = 1\n{LEAST}"), 1),
        (
            format!("{head}{TOKEN_TEXT} = 1\n[templates]\nother = \"x\"\n"),
            3,
        ),
        (format!("{LEAST}{TOKEN_TEXT} = 1\n"), 7),
        (format!("{LEAST}\"{TOKEN_TEXT}\" = \"x\"\n"), 7),
        (
            format!("{head}[templates]\nother = \"x\"\n[{TOKEN_TEXT}]\n"),
            5,
        ),
        (
            format!("{head}{TOKEN_TEXT}.other = \"x\"\n[templates]\nother = \"x\"\n"),
            3,
        ),
        (
            format!("{head}templates.{TOKEN_TEXT} = \"x\"\ntemplates.other = \"x\"\n"),
            3,
        ),
        (
            format!("{head}templates = {{ other = \"x\", {TOKEN_TEXT} = 1 }}\n"),
            3,
        ),
    ] {
        let refused = read(&text).unwrap_err();
        assert_eq!(refused, format!("line {line}: {other}"), "{text}");
    }
}

#[test]
fn a_key_of_its_own_where_it_is_not_one_is_named() {
    let head = "phone_number_id = \"1234567890\"\nlanguage = \"pt_BR\"\n";
    for (text, line, key) in [
        (format!("card = \"symphony_card\"\n{LEAST}"), 1, "card"),
        (format!("{LEAST}language = \"en\"\n"), 7, "language"),
        (
            format!("{head}other = \"x\"\n[templates]\nother = \"x\"\n"),
            3,
            "other",
        ),
        (
            format!("{head}templates.api = \"x\"\ntemplates.other = \"x\"\n"),
            3,
            "api",
        ),
    ] {
        let refused = read(&text).unwrap_err();
        assert_eq!(
            refused,
            format!("line {line}: `{key}` is not a key where it is written"),
            "{text}"
        );
    }
}

#[test]
fn a_value_of_another_type_is_refused_by_its_key_and_never_said() {
    let token = format!("\"{TOKEN_TEXT}\"");
    let tokens = format!("[{token}]");
    let table = format!("{{ name = {token} }}");
    let head = "phone_number_id = \"1234567890\"\nlanguage = \"pt_BR\"\n";
    let cases = [
        (
            with("phone_number_id", "106540352242922"),
            2,
            "`phone_number_id`",
        ),
        (with("language", &tokens), 3, "`language`"),
        (with("api", "true"), 2, "`api`"),
        (with("token_file", &table), 2, "`token_file`"),
        (with("templates.other", "5"), 6, "`other`"),
        (with("templates.card", &tokens), 7, "`card`"),
        (
            read(&format!("{head}templates = {token}\n")),
            3,
            "`templates`",
        ),
        (
            read(&format!("{head}templates.other = 106540352242922\n")),
            3,
            "`templates.other`",
        ),
        (
            read(&format!(
                "{head}[templates]\nother = \"x\"\ncard = {table}\n"
            )),
            5,
            "`card`",
        ),
    ];
    for (read, line, key) in cases {
        let refused = read.unwrap_err();
        assert_eq!(
            refused,
            format!("line {line}: {key} holds a value of another type than it takes")
        );
    }
    // A key of its own made a table names a key that is not its own (`code`), so none is said.
    let text = format!("phone_number_id = \"1234567890\"\nlanguage.code = {token}\n");
    assert_eq!(
        read(&format!("{text}[templates]\nother = \"x\"\n")).unwrap_err(),
        "line 2: a value is of another type than its key takes"
    );
}

#[test]
fn a_bad_unicode_escape_is_said_as_the_syntax_error_it_is() {
    let refused = with("language", "\"\\uD800\"").unwrap_err();
    assert!(refused.starts_with("line 3: invalid value"), "{refused}");
    assert!(!refused.contains("another type"), "{refused}");
}

#[test]
fn a_key_with_no_default_is_needed() {
    for (key, missing) in [
        ("phone_number_id", "phone_number_id = \"1234567890\""),
        ("language", "language = \"pt_BR\""),
        ("other", "other = \"symphony_waits\""),
    ] {
        let refused = read(&LEAST.replace(missing, "")).unwrap_err();
        assert!(
            refused.contains(&format!("missing field `{key}`")),
            "{refused}"
        );
    }
    let refused = read("phone_number_id = \"1\"\nlanguage = \"en\"\n").unwrap_err();
    assert!(refused.contains("missing field `templates`"), "{refused}");
}

#[test]
fn each_value_is_checked_and_its_refusal_names_the_key() {
    for (key, value, refusal) in [
        (
            "phone_number_id",
            r#""""#,
            "`phone_number_id` is no phone number's id",
        ),
        (
            "phone_number_id",
            r#""12a4""#,
            "`phone_number_id` is no phone number's id",
        ),
        (
            "phone_number_id",
            r#""+1555""#,
            "`phone_number_id` is no phone number's id",
        ),
        (
            "phone_number_id",
            &format!("\"{}\"", "1".repeat(33)),
            "`phone_number_id`",
        ),
        (
            "language",
            r#""""#,
            "`language` is no template language's code",
        ),
        ("language", r#""e""#, "`language`"),
        ("language", r#""engl""#, "`language`"),
        ("language", r#""EN_us""#, "`language`"),
        ("language", r#""en_""#, "`language`"),
        ("language", r#""en_USAXX""#, "`language`"),
        ("language", r#""en-US""#, "`language`"),
        ("language", r#""en_US_x""#, "`language`"),
        (
            "templates.other",
            r#""""#,
            "`templates.other` is no template's name",
        ),
        ("templates.other", r#""Symphony""#, "`templates.other`"),
        ("templates.card", r#""symphony card""#, "`templates.card`"),
        (
            "templates.question",
            r#""symphony-question""#,
            "`templates.question`",
        ),
        (
            "templates.verdict",
            &format!("\"{}\"", "a".repeat(513)),
            "`templates.verdict`",
        ),
        (
            "api",
            r#""http://graph.facebook.com/v27.0""#,
            "`api` is no https URL",
        ),
        ("api", r#""https://""#, "`api`"),
        ("api", r#""https:///v27.0""#, "`api`"),
        ("api", r#""https://graph.facebook.com/v27.0?x=1""#, "`api`"),
        ("api", r#""https://graph.facebook.com/v27.0#x""#, "`api`"),
        ("api", r#""https://graph facebook.com""#, "`api`"),
        ("api", r#""https://graph.facebook.com:443/v27.0""#, "`api`"),
        ("api", r#""https://graph.facebook.com/v\"27""#, "`api`"),
        ("api", r#""http://127.0.0.1:/v27.0""#, "`api`"),
        ("api", r#""http://127.0.0.1:80a""#, "`api`"),
        ("api", r#""http://localhost:8080""#, "`api`"),
        ("api", r#""ftp://graph.facebook.com""#, "`api`"),
        (
            "token_file",
            &format!("\"{TOKEN_TEXT}\""),
            "`token_file` is no absolute path",
        ),
        (
            "token_file",
            r#""keys/whatsapp.token""#,
            "`token_file` is no absolute path",
        ),
    ] {
        let refused = with(key, value).unwrap_err();
        assert!(refused.starts_with(refusal), "{key} = {value}: {refused}");
        assert!(!refused.contains(TOKEN_TEXT), "{refused}");
    }
    let longest_id = format!("\"{}\"", "1".repeat(32));
    let longest_name = format!("\"{}\"", "a".repeat(512));
    for (key, value) in [
        ("phone_number_id", longest_id.as_str()),
        ("language", r#""en""#),
        ("language", r#""fil_PH""#),
        ("language", r#""zh_HK""#),
        ("language", r#""es_419""#),
        ("templates.other", longest_name.as_str()),
        ("templates.other", r#""symphony_2""#),
        ("api", r#""http://127.0.0.1:8080/v27.0""#),
        ("api", r#""http://127.0.0.1:8080""#),
    ] {
        assert!(with(key, value).is_ok(), "{key} = {value}");
    }
}

#[test]
fn the_token_is_read_from_its_file_its_owners_alone_and_never_said() {
    let scratch = Scratch::new();
    let notices = scratch.notices(&[("whatsapp.token", &format!("  {TOKEN_TEXT}\n"))]);
    let settings = Settings::read(LEAST, &notices).unwrap();
    let token = settings.token().unwrap();
    assert_eq!(token.expose(), TOKEN_TEXT);
    assert_eq!(format!("{token:?}"), "Secret(redacted)");
    assert_eq!(
        token.redact(&format!("Bearer {TOKEN_TEXT} refused, {TOKEN_TEXT}")),
        "Bearer [hidden] refused, [hidden]"
    );

    let file = notices.join("whatsapp.token");
    let named = file.display().to_string();
    mode(&file, 0o644);
    let refused = settings.token().unwrap_err();
    assert_eq!(
        refused,
        format!("{named} is open to others than its owner; chmod 600 it")
    );

    let more = "holds more than one token of printable characters";
    for (text, refusal) in [
        ("", "is empty"),
        (" \n\t", "is empty"),
        ("two tokens", more),
        (&format!("{TOKEN_TEXT} = 1"), more),
        ("one\nper line", more),
        ("quo\"te", more),
        ("back\\slash", more),
        ("caf\u{e9}", more),
        (&"x".repeat(4097), more),
    ] {
        fs::write(&file, text).unwrap();
        mode(&file, 0o600);
        let refused = settings.token().unwrap_err();
        assert_eq!(refused, format!("{named} {refusal}"), "{text:?}");
    }
    fs::write(&file, "x".repeat(4096)).unwrap();
    assert!(settings.token().is_ok());

    fs::write(&file, b"EAAG\xff\xfe").unwrap();
    let unreadable = settings.token().unwrap_err();
    assert!(
        unreadable.starts_with(&format!("{named}: ")) && !unreadable.contains("EAAG"),
        "{unreadable}"
    );

    fs::remove_file(&file).unwrap();
    let missing = settings.token().unwrap_err();
    assert!(missing.starts_with(&format!("{named}: ")), "{missing}");
    fs::create_dir(&file).unwrap();
    assert_eq!(
        settings.token().unwrap_err(),
        format!("{named} is not a file")
    );
    // A FIFO is opened without waiting for a writer, seen to be no file and never read.
    fs::remove_dir(&file).unwrap();
    let made = std::process::Command::new("mkfifo")
        .args(["-m", "600"])
        .arg(&file)
        .status()
        .unwrap();
    assert!(made.success());
    assert_eq!(
        settings.token().unwrap_err(),
        format!("{named} is not a file")
    );
}

#[test]
fn a_token_file_the_settings_name_is_held_to_the_same() {
    let scratch = Scratch::new();
    let file = scratch.path().join("meta.token");
    fs::write(&file, TOKEN_TEXT).unwrap();
    mode(&file, 0o604);
    let settings = with("token_file", &format!("\"{}\"", file.display())).unwrap();
    assert!(settings.token().unwrap_err().ends_with("chmod 600 it"));
    mode(&file, 0o400);
    assert_eq!(settings.token().unwrap().expose(), TOKEN_TEXT);
}

#[test]
fn a_link_in_the_place_of_the_folder_or_a_file_is_refused_unread() {
    // What each link leads to is kept as it should be: the link alone is refused.
    let elsewhere = Scratch::new();
    let kept = elsewhere.notices(&[("whatsapp.toml", LEAST), ("whatsapp.token", TOKEN_TEXT)]);
    let settings = Settings::load(&kept).unwrap();
    assert_eq!(settings.token().unwrap().expose(), TOKEN_TEXT);

    let scratch = Scratch::new();
    std::os::unix::fs::symlink(&kept, scratch.path().join("notices")).unwrap();
    assert_eq!(
        folder(scratch.path()).unwrap_err(),
        format!(
            "{} is a link; keep the folder itself there",
            scratch.path().join("notices").display()
        )
    );

    let scratch = Scratch::new();
    let notices = scratch.notices(&[]);
    for name in ["whatsapp.toml", "whatsapp.token"] {
        std::os::unix::fs::symlink(kept.join(name), notices.join(name)).unwrap();
    }
    let link = |name: &str| {
        format!(
            "{} is a link; keep the file itself there",
            notices.join(name).display()
        )
    };
    assert_eq!(Settings::load(&notices).unwrap_err(), link("whatsapp.toml"));
    let settings = Settings::read(LEAST, &notices).unwrap();
    let refused = settings.token().unwrap_err();
    assert_eq!(refused, link("whatsapp.token"));
    assert!(!refused.contains(TOKEN_TEXT));

    let named = scratch.path().join("meta.token");
    std::os::unix::fs::symlink(kept.join("whatsapp.token"), &named).unwrap();
    let settings = with("token_file", &format!("\"{}\"", named.display())).unwrap();
    assert_eq!(
        settings.token().unwrap_err(),
        format!("{} is a link; keep the file itself there", named.display())
    );
}
