use std::path::PathBuf;

use serde_json::json;

use super::*;

/// A folder of the test's own under the system's temporary folder, removed with it.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let path = std::env::temp_dir().join(format!("synced-kept-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Folder(path)
    }

    fn keep(&self, name: &str, record: &Value) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, record.to_string()).unwrap();
        path
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn visit(run: usize, probe: Value, heap: f64) -> Visit {
    Visit {
        run,
        probe,
        busy: Some(80.0),
        memory: Memory {
            heap: Some(heap),
            server: Some(95e6),
            databases: Some(420e6),
            database_count: 1,
        },
        ..Visit::default()
    }
}

fn run(at: &str, epoch: i64) -> Run<'static> {
    Run {
        base: "http://h:1/p",
        runs: 2,
        tabs: 0,
        note: Some("before"),
        at: at.into(),
        epoch,
        checkout: Some("0123456789abcdef".into()),
        load: [Some([200.0, 180.0, 150.0]), Some([210.0, 190.0, 160.0])],
        busy: Some(91.5),
    }
}

fn measured() -> Vec<(String, Vec<Visit>)> {
    vec![(
        "/tasks".into(),
        vec![
            visit(
                0,
                json!({"synced": 700.0, "build": "/assets/symphony-shell-ab.js"}),
                9e6,
            ),
            visit(
                1,
                json!({"drawn": 100.0, "checked": 230.0, "synced": 240.0}),
                12e6,
            ),
            visit(
                2,
                json!({"drawn": 90.0, "checked": 200.0, "synced": 900.0}),
                13e6,
            ),
        ],
    )]
}

#[test]
fn a_record_holds_the_run_each_pages_median_and_first_and_every_visit() {
    let record = record(&run("2026-11-15 09:54:12", 100), &measured());
    assert_eq!(record["record"], 1);
    assert_eq!(record["at"], "2026-11-15 09:54:12");
    assert_eq!(record["base"], "http://h:1/p");
    assert_eq!(record["note"], "before");
    assert_eq!(record["checkout"], "0123456789abcdef");
    assert_eq!(record["load"]["end"], json!([210.0, 190.0, 160.0]));
    assert_eq!(record["cpu_busy"], 91.5);
    assert_eq!(record["memory"]["server"], 95e6);
    assert_eq!(record["memory"]["database_count"], 1);
    let page = &record["pages"][0];
    assert_eq!(page["page"], "/tasks");
    assert_eq!(page["build"], "/assets/symphony-shell-ab.js");
    assert_eq!(page["first"]["synced"], 700.0);
    assert_eq!(
        page["median"],
        json!({"run": 1, "drawn": 100.0, "checked": 230.0, "synced": 240.0, "heap": 12e6})
    );
    assert_eq!(page["visits"].as_array().map(Vec::len), Some(3));
    assert_eq!(page["visits"][2]["cpu_busy"], 80.0);
    assert_eq!(page["visits"][2]["memory"]["heap"], 13e6);
}

#[test]
fn a_run_is_kept_under_the_moment_it_began() {
    assert_eq!(file_name("2026-11-15 09:54:12"), "2026-11-15-095412.json");
}

#[test]
fn the_earlier_run_is_the_newest_kept_of_the_same_address() {
    let folder = Folder::new("earlier");
    assert!(
        earlier(&folder.0.join("none"), "http://h:1/p").is_none(),
        "no folder yet"
    );
    assert!(
        earlier(&folder.0, "http://h:1/p").is_none(),
        "nothing kept yet"
    );
    let older = record(&run("2026-11-15 08:00:00", 100), &measured());
    let newer = record(&run("2026-11-15 09:00:00", 200), &measured());
    let mut elsewhere = record(&run("2026-11-15 10:00:00", 300), &measured());
    elsewhere["base"] = json!("http://other:2");
    folder.keep("b.json", &older);
    let kept = folder.keep("a.json", &newer);
    folder.keep("c.json", &elsewhere);
    folder.keep("d.json", &json!({"not": "a run"}));
    std::fs::write(folder.0.join("e.json"), "not json").unwrap();
    folder.keep("f.txt", &newer);
    let (path, found) = earlier(&folder.0, "http://h:1/p").unwrap();
    assert_eq!(path, kept);
    assert_eq!(found["epoch"], 200);
}

