use super::*;
use std::path::Path;

fn repository() -> Repository {
    Repository {
        owner: "Owner".into(),
        name: "a.name".into(),
    }
}

#[test]
fn only_an_address_of_the_repository_on_github_names_it() {
    for url in [
        "https://github.com/Owner/a.name",
        "https://github.com/owner/A.NAME.git",
        "https://github.com/Owner/a.name/",
        "git@github.com:Owner/a.name.git",
        "ssh://git@github.com/Owner/a.name",
    ] {
        assert!(names(url, &repository()), "{url}");
    }
    for url in [
        "https://github.com/Owner/a.name2",
        "https://github.com/Other/a.name",
        "https://github.example.com/Owner/a.name",
        "https://example.com/github.com/Owner/a.name",
        "http://github.com/Owner/a.name",
        "/srv/git/Owner/a.name.git",
        "",
    ] {
        assert!(!names(url, &repository()), "{url}");
    }
}

fn snapshot(eligible: bool, standing: &str) -> Snapshot {
    Snapshot {
        version: 1,
        slug: "sim/jump".into(),
        task: 7,
        head: "a".repeat(40),
        assessed: eligible,
        eligible,
        revision: "r1".into(),
        stamps: vec![Stamp {
            name: "tests".into(),
            standing: standing.into(),
        }],
    }
}

#[test]
fn the_verdict_is_last_when_it_passes_and_first_when_it_fails() {
    let said = |snapshot: &Snapshot| -> Vec<(String, &'static str)> {
        (checks(snapshot, 77).unwrap().into_iter())
            .map(|check| (check.name, check.conclusion))
            .collect()
    };
    assert_eq!(
        said(&snapshot(true, "met")),
        [
            (ASSESSMENT.to_string(), "success"),
            ("symphony/tests".to_string(), "success"),
            (ELIGIBLE.to_string(), "success"),
        ]
    );
    assert_eq!(
        said(&snapshot(false, "missing")),
        [
            (ELIGIBLE.to_string(), "failure"),
            (ASSESSMENT.to_string(), "failure"),
            ("symphony/tests".to_string(), "failure"),
        ]
    );
    assert_eq!(said(&snapshot(true, "waived"))[1].1, "neutral");
    assert!(checks(&snapshot(true, "perhaps"), 77).is_err());
}

#[test]
fn a_check_is_found_again_only_by_everything_it_says() {
    let id =
        |snapshot: &Snapshot, repository: i64| checks(snapshot, repository).unwrap()[1].id.clone();
    let first = snapshot(true, "met");
    assert_eq!(id(&first, 77), id(&snapshot(true, "met"), 77));
    assert_ne!(id(&first, 77), id(&first, 78), "another repository");
    let mut other = snapshot(true, "met");
    other.revision = "r2".into();
    assert_ne!(id(&first, 77), id(&other, 77), "other evidence");
    let mut other = snapshot(true, "met");
    other.head = "b".repeat(40);
    assert_ne!(id(&first, 77), id(&other, 77), "another head");
    let mut other = snapshot(true, "met");
    other.task = 8;
    assert_ne!(id(&first, 77), id(&other, 77), "another task of the slug");
}

fn git(dir: &Path, args: &[&str]) -> String {
    let ran = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8_lossy(&ran.stdout).trim().to_string()
}

#[test]
fn the_projects_git_pushes_a_head_to_its_branch_and_never_over_another_history() {
    let dir = std::env::temp_dir().join(format!(
        "symphony-github-deliver-git-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let (main, bare) = (dir.join("main"), dir.join("origin.git"));
    std::fs::create_dir_all(&main).unwrap();
    git(
        &dir,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&main, &["init", "-q", "-b", "main"]);
    let remote = Git { main: main.clone() };
    assert!(remote.url().is_err(), "no origin");
    git(&main, &["remote", "add", "origin", bare.to_str().unwrap()]);
    assert_eq!(remote.url().unwrap(), bare.to_str().unwrap());
    git(&main, &["commit", "-q", "--allow-empty", "-m", "one"]);
    let one = git(&main, &["rev-parse", "HEAD"]);
    remote.push(&one, "sim/jump").unwrap();
    remote.push(&one, "sim/jump").unwrap();
    assert_eq!(git(&bare, &["rev-parse", "refs/heads/sim/jump"]), one);
    // A commit that does not descend from what the branch holds there is not forced over it.
    git(&main, &["checkout", "-q", "--orphan", "other"]);
    git(
        &main,
        &["commit", "-q", "--allow-empty", "-m", "another history"],
    );
    let other = git(&main, &["rev-parse", "HEAD"]);
    assert!(remote.push(&other, "sim/jump").is_err());
    assert_eq!(git(&bare, &["rev-parse", "refs/heads/sim/jump"]), one);
    let _ = std::fs::remove_dir_all(&dir);
}
