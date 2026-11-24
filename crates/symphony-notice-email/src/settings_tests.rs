use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A machine folder of its own, with a `notices/` its owner alone reaches.
fn machine(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "symphony-notice-email-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
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
fn the_machine_folder_is_the_servers() {
    let var = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            (pairs.iter())
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    };
    assert_eq!(
        machine_folder(var(&[(MACHINE_DIR, "/m"), ("HOME", "/h")]), true).unwrap(),
        PathBuf::from("/m")
    );
    assert!(
        machine_folder(var(&[(MACHINE_DIR, "m")]), true)
            .unwrap_err()
            .contains("not an absolute folder")
    );
    assert_eq!(
        machine_folder(var(&[(MACHINE_DIR, ""), ("HOME", "/h")]), true).unwrap(),
        PathBuf::from("/h/Library/Application Support/Symphony")
    );
    assert_eq!(
        machine_folder(var(&[("HOME", "/h"), ("XDG_DATA_HOME", "/x")]), false).unwrap(),
        PathBuf::from("/x/symphony")
    );
    assert_eq!(
        machine_folder(var(&[("HOME", "/h"), ("XDG_DATA_HOME", "x")]), false).unwrap(),
        PathBuf::from("/h/.local/share/symphony")
    );
    assert!(
        machine_folder(var(&[("HOME", "h")]), true)
            .unwrap_err()
            .contains("needs HOME")
    );
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
fn a_link_in_the_place_of_the_folder_or_a_file_is_refused() {
    let dir = machine("link");
    let elsewhere = machine("link-target");
    write(&elsewhere, SETTINGS, SETTINGS_TEXT, 0o600);
    std::os::unix::fs::symlink(
        elsewhere.join(FOLDER).join(SETTINGS),
        dir.join(FOLDER).join(SETTINGS),
    )
    .unwrap();
    assert!(refused(&dir).contains("email.toml is not a plain file"));
    fs::remove_dir_all(dir.join(FOLDER)).unwrap();
    std::os::unix::fs::symlink(elsewhere.join(FOLDER), dir.join(FOLDER)).unwrap();
    assert!(refused(&dir).contains("notices is not a folder"));
    fs::remove_file(dir.join(FOLDER)).unwrap();
    fs::write(dir.join(FOLDER), "").unwrap();
    assert!(refused(&dir).contains("notices is not a folder"));
}

#[test]
fn settings_that_do_not_read_name_the_line_and_never_its_text() {
    let dir = machine("unread");
    write(
        &dir,
        SETTINGS,
        "server = \"smtps://smtp.example.com\"\nfrom = \"symphony@example.com\"\npassword = \"se\\\"cr\"\n",
        0o600,
    );
    let why = refused(&dir);
    assert!(
        why.contains("line 3") && why.contains("unknown field `password`"),
        "{why}"
    );
    write(
        &dir,
        SETTINGS,
        "server = \"smtps://smtp.example.com\"\n",
        0o600,
    );
    let why = refused(&dir);
    assert!(why.contains("missing field `from`"), "{why}");
    write(&dir, SETTINGS, "server = \"smtps://x\nfrom = 1\n", 0o600);
    let why = refused(&dir);
    assert!(why.contains("email.toml line 1:"), "{why}");
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
