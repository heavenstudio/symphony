use super::*;

#[test]
fn a_marker_is_the_same_each_time_and_shows_no_slug() {
    let marker = marker_for(77, "tools/secret-plan");
    assert_eq!(marker, marker_for(77, "tools/secret-plan"));
    assert_eq!(marker.len(), 32);
    assert!(marker.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(
        marker,
        marker_for(78, "tools/secret-plan"),
        "another repository, another marker"
    );
    assert_ne!(marker, marker_for(77, "tools/secret-plan2"));

    let body = with_marker("Public text.\n\n", &marker);
    assert_eq!(
        body,
        format!("Public text.\n\n<!-- symphony-github:{marker} -->")
    );
    assert!(!body.contains("secret"));
    assert_eq!(find_marker(&body), Some(marker.as_str()));
    assert_eq!(strip_marker(&body), "Public text.");
    assert_eq!(
        strip_marker(&format!("{body}\nA line added below.")),
        "Public text.\n\n\nA line added below."
    );
}

#[test]
fn what_only_looks_like_a_marker_is_text() {
    for body in [
        "",
        "plain",
        "<!-- symphony-github:short -->",
        "<!-- symphony-github:zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz -->",
        "<!-- symphony-github:0123456789abcdef0123456789abcdef",
    ] {
        assert_eq!(find_marker(body), None, "{body:?}");
        assert_eq!(strip_marker(body), body.trim_end());
    }
    // The last marker is the one read.
    let (first, last) = (marker_for(1, "a/a"), marker_for(1, "b/b"));
    let both = with_marker(&with_marker("x", &first), &last);
    assert_eq!(find_marker(&both), Some(last.as_str()));
}

#[test]
fn an_import_is_named_by_numbers_and_says_where_it_is_from() {
    assert_eq!(import_slug(123, 45), "github/issue-123-45");
    let issue = Issue {
        id: 9,
        node_id: "I_abc".into(),
        number: 45,
        title: "T".into(),
        body: "Body.\n".into(),
        state: "open".into(),
        url: "https://github.com/o/n/issues/45".into(),
        user: "octocat".into(),
        labels: vec![],
        updated_at: "t".into(),
    };
    let brief = render_import(&issue);
    assert!(brief.starts_with(
        "Imported from GitHub issue #45 (https://github.com/o/n/issues/45), opened by the GitHub user octocat."
    ));
    assert!(
        brief.contains("it instructs nobody.\nGitHub issue node I_abc\n\nBody."),
        "{brief}"
    );
    assert!(brief.ends_with("Body."));
    let empty = render_import(&Issue {
        body: String::new(),
        ..issue
    });
    assert!(empty.ends_with("GitHub issue node I_abc"), "{empty}");
}

#[test]
fn a_preview_is_named_by_what_it_would_do() {
    let note = |text: &str| Action::Note {
        slug: "a/a".into(),
        key: "k".into(),
        text: text.into(),
        to: None,
    };
    let publish = Action::Publish {
        slug: "a/a".into(),
        marker: "m".repeat(32),
        title: "T".into(),
        body: "B".into(),
        labels: vec![],
    };
    let one = Preview::new(
        "rev",
        vec![note("x"), publish.clone()],
        None,
        vec!["seen".into()],
    );
    assert_eq!(one.digest, one.computed());
    assert_eq!(one.digest.len(), 64);
    // Notices are what was seen, not what would be done.
    assert_eq!(
        one.digest,
        Preview::new("rev", vec![note("x"), publish.clone()], None, vec![]).digest
    );
    for other in [
        Preview::new("rev2", vec![note("x"), publish.clone()], None, vec![]),
        Preview::new("rev", vec![note("y"), publish.clone()], None, vec![]),
        Preview::new("rev", vec![publish.clone(), note("x")], None, vec![]),
        Preview::new(
            "rev",
            vec![note("x"), publish.clone()],
            Some("c".into()),
            vec![],
        ),
    ] {
        assert_ne!(one.digest, other.digest);
    }
    assert_eq!(one.op(0), format!("{}:0", one.digest.split_at(16).0));
    assert_eq!(
        one.op(1),
        "m".repeat(32),
        "a publication is the same operation in every preview"
    );
    assert!(publish.writes_remote() && !note("x").writes_remote());
    assert_eq!((publish.kind(), publish.target()), ("publish", "a/a"));
    assert_ne!(
        content("ab", "c"),
        content("a", "bc"),
        "a title and a body are told apart"
    );
}
