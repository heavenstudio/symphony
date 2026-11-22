use super::*;

const ON: &str = "enabled = true\nrepository = \"o/n\"\ntoken_file = \"/secrets/token\"\n";
/// A delivery's settings, its token kept by hand.
const PUBLISH: &str = "enabled = true\nrepository = \"o/n\"\ntoken_file = \"/secrets/token\"\n\
                       pull_requests = \"publish\"\napp_id = 43\n";

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
                "{ON}issues = \"{issues}\"\npull_requests = \"{pulls}\"\nimport_labels = \
                 [\"a\"]\napp_id = 43\n"
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
            &format!("{ON}pull_requests = \"publish\"\n"),
            "pull_requests = \"publish\" needs app_id: the GitHub App token_file's token is of",
        ),
        ("app_id = -1\n", "app_id is a GitHub App's number"),
        ("app_id = 0\n", "app_id is a GitHub App's number"),
        ("close_issues = \"yes\"\n", "expected a boolean"),
        (
            "upstream_id = 0\n",
            "upstream_id is a GitHub repository's number",
        ),
        (
            "upstream_id = 88\n",
            "upstream_id needs pull_requests = \"publish\": an import is delivered by a pull request",
        ),
        (
            &format!("{ON}pull_requests = \"track\"\nupstream_id = 88\n"),
            "upstream_id needs pull_requests = \"publish\"",
        ),
        ("upstream_id = \"up/stream\"\n", "invalid type"),
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
fn an_upstream_is_pinned_by_its_number_where_pull_requests_are_published() {
    assert_eq!(parse(PUBLISH).unwrap().upstream_id, None);
    let config = parse(&format!("{PUBLISH}upstream_id = 88\n")).unwrap();
    assert_eq!(config.upstream_id, Some(88));
}

#[test]
fn the_keys_of_three_apps_are_each_refused_by_name_and_say_where_one_app_is() {
    for key in RETIRED {
        let value = if key.ends_with("_file") {
            "\"/secrets/x\""
        } else {
            "42"
        };
        for text in [
            format!("{key} = {value}\n"),
            format!("{PUBLISH}{key} = {value}\n"),
        ] {
            let refused = parse(&text).unwrap_err();
            assert!(
                refused.contains(&format!("{key} is gone: one GitHub App"))
                    && refused.contains("app_id, installation_id and key_file")
                    && refused.contains("docs/operations/GITHUB_SETUP.md"),
                "{text:?}: {refused}"
            );
        }
    }
    // A key no build ever read is still refused as unknown, never as retired.
    assert!(
        parse("policy_reader = 1\n")
            .unwrap_err()
            .contains("unknown field")
    );
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

#[test]
fn the_apps_installation_and_key_make_its_token_file_the_programs() {
    let config = parse(PUBLISH).unwrap();
    assert_eq!(config.app(), None, "kept by hand");
    assert_eq!(config.app_id, Some(43));
    let config = parse(&format!(
        "{PUBLISH}installation_id = 143\nkey_file = \"/keys/app.pem\"\n"
    ))
    .unwrap();
    assert_eq!(
        config.app(),
        Some(App {
            id: 43,
            installation: 143,
            key_file: "/keys/app.pem".into(),
        })
    );
    // The exchange alone may have its token made too, by the App app_id names.
    let exchange = parse(&format!(
        "{ON}issues = \"export\"\nexport_label = \"x\"\napp_id = 43\ninstallation_id = 143\n\
         key_file = \"/keys/app.pem\"\n"
    ))
    .unwrap();
    assert_eq!(exchange.app().unwrap().id, 43);
}

#[test]
fn the_apps_keys_that_do_not_hold_together_are_refused_by_name() {
    for (more, why) in [
        ("installation_id = 143\n", "installation_id needs key_file"),
        (
            "key_file = \"/keys/app.pem\"\n",
            "key_file needs installation_id",
        ),
        (
            "installation_id = 0\nkey_file = \"/keys/app.pem\"\n",
            "installation_id is a GitHub App installation's number",
        ),
        (
            "installation_id = 143\nkey_file = \"keys/app.pem\"\n",
            "key_file is an absolute path outside the repository",
        ),
        (
            "installation_id = 143\nkey_file = \"/secrets/token\"\n",
            "key_file is token_file: a token made would be written over the key",
        ),
    ] {
        let refused = parse(&format!("{PUBLISH}{more}")).unwrap_err();
        assert!(refused.contains(why), "{more}: {refused}");
    }
    for (text, why) in [
        (
            format!("{ON}installation_id = 143\nkey_file = \"/keys/a.pem\"\n"),
            "key_file needs app_id: the App the key is of",
        ),
        (
            "app_id = 43\ninstallation_id = 143\nkey_file = \"/keys/a.pem\"\n".to_string(),
            "key_file needs token_file: where the token it makes is kept",
        ),
    ] {
        let refused = parse(&text).unwrap_err();
        assert!(refused.contains(why), "{text}: {refused}");
    }
}

#[test]
fn verification_is_off_unless_the_settings_say_hosted_and_any_other_word_is_refused() {
    assert_eq!(parse(ON).unwrap().verification, Verification::Off);
    assert_eq!(Verification::Off.word(), "off");
    let on = parse(&format!("{ON}verification = \"hosted\"\n")).unwrap();
    assert_eq!(on.verification, Verification::Hosted);
    assert_eq!(on.verification.word(), "hosted");
    // It turns on nothing else.
    assert_eq!(on.releases, Releases::Off);
    assert_eq!(on.pull_requests, PullRequests::Off);
    assert!(parse(&format!("{ON}verification = \"github\"\n")).is_err());
    assert!(parse(&format!("{ON}verification = true\n")).is_err());
}

#[test]
fn releases_are_off_unless_the_settings_say_publish_and_any_other_word_is_refused() {
    assert_eq!(parse(ON).unwrap().releases, Releases::Off);
    assert_eq!(Releases::Off.word(), "off");
    let on = parse(&format!("{ON}releases = \"publish\"\n")).unwrap();
    assert_eq!(on.releases, Releases::Publish);
    assert_eq!(on.releases.word(), "publish");
    // It asks for nothing a delivery does: the one token and the repository `enabled` needs.
    assert_eq!(on.pull_requests, PullRequests::Off);
    assert!(parse(&format!("{ON}releases = \"draft\"\n")).is_err());
    assert!(parse(&format!("{ON}releases = true\n")).is_err());
}
