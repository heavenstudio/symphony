//! `symphony-github release`, what a release run asks for a tag it pushed: one GitHub Release
//! made for it, none made twice by a second run or after a lost answer, nothing asked of a
//! repository that is turned off, not the one bound, or without the tag at the release's commit,
//! and a GitHub that does not answer leaving nothing made and nothing kept.

use crate::github::Fault;
use crate::{Ran, Rig, TOKEN};
use serde_json::{Value, json};
use symphony_github::state::Event;

const TAG: &str = "release-2026-11-19";
const COMMIT: &str = "cccccccccccccccccccccccccccccccccccccccc";
const OTHER: &str = "dddddddddddddddddddddddddddddddddddddddd";
const RELEASES: &str = "/repos/o/n/releases";

/// A rig serving releases, the tag pushed.
fn rig(name: &str) -> Rig {
    let rig = Rig::new(name, "releases = \"publish\"");
    rig.github.tag(TAG, COMMIT);
    rig
}

fn request(tag: &str, commit: &str) -> Value {
    json!({
        "tag": tag, "commit": commit, "latest": true, "by": "release run",
        "body": "## release-2026-11-19\n\nCommit: cccc\n\n### Board\n\nNothing moves.\n",
    })
}

/// `release --request FILE` of `request`: what it ran as.
fn run(rig: &Rig, request: &Value) -> Ran {
    let file = rig.dir.0.join("release.json");
    std::fs::write(&file, request.to_string()).unwrap();
    rig.run(&["release", "--request", file.to_str().unwrap()])
}

/// The same, which must answer one line of JSON.
fn ask(rig: &Rig, request: &Value) -> Value {
    let ran = run(rig, request).ok();
    assert_eq!(ran.out.lines().count(), 1, "one line of JSON: {}", ran.out);
    assert!(!ran.out.contains(TOKEN), "{}", ran.out);
    serde_json::from_str(&ran.out).unwrap()
}

/// The requests made that change the repository.
fn writes(rig: &Rig) -> Vec<String> {
    (rig.github.asked().into_iter())
        .filter(|(method, ..)| method != "GET")
        .map(|(method, path, _)| format!("{method} {path}"))
        .collect()
}

fn events(rig: &Rig) -> Vec<Event> {
    (rig.journal().iter())
        .filter(|entry| entry.kind == "release")
        .map(|entry| entry.event)
        .collect()
}

#[test]
fn a_tag_is_published_once_with_its_notes_and_nothing_uploaded() {
    let rig = rig("release-once");
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "published", "{answer}");
    assert_eq!(answer["release"], TAG);
    assert_eq!(
        answer["url"],
        "https://github.com/o/n/releases/tag/release-2026-11-19"
    );
    let releases = rig.github.releases();
    assert_eq!(releases.len(), 1);
    let made = &releases[0];
    assert_eq!(made["tag_name"], TAG);
    assert_eq!(made["name"], TAG);
    assert_eq!(made["target_commitish"], COMMIT);
    assert_eq!(
        made["body"],
        "## release-2026-11-19\n\nCommit: cccc\n\n### Board\n\nNothing moves."
    );
    assert_eq!(made["draft"], false);
    assert_eq!(made["prerelease"], false);
    assert_eq!(made["make_latest"], "true");
    // One write, the release's creation: no asset, no edit, no tag.
    assert_eq!(writes(&rig), [format!("POST {RELEASES}")]);
    assert_eq!(rig.github.tags_made().len(), 0);
    assert_eq!(events(&rig), [Event::Intent, Event::Ack]);
    let state = rig.state();
    assert_eq!(state.binding.unwrap().repository_id, 77);
    let kept = &state.releases[TAG];
    assert_eq!((kept.id, kept.commit.as_str()), (701, COMMIT));

    // A second run finds it by its tag and makes none.
    let again = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(again["state"], "found", "{again}");
    assert_eq!(again["id"], 701);
    assert_eq!(writes(&rig).len(), 1);
    assert_eq!(rig.github.releases().len(), 1);
    assert_eq!(events(&rig), [Event::Intent, Event::Ack]);

    let status = rig.run(&["status"]).ok().out;
    assert!(
        status.contains("releases: publish (a release run's GitHub Release: `release`)"),
        "{status}"
    );
    assert!(
        status.contains(
            "  release-2026-11-19 is published as \
             https://github.com/o/n/releases/tag/release-2026-11-19"
        ),
        "{status}"
    );
}

