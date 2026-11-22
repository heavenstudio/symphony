use super::*;
use crate::config::{Issues, parse};
use std::sync::OnceLock;

/// A folder of its own under the temp folder.
fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "symphony-github-mint-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn private(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// An RSA key of 2048 bits and its public half, and a key of another kind: made once.
fn keys() -> &'static Path {
    static KEYS: OnceLock<PathBuf> = OnceLock::new();
    KEYS.get_or_init(|| {
        let kept = std::env::temp_dir().join("symphony-github-mint-keys");
        if kept.join("locked.pem").is_file() {
            return kept.canonicalize().unwrap();
        }
        let keys = dir("keys");
        for args in [
            vec!["genrsa", "-out", "rsa.pem", "2048"],
            vec!["rsa", "-pubout", "-in", "rsa.pem", "-out", "rsa.pub"],
            vec![
                "ecparam",
                "-name",
                "prime256v1",
                "-genkey",
                "-noout",
                "-out",
                "ec.pem",
            ],
            vec![
                "genrsa",
                "-aes128",
                "-passout",
                "pass:word",
                "-out",
                "locked.pem",
                "2048",
            ],
        ] {
            let ran = Command::new("openssl")
                .args(&args)
                .current_dir(&keys)
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(ran.success(), "openssl {args:?}");
        }
        for key in ["rsa.pem", "ec.pem", "locked.pem"] {
            fs::set_permissions(keys.join(key), fs::Permissions::from_mode(0o600)).unwrap();
        }
        // Made once and kept for every later run, as making them takes longer than the tests;
        // another run that made them first keeps its own.
        if fs::rename(&keys, &kept).is_err() {
            let _ = fs::remove_dir_all(&keys);
        }
        kept.canonicalize().unwrap()
    })
}

