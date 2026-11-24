use super::*;
use crate::test_tree::Tree;

/// A machine folder whose `notices/programs.toml` holds `text`, the folder 700 and the file 600.
fn machine(text: &str) -> Tree {
    let tree = Tree::new();
    tree.write("notices/programs.toml", text);
    chmod(&tree.path("notices"), 0o700);
    chmod(&tree.path("notices/programs.toml"), 0o600);
    tree
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn the_machine_names_each_ways_program_by_its_absolute_path() {
    let tree = machine(
        "email = \"/usr/local/bin/symphony-notice-email\"\n\
         slack = \"/Users/ana/.cargo/bin/symphony-notice-slack\"\n",
    );
    let programs = Programs::read(&tree.path("")).unwrap();
    assert_eq!(
        programs.program("email"),
        Some(Path::new("/usr/local/bin/symphony-notice-email"))
    );
    assert_eq!(
        programs.program("slack"),
        Some(Path::new("/Users/ana/.cargo/bin/symphony-notice-slack"))
    );
    assert_eq!(programs.program("whatsapp"), None, "a way named nowhere");
    assert!(!programs.is_empty());
    // Read by its owner alone or by anyone, the file is taken: only a write is guarded.
    chmod(&tree.path("notices/programs.toml"), 0o644);
    chmod(&tree.path("notices"), 0o755);
    assert_eq!(Programs::read(&tree.path("")).unwrap(), programs);
}

#[test]
fn a_machine_naming_no_program_sends_nothing() {
    let tree = Tree::new();
    assert!(
        Programs::read(&tree.path("")).unwrap().is_empty(),
        "no folder"
    );
    tree.write("notices/slack.token", "xoxb-1");
    chmod(&tree.path("notices"), 0o700);
    assert!(
        Programs::read(&tree.path("")).unwrap().is_empty(),
        "no file"
    );
    let empty = machine("");
    assert!(Programs::read(&empty.path("")).unwrap().is_empty());
}

#[test]
fn a_file_or_folder_another_account_may_write_is_refused() {
    let tree = machine("email = \"/bin/notice\"\n");
    for mode in [0o620, 0o602, 0o666] {
        chmod(&tree.path("notices/programs.toml"), mode);
        let why = Programs::read(&tree.path("")).unwrap_err();
        assert!(
            why.contains("programs.toml may be written by another account"),
            "{why}"
        );
        assert!(why.contains("chmod go-w"), "{why}");
    }
    chmod(&tree.path("notices/programs.toml"), 0o600);
    for mode in [0o770, 0o707, 0o777] {
        chmod(&tree.path("notices"), mode);
        let why = Programs::read(&tree.path("")).unwrap_err();
        assert!(
            why.contains("notices may be written by another account"),
            "{why}"
        );
    }
}

#[test]
fn a_link_in_the_place_of_the_file_or_the_folder_is_refused() {
    let tree = machine("email = \"/bin/notice\"\n");
    tree.write("elsewhere/programs.toml", "email = \"/tmp/anything\"\n");
    chmod(&tree.path("elsewhere/programs.toml"), 0o600);
    fs::remove_file(tree.path("notices/programs.toml")).unwrap();
    std::os::unix::fs::symlink(
        tree.path("elsewhere/programs.toml"),
        tree.path("notices/programs.toml"),
    )
    .unwrap();
    let why = Programs::read(&tree.path("")).unwrap_err();
    assert!(why.contains("programs.toml is a link"), "{why}");
    assert!(
        !why.contains("/tmp/anything"),
        "nothing of it is read: {why}"
    );

    let linked = Tree::new();
    chmod(&tree.path("elsewhere"), 0o700);
    std::os::unix::fs::symlink(tree.path("elsewhere"), linked.path("notices")).unwrap();
    let why = Programs::read(&linked.path("")).unwrap_err();
    assert!(why.contains("notices is a link"), "{why}");
    assert!(why.contains("keep the folder itself there"), "{why}");
}

#[test]
fn what_is_not_a_folder_or_a_file_is_refused() {
    let tree = Tree::new();
    tree.write("notices", "a file where the folder goes");
    let why = Programs::read(&tree.path("")).unwrap_err();
    assert!(why.ends_with("notices is not a folder"), "{why}");
    let held = Tree::new();
    fs::create_dir_all(held.path("notices/programs.toml")).unwrap();
    chmod(&held.path("notices"), 0o700);
    chmod(&held.path("notices/programs.toml"), 0o700);
    let why = Programs::read(&held.path("")).unwrap_err();
    assert!(why.ends_with("programs.toml is not a file"), "{why}");
}

#[test]
fn a_way_or_a_program_by_another_rule_is_refused_by_its_key() {
    for (text, said) in [
        (
            "email = \"bin/notice\"\n",
            "email: a program is named by its absolute path",
        ),
        (
            "email = 3\n",
            "email: a program is named by its absolute path",
        ),
        (
            "[email]\nprogram = \"/bin/x\"\n",
            "email: a program is named",
        ),
        ("Email = \"/bin/x\"\n", "\"Email\" is no way"),
        ("-slack = \"/bin/x\"\n", "\"-slack\" is no way"),
        ("slack- = \"/bin/x\"\n", "\"slack-\" is no way"),
        ("we--chat = \"/bin/x\"\n", "\"we--chat\" is no way"),
        ("\"\" = \"/bin/x\"\n", "\"\" is no way"),
        ("email = \n", "not TOML"),
    ] {
        let why = Programs::parse(text).unwrap_err();
        assert!(why.contains(said), "{text:?}: {why}");
    }
    let tree = machine("email = \"bin/notice\"\n");
    let why = Programs::read(&tree.path("")).unwrap_err();
    assert!(
        why.contains("notices/programs.toml: email:"),
        "said by its path: {why}"
    );
    // Another way is another word, which a plugin of a later way takes.
    let later = Programs::parse("we-chat2 = \"/bin/x\"\n").unwrap();
    assert_eq!(later.program("we-chat2"), Some(Path::new("/bin/x")));
}

#[test]
fn a_kind_is_its_word() {
    for kind in [Kind::Card, Kind::Question, Kind::Verdict] {
        assert_eq!(Kind::of(kind.word()), Some(kind));
    }
    assert_eq!(Kind::of("poll"), None);
}

#[test]
fn the_message_is_one_line_the_notice_crate_reads_as_written() {
    let line = line(
        "0193f1c2",
        Kind::Verdict,
        "symphony",
        "http://studio.tailnet.ts.net:8765/symphony/task/app/x",
        "slack",
        "U024BE7LH",
    );
    assert!(!line.contains('\n'));
    assert!(
        line.starts_with("{\"notice\":1,\"id\":\"0193f1c2\""),
        "{line}"
    );
    let read = symphony_notice::Notice::read(&line).unwrap();
    assert_eq!(read.id(), "0193f1c2");
    assert_eq!(read.kind(), symphony_notice::Kind::Verdict);
    assert_eq!(read.project(), "symphony");
    assert_eq!(
        read.link(),
        "http://studio.tailnet.ts.net:8765/symphony/task/app/x"
    );
    assert_eq!(
        (read.to().way(), read.to().address()),
        ("slack", "U024BE7LH")
    );
    for kind in [Kind::Card, Kind::Question, Kind::Verdict] {
        let line = super::line("n-1", kind, "p", "https://h/", "email", "ana@example.com");
        let read = symphony_notice::Notice::read(&line).unwrap();
        assert_eq!(read.kind(), symphony_notice::Kind::of(kind.word()));
    }
}

#[test]
fn an_exit_status_and_an_unstarted_program_come_to_what_the_plugins_say() {
    assert_eq!(SENT, i32::from(symphony_notice::SENT));
    assert_eq!(LATER, i32::from(symphony_notice::LATER));
    assert_eq!(came(Some(0)), Came::Sent);
    assert_eq!(came(Some(75)), Came::Later);
    for code in [1, 2, 64, 74, 76, 127, 255] {
        assert_eq!(came(Some(code)), Came::Never, "{code}");
    }
    assert_eq!(
        came(None),
        Came::Never,
        "a signal is no status of the format's"
    );
    for error in [
        io::Error::from(io::ErrorKind::WouldBlock),
        io::Error::from(io::ErrorKind::OutOfMemory),
        io::Error::from(io::ErrorKind::ExecutableFileBusy),
        io::Error::from_raw_os_error(23),
        io::Error::from_raw_os_error(24),
        io::Error::from(io::ErrorKind::NotFound),
        io::Error::from(io::ErrorKind::PermissionDenied),
        io::Error::from_raw_os_error(libc::ENOEXEC),
    ] {
        let theirs = match symphony_notice::unstarted(&error, String::new()) {
            symphony_notice::Unsent::Later(_) => Came::Later,
            symphony_notice::Unsent::Never(_) => Came::Never,
        };
        assert_eq!(unstarted(&error), theirs, "{error:?}");
    }
}

#[test]
fn the_machines_key_is_made_once_its_owners_alone_and_read_back_the_same() {
    let tree = Tree::new();
    let key = Key::of(&tree.path("")).unwrap();
    let mode = |path: &str| fs::metadata(tree.path(path)).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        (mode("notices"), mode("notices/record.key")),
        (0o700, 0o600)
    );
    let kept = tree.read("notices/record.key");
    assert_eq!(kept.len(), 65, "64 hex digits and a newline");
    assert_eq!(Key::of(&tree.path("")).unwrap(), key, "made once");
    assert_eq!(tree.read("notices/record.key"), kept);
    assert_eq!(key.digest("+15551234567"), key.digest("+15551234567"));
    assert_ne!(key.digest("+15551234567"), key.digest("+15551234568"));
    let other = Key::of(&Tree::new().path("")).unwrap();
    assert_ne!(
        other.digest("+15551234567"),
        key.digest("+15551234567"),
        "each machine's own"
    );
    assert_eq!(format!("{key:?}"), "Key(<hidden>)");
    // A notices folder the plugins made first is taken as it is.
    let made = machine("");
    Key::of(&made.path("")).unwrap();
    assert!(made.path("notices/programs.toml").is_file());
}