#[test]
fn an_older_release_is_not_marked_the_latest_and_a_lightweight_tag_is_read_too() {
    let rig = Rig::new("release-older", "releases = \"publish\"");
    rig.github.tag_light("release-2026-11-12.2", OTHER);
    let mut older = request("release-2026-11-12.2", OTHER);
    older["latest"] = json!(false);
    assert_eq!(ask(&rig, &older)["state"], "published");
    assert_eq!(rig.github.releases()[0]["make_latest"], "false");
}

#[test]
fn a_lost_answer_finds_the_release_by_its_tag_and_makes_no_second() {
    let rig = rig("release-lost");
    rig.github.fault("POST", RELEASES, Fault::LoseAnswer);
    // The release was made and its answer lost; looked for at once, it is found.
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "published", "{answer}");
    assert_eq!(rig.github.releases().len(), 1);
    assert_eq!(events(&rig), [Event::Intent, Event::Unknown, Event::Ack]);
    assert_eq!(writes(&rig).len(), 1);
}

#[test]
fn a_lost_answer_not_found_at_once_is_unknown_and_the_next_run_finds_it() {
    let rig = rig("release-unknown");
    // The first look finds none, as it would; the creation's answer is lost; the look after it
    // is not answered either.
    rig.github.fault(
        "GET",
        "/repos/o/n/releases/tags/",
        Fault::Status(404, vec![]),
    );
    rig.github.fault("POST", RELEASES, Fault::LoseAnswer);
    rig.github.fault(
        "GET",
        "/repos/o/n/releases/tags/",
        Fault::Status(502, vec![]),
    );
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "unknown", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("its answer was lost"),
        "{answer}"
    );
    assert_eq!(rig.state().releases.len(), 0);
    assert_eq!(events(&rig), [Event::Intent, Event::Unknown]);

    let next = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(next["state"], "found", "{next}");
    assert_eq!(rig.github.releases().len(), 1);
    assert_eq!(writes(&rig).len(), 1, "no second release is asked for");
    assert_eq!(rig.state().releases[TAG].commit, COMMIT);
}

#[test]
fn turned_off_or_not_serving_releases_asks_github_nothing() {
    for (name, rig) in [
        ("none", Rig::off("release-off-none")),
        ("issues only", Rig::new("release-off-key", "")),
        ("not enabled", {
            let rig = Rig::off("release-off-disabled");
            rig.settings.set(Some(
                "enabled = false\nrepository = \"o/n\"\nreleases = \"publish\"\n".into(),
            ));
            rig
        }),
    ] {
        rig.github.tag(TAG, COMMIT);
        let answer = ask(&rig, &request(TAG, COMMIT));
        assert_eq!(answer["state"], "refused", "{name}: {answer}");
        assert!(
            answer["why"].as_str().unwrap().starts_with("turned off"),
            "{name}: {answer}"
        );
        assert!(rig.github.asked().is_empty(), "{name}");
        assert!(!rig.dir.0.join("state").exists(), "{name}: nothing is kept");
    }
    // Settings that do not read are turned off too.
    let rig = Rig::off("release-off-unread");
    rig.settings.set(Some("releases = \"sometimes\"\n".into()));
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "refused");
    assert_eq!(rig.github.asked().len(), 0);
}