#[test]
fn a_run_is_named_by_its_file_or_its_name_in_the_folder() {
    let folder = Folder::new("named");
    let kept = folder.keep(
        "2026-11-15-090000.json",
        &record(&run("2026-11-15 09:00:00", 1), &measured()),
    );
    for name in [
        kept.display().to_string(),
        "2026-11-15-090000.json".into(),
        "2026-11-15-090000".into(),
    ] {
        let (path, found) = named(&folder.0, &name).unwrap();
        assert!(
            path.ends_with("2026-11-15-090000.json"),
            "{name}: {}",
            path.display()
        );
        assert_eq!(found["epoch"], 1, "{name}");
    }
    let missing = named(&folder.0, "2026-01-01").unwrap_err();
    assert!(missing.contains("no kept run \"2026-01-01\""), "{missing}");
    folder.keep("other.json", &json!({"base": "x"}));
    let other = named(&folder.0, "other").unwrap_err();
    assert!(other.contains("is no kept run of synced"), "{other}");
}

#[test]
fn a_run_is_printed_beside_an_earlier_one_page_by_page() {
    let then = record(&run("2026-11-15 08:00:00", 100), &measured());
    let mut faster = measured();
    faster[0].1[1].probe = json!({"drawn": 20.0, "checked": 40.0, "synced": 48.0});
    faster[0].1[1].memory.heap = Some(11e6);
    faster.push((
        "/queue".into(),
        vec![visit(0, json!({"synced": 50.0}), 1e6)],
    ));
    let mut now_run = run("2026-11-15 10:00:00", 200);
    now_run.note = None;
    now_run.tabs = 5;
    let now = record(&now_run, &faster);
    let said = compare("artifacts/synced/2026-11-15-080000.json", &then, &now);
    let lines: Vec<&str> = said.lines().collect();
    assert!(
        lines[1].starts_with(
            "beside the run kept as artifacts/synced/2026-11-15-080000.json, made \
                              2026-11-15 08:00:00 (before); this one 2026-11-15 10:00:00"
        ),
        "{said}"
    );
    let side = |label: &str, then: &str| format!("  {label:<18} then {then:<44} now ");
    assert!(
        said.contains(&side("at the start", "load 200 180 150")),
        "{said}"
    );
    assert!(said.contains(&side("CPU busy", "92%")), "{said}");
    assert!(
        said.contains(&side(
            "memory at the end",
            "server 95 MB  board db 420 MB (1)"
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!("{}5\n", side("other tabs", "0"))),
        "{said}"
    );
    assert!(said.contains("then 0123456789"), "{said}");
    assert!(said.contains("then /assets/symphony-shell-ab.js"), "{said}");
    let tasks = lines
        .iter()
        .find(|line| line.trim_start().starts_with("/tasks"))
        .unwrap();
    assert!(tasks.contains("100/230/240 → 20/40/48"), "{tasks}");
    assert!(tasks.contains("5.0x faster"), "{tasks}");
    assert!(tasks.contains("700 → 700"), "{tasks}");
    assert!(tasks.ends_with("12 → 11 MB"), "{tasks}");
    assert!(
        said.contains(&format!("  {:<32} not measured then", "/queue")),
        "{said}"
    );
}

#[test]
fn a_time_moved_is_said_as_how_many_times_faster_or_slower() {
    assert_eq!(times(Some(1000.0), Some(100.0)), "10.0x faster");
    assert_eq!(times(Some(100.0), Some(150.0)), "1.5x slower");
    assert_eq!(times(Some(100.0), Some(100.0)), "the same");
    assert_eq!(times(None, Some(100.0)), "-");
    assert_eq!(times(Some(100.0), None), "-");
    assert_eq!(times(Some(0.0), Some(1.0)), "-");
}
