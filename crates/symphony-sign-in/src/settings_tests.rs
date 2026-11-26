//! The service's files: its settings read with every value checked, and refused by the rule, never
//! the value; each file its account's alone, never a link; the key made once, its line one the
//! server's own `sign-in.toml` takes; and a start refused where that file names another key or
//! origin.

use super::*;
use crate::stand_in::{CLIENT_SECRET, folder, key, write};
use std::os::unix::fs::{PermissionsExt as _, symlink};

const GOOD: &str = r#"listen = "127.0.0.1:8766"
upstream = "127.0.0.1:8765"
origin = "https://board.example.com"

[google]
client-id = "1234-stand-in.apps.googleusercontent.com"
client-secret = "GOCSPX-stand-in-secret-0000"
"#;

/// Every value the settings name is read, checked.
#[test]
fn the_settings_name_where_to_listen_the_server_behind_the_origin_and_googles_client() {
    let settings = Settings::parse(GOOD).unwrap();
    assert_eq!(settings.listen, "127.0.0.1:8766".parse().unwrap());
    assert_eq!(settings.upstream, "127.0.0.1:8765".parse().unwrap());
    assert_eq!(settings.origin, "https://board.example.com");
    assert_eq!(
        settings.google.id,
        "1234-stand-in.apps.googleusercontent.com"
    );
    assert_eq!(settings.google.secret.expose(), CLIENT_SECRET);
    let ipv6 = GOOD.replace("127.0.0.1:8766", "[::1]:8766");
    assert!(Settings::parse(&ipv6).is_ok());
}

/// A setting missing, one unknown, a value of the wrong kind or breaking its rule is refused by
/// its rule, and the refusal never quotes the client secret.
#[test]
fn a_setting_that_breaks_its_rule_is_refused_by_the_rule_never_the_value() {
    let cases = [
        (
            GOOD.replace("listen = \"127.0.0.1:8766\"\n", ""),
            "missing field `listen`",
        ),
        (format!("{GOOD}extra = \"x\"\n"), "unknown field `extra`"),
        (
            GOOD.replace("client-secret", "secret"),
            "line 7: unknown field `secret`",
        ),
        (
            GOOD.replace("\"127.0.0.1:8766\"", "8766"),
            "line 1: a value that does not read",
        ),
        (
            GOOD.replace("\"GOCSPX-stand-in-secret-0000\"", "1234567"),
            "line 7: a value that does not read",
        ),
        (
            GOOD.replace("127.0.0.1:8766", "localhost:8766"),
            "listen is not an address and a port",
        ),
        (
            GOOD.replace("127.0.0.1:8765", "127.0.0.1"),
            "upstream is not an address and a port",
        ),
        (
            GOOD.replace("127.0.0.1:8766", "127.0.0.1:8765"),
            "listen and upstream are one address",
        ),
        (
            GOOD.replace("https://board.example.com", "https://board.example.com/"),
            "the origin is not http:// or https://",
        ),
        (
            GOOD.replace("1234-stand-in.apps.googleusercontent.com", ""),
            "google's client-id is not 1 to 256",
        ),
        (
            GOOD.replace("GOCSPX-stand-in-secret-0000", "GOCSPX stand-in"),
            "google's client-secret is not 1 to 4096",
        ),
        (
            GOOD.replace("GOCSPX-stand-in-secret-0000", ""),
            "google's client-secret is not 1 to 4096",
        ),
    ];
    for (text, rule) in cases {
        let refused = Settings::parse(&text).unwrap_err();
        assert!(refused.contains(rule), "{rule}: {refused}");
        assert!(!refused.contains("GOCSPX"), "{refused}");
        assert!(!refused.contains("1234567"), "{refused}");
    }
}

/// The origin's rule is the server's own: every origin one takes, the other takes, and no other.
#[test]
fn the_origin_rule_is_the_servers_own() {
    let public = key().public();
    for origin in [
        "https://board.example.com",
        "https://board.example.com:8443",
        "http://localhost:8766",
        "http://127.0.0.1:8766",
        "https://[::1]:8443",
        "http://board",
        "https://board.example.com:443",
        "http://board.example.com:80",
        "https://Board.example.com",
        "https://board.example.com/",
        "https://board.example.com/path",
        "https://board.example.com:0",
        "https://board.example.com:99999",
        "https://board.example.com:",
        "ftp://board.example.com",
        "https://",
        "https://[]",
        "https://[::1",
        "https://user@board.example.com",
        "board.example.com",
    ] {
        assert_eq!(
            checked_origin(origin).is_ok(),
            symphony::signed_in::Settings::new(&public, origin).is_ok(),
            "{origin}"
        );
    }
}

/// The settings are read from the machine folder; a file not there says how to make it.
#[test]
fn the_settings_are_read_from_the_machine_folder() {
    let machine = folder("settings");
    let missing = Settings::read(&machine).unwrap_err();
    assert!(
        missing.contains("sign-in-service.toml is not there"),
        "{missing}"
    );
    write(&machine, SETTINGS, GOOD, 0o600);
    assert_eq!(
        Settings::read(&machine).unwrap(),
        Settings::parse(GOOD).unwrap()
    );
    write(&machine, SETTINGS, "listen = 1", 0o600);
    let unread = Settings::read(&machine).unwrap_err();
    assert!(
        unread.starts_with(&machine.join(SETTINGS).display().to_string()),
        "{unread}"
    );
}