#[test]
fn a_release_turned_off_names_its_own_setting_and_unread_settings_say_why() {
    let rig = Rig::new("release-off-said", "pull_requests = \"track\"");
    assert_eq!(
        ask(&rig, &request(TAG, COMMIT)),
        json!({
            "state": "refused",
            "why": "turned off: the committed settings do not say enabled = true and releases = \
                    \"publish\"",
        })
    );
    rig.settings.set(Some("releases = \"sometimes\"\n".into()));
    let answer = ask(&rig, &request(TAG, COMMIT));
    let why = answer["why"].as_str().unwrap();
    assert!(
        why.starts_with("turned off: its settings do not read: ") && why.contains("sometimes"),
        "{answer}"
    );
    assert!(!rig.dir.0.join("state").exists(), "nothing is kept");
}

#[test]
fn another_repository_than_the_one_bound_is_refused() {
    let rig = rig("release-unbound");
    assert_eq!(ask(&rig, &request(TAG, COMMIT))["state"], "published");
    rig.github.replace_repository(78);
    rig.github.tag("release-2026-11-20", OTHER);
    let answer = ask(&rig, &request("release-2026-11-20", OTHER));
    assert_eq!(answer["state"], "refused", "{answer}");
    assert!(
        (answer["why"].as_str().unwrap()).contains("nothing is published to another repository"),
        "{answer}"
    );
    assert_eq!(rig.github.releases().len(), 1);

    // A repository that is not there is refused, and one that moved.
    let gone = Rig::new("release-gone", "releases = \"publish\"");
    gone.github.rename("m");
    let answer = ask(&gone, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "refused", "{answer}");
    assert_eq!(writes(&gone).len(), 0);
}

#[test]
fn a_tag_github_does_not_hold_at_the_commit_is_never_made_by_a_release() {
    // No tag there: GitHub would make one.
    let rig = Rig::new("release-no-tag", "releases = \"publish\"");
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "refused", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("o/n holds no tag release-2026-11-19"),
        "{answer}"
    );
    // The tag names another commit, annotated or not.
    for light in [false, true] {
        let rig = Rig::new("release-other-commit", "releases = \"publish\"");
        if light {
            rig.github.tag_light(TAG, OTHER);
        } else {
            rig.github.tag(TAG, OTHER);
        }
        let answer = ask(&rig, &request(TAG, COMMIT));
        assert_eq!(answer["state"], "refused", "{answer}");
        assert!(
            (answer["why"].as_str().unwrap()).contains(&format!("names {OTHER}")),
            "{answer}"
        );
        assert_eq!(writes(&rig).len(), 0);
        assert_eq!(rig.github.tags_made().len(), 0);
        assert!(events(&rig).is_empty(), "nothing was dispatched");
    }
    assert_eq!(writes(&rig).len(), 0);
    assert_eq!(rig.github.tags_made().len(), 0);
}

#[test]
fn a_request_that_names_no_release_or_commit_is_refused_before_anything_is_asked() {
    let rig = rig("release-names");
    for (tag, commit) in [
        ("", COMMIT),
        ("release/2026", COMMIT),
        ("-release", COMMIT),
        ("release 1", COMMIT),
        (TAG, "cccc"),
        (TAG, "main"),
    ] {
        let answer = ask(&rig, &request(tag, commit));
        assert_eq!(answer["state"], "refused", "{tag:?} {commit:?}: {answer}");
    }
    assert_eq!(rig.github.asked().len(), 0);

    // A request that is no JSON object is the command's own refusal.
    let file = rig.dir.0.join("release.json");
    std::fs::write(&file, "[]").unwrap();
    let ran = rig.run(&["release", "--request", file.to_str().unwrap()]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.contains("a request is one JSON object"),
        "{}",
        ran.err
    );
    assert_eq!(rig.run(&["release"]).code, 1);
    assert_eq!(
        rig.run(&["release", "--request", file.to_str().unwrap(), "more"])
            .code,
        1
    );
}

