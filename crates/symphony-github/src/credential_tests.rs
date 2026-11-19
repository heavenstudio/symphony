use super::*;
use std::fs;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "symphony-github-credential-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn file(dir: &Path, name: &str, text: &str, mode: u32) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    path
}

#[test]
fn a_token_is_never_printed() {
    let secret = Secret::new("  ghp_abc123\n").unwrap();
    assert_eq!(secret.expose(), "ghp_abc123");
    assert_eq!(format!("{secret:?}"), "Secret(redacted)");
    assert_eq!(
        secret.redact("curl: header Bearer ghp_abc123 refused, ghp_abc123"),
        "curl: header Bearer [redacted] refused, [redacted]"
    );
    for wrong in [
        "",
        "  \n",
        "two tokens",
        "quo\"te",
        "back\\slash",
        "line\nbreak",
        "caf\u{e9}",
    ] {
        let refused = Secret::new(wrong).unwrap_err();
        assert!(
            wrong.trim().is_empty() != refused.contains("more than one token"),
            "{wrong:?}: {refused}"
        );
    }
    assert!(Secret::new(&"x".repeat(4097)).is_err());
}

#[test]
fn a_token_file_is_outside_the_repository_plain_and_private() {
    let dir = dir("read");
    let repository = dir.join("repository");
    fs::create_dir_all(repository.join("nested")).unwrap();
    let good = file(&dir, "token", "ghp_good\n", 0o600);
    assert_eq!(read(&good, &[&repository]).unwrap().expose(), "ghp_good");
    assert_eq!(
        read(&file(&dir, "ro", "ghp_ro", 0o400), &[&repository])
            .unwrap()
            .expose(),
        "ghp_ro"
    );

    let cases: Vec<(PathBuf, &str)> = vec![
        (PathBuf::from("relative/token"), "not an absolute path"),
        (dir.join("absent"), "No such file"),
        (
            file(&repository.join("nested"), "token", "ghp_inside", 0o600),
            "inside the repository",
        ),
        (dir.clone(), "is not a file"),
        (
            file(&dir, "group", "ghp_group", 0o640),
            "readable by others",
        ),
        (
            file(&dir, "world", "ghp_world", 0o604),
            "readable by others",
        ),
        (file(&dir, "empty", "\n", 0o600), "the token file is empty"),
        (
            file(&dir, "two", "ghp_a\nghp_b\n", 0o600),
            "more than one token",
        ),
    ];
    for (path, why) in cases {
        let refused = read(&path, &[&repository]).unwrap_err();
        assert!(refused.contains(why), "{}: {refused}", path.display());
        assert!(
            !refused.contains("ghp_"),
            "a refusal shows nothing of the file: {refused}"
        );
    }

    // A link into the repository is read as where it leads.
    let link = dir.join("link");
    std::os::unix::fs::symlink(repository.join("nested/token"), &link).unwrap();
    assert!(
        read(&link, &[&repository])
            .unwrap_err()
            .contains("inside the repository")
    );
    let _ = fs::remove_dir_all(&dir);
}
