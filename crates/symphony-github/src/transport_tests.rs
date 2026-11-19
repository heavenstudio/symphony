use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const TOKEN: &str = "ghp_transportTOKEN123";

fn curl(base: &str) -> Curl {
    Curl::new(base, Secret::new(TOKEN).unwrap()).unwrap()
}

fn get(path: &str) -> Request {
    Request {
        method: "GET",
        path: path.into(),
        body: None,
    }
}

/// A program at a path of its own that runs `script` under `sh`.
fn program(name: &str, script: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "symphony-github-curl-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("curl");
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn only_an_https_origin_or_a_loopback_port_is_reached() {
    for base in [
        GITHUB,
        "https://github.example.com",
        "http://127.0.0.1:8080",
    ] {
        assert!(
            Curl::new(base, Secret::new(TOKEN).unwrap()).is_ok(),
            "{base}"
        );
    }
    for base in [
        "http://api.github.com",
        "http://127.0.0.1",
        "http://127.0.0.1:80/path",
        "http://localhost:80",
        "https://",
        "https://host/path",
        "https://user@host",
        "file:///etc/passwd",
        "",
    ] {
        let refused = Curl::new(base, Secret::new(TOKEN).unwrap())
            .err()
            .expect(base);
        assert!(refused.contains("not an https origin"), "{refused}");
    }
}

#[test]
fn the_whole_request_is_curls_standard_input() {
    let request = Request {
        method: "POST",
        path: "/repos/o/n/issues".into(),
        body: Some(r#"{"title":"a \"quoted\" \\ title","body":"line\nbreak"}"#.into()),
    };
    let config = curl(GITHUB).config(&request).unwrap();
    assert_eq!(
        config,
        format!(
            "url = \"https://api.github.com/repos/o/n/issues\"\n\
             request = \"POST\"\n\
             header = \"Authorization: Bearer {TOKEN}\"\n\
             header = \"Accept: application/vnd.github+json\"\n\
             header = \"X-GitHub-Api-Version: 2022-11-28\"\n\
             header = \"User-Agent: symphony-github\"\n\
             header = \"Content-Type: application/json\"\n\
             data-binary = \"{{\\\"title\\\":\\\"a \\\\\\\"quoted\\\\\\\" \\\\\\\\ title\\\",\\\"body\\\":\\\"line\\\\nbreak\\\"}}\"\n"
        )
    );
    assert!(
        !curl(GITHUB)
            .config(&get("/repos/o/n"))
            .unwrap()
            .contains("data-binary")
    );

    for path in [
        "repos/o/n",
        "/repos/o/n\"\nurl = \"https://evil",
        "/a b",
        "/a\\b",
        "/a\nb",
    ] {
        assert!(
            matches!(curl(GITHUB).config(&get(path)), Err(Failure::NotSent(_))),
            "{path:?}"
        );
    }
    for body in ["@/etc/passwd", "{\"a\":1}\n{\"b\":2}"] {
        let request = Request {
            method: "POST",
            path: "/x".into(),
            body: Some(body.into()),
        };
        assert!(
            matches!(curl(GITHUB).config(&request), Err(Failure::NotSent(_))),
            "{body:?}"
        );
    }
}

#[test]
fn an_answer_is_read_past_interim_ones() {
    let read = parse(
        b"HTTP/1.1 100 Continue\r\n\r\nHTTP/2 201 \r\nX-RateLimit-Remaining: 4999\r\nLink: <u>; rel=\"next\"\r\n\r\n{\"id\":1}\r\n\r\ntail",
    )
    .unwrap();
    assert_eq!(read.status, 201);
    assert_eq!(read.headers["x-ratelimit-remaining"], "4999");
    assert_eq!(read.headers["link"], "<u>; rel=\"next\"");
    assert_eq!(read.body, "{\"id\":1}\r\n\r\ntail");
    assert_eq!(parse(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap().body, "");
    for wrong in [
        &b""[..],
        b"HTTP/1.1 200 OK\r\nno end",
        b"garbage\r\n\r\n",
        b"HTTP/1.1 abc\r\n\r\n",
    ] {
        assert!(
            parse(wrong).is_err(),
            "{:?}",
            String::from_utf8_lossy(wrong)
        );
    }
}

#[test]
fn the_token_is_in_no_argument_and_no_environment_variable() {
    let seen =
        std::env::temp_dir().join(format!("symphony-github-curl-seen-{}", std::process::id()));
    let fake = program(
        "records",
        &format!(
            "{{ echo \"ARGS: $*\"; env; }} > '{0}'\ncat > '{0}.stdin'\nprintf 'HTTP/1.1 200 OK\\r\\n\\r\\n{{}}'",
            seen.display()
        ),
    );
    let response = curl(GITHUB)
        .program(fake.to_str().unwrap())
        .send(&get("/repos/o/n"))
        .unwrap();
    assert_eq!((response.status, response.body.as_str()), (200, "{}"));
    let outside = fs::read_to_string(&seen).unwrap();
    assert!(
        outside.starts_with("ARGS: -q --config - --silent --show-error --include --max-time 60 --max-redirs 0 --proto =http,https\n"),
        "{outside}"
    );
    assert!(
        !outside.contains(TOKEN),
        "neither argv nor the environment holds the token"
    );
    assert!(!outside.contains("api.github.com"), "nor the URL");
    let stdin = fs::read_to_string(format!("{}.stdin", seen.display())).unwrap();
    assert!(stdin.contains(&format!("Authorization: Bearer {TOKEN}")));
    let _ = fs::remove_file(&seen);
    let _ = fs::remove_file(format!("{}.stdin", seen.display()));
}

#[test]
fn a_failure_says_whether_the_request_left_and_never_the_token() {
    // curl's own words may repeat what it was given: they are redacted.
    for (code, sent) in [
        (2, false),
        (3, false),
        (6, false),
        (7, false),
        (28, true),
        (35, true),
        (52, true),
        (56, true),
    ] {
        let fake = program(
            &format!("fails-{code}"),
            &format!(
                "cat > /dev/null\necho 'curl: ({code}) failed with header Authorization: Bearer {TOKEN}' >&2\nexit {code}"
            ),
        );
        let failure = curl(GITHUB)
            .program(fake.to_str().unwrap())
            .send(&get("/repos/o/n"))
            .unwrap_err();
        let (why, left) = match &failure {
            Failure::NotSent(why) => (why, false),
            Failure::Unknown(why) => (why, true),
        };
        assert_eq!(left, sent, "curl exit {code}: {failure:?}");
        assert!(why.contains("Bearer [redacted]"), "{why}");
        assert!(!why.contains(TOKEN), "{why}");
    }

    // An answer that is no HTTP answer, the token echoed in it, is not known to be unsent.
    let fake = program(
        "echoes",
        &format!("cat > /dev/null\nprintf 'not http {TOKEN}'"),
    );
    let failure = curl(GITHUB)
        .program(fake.to_str().unwrap())
        .send(&get("/x"))
        .unwrap_err();
    assert!(
        matches!(&failure, Failure::Unknown(why) if !why.contains(TOKEN)),
        "{failure:?}"
    );
}

#[test]
fn a_machine_without_curl_is_told_so_and_nothing_is_sent() {
    let failure = curl(GITHUB)
        .program("/nonexistent/symphony-github-no-curl")
        .send(&get("/repos/o/n"))
        .unwrap_err();
    let Failure::NotSent(why) = failure else {
        panic!("{failure:?}");
    };
    assert!(why.contains("is not on PATH"), "{why}");
    assert!(why.contains("install it; nothing was sent"), "{why}");
    assert!(!why.contains(TOKEN));
}

#[test]
fn the_real_curl_fails_closed_on_a_port_nobody_answers() {
    // A port bound and released: nothing listens there, so the connection is refused.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let failure = curl(&format!("http://127.0.0.1:{port}"))
        .send(&get("/repos/o/n"))
        .unwrap_err();
    match failure {
        Failure::NotSent(why) => assert!(!why.contains(TOKEN), "{why}"),
        Failure::Unknown(why) => panic!("a refused connection sent nothing: {why}"),
    }
}
