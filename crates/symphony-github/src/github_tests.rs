use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;

/// Answers handed out in order, each request kept.
#[derive(Default)]
struct Script {
    answers: RefCell<VecDeque<Result<Response, Failure>>>,
    asked: RefCell<Vec<Request>>,
}

impl Script {
    fn of(answers: Vec<Result<Response, Failure>>) -> Script {
        Script {
            answers: RefCell::new(answers.into()),
            asked: RefCell::default(),
        }
    }
}

impl Transport for Script {
    fn send(&self, request: &Request) -> Result<Response, Failure> {
        self.asked.borrow_mut().push(request.clone());
        self.answers
            .borrow_mut()
            .pop_front()
            .expect("an answer for each request")
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "an answer among the failures a script hands out"
)]
fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> Result<Response, Failure> {
    Ok(Response {
        status,
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_string(), (*v).to_string()))
            .collect(),
        body: body.into(),
    })
}

fn repository() -> Repository {
    Repository {
        owner: "o".into(),
        name: "n".into(),
    }
}

fn item(id: i64) -> Value {
    json!({"id": id, "node_id": format!("I{id}"), "number": id, "title": "t", "body": "b",
           "state": "open", "html_url": "u", "user": {"login": "l"}, "labels": [{"name": "x"}, "y"],
           "updated_at": "2026-01-01T00:00:00Z"})
}

#[test]
fn remote_text_is_cleaned_to_content() {
    assert_eq!(
        clean_title("  a\tb\r\nc\u{0}d\u{202e}  "),
        "a b c d\u{202e}"
    );
    assert_eq!(clean_title(""), "");
    assert_eq!(clean_title(&"t".repeat(500)).chars().count(), 200);
    assert_eq!(clean_body("a\r\nb\u{0}\u{8}c\td\n\n"), "a\nbc\td");
    let long = clean_body(&"\u{e9}".repeat(60_001));
    assert!(long.ends_with("\n\n[cut here: the remote text is longer]"));
    assert_eq!(long.chars().filter(|c| *c == '\u{e9}').count(), 60_000);
    assert_eq!(
        clean_body(&"x".repeat(60_000)).len(),
        60_000,
        "the longest body kept whole"
    );
}

#[test]
fn every_status_is_read_for_what_it_means() {
    let repo = repository();
    let get = |answered| {
        let script = Script::of(vec![answered]);
        Client::new(&script, &repo, 100).repository()
    };
    assert_eq!(
        get(answer(
            200,
            &[],
            r#"{"id":7,"node_id":"R7","full_name":"o/n"}"#
        )),
        Ok(Repo {
            id: 7,
            node_id: "R7".into(),
            full_name: "o/n".into()
        })
    );
    assert_eq!(get(answer(401, &[], "{}")), Err(ApiError::Unauthorized));
    assert_eq!(get(answer(404, &[], "{}")), Err(ApiError::NotFound));
    assert_eq!(get(answer(410, &[], "{}")), Err(ApiError::NotFound));
    assert_eq!(get(answer(301, &[], "{}")), Err(ApiError::Moved));
    assert_eq!(
        get(answer(429, &[("retry-after", "30")], "{}")),
        Err(ApiError::RateLimited(Some(130)))
    );
    assert_eq!(
        get(answer(429, &[], "{}")),
        Err(ApiError::RateLimited(None))
    );
    assert_eq!(
        get(answer(403, &[("retry-after", "-5")], "{}")),
        Err(ApiError::RateLimited(Some(100)))
    );
    assert_eq!(
        get(answer(
            403,
            &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "900")],
            "{}"
        )),
        Err(ApiError::RateLimited(Some(900)))
    );
    // A 403 that is no rate limit, a 500 to a read and an answer that is not JSON are none of
    // those: plain failures of a read.
    assert!(matches!(
        get(answer(403, &[("x-ratelimit-remaining", "12")], "{}")),
        Err(ApiError::Unexpected(_))
    ));
    assert!(matches!(
        get(answer(500, &[], "")),
        Err(ApiError::Unexpected(_))
    ));
    assert!(matches!(
        get(answer(200, &[], "<html>")),
        Err(ApiError::Unexpected(_))
    ));
    assert!(
        matches!(get(answer(200, &[], "{}")), Err(ApiError::Unexpected(_))),
        "no id"
    );
    assert!(matches!(
        get(Err(Failure::NotSent("x".into()))),
        Err(ApiError::NotSent(_))
    ));
    assert!(matches!(
        get(Err(Failure::Unknown("x".into()))),
        Err(ApiError::Unexpected(_))
    ));
    for error in [
        ApiError::Unauthorized,
        ApiError::NotFound,
        ApiError::Moved,
        ApiError::RateLimited(None),
        ApiError::RateLimited(Some(5)),
        ApiError::Unknown("u".into()),
        ApiError::NotSent("n".into()),
    ] {
        assert_ne!(error.said(), "");
    }
}

