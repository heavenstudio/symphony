use super::*;
use crate::scratch::Scratch;

/// A machine folder of its own, with a `notices/` its owner alone reaches.
fn machine(name: &str) -> Scratch {
    let dir = Scratch::new(name);
    fs::create_dir_all(dir.join(FOLDER)).unwrap();
    fs::set_permissions(dir.join(FOLDER), fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn write(dir: &Path, name: &str, text: &str, mode: u32) {
    let path = dir.join(FOLDER).join(name);
    fs::write(&path, text).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
}

const SETTINGS_TEXT: &str =
    "server = \"smtps://smtp.example.com\"\nfrom = \"symphony@example.com\"\nuser = \"apikey\"\n";
const PASSWORD: &str = "se\"cr\\et pass";

fn refused(dir: &Path) -> String {
    let why = read(dir).unwrap_err();
    assert!(!why.contains("se\"cr"), "{why}");
    why
}

#[test]
fn settings_with_a_login_read_with_the_password_hidden() {
    let dir = machine("login");
    write(&dir, SETTINGS, SETTINGS_TEXT, 0o600);
    write(&dir, TOKEN, &format!("  {PASSWORD}\n"), 0o400);
    let settings = read(&dir).unwrap();
    assert_eq!(settings.server(), "smtps://smtp.example.com");
    assert_eq!(settings.tls(), Tls::Implicit);
    assert_eq!(settings.from(), "symphony@example.com");
    let (user, password) = settings.login().unwrap();
    assert_eq!((user, password.expose()), ("apikey", PASSWORD));
    let printed = format!("{settings:?}");
    assert!(
        printed.contains("Secret(hidden)") && !printed.contains("cr\\"),
        "{printed}"
    );
}

#[test]
fn a_relay_needs_no_login() {
    let dir = machine("relay");
    write(
        &dir,
        SETTINGS,
        "server = \"smtp://127.0.0.1:2525\"\nfrom = \"symphony@example.com\"\n",
        0o600,
    );
    let settings = read(&dir).unwrap();
    assert_eq!(settings.tls(), Tls::Plain);
    assert_eq!(settings.login(), None);
}

#[test]
fn smtp_off_the_loopback_needs_starttls_and_on_it_goes_as_written() {
    for (url, tls) in [
        ("smtps://smtp.example.com:465", Tls::Implicit),
        ("smtps://127.0.0.1:465", Tls::Implicit),
        ("smtp://smtp.example.com:587", Tls::Required),
        ("smtp://smtp.example.com", Tls::Required),
        ("smtp://10.0.0.1:25", Tls::Required),
        ("smtp://[2001:db8::1]:587", Tls::Required),
        ("smtp://localhost:25", Tls::Plain),
        ("smtp://LocalHost", Tls::Plain),
        ("smtp://127.0.0.1", Tls::Plain),
        ("smtp://[::1]:2525", Tls::Plain),
    ] {
        assert_eq!(server(url), Ok((url.to_string(), tls)), "{url}");
    }
}

#[test]
fn a_server_that_is_no_smtp_host_and_port_is_refused() {
    for url in [
        "",
        "smtp.example.com",
        "https://smtp.example.com",
        "smtp://",
        "smtps://:465",
        "smtp://user:pass@smtp.example.com",
        "smtp://smtp.example.com/ehlo.example.com",
        "smtp://smtp.example.com:587/",
        "smtp://smtp.example.com?x=1",
        "smtp://smtp.example.com:0",
        "smtp://smtp.example.com:65536",
        "smtp://smtp.example.com:+25",
        "smtp://smtp.example.com:",
        "smtp://smtp_example.com",
        "smtp://smtp.exämple.com",
        "smtp://[::1",
        "smtp://[]:25",
        "smtp://[::1]x",
        "smtp://[fe80::1%en0]:25",
        "smtp://smtp.example.com\n",
    ] {
        let why = server(url).unwrap_err();
        assert!(why.contains("`server` is no"), "{url:?}: {why}");
    }
}

#[test]
fn nothing_set_up_says_where_the_settings_go() {
    let dir = machine("absent");
    fs::remove_dir(dir.join(FOLDER)).unwrap();
    let why = refused(&dir);
    assert!(
        why.contains("not set up") && why.contains("notices/email.toml"),
        "{why}"
    );
    fs::create_dir(dir.join(FOLDER)).unwrap();
    fs::set_permissions(dir.join(FOLDER), fs::Permissions::from_mode(0o700)).unwrap();
    let why = refused(&dir);
    assert!(
        why.contains("not set up") && why.contains("NOTICE_EMAIL.md"),
        "{why}"
    );
}

#[test]
fn a_folder_or_a_file_others_reach_is_refused_unread() {
    let dir = machine("open");
    write(&dir, SETTINGS, SETTINGS_TEXT, 0o600);
    write(&dir, TOKEN, PASSWORD, 0o600);
    fs::set_permissions(dir.join(FOLDER), fs::Permissions::from_mode(0o750)).unwrap();
    assert!(refused(&dir).contains("open to others: chmod 700"));
    fs::set_permissions(dir.join(FOLDER), fs::Permissions::from_mode(0o700)).unwrap();
    write(&dir, SETTINGS, SETTINGS_TEXT, 0o644);
    let why = refused(&dir);
    assert!(
        why.contains("email.toml is readable by others: chmod 600"),
        "{why}"
    );
    write(&dir, SETTINGS, SETTINGS_TEXT, 0o600);
    write(&dir, TOKEN, PASSWORD, 0o640);
    let why = refused(&dir);
    assert!(
        why.contains("email.token is readable by others: chmod 600"),
        "{why}"
    );
}

#[test]
fn a_file_its_owner_cannot_read_is_said_by_why() {
    let dir = machine("unreadable");
    write(&dir, SETTINGS, SETTINGS_TEXT, 0o000);
    let settings = dir.join(FOLDER).join(SETTINGS);
    assert_eq!(
        refused(&dir),
        format!("{}: permission denied", settings.display())
    );
}

#[test]
fn a_fifo_in_the_place_of_a_file_is_refused_without_waiting_on_it() {
    let dir = machine("fifo");
    let settings = dir.join(FOLDER).join(SETTINGS);
    let made = std::process::Command::new("mkfifo")
        .args(["-m", "600"])
        .arg(&settings)
        .status()
        .unwrap();
    assert!(made.success());
    // Opened without waiting for a writer, it is seen to be no file and never read.
    assert_eq!(
        refused(&dir),
        format!("{} is not a plain file", settings.display())
    );
}

#[test]
fn a_link_in_the_place_of_the_folder_or_a_file_is_refused() {
    // What each link leads to is kept as it should be: the link alone is refused.
    let elsewhere = machine("link-target");
    write(&elsewhere, SETTINGS, SETTINGS_TEXT, 0o600);
    write(&elsewhere, TOKEN, PASSWORD, 0o600);
    assert!(read(&elsewhere).is_ok());
    let kept = elsewhere.join(FOLDER);

    for name in [SETTINGS, TOKEN] {
        let dir = machine("linked-file");
        write(&dir, SETTINGS, SETTINGS_TEXT, 0o600);
        write(&dir, TOKEN, PASSWORD, 0o600);
        let file = dir.join(FOLDER).join(name);
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(kept.join(name), &file).unwrap();
        assert_eq!(
            refused(&dir),
            format!("{} is a link: keep the file itself there", file.display())
        );
    }

    let dir = machine("linked-folder");
    fs::remove_dir(dir.join(FOLDER)).unwrap();
    std::os::unix::fs::symlink(&kept, dir.join(FOLDER)).unwrap();
    assert_eq!(
        refused(&dir),
        format!(
            "{} is a link: keep the folder itself there",
            dir.join(FOLDER).display()
        )
    );
    fs::remove_file(dir.join(FOLDER)).unwrap();
    fs::write(dir.join(FOLDER), "").unwrap();
    assert!(refused(&dir).ends_with("notices is not a folder"));
}

/// Settings of every key, each on a line of its own: the server on line 1, `from` on 2 and `user`
/// on 3.
const LINES: [&str; 3] = [
    "server = \"smtps://smtp.example.com\"",
    "from = \"symphony@example.com\"",
    "user = \"apikey\"",
];

/// A token as one written by mistake into the settings would be: no real token is in the tree.
const MISTAKE: &str = "SG-test-key-not-a-real-one";

/// What the settings `text` are refused with, past the file's path, which never holds the
/// mistake.
fn unread_by(text: &str) -> String {
    let dir = machine("unread");
    write(&dir, SETTINGS, text, 0o600);
    write(&dir, TOKEN, PASSWORD, 0o600);
    let why = refused(&dir);
    assert!(!why.contains(MISTAKE), "{text:?}: {why}");
    let prefix = format!("{}: ", dir.join(FOLDER).join(SETTINGS).display());
    why.strip_prefix(&prefix)
        .unwrap_or_else(|| panic!("{why}"))
        .to_string()
}

/// [`LINES`] with `line` added after the first `at`.
fn with_line(at: usize, line: &str) -> String {
    let mut lines = LINES.to_vec();
    lines.insert(at, line);
    lines.join("\n") + "\n"
}

#[test]
fn a_key_the_settings_do_not_have_is_refused_by_its_line_never_its_text() {
    let other = "a key this program does not know";
    for (text, line) in [
        (with_line(3, &format!("{MISTAKE} = 1")), 4),
        (with_line(1, &format!("{MISTAKE} = \"x\"")), 2),
        (with_line(3, &format!("password = \"{MISTAKE}\"")), 4),
        (with_line(3, &format!("\"{MISTAKE}\" = 1")), 4),
        (with_line(3, &format!("[{MISTAKE}]")), 4),
        (with_line(3, &format!("login.{MISTAKE} = 1")), 4),
    ] {
        assert_eq!(unread_by(&text), format!("line {line}: {other}"), "{text}");
    }
    // The settings have no table: one of their keys under a table is refused by the table's line,
    // and in an inline table by its own key's.
    assert_eq!(
        unread_by(&with_line(3, "[login]\nuser = \"apikey\"")),
        "line 4: a key this program does not know"
    );
    assert_eq!(
        unread_by(
            &LINES
                .join("\n")
                .replace(LINES[2], "user = { user = \"apikey\" }")
        ),
        "line 3: `user` holds a value of another type than it takes",
    );
}

#[test]
fn a_value_of_another_type_is_refused_by_its_key_and_never_said() {
    let token = format!("\"{MISTAKE}\"");
    for (text, line, key) in [
        (
            LINES
                .join("\n")
                .replace(&token_of(LINES[0]), "106540352242922"),
            1,
            "server",
        ),
        (
            LINES
                .join("\n")
                .replace(&token_of(LINES[1]), &format!("[{token}]")),
            2,
            "from",
        ),
        (
            LINES
                .join("\n")
                .replace(&token_of(LINES[2]), &format!("{{ k = {token} }}")),
            3,
            "user",
        ),
        (
            LINES
                .join("\n")
                .replace(&token_of(LINES[2]), "1979-05-27T07:32:00Z"),
            3,
            "user",
        ),
        (
            LINES.join("\n").replace(&token_of(LINES[2]), "true"),
            3,
            "user",
        ),
    ] {
        assert_eq!(
            unread_by(&text),
            format!("line {line}: `{key}` holds a value of another type than it takes"),
            "{text}"
        );
    }
    // A key of its own made a table names a key that is not its own (`host`), so none is said.
    assert_eq!(
        unread_by(
            &LINES
                .join("\n")
                .replace(LINES[0], "server.host = \"smtp.example.com\"")
        ),
        "line 1: a value is of another type than its key takes"
    );
}

#[test]
fn a_bad_unicode_escape_is_said_as_the_syntax_error_it_is() {
    let text = LINES.join("\n").replace(&token_of(LINES[2]), "\"\\uD800\"");
    let why = unread_by(&text);
    assert!(why.starts_with("line 3: invalid value"), "{why}");
    assert!(!why.contains("another type"), "{why}");
}

/// The value `line` gives its key.
fn token_of(line: &str) -> String {
    line.split_once(" = ").unwrap().1.to_string()
}

#[test]
fn settings_that_do_not_read_name_the_line_and_never_its_text() {
    assert_eq!(unread_by(LINES[0]), "line 1: missing field `from`");
    for (text, line) in [
        (format!("server = \"{MISTAKE}\nfrom = 1\n"), 1),
        (format!("{MISTAKE}\n"), 1),
        (with_line(1, &format!("server = \"x\" \"{MISTAKE}\"")), 2),
        (format!("server = '''{MISTAKE}"), 1),
        (with_line(3, &format!("server = \"{MISTAKE}\"")), 4),
    ] {
        let why = unread_by(&text);
        assert!(
            why.starts_with(&format!("line {line}: ")),
            "{text:?}: {why}"
        );
    }
}

#[test]
fn a_from_that_is_no_address_is_refused() {
    let dir = machine("from");
    write(
        &dir,
        SETTINGS,
        "server = \"smtps://smtp.example.com\"\nfrom = \"Symphony <symphony@example.com>\"\n",
        0o600,
    );
    assert!(refused(&dir).contains("`from` is no plain email address"));
    write(
        &dir,
        SETTINGS,
        "server = \"ftp://smtp.example.com\"\nfrom = \"symphony@example.com\"\n",
        0o600,
    );
    assert!(refused(&dir).contains("email.toml: `server` is no"));
}

#[test]
fn a_user_and_a_password_come_together() {
    let dir = machine("together");
    write(&dir, SETTINGS, SETTINGS_TEXT, 0o600);
    let why = refused(&dir);
    assert!(
        why.contains("names a `user`") && why.contains("email.token does not exist"),
        "{why}"
    );
    write(
        &dir,
        SETTINGS,
        "server = \"smtps://smtp.example.com\"\nfrom = \"symphony@example.com\"\n",
        0o600,
    );
    write(&dir, TOKEN, PASSWORD, 0o600);
    let why = refused(&dir);
    assert!(
        why.contains("holds a password") && why.contains("no `user`"),
        "{why}"
    );
}

#[test]
fn a_user_curl_cannot_carry_or_a_password_of_more_than_a_line_is_refused() {
    let dir = machine("login-rules");
    write(&dir, TOKEN, PASSWORD, 0o600);
    for user in ["", "a:b", "a\\tb"] {
        let text = format!(
            "server = \"smtps://smtp.example.com\"\nfrom = \"symphony@example.com\"\nuser = \"{user}\"\n"
        );
        write(&dir, SETTINGS, &text, 0o600);
        assert!(
            refused(&dir).contains("`user` is empty, holds a `:`"),
            "{user:?}"
        );
    }
    write(&dir, SETTINGS, SETTINGS_TEXT, 0o600);
    let long = "p".repeat(PASSWORD_MOST + 1);
    for password in [" \n", "two\nlines", long.as_str()] {
        write(&dir, TOKEN, password, 0o600);
        let why = refused(&dir);
        assert!(why.contains("holds no password of one line"), "{why}");
        assert!(!why.contains("two") && !why.contains("ppp"), "{why}");
    }
    write(&dir, TOKEN, &"p".repeat(PASSWORD_MOST), 0o600);
    assert!(read(&dir).is_ok());
}
