use super::*;

const ON: &str = "enabled = true\nrepository = \"o/n\"\ntoken_file = \"/secrets/token\"\n";

#[test]
fn nothing_written_is_everything_off() {
    assert_eq!(parse("").unwrap(), Config::default());
    let off = Config::default();
    assert!(!off.enabled && off.issues == Issues::Off && off.pull_requests == PullRequests::Off);
    assert_eq!(Loaded::absent().config, off);
}

#[test]
fn each_capability_is_its_own_setting() {
    for (issues, imports, exports) in [
        ("off", false, false),
        ("import", true, false),
        ("export", false, true),
        ("exchange", true, true),
    ] {
        for pulls in ["off", "track", "publish"] {
            let text = format!(
                "{ON}issues = \"{issues}\"\npull_requests = \"{pulls}\"\nimport_labels = [\"a\"]\n"
            );
            let config = parse(&text).unwrap();
            assert_eq!(
                (config.issues.imports(), config.issues.exports()),
                (imports, exports)
            );
            assert_eq!(
                (config.issues.word(), config.pull_requests.word()),
                (issues, pulls)
            );
        }
    }
}

#[test]
fn what_does_not_hold_together_is_refused_by_name() {
    for (text, why) in [
        ("enabled = true\n", "enabled needs repository"),
        (
            "enabled = true\nrepository = \"o/n\"\n",
            "enabled needs token_file",
        ),
        (&format!("{ON}issues = \"import\"\n"), "needs import_labels"),
        (
            &format!("{ON}issues = \"exchange\"\n"),
            "needs import_labels",
        ),
        (&format!("{ON}issues = \"sync\"\n"), "unknown variant"),
        (
            &format!("{ON}pull_requests = \"merge\"\n"),
            "unknown variant",
        ),
        (&format!("{ON}webhook = true\n"), "unknown field"),
        ("token = \"ghp_x\"\n", "unknown field"),
        ("enabled = \"yes\"\n", "invalid type"),
        ("token_file = \"relative/token\"\n", "absolute path"),
        ("repository = \"https://github.com/o/n\"\n", "never a URL"),
        ("repository = \"o\"\n", "OWNER/NAME"),
        ("repository = \"o/n/x\"\n", "OWNER/NAME"),
        ("repository = \"o/..\"\n", "OWNER/NAME"),
        ("repository = \"ghe.example.com:o/n\"\n", "OWNER/NAME"),
        ("repository = \"o/n?x=1\"\n", "OWNER/NAME"),
        ("import_labels = [\"\"]\n", "1 to 50 characters"),
        ("import_labels = [\"a,b\"]\n", "no comma"),
        ("export_label = \"a\\nb\"\n", "control character"),
    ] {
        let refused = parse(text).unwrap_err();
        assert!(refused.contains(why), "{text:?}: {refused}");
        assert!(refused.starts_with("symphony-github.toml: "), "{refused}");
    }
}

#[test]
fn a_repository_is_named_and_a_revision_follows_the_text() {
    let config = parse(&format!("{ON}export_label = \"from symphony\"\n")).unwrap();
    let repository = config.repository.unwrap();
    assert_eq!(
        (repository.owner.as_str(), repository.name.as_str()),
        ("o", "n")
    );
    assert_eq!(repository.full_name(), "o/n");
    assert_eq!(config.export_label.as_deref(), Some("from symphony"));
    assert_eq!(
        parse("repository = \"My-Org/my.repo_1\"\n")
            .unwrap()
            .repository
            .unwrap()
            .name,
        "my.repo_1"
    );

    let (one, same, other) = (
        load(ON).unwrap(),
        load(ON).unwrap(),
        load(&format!("{ON}\n")).unwrap(),
    );
    assert_eq!(one.revision, same.revision);
    assert_ne!(
        one.revision, other.revision,
        "any change of the text is another revision"
    );
    assert_ne!(one.revision, Loaded::absent().revision);
}

#[test]
fn a_folder_in_no_repository_has_no_checkouts() {
    let outside = std::env::temp_dir();
    // The temp folder may itself be inside a repository on some machine; only the refusal's
    // words are held when it is not.
    if let Err(why) = checkouts(&outside) {
        assert!(why.contains("is in no git repository"), "{why}");
    }
    assert!(checkouts(Path::new("/nonexistent/folder")).is_err());
}