#[test]
fn a_write_whose_result_is_not_known_is_told_from_one_refused() {
    let repo = repository();
    let create = |answered| {
        let script = Script::of(vec![answered]);
        Client::new(&script, &repo, 0).create("t", "b", &["l".to_string()])
    };
    assert_eq!(
        create(answer(201, &[], &item(3).to_string()))
            .unwrap()
            .number,
        3
    );
    assert!(matches!(
        create(Err(Failure::Unknown("cut".into()))),
        Err(ApiError::Unknown(_))
    ));
    assert!(matches!(
        create(answer(502, &[], "")),
        Err(ApiError::Unknown(_))
    ));
    assert!(
        matches!(
            create(answer(201, &[], "not json")),
            Err(ApiError::Unknown(_))
        ),
        "made, unread"
    );
    assert!(
        matches!(create(answer(201, &[], "{}")), Err(ApiError::Unknown(_))),
        "made, no id"
    );
    assert!(matches!(
        create(Err(Failure::NotSent("x".into()))),
        Err(ApiError::NotSent(_))
    ));
    assert!(
        matches!(create(answer(422, &[], "{}")), Err(ApiError::Unexpected(_))),
        "refused: not made"
    );

    let script = Script::of(vec![answer(200, &[], &item(3).to_string())]);
    Client::new(&script, &repo, 0).update(3, "T", "B").unwrap();
    let asked = script.asked.borrow();
    assert_eq!(
        (asked[0].method, asked[0].path.as_str()),
        ("PATCH", "/repos/o/n/issues/3")
    );
    let sent: Value = serde_json::from_str(asked[0].body.as_deref().unwrap()).unwrap();
    assert_eq!(
        sent,
        json!({"title": "T", "body": "B"}),
        "labels, state and people are not sent"
    );
}

#[test]
fn a_listing_reads_each_page_leaves_pull_requests_out_and_says_what_it_is() {
    let repo = repository();
    let next = [(
        "link",
        "<https://api.github.com/x?page=2>; rel=\"next\", <y>; rel=\"last\"",
    )];
    let pull = json!({"id": 50, "number": 50, "pull_request": {"url": "u"}});
    let script = Script::of(vec![
        answer(200, &next, &json!([item(1), pull, item(2)]).to_string()),
        answer(
            200,
            &[("link", "<y>; rel=\"prev\"")],
            &json!([item(2), item(3)]).to_string(),
        ),
    ]);
    let listing = Client::new(&script, &repo, 0)
        .issues(Some("2026-01-01T00:00:00Z"), 20)
        .unwrap();
    assert_eq!(
        listing
            .issues
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(listing.all);
    assert!(!listing.stable, "issue 2 was read twice");
    assert_eq!(listing.issues[0].labels, ["x", "y"]);
    let asked = script.asked.borrow();
    assert_eq!(
        asked[0].path,
        "/repos/o/n/issues?state=all&sort=updated&direction=asc&per_page=100&page=1&since=2026-01-01T00:00:00Z"
    );
    assert!(
        asked[1].path.contains("&page=2&since="),
        "the next page by its number, never a link's URL"
    );

    // The page limit: what was read, said not to be all.
    let script = Script::of(vec![answer(200, &next, &json!([item(1)]).to_string())]);
    let listing = Client::new(&script, &repo, 0).issues(None, 1).unwrap();
    assert!(listing.stable && !listing.all);
    assert!(!script.asked.borrow()[0].path.contains("since"));

    // A cursor that is no timestamp is not sent; a page that fails fails the listing.
    let script = Script::of(vec![answer(200, &next, "[]"), answer(500, &[], "")]);
    assert!(
        Client::new(&script, &repo, 0)
            .issues(Some("x&labels=y"), 5)
            .is_err()
    );
    assert!(!script.asked.borrow()[0].path.contains("labels"));
    for wrong in ["{}", "[{\"title\":\"no id\"}]"] {
        let script = Script::of(vec![answer(200, &[], wrong)]);
        assert!(matches!(
            Client::new(&script, &repo, 0).issues(None, 5),
            Err(ApiError::Unexpected(_))
        ));
    }
}

#[test]
fn one_issue_and_its_comments_are_read() {
    let repo = repository();
    let script = Script::of(vec![
        answer(200, &[], &item(4).to_string()),
        answer(200, &[], &json!({"id": 9, "number": 9, "pull_request": {}}).to_string()),
        answer(404, &[], "{}"),
        answer(200, &[], &json!([
            {"id": 1, "user": {"login": "a\nb"}, "body": "x\r\ny", "html_url": "u", "updated_at": "t"},
            {"body": "no id"}
        ]).to_string()),
        answer(200, &[], "{}"),
    ]);
    let client = Client::new(&script, &repo, 0);
    assert_eq!(client.issue(4).unwrap().user, "l");
    assert!(
        matches!(client.issue(9), Err(ApiError::Unexpected(why)) if why.contains("pull request"))
    );
    assert_eq!(client.issue(5), Err(ApiError::NotFound));
    let comments = client.comments(4, Some("2026-01-01T00:00:00Z")).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(
        (comments[0].user.as_str(), comments[0].body.as_str()),
        ("a b", "x\ny")
    );
    assert_eq!(client.comments(4, Some("bad value")).unwrap(), []);
    let asked = script.asked.borrow();
    assert_eq!(
        asked[3].path,
        "/repos/o/n/issues/4/comments?per_page=100&since=2026-01-01T00:00:00Z"
    );
    assert_eq!(asked[4].path, "/repos/o/n/issues/4/comments?per_page=100");
}