#[test]
fn a_key_open_to_another_account_linked_or_not_a_key_is_refused_without_its_text() {
    let tree = Tree::new();
    Key::of(&tree.path("")).unwrap();
    let kept = tree.read("notices/record.key");
    let refused = |tree: &Tree| Key::of(&tree.path("")).unwrap_err();
    chmod(&tree.path("notices/record.key"), 0o640);
    let why = refused(&tree);
    assert!(
        why.contains("record.key is open to another account"),
        "{why}"
    );
    assert!(!why.contains(kept.trim()));
    chmod(&tree.path("notices/record.key"), 0o600);
    chmod(&tree.path("notices"), 0o770);
    assert!(refused(&tree).contains("notices is open to another account"));
    chmod(&tree.path("notices"), 0o755);
    Key::of(&tree.path("")).unwrap();

    let linked = Tree::new();
    linked.write("elsewhere/record.key", &kept);
    chmod(&linked.path("elsewhere/record.key"), 0o600);
    fs::create_dir(linked.path("notices")).unwrap();
    chmod(&linked.path("notices"), 0o700);
    std::os::unix::fs::symlink(
        linked.path("elsewhere/record.key"),
        linked.path("notices/record.key"),
    )
    .unwrap();
    assert!(refused(&linked).contains("record.key is a link"));
    let folder = Tree::new();
    std::os::unix::fs::symlink(linked.path("elsewhere"), folder.path("notices")).unwrap();
    assert!(refused(&folder).contains("notices is a link"));

    for text in ["not a key\n", "abc\n", &"g".repeat(64)] {
        let bad = Tree::new();
        bad.write("notices/record.key", text);
        chmod(&bad.path("notices"), 0o700);
        chmod(&bad.path("notices/record.key"), 0o600);
        let why = refused(&bad);
        assert!(why.contains("record.key is no key"), "{why}");
        assert!(!why.contains(text.trim()), "{why}");
    }
    let dir = Tree::new();
    fs::create_dir_all(dir.path("notices/record.key")).unwrap();
    chmod(&dir.path("notices"), 0o700);
    assert!(refused(&dir).contains("is not what it should be"));
    let file = Tree::new();
    file.write("notices", "a file");
    assert!(refused(&file).contains("notices is not what it should be"));
}

#[test]
fn the_digest_is_rfc_4231s_hmac_sha256() {
    let hmac = |key: &[u8], message: &str| hex(&hmac_sha256(key, message.as_bytes()));
    assert_eq!(
        hmac(&[0x0b; 20], "Hi There"),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        hmac(b"Jefe", "what do ya want for nothing?"),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
        hmac(
            &[0xaa; 131],
            "Test Using Larger Than Block-Size Key - Hash Key First"
        ),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
    let key = Key::parse(&"ab".repeat(32)).unwrap();
    assert_eq!(key.digest("x"), hmac(&[0xab; 32], "x"));
    assert_eq!(Key::parse(&"AB".repeat(32)), Some(key));
}