#[test]
fn a_github_that_does_not_answer_makes_nothing_and_a_later_run_publishes() {
    for (method, path, fault, wanted) in [
        (
            "GET",
            "/repos/o/n",
            Fault::Status(503, vec![]),
            "unavailable",
        ),
        (
            "GET",
            "/repos/o/n",
            Fault::Status(401, vec![]),
            "unavailable",
        ),
        (
            "GET",
            "/repos/o/n/releases/tags/",
            Fault::Status(429, vec![("retry-after", "30".into())]),
            "unavailable",
        ),
        (
            "GET",
            "/repos/o/n/git/ref/tags/",
            Fault::Status(500, vec![]),
            "unavailable",
        ),
        ("POST", RELEASES, Fault::Status(401, vec![]), "unavailable"),
        ("POST", RELEASES, Fault::Status(409, vec![]), "unavailable"),
        // The token may not write contents: running again as things are makes none.
        ("POST", RELEASES, Fault::Status(403, vec![]), "refused"),
        ("POST", RELEASES, Fault::Status(422, vec![]), "refused"),
    ] {
        let rig = rig("release-unanswered");
        rig.github.fault(method, path, fault.clone());
        let answer = ask(&rig, &request(TAG, COMMIT));
        assert_eq!(
            answer["state"], wanted,
            "{method} {path} {fault:?}: {answer}"
        );
        assert_eq!(rig.github.releases().len(), 0);
        assert_eq!(rig.state().releases.len(), 0);
        if method == "POST" {
            assert_eq!(events(&rig), [Event::Intent, Event::Failed]);
        } else {
            assert_eq!(events(&rig).len(), 0);
        }
        let later = ask(&rig, &request(TAG, COMMIT));
        assert_eq!(later["state"], "published", "{later}");
        assert_eq!(rig.github.releases().len(), 1);
    }
}

#[test]
fn a_release_made_meanwhile_is_found_and_one_removed_there_is_not_made_again() {
    let rig = rig("release-raced");
    assert_eq!(ask(&rig, &request(TAG, COMMIT))["state"], "published");
    // Another run made it between this one's look, answered with nothing there, and its
    // creation: the creation is refused as there already, and the look after it finds it.
    rig.github.fault(
        "GET",
        "/repos/o/n/releases/tags/",
        Fault::Status(404, vec![]),
    );
    let mut state = rig.state();
    state.releases.clear();
    rig.store().save(&state).unwrap();
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "found", "{answer}");
    assert_eq!(rig.github.releases().len(), 1);

    // A person removed the release at GitHub: what was published here is not made again.
    rig.github.remove_release(TAG);
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "refused", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("it is not made again"),
        "{answer}"
    );
    assert_eq!(rig.github.releases().len(), 0);
}

#[test]
fn a_token_file_that_is_not_kept_right_is_refused_and_shown_nowhere() {
    let rig = rig("release-token");
    std::fs::remove_file(&rig.token_file).unwrap();
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "refused", "{answer}");
    assert_eq!(rig.github.asked().len(), 0);
}

#[test]
fn a_second_writer_of_the_state_and_an_annotated_tag_not_answered_make_nothing() {
    // Another run holds the state's lock: nothing is asked of GitHub.
    let rig = rig("release-locked");
    assert_eq!(ask(&rig, &request(TAG, COMMIT))["state"], "published");
    let asked = rig.github.asked().len();
    {
        let _held = rig.store().lock().unwrap();
        let answer = ask(&rig, &request(TAG, COMMIT));
        assert_eq!(answer["state"], "unavailable", "{answer}");
        assert!(
            answer["why"]
                .as_str()
                .unwrap()
                .contains("one writer at a time"),
            "{answer}"
        );
    }
    assert_eq!(rig.github.asked().len(), asked);

    // The tag's own object is not answered: unavailable, and nothing is dispatched.
    let rig = rig_with_tag_fault();
    let answer = ask(&rig, &request(TAG, COMMIT));
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert_eq!(writes(&rig).len(), 0);
    assert_eq!(events(&rig).len(), 0);
    assert_eq!(ask(&rig, &request(TAG, COMMIT))["state"], "published");
}

fn rig_with_tag_fault() -> Rig {
    let rig = rig("release-tag-object");
    rig.github
        .fault("GET", "/repos/o/n/git/tags/", Fault::Status(500, vec![]));
    rig
}
