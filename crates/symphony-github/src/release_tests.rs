use super::*;

#[test]
fn a_body_is_cleaned_to_content_and_cut_with_a_line_saying_where_the_rest_is() {
    assert_eq!(body("  one\r\ntwo\u{7}\tthree\n\n"), "one\ntwo\tthree");
    assert_eq!(body(""), "");
    let exact = "a".repeat(BODY_CHARS);
    assert_eq!(body(&exact), exact);
    let long = "é".repeat(BODY_CHARS + 1);
    let cut = body(&long);
    assert_eq!(
        cut,
        format!(
            "{}\n\n[cut here: `symphony release-notes` prints the release's notes whole]",
            "é".repeat(BODY_CHARS)
        )
    );
}

#[test]
fn a_release_answered_without_a_number_is_none() {
    let commit = "c".repeat(40);
    assert_eq!(released(&json!({"html_url": "u"}), &commit), None);
    assert_eq!(
        released(&json!({"id": 7}), &commit),
        Some(Released {
            id: 7,
            url: String::new(),
            commit
        })
    );
}