/// Each file is refused where another account may read it or write it, where it is a link or not
/// a file, and nothing it holds is said.
#[test]
fn a_file_another_account_may_reach_or_a_link_is_refused_unread() {
    let machine = folder("modes");
    for mode in [0o644, 0o640, 0o604, 0o620, 0o602, 0o660] {
        let path = write(&machine, SETTINGS, GOOD, mode);
        let refused = Settings::read(&machine).unwrap_err();
        assert!(
            refused.contains("may be read or written by another account"),
            "{mode:o}: {refused}"
        );
        assert!(!refused.contains("GOCSPX"), "{refused}");
        fs::remove_file(path).unwrap();
    }
    let real = write(&machine, "real.toml", GOOD, 0o600);
    symlink(&real, machine.join(SETTINGS)).unwrap();
    let linked = Settings::read(&machine).unwrap_err();
    assert!(linked.contains("is a link"), "{linked}");
    fs::remove_file(machine.join(SETTINGS)).unwrap();
    let unopened = write(&machine, SETTINGS, GOOD, 0o000);
    let refused = Settings::read(&machine).unwrap_err();
    assert!(refused.contains("Permission denied"), "{refused}");
    fs::remove_file(unopened).unwrap();
    fs::create_dir(machine.join(SETTINGS)).unwrap();
    let folder = Settings::read(&machine).unwrap_err();
    assert!(folder.contains("is not a file"), "{folder}");
}

/// A key the machine folder will not take is said with the folder's own error.
#[test]
fn a_key_the_folder_will_not_take_is_said_by_its_error() {
    let machine = folder("unwritable");
    fs::set_permissions(&*machine, fs::Permissions::from_mode(0o500)).unwrap();
    let refused = make_key(&machine).unwrap_err();
    fs::set_permissions(&*machine, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(refused.contains("Permission denied"), "{refused}");
    assert!(!machine.join(KEY).exists());
}

/// The key is made once, mode 600, in a machine folder made mode 700 where it was not there; its
/// line is one the server's `sign-in.toml` takes, for the key read back; a second make is refused
/// and leaves the first.
#[test]
fn the_key_is_made_once_and_its_line_is_one_the_server_takes() {
    let root = folder("key");
    let machine = root.join("machine");
    let line = make_key(&machine).unwrap();
    assert_eq!(
        fs::metadata(&machine).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let path = machine.join(KEY);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.len(), 65, "{text}");
    let read = Key::read(&machine).unwrap();
    assert_eq!(line, format!("key = \"{}\"", read.public()));
    let server = format!("{line}\norigin = \"https://board.example.com\"\n");
    assert!(
        symphony::signed_in::Settings::parse(&server).is_ok(),
        "{server}"
    );
    let again = make_key(&machine).unwrap_err();
    assert!(again.contains("is there already"), "{again}");
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert_eq!(format!("{read:?}"), "Key(hidden)");
    assert!(!format!("{:?}", Settings::parse(GOOD).unwrap()).contains("GOCSPX"));
}

/// A key file that is not there, not 64 lowercase hex digits, or that another account may read is
/// refused, saying how to make one.
#[test]
fn a_key_not_there_or_not_its_form_is_refused() {
    let machine = folder("bad-key");
    let missing = Key::read(&machine).unwrap_err();
    assert!(
        missing.contains("make it with `symphony-sign-in key`"),
        "{missing}"
    );
    for text in ["", "abc", &"A".repeat(64), &"g".repeat(64), &"0".repeat(66)] {
        write(&machine, KEY, text, 0o600);
        let refused = Key::read(&machine).unwrap_err();
        assert!(
            refused.contains("is not 64 lowercase hex digits"),
            "{text}: {refused}"
        );
    }
    write(&machine, KEY, &"0".repeat(64), 0o644);
    let open = Key::read(&machine).unwrap_err();
    assert!(open.contains("chmod 600"), "{open}");
    write(&machine, KEY, &format!("{}\n", "0".repeat(64)), 0o600);
    assert!(Key::read(&machine).is_ok());
}

/// A start checks the server's `sign-in.toml` beside it, where there is one, names the service's
/// key and origin; it says the line to put there, and asks nothing of a folder without one.
#[test]
fn a_start_is_refused_where_the_servers_file_beside_it_names_another_key_or_origin() {
    let machine = folder("matches");
    let origin = "https://board.example.com";
    assert_eq!(matches_server(&machine, &key(), origin), Ok(()));
    crate::stand_in::server_names(&machine, origin);
    assert_eq!(matches_server(&machine, &key(), origin), Ok(()));
    let other = Key::from_seed(&[9; 32]);
    let refused = matches_server(&machine, &other, origin).unwrap_err();
    assert!(
        refused.contains(&format!(
            "put this line there: key = \"{}\"",
            other.public()
        )),
        "{refused}"
    );
    let elsewhere = matches_server(&machine, &key(), "https://other.example.com").unwrap_err();
    assert!(elsewhere.contains("names another origin"), "{elsewhere}");
    write(&machine, SERVER, "key = ", 0o644);
    let unread = matches_server(&machine, &key(), origin).unwrap_err();
    assert!(unread.contains("does not read"), "{unread}");
    fs::remove_file(machine.join(SERVER)).unwrap();
    fs::create_dir(machine.join(SERVER)).unwrap();
    let folder = matches_server(&machine, &key(), origin).unwrap_err();
    assert!(
        folder.starts_with(&machine.join(SERVER).display().to_string()),
        "{folder}"
    );
}