#[test]
fn githubs_time_reads_as_seconds_since_1970_and_nothing_else_does() {
    for (text, at) in [
        ("1970-01-01T00:00:00Z", 0),
        ("1970-01-01T01:16:40Z", 4_600),
        ("2016-07-11T22:14:10Z", 1_468_275_250),
        ("2000-02-29T12:00:00Z", 951_825_600),
        ("2024-03-01T00:00:00Z", 1_709_251_200),
        ("2026-11-19T18:04:05Z", 1_795_111_445),
        ("1969-12-31T23:59:59Z", -1),
    ] {
        let expiry = Expiry::parse(text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!((expiry.at, expiry.text.as_str()), (at, text));
    }
    for wrong in [
        "",
        "2016-07-11 22:14:10Z",
        "2016-07-11T22:14:10",
        "2016-07-11T22:14:10+00:00",
        "2016-07-11T22:14:10.5Z",
        "2016-13-11T22:14:10Z",
        "2016-00-11T22:14:10Z",
        "2016-07-32T22:14:10Z",
        "2016-07-00T22:14:10Z",
        "2016-07-11T24:14:10Z",
        "2016-07-11T22:60:10Z",
        "2016-07-11T22:14:61Z",
        "+016-07-11T22:14:10Z",
        "2016-07-11T22:14:1\u{e9}",
    ] {
        assert_eq!(Expiry::parse(wrong), None, "{wrong:?}");
    }
}

#[test]
fn a_file_holds_a_token_its_expiry_and_its_permissions_or_nothing_this_program_wrote() {
    let token = |held: &Held| match held {
        Held::Token {
            token,
            expires,
            permissions,
        } => Some((token.expose().to_string(), expires.at, permissions.clone())),
        _ => None,
    };
    let asked = Some("checks=write,contents=write".to_string());
    assert_eq!(
        token(&held(
            "ghs_abc\nexpires_at 1970-01-01T01:00:00Z\npermissions checks=write,contents=write\n"
        )),
        Some(("ghs_abc".into(), 3_600, asked.clone()))
    );
    assert_eq!(
        token(&held(
            "ghs_abc\nexpires_at 1970-01-01T01:00:00Z\npermissions checks=write,contents=write"
        )),
        Some(("ghs_abc".into(), 3_600, asked)),
        "no last line end"
    );
    // A file an older build wrote says no permissions.
    let held_now = held("ghs_abc\nexpires_at 1970-01-01T01:00:00Z\n");
    assert_eq!(token(&held_now), Some(("ghs_abc".into(), 3_600, None)));
    assert_eq!(
        token(&held("ghs_abc\nexpires_at 1970-01-01T01:00:00Z")),
        Some(("ghs_abc".into(), 3_600, None)),
        "no last line end"
    );
    for unknown in [
        "",
        "\n",
        "ghs_abc\n",
        "ghs_abc",
        "ghs_abc\nexpires_at\n",
        "ghs_abc\nexpires 1970-01-01T01:00:00Z\n",
        "ghs_abc\nexpires_at tomorrow\n",
        "ghs_abc\nexpires_at 1970-01-01T01:00:00Z\nmore\n",
        "ghs_abc\nexpires_at 1970-01-01T01:00:00Z\npermissions\n",
        "ghs_abc\nexpires_at 1970-01-01T01:00:00Z\npermissions \n",
        "ghs_abc\nexpires_at 1970-01-01T01:00:00Z\npermissions checks=write\nmore\n",
        "two tokens\nexpires_at 1970-01-01T01:00:00Z\n",
        "\nexpires_at 1970-01-01T01:00:00Z\n",
    ] {
        assert!(matches!(held(unknown), Held::Unknown), "{unknown:?}");
    }
}

#[test]
fn a_token_is_current_only_past_the_margin() {
    let at = |expires: i64| Held::Token {
        token: Secret::new("ghs_abc").unwrap(),
        expires: Expiry {
            at: expires,
            text: String::new(),
        },
        permissions: None,
    };
    let now = 10_000;
    assert!(at(now + MARGIN + 1).current(now));
    assert!(!at(now + MARGIN).current(now));
    assert!(!at(now).current(now));
    assert!(!at(now - 1).current(now));
    assert!(!Held::Absent.current(now));
    assert!(!Held::Unknown.current(now));
}

#[test]
fn a_token_serves_only_while_current_and_made_for_what_is_asked_now() {
    let now = 10_000;
    let made = |expires: i64, permissions: Option<&str>| Held::Token {
        token: Secret::new("ghs_abc").unwrap(),
        expires: Expiry {
            at: expires,
            text: String::new(),
        },
        permissions: permissions.map(str::to_string),
    };
    let asked = "checks=write,contents=write,pull_requests=write";
    assert!(made(now + MARGIN + 1, Some(asked)).serves(now, asked));
    // Made for less, before the settings asked for checks, or by a build that said nothing.
    let fewer = made(now + MARGIN + 1, Some("contents=write,pull_requests=write"));
    assert!(fewer.current(now) && !fewer.serves(now, asked));
    assert!(!made(now + MARGIN + 1, None).serves(now, asked));
    // Made for more is not what is asked either: the least is asked for again.
    assert!(!made(now + MARGIN + 1, Some(asked)).serves(now, "contents=write"));
    assert!(!made(now + MARGIN, Some(asked)).serves(now, asked));
    assert!(!Held::Absent.serves(now, asked));
    assert!(!Held::Unknown.serves(now, asked));
}

#[test]
fn the_token_is_asked_for_the_least_its_calls_need() {
    let base = "enabled = true\nrepository = \"o/n\"\ntoken_file = \"/s/t\"\n";
    let publish = "pull_requests = \"publish\"\napp_id = 2\n";
    let asked = |more: &str| {
        let config = parse(&format!("{base}{more}")).unwrap();
        permissions(&config).into_iter().collect::<Vec<_>>()
    };
    // A delivery pushes, opens the pull request, writes its checks and merges: one App, and
    // never the repository's administration.
    assert_eq!(
        asked(publish),
        [
            ("checks", "write"),
            ("contents", "write"),
            ("pull_requests", "write")
        ]
    );
    assert_eq!(
        asked("releases = \"publish\"\n"),
        [("contents", "write")],
        "a release's creation"
    );
    assert_eq!(
        asked("verification = \"hosted\"\n"),
        [("actions", "write")],
        "a hosted verification's dispatch and the reads of its run"
    );
    assert_eq!(
        asked(&format!("{publish}releases = \"publish\"\n")),
        [
            ("checks", "write"),
            ("contents", "write"),
            ("pull_requests", "write")
        ]
    );
    assert_eq!(
        asked(&format!("{publish}close_issues = true\n")),
        [
            ("checks", "write"),
            ("contents", "write"),
            ("issues", "write"),
            ("pull_requests", "write")
        ]
    );
    for (issues, level) in [
        ("import", "read"),
        ("export", "write"),
        ("exchange", "write"),
    ] {
        let more = format!("issues = \"{issues}\"\nimport_labels = [\"a\"]\n");
        assert_eq!(asked(&more), [("issues", level)], "{issues}");
        assert_eq!(
            asked(&format!("{more}{publish}")),
            [
                ("checks", "write"),
                ("contents", "write"),
                ("issues", level),
                ("pull_requests", "write")
            ],
            "{issues} and publish"
        );
    }
    let config = parse(base).unwrap();
    assert_eq!(config.issues, Issues::Off);
    assert_eq!(
        permissions(&config).into_iter().collect::<Vec<_>>(),
        [("metadata", "read")],
        "never every permission the App holds"
    );
    for more in ["", publish, "releases = \"publish\"\nclose_issues = true\n"] {
        assert!(
            !asked(more)
                .iter()
                .any(|(name, _)| *name == "administration"),
            "{more}"
        );
    }
}

#[test]
fn a_jwt_is_rs256_of_the_app_and_verifies_with_its_public_key() {
    let keys = keys();
    let now = 1_700_000_000;
    let jwt = jwt("openssl", 1_000_001, &keys.join("rsa.pem"), now, "key_file").unwrap();
    let parts: Vec<&str> = jwt.split('.').collect();
    assert_eq!(parts.len(), 3, "{jwt}");
    let read = |part: &str| -> Value {
        serde_json::from_slice(&BASE64URL_NOPAD.decode(part.as_bytes()).unwrap()).unwrap()
    };
    assert_eq!(read(parts[0]), json!({"alg": "RS256", "typ": "JWT"}));
    assert_eq!(
        read(parts[1]),
        json!({"iat": now - 60, "exp": now + 540, "iss": "1000001"})
    );
    let signature = BASE64URL_NOPAD.decode(parts[2].as_bytes()).unwrap();
    assert_eq!(signature.len(), 256);
    let checked = dir("verify");
    fs::write(checked.join("signature"), &signature).unwrap();
    let mut verify = Command::new("openssl")
        .args(["dgst", "-sha256", "-verify"])
        .arg(keys.join("rsa.pub"))
        .args(["-signature", "signature"])
        .current_dir(&checked)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = verify.stdin.take().unwrap();
    stdin
        .write_all(format!("{}.{}", parts[0], parts[1]).as_bytes())
        .unwrap();
    drop(stdin);
    assert!(verify.wait().unwrap().success(), "openssl verifies it");
    let _ = fs::remove_dir_all(&checked);
    assert!(!jwt.contains('='), "base64url without padding: {jwt}");
}

#[test]
fn what_openssl_cannot_sign_with_is_refused_naming_what_to_fix() {
    let keys = keys();
    let refused = |openssl: &str, key: &Path| match jwt(openssl, 1, key, 0, "key_file") {
        Err(Stop::Refused(why)) => why,
        other => panic!("{other:?}"),
    };
    let why = refused("/nowhere/openssl", &keys.join("rsa.pem"));
    assert_eq!(
        why,
        "/nowhere/openssl is not on PATH: a GitHub App's token is asked for with a signature \
         it makes, so install it; nothing was sent"
    );
    let ec = keys.join("ec.pem");
    assert_eq!(
        refused("openssl", &ec),
        format!(
            "key_file {} is no RSA key of 2048 bits or more, which a GitHub App's key \
             is; nothing was sent",
            ec.display()
        )
    );
    // A key that asks for a passphrase is refused, never waited on.
    for key in [keys.join("locked.pem"), keys.join("rsa.pub")] {
        assert_eq!(
            refused("openssl", &key),
            format!(
                "openssl could not sign with key_file {}: write the App's private key \
                 there as GitHub gave it, a PEM file with no passphrase; nothing was sent",
                key.display()
            )
        );
    }
}

#[test]
fn a_file_the_program_keeps_is_placed_and_read_as_a_token_file_is() {
    let dir = dir("place");
    let repository = dir.join("repository");
    fs::create_dir_all(&repository).unwrap();
    let folders = [repository.as_path()];
    let file = dir.join("token");
    assert_eq!(place(&file, &folders, "token_file").unwrap(), file);
    assert!(matches!(kept(&file, "token_file").unwrap(), Held::Absent));
    let named = |why: &str| Err::<PathBuf, _>(Stop::Refused(why.into()));
    assert_eq!(
        place(Path::new("token"), &folders, "token_file"),
        named("token_file token is not an absolute path")
    );
    let inside = repository.join("token");
    assert_eq!(
        place(&inside, &folders, "token_file"),
        named(&format!(
            "token_file {} is inside the repository; keep it outside",
            inside.display()
        ))
    );
    let nowhere = dir.join("missing/token");
    assert!(matches!(
        place(&nowhere, &folders, "token_file"),
        Err(Stop::Refused(why)) if why.starts_with(&format!("token_file {}: its folder: ", nowhere.display()))
    ));
    // Through a link, the file is the one it leads to.
    let real = dir.join("real");
    private(&real, "ghs_a\n");
    let link = dir.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(place(&link, &folders, "token_file").unwrap(), real);
    fs::set_permissions(&real, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        kept(&real, "token_file").map(|_| ()),
        Err(Stop::Refused(format!(
            "token_file {} is readable by others; chmod 600 it",
            real.display()
        )))
    );
    assert_eq!(
        kept(&dir, "token_file").map(|_| ()),
        Err(Stop::Refused(format!(
            "token_file {} is not a file",
            dir.display()
        )))
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_key_file_is_held_as_a_token_file_is_and_is_never_the_token_file() {
    let dir = dir("key");
    let repository = dir.join("repository");
    fs::create_dir_all(&repository).unwrap();
    let folders = [repository.as_path()];
    let app = |key_file: PathBuf| App {
        id: 1,
        installation: 2,
        key_file,
    };
    let token_file = dir.join("token");
    let key = dir.join("key.pem");
    private(&key, "a key\n");
    assert_eq!(
        key_file(&app(key.clone()), &folders, &token_file).unwrap(),
        key
    );
    let refused = |key: &Path, token_file: &Path| match key_file(
        &app(key.to_path_buf()),
        &folders,
        token_file,
    ) {
        Err(Stop::Refused(why)) => why,
        other => panic!("{other:?}"),
    };
    let named = key.display();
    assert_eq!(
        refused(&key, &key),
        format!("key_file {named} is token_file: a token made would be written over the key")
    );
    fs::set_permissions(&key, fs::Permissions::from_mode(0o604)).unwrap();
    assert_eq!(
        refused(&key, &token_file),
        format!("key_file {named} is readable by others; chmod 600 it")
    );
    let inside = repository.join("key.pem");
    private(&inside, "a key\n");
    assert_eq!(
        refused(&inside, &token_file),
        format!(
            "key_file {} is inside the repository; keep it outside",
            inside.display()
        )
    );
    let missing = dir.join("missing.pem");
    assert!(
        refused(&missing, &token_file).starts_with(&format!("key_file {}: ", missing.display())),
    );
    assert_eq!(
        refused(&dir, &token_file),
        format!("key_file {} is not a file", dir.display())
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_token_made_is_written_whole_at_mode_600_or_not_at_all() {
    let dir = dir("write");
    let file = dir.join("token");
    private(&file, "ghs_old\n");
    let token = Secret::new("ghs_new").unwrap();
    let expires = Expiry::parse("1970-01-01T01:00:00Z").unwrap();
    write(&file, &token, &expires, "checks=write", "token_file").unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "ghs_new\nexpires_at 1970-01-01T01:00:00Z\npermissions checks=write\n"
    );
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(matches!(
        kept(&file, "token_file").unwrap(),
        Held::Token { .. }
    ));
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        1,
        "nothing left beside it"
    );
    // A folder it cannot write leaves the file as it was.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let refused = write(
        &file,
        &Secret::new("ghs_newer").unwrap(),
        &expires,
        "checks=write",
        "token_file",
    );
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        refused,
        Err(Stop::Refused(format!(
            "token_file {}: the token made was not written: permission denied",
            file.display()
        )))
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "ghs_new\nexpires_at 1970-01-01T01:00:00Z\npermissions checks=write\n"
    );
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_stop_says_its_line() {
    assert_eq!(Stop::Refused("a".into()).said(), "a");
    assert_eq!(Stop::Unavailable("b".into()).said(), "b");
}
