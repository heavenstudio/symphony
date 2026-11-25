//! The Runs page's reads over fixture day files: the asks read from the query; the list newest
//! first, each run once across the days its lines are in, its steps left out, its filters and its
//! pages, reading no further back than a full page needs; what is not read named; each file's keys
//! kept between reads until the file changes, for the window's days alone, a file that grew read
//! on from its last line, once whatever other answers read it at once or hold it, one that shrank,
//! was replaced or changed under a kept offset parsed whole, each answer as a whole parse would
//! give it; a run opened on its steps in order; and the routes refusing a bad ask before any hub
//! and answering through one.

use super::*;
use crate::testing::Tree;
use std::sync::Arc;
use symphony_api::{FailedTest, RUN_SCHEMA};

/// 2026-11-15, 00:00 UTC.
const TODAY: f64 = 1_794_700_800.0;
const NOW: f64 = TODAY + 12.0 * 3_600.0;

type DayLockHook = Box<dyn FnOnce()>;
thread_local! {
    static BEFORE_DAY_LOCK: std::cell::RefCell<Option<DayLockHook>> = const { std::cell::RefCell::new(None) };
}

/// Hold a reader before the index's lock, on this test's thread alone.
pub(super) fn before_day_lock() {
    let hook = BEFORE_DAY_LOCK.take();
    if let Some(hook) = hook {
        hook();
    }
}

/// A run's id, 32 hex digits, from a short name.
fn id(name: &str) -> String {
    format!("{name:0>32}")
}

/// A start line of `name`, asked for at `queued`, of `pipeline`.
fn start(name: &str, pipeline: &str, queued: f64) -> RunLine {
    RunLine {
        schema: RUN_SCHEMA,
        run: id(name),
        parent: None,
        project: Some("symphony".into()),
        machine: None,
        pipeline: pipeline.into(),
        step: None,
        label: format!("just {pipeline}"),
        task: None,
        handle: None,
        trigger: "owner".into(),
        branch: Some("main".into()),
        commit: None,
        tree: None,
        dirty: Some(false),
        queued,
        started: Some(queued + 1.0),
        ended: None,
        end: None,
        exit: None,
        signal: None,
        cpu_s: None,
        cpu_partial: None,
        rerun_of: None,
        log: None,
        log_bytes: None,
        suites: None,
        failed: None,
    }
}

/// `line` as its end line, ended `after` seconds after it started, as `end`.
fn ended(line: &RunLine, end: End, after: f64) -> RunLine {
    RunLine {
        ended: Some(line.started.unwrap() + after),
        end: Some(end),
        exit: Some(if end == End::Green { 0 } else { 1 }),
        cpu_s: Some(after / 2.0),
        ..line.clone()
    }
}

/// `line` as a step `name` of the run or step `parent`.
fn step_of(line: RunLine, parent: &str, name: &str) -> RunLine {
    RunLine {
        parent: Some(id(parent)),
        step: Some(name.into()),
        label: name.into(),
        ..line
    }
}

/// The day file of the UTC day starting at `day` in `runs`, holding `lines` and then `tail`.
fn write_day(runs: &Path, day: f64, lines: &[RunLine], tail: &str) {
    std::fs::create_dir_all(runs).unwrap();
    let mut text: String = lines
        .iter()
        .map(|line| serde_json::to_string(line).unwrap() + "\n")
        .collect();
    text.push_str(tail);
    std::fs::write(runs.join(format!("{}.jsonl", run_day(day))), text).unwrap();
}

fn names(page: &RunsPage) -> Vec<String> {
    page.runs
        .iter()
        .map(|run| run.run.trim_start_matches('0').to_string())
        .collect()
}

#[test]
fn an_ask_names_each_filter_whole_and_refuses_what_none_could_match() {
    assert_eq!(
        asked(None, None, None, None, None).unwrap(),
        Filters::default()
    );
    assert_eq!(
        asked(Some(""), Some(""), Some(""), Some(""), Some("")).unwrap(),
        Filters::default(),
        "an empty filter is none"
    );
    assert_eq!(
        asked(
            Some("review-callback"),
            Some("timed_out"),
            Some("tools/runs-page"),
            Some("tools/runs-page"),
            Some("1794700800.5")
        )
        .unwrap(),
        Filters {
            pipeline: Some("review-callback".into()),
            end: Some(Some(End::TimedOut)),
            task: Some("tools/runs-page".into()),
            branch: Some("tools/runs-page".into()),
            before: Some(1_794_700_800.5),
        }
    );
    assert_eq!(
        asked(None, Some("started"), None, None, None).unwrap().end,
        Some(None),
        "a run with no end line yet"
    );
    assert_eq!(
        asked(Some("two words"), None, None, None, None).unwrap_err(),
        "pipeline is a pipeline's name, not \"two words\""
    );
    for end in ["Green", "running", "red "] {
        assert_eq!(
            asked(None, Some(end), None, None, None).unwrap_err(),
            format!(
                "end is green, red, timed_out, interrupted, incomplete or started, not {end:?}"
            )
        );
    }
    for before in ["soon", "NaN", "inf"] {
        assert_eq!(
            asked(None, None, None, None, Some(before)).unwrap_err(),
            format!("before is a time in seconds, not {before:?}")
        );
    }
}

#[test]
fn the_list_is_each_run_once_newest_first_without_its_steps() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    // Yesterday: a run that ended today, whose end line is in today's file, and one that ended.
    let overnight = start("a", "verify", TODAY - 600.0);
    let yesterday = start("b", "lint", TODAY - 3_600.0);
    write_day(
        &runs,
        TODAY - DAY,
        &[
            yesterday.clone(),
            overnight.clone(),
            ended(&yesterday, End::Green, 30.0),
        ],
        "",
    );
    let lost = start("c", "test", TODAY + 60.0);
    let red = start("d", "test", TODAY + 120.0);
    let step = step_of(start("e", "verify", TODAY + 30.0), "a", "tests");
    write_day(
        &runs,
        TODAY,
        &[
            lost.clone(),
            red.clone(),
            step.clone(),
            ended(&step, End::Green, 5.0),
            ended(&overnight, End::Red, 900.0),
            ended(&red, End::Red, 10.0),
            // A sweep's second end line of a run that had ended: the first stands.
            ended(&red, End::Incomplete, 20.0),
        ],
        // A line a writer that died cut short, then an empty one.
        "{\"schema\":1,\"run\":\"ff\n\n",
    );
    let page = page(&Index::default(), &runs, &Filters::default(), NOW);
    assert_eq!(names(&page), ["d", "c", "a", "b"]);
    assert_eq!(page.runs[0].end, Some(End::Red));
    assert_eq!(page.runs[0].ended, red.started.map(|at| at + 10.0));
    assert_eq!(
        page.runs[1], lost,
        "a run with no end line is its start line"
    );
    assert_eq!(
        page.runs[2],
        ended(&overnight, End::Red, 900.0),
        "a run whose lines are in two days' files is its end line"
    );
    assert!(!page.more);
    assert_eq!(page.pipelines, ["lint", "test", "verify"]);
    assert_eq!(page.unread, Vec::<String>::new());
}

#[test]
fn the_filters_take_a_pipeline_an_end_a_task_and_a_branch_whole() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let mut lines = Vec::new();
    for (name, pipeline, task, branch, end) in [
        ("1", "test", Some("tools/a"), "tools/a", Some(End::Green)),
        ("2", "test", Some("tools/ab"), "tools/ab", Some(End::Red)),
        ("3", "lint", Some("tools/a"), "tools/a", Some(End::Red)),
        ("4", "test", None, "main", None),
        ("5", "client", Some("tools/a"), "main", Some(End::TimedOut)),
    ] {
        let mut line = start(name, pipeline, TODAY + name.parse::<f64>().unwrap() * 60.0);
        line.task = task.map(Into::into);
        line.branch = Some(branch.into());
        lines.push(line.clone());
        if let Some(end) = end {
            lines.push(ended(&line, end, 5.0));
        }
    }
    write_day(&runs, TODAY, &lines, "");
    let ask = |pipeline, end, task, branch| {
        names(&page(
            &Index::default(),
            &runs,
            &asked(pipeline, end, task, branch, None).unwrap(),
            NOW,
        ))
    };
    assert_eq!(ask(Some("test"), None, None, None), ["4", "2", "1"]);
    assert_eq!(ask(None, Some("red"), None, None), ["3", "2"]);
    assert_eq!(ask(None, Some("started"), None, None), ["4"]);
    assert_eq!(ask(None, None, Some("tools/a"), None), ["5", "3", "1"]);
    assert_eq!(ask(None, None, None, Some("main")), ["5", "4"]);
    assert_eq!(
        ask(
            Some("test"),
            Some("red"),
            Some("tools/ab"),
            Some("tools/ab")
        ),
        ["2"]
    );
    assert_eq!(
        ask(Some("verify"), None, None, None),
        Vec::<String>::new(),
        "a pipeline no run names matches none"
    );
    let named = page(
        &Index::default(),
        &runs,
        &asked(Some("verify"), None, None, None, None).unwrap(),
        NOW,
    );
    assert_eq!(
        named.pipelines,
        ["client", "lint", "test", "verify"],
        "the pipeline asked for is offered with those read"
    );
}

#[test]
fn a_full_page_reads_no_older_day_and_the_next_page_starts_at_its_last_run() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let count = u32::try_from(RUNS_PAGE).unwrap() + 5;
    let today: Vec<RunLine> = (0..count)
        .map(|n| {
            // The fifth and sixth were asked for in the same second: the page's cut falls between.
            let at = if n == 4 { 5 } else { n };
            let line = start(&format!("1{n:03}"), "test", TODAY + 60.0 * f64::from(at));
            ended(&line, End::Green, 5.0)
        })
        .collect();
    write_day(&runs, TODAY, &today, "");
    // Yesterday's file is no regular file: read, it would be named as not read.
    std::fs::create_dir_all(runs.join(format!("{}.jsonl", run_day(TODAY - DAY)))).unwrap();
    let first = page(&Index::default(), &runs, &Filters::default(), NOW);
    assert_eq!(first.runs.len(), RUNS_PAGE);
    assert!(first.more);
    assert_eq!(
        first.unread,
        Vec::<String>::new(),
        "a page today's file fills reads no other"
    );
    assert_eq!(first.runs[0].run, id(&format!("1{:03}", RUNS_PAGE + 4)));
    assert_eq!(names(&first).last().unwrap(), "1004");
    let before = first.runs.last().unwrap().queued;
    let next = page(
        &Index::default(),
        &runs,
        &Filters {
            before: Some(before),
            ..Filters::default()
        },
        NOW,
    );
    assert_eq!(
        names(&next),
        ["1004", "1005", "1003", "1002", "1001", "1000"],
        "the next page holds its last run again and the one asked for in the same second"
    );
    assert!(!next.more);
    assert_eq!(
        next.unread.len(),
        1,
        "the last page reads back to the window's start"
    );
    assert!(
        next.unread[0].ends_with("not a regular file"),
        "{:?}",
        next.unread
    );
}

#[test]
fn a_day_file_not_read_is_named_and_days_past_the_window_are_never_read() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let outside = tree.path("outside.jsonl");
    let old = start("9", "test", TODAY - 10.0 * DAY);
    write_day(&runs, TODAY, &[start("1", "test", TODAY + 5.0)], "");
    std::fs::write(&outside, serde_json::to_string(&old).unwrap() + "\n").unwrap();
    // A link is not followed out of the day files' folder.
    std::os::unix::fs::symlink(
        &outside,
        runs.join(format!("{}.jsonl", run_day(TODAY - DAY))),
    )
    .unwrap();
    write_day(&runs, TODAY - 10.0 * DAY, std::slice::from_ref(&old), "");
    write_day(
        &runs,
        TODAY - f64::from(RUNS_DAYS) * DAY,
        &[start("8", "test", TODAY - f64::from(RUNS_DAYS) * DAY)],
        "",
    );
    let page = page(&Index::default(), &runs, &Filters::default(), NOW);
    assert_eq!(
        names(&page),
        ["1", "9"],
        "the fifteenth day back is past the window"
    );
    assert_eq!(page.unread.len(), 1, "{:?}", page.unread);
    assert!(
        page.unread[0].starts_with("2026-11-14.jsonl: "),
        "a file not read is named by its name alone: {:?}",
        page.unread
    );
    assert!(
        !page.unread[0].contains(&runs.display().to_string()),
        "{:?}",
        page.unread
    );
    // A folder with no day files has no runs.
    let empty = super::page(
        &Index::default(),
        &tree.path("none"),
        &Filters::default(),
        NOW,
    );
    assert_eq!(empty, RunsPage::default());
}

#[test]
fn a_day_file_is_read_again_once_its_status_changes_or_a_line_answered_is_another_run_s() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    write_day(&runs, TODAY, &[start("1", "test", TODAY + 5.0)], "");
    let path = day_path(&runs, TODAY);
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    let all = Filters::default();
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["1"]);
    // Bytes of the same length under the same modification time are not parsed again for their
    // status, but the line answered is read again from them, and holds another run now: the day is
    // parsed again, and answers what it holds.
    write_day(&runs, TODAY, &[start("2", "test", TODAY + 5.0)], "");
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(written).unwrap();
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["2"]);
    assert_eq!(names(&page(&Index::default(), &runs, &all, NOW)), ["2"]);
    // A line appended is read, by the list and by a run's page alike.
    write_day(
        &runs,
        TODAY,
        &[
            start("2", "test", TODAY + 5.0),
            start("3", "test", TODAY + 9.0),
        ],
        "",
    );
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["3", "2"]);
    assert!(run(&index, &runs, &id("3"), NOW).is_ok());
    // The same length written again later is read again too.
    write_day(
        &runs,
        TODAY,
        &[
            start("2", "test", TODAY + 5.0),
            start("4", "test", TODAY + 9.0),
        ],
        "",
    );
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(written + std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["4", "2"]);
    assert_eq!(index.len(), 1);
}

#[test]
fn the_index_keeps_the_window_s_days_alone_and_nothing_of_a_file_gone_or_refused() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    let oldest = TODAY - f64::from(RUNS_DAYS - 1) * DAY;
    write_day(&runs, TODAY, &[start("1", "test", TODAY + 5.0)], "");
    write_day(&runs, TODAY - DAY, &[start("2", "test", TODAY - DAY)], "");
    write_day(&runs, oldest, &[start("3", "test", oldest)], "");
    // A pipeline no run names reads every day of the window.
    let none = Filters {
        pipeline: Some("none".into()),
        ..Filters::default()
    };
    assert!(page(&index, &runs, &none, NOW).runs.is_empty());
    assert_eq!(index.len(), 3, "a day with no file keeps nothing");
    // A day later the oldest is past the window.
    let later = NOW + DAY;
    assert!(page(&index, &runs, &none, later).runs.is_empty());
    assert_eq!(index.len(), 2);
    // A file gone, or one that is no longer a regular file, is no longer kept.
    std::fs::remove_file(day_path(&runs, TODAY - DAY)).unwrap();
    let today = day_path(&runs, TODAY);
    std::fs::remove_file(&today).unwrap();
    std::fs::create_dir(&today).unwrap();
    let read = page(&index, &runs, &none, later);
    assert_eq!(read.unread.len(), 1, "{:?}", read.unread);
    assert_eq!(index.len(), 0);
}

/// `lines` and then `tail` appended to the day file of the UTC day starting at `day` in `runs`.
fn append(runs: &Path, day: f64, lines: &[RunLine], tail: &str) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(day_path(runs, day))
        .unwrap();
    for line in lines {
        writeln!(file, "{}", serde_json::to_string(line).unwrap()).unwrap();
    }
    file.write_all(tail.as_bytes()).unwrap();
}

/// The length of the day file of the UTC day starting at `day` in `runs`.
fn length(runs: &Path, day: f64) -> u64 {
    std::fs::metadata(day_path(runs, day)).unwrap().len()
}

/// Asserts that `index` answers the list, as filtered by `filters`, and the run `id` as an index of
/// its own does, which parses every file whole.
fn answers_as_parsed_whole(index: &Index, runs: &Path, filters: &Filters, id: &str) {
    assert_eq!(
        page(index, runs, filters, NOW),
        page(&Index::default(), runs, filters, NOW)
    );
    assert_eq!(
        run(index, runs, id, NOW),
        run(&Index::default(), runs, id, NOW)
    );
}

#[test]
fn a_file_that_grew_is_read_on_from_its_last_line_and_never_parsed_whole_again() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    let all = Filters::default();
    let verify = start("a", "verify", TODAY + 10.0);
    let lint = start("b", "lint", TODAY + 20.0);
    write_day(&runs, TODAY, &[verify.clone(), lint.clone()], "");
    answers_as_parsed_whole(&index, &runs, &all, &id("a"));
    assert_eq!(index.parsed(), length(&runs, TODAY));
    // Each line the recorder appends is parsed alone and folded in as a fold of the whole file
    // takes it: a step, a newer start line, an end line, a second end line, a new run.
    let step = step_of(start("c", "verify", TODAY + 30.0), "a", "tests");
    let appended = [
        step.clone(),
        RunLine {
            label: "just verify again".into(),
            ..verify.clone()
        },
        ended(&step, End::Green, 5.0),
        ended(&verify, End::Red, 60.0),
        ended(&verify, End::Incomplete, 90.0),
        ended(&lint, End::Green, 3.0),
        start("d", "test", TODAY + 40.0),
    ];
    for line in &appended {
        let before = length(&runs, TODAY);
        let parsed = index.parsed();
        append(&runs, TODAY, std::slice::from_ref(line), "");
        answers_as_parsed_whole(&index, &runs, &all, &id("a"));
        assert_eq!(
            index.parsed() - parsed,
            length(&runs, TODAY) - before,
            "only the line appended is parsed"
        );
    }
    let opened = run(&index, &runs, &id("a"), NOW).unwrap();
    assert_eq!(opened.run, ended(&verify, End::Red, 60.0));
    assert_eq!(opened.steps, [ended(&step, End::Green, 5.0)]);
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["d", "b", "a"]);
    assert_eq!(
        index.parsed(),
        length(&runs, TODAY),
        "each byte parsed once"
    );
}

#[test]
fn a_line_cut_short_is_parsed_again_once_it_is_whole() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    let all = Filters::default();
    write_day(&runs, TODAY, &[start("a", "verify", TODAY + 10.0)], "");
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["a"]);
    // A line the recorder has written only part of when it is read: the rest comes after.
    let line = serde_json::to_string(&start("b", "test", TODAY + 20.0)).unwrap() + "\n";
    let (head, rest) = line.split_at(line.len() / 2);
    append(&runs, TODAY, &[], head);
    answers_as_parsed_whole(&index, &runs, &all, &id("b"));
    append(&runs, TODAY, &[], rest);
    answers_as_parsed_whole(&index, &runs, &all, &id("b"));
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["b", "a"]);
    // Once the file ends a line again, the next is read on from there.
    let parsed = index.parsed();
    let before = length(&runs, TODAY);
    append(&runs, TODAY, &[start("c", "test", TODAY + 30.0)], "");
    answers_as_parsed_whole(&index, &runs, &all, &id("c"));
    assert_eq!(index.parsed() - parsed, length(&runs, TODAY) - before);
}

#[test]
fn a_day_an_answer_has_still_to_read_its_lines_from_is_read_on_by_another() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    let all = Filters::default();
    let verify = start("a", "verify", TODAY + 10.0);
    write_day(&runs, TODAY, std::slice::from_ref(&verify), "");
    // An answer that has read the day and taken its runs, and not yet read their lines back.
    let mut budget = RUNS_READ_BYTES;
    let mut found = Vec::new();
    let held = day_file(&index, &runs, TODAY, &mut budget, &mut |day| {
        found.extend(day.runs.iter().map(|(run, key)| Found {
            run: run.clone(),
            key: key.clone(),
            day: 0,
        }));
    })
    .unwrap()
    .unwrap();
    assert_eq!(index.parsed(), length(&runs, TODAY));
    append(&runs, TODAY, &[start("b", "test", TODAY + 20.0)], "");
    answers_as_parsed_whole(&index, &runs, &all, &id("b"));
    assert_eq!(
        index.parsed(),
        length(&runs, TODAY),
        "only the line appended is parsed"
    );
    assert_eq!(
        lines_of(&[held], &found),
        (vec![verify], Vec::new()),
        "the first answer reads its line back all the same"
    );
}

#[test]
fn answers_asked_for_at_once_parse_each_byte_appended_once() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    let all = Filters::default();
    write_day(&runs, TODAY, &[start("a", "verify", TODAY + 10.0)], "");
    let readers = 4;
    for round in 0..10_u32 {
        let name = format!("b{round}");
        let line = start(&name, "test", TODAY + 20.0 + f64::from(round));
        append(&runs, TODAY, &[line], "");
        // Two lists and two pages of the run just appended, asked for together, each as a whole
        // parse answers it.
        let together = std::sync::Barrier::new(readers);
        std::thread::scope(|scope| {
            for reader in 0..readers {
                let (index, runs, all, together) = (&index, &runs, &all, &together);
                let run_id = id(&name);
                scope.spawn(move || {
                    together.wait();
                    if reader % 2 == 0 {
                        assert_eq!(
                            page(index, runs, all, NOW),
                            page(&Index::default(), runs, all, NOW)
                        );
                    } else {
                        assert_eq!(
                            run(index, runs, &run_id, NOW),
                            run(&Index::default(), runs, &run_id, NOW)
                        );
                    }
                });
            }
        });
    }
    assert_eq!(
        index.parsed(),
        length(&runs, TODAY),
        "each byte parsed once, whoever asked"
    );
}

#[test]
fn a_reader_that_opened_before_an_append_takes_the_longer_day_kept_while_it_waited() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    let all = Filters::default();
    write_day(&runs, TODAY, &[start("a", "test", TODAY + 10.0)], "");
    let opened = Arc::new(std::sync::Barrier::new(2));
    let resume = Arc::new(std::sync::Barrier::new(2));
    let length_seen = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let before = length(&runs, TODAY);
    std::thread::scope(|scope| {
        let (slow_opened, slow_resume, slow_seen) =
            (opened.clone(), resume.clone(), length_seen.clone());
        let (index, runs, all) = (&index, &runs, &all);
        let first = scope.spawn(move || {
            let path = day_path(runs, TODAY);
            BEFORE_DAY_LOCK.set(Some(Box::new(move || {
                slow_seen.store(
                    std::fs::metadata(path).unwrap().len(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                slow_opened.wait();
                slow_resume.wait();
            })));
            page(index, runs, all, NOW)
        });
        // The first reader has seen the shorter file but cannot take the lock until the second
        // has parsed the append. Its next status must describe what the index now keeps.
        opened.wait();
        append(runs, TODAY, &[start("b", "test", TODAY + 20.0)], "");
        let second = page(index, runs, all, NOW);
        resume.wait();
        assert_eq!(first.join().unwrap(), second);
    });
    assert_eq!(
        length_seen.load(std::sync::atomic::Ordering::Relaxed),
        before
    );
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["b", "a"]);
    assert_eq!(
        index.parsed(),
        length(&runs, TODAY),
        "each byte parsed once"
    );
}

#[test]
fn a_file_that_shrank_was_replaced_or_no_longer_holds_its_last_line_is_parsed_whole() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    let all = Filters::default();
    let lines: Vec<RunLine> = ["1", "2", "3"]
        .iter()
        .zip(1..)
        .map(|(name, n)| start(name, "test", TODAY + f64::from(n)))
        .collect();
    write_day(&runs, TODAY, &lines, "");
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["3", "2", "1"]);
    let whole = |index: &Index, parsed: u64| {
        assert_eq!(
            index.parsed() - parsed,
            length(&runs, TODAY),
            "parsed whole"
        );
    };
    // Shorter, in place.
    let parsed = index.parsed();
    write_day(&runs, TODAY, &lines[..2], "");
    answers_as_parsed_whole(&index, &runs, &all, &id("1"));
    whole(&index, parsed);
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["2", "1"]);
    // Another file renamed over it: another inode, though longer.
    let parsed = index.parsed();
    let other = tree.path("other.jsonl");
    let replaced = [
        ended(&lines[0], End::Red, 5.0),
        lines[1].clone(),
        lines[2].clone(),
    ];
    let text: String = replaced
        .iter()
        .map(|line| serde_json::to_string(line).unwrap() + "\n")
        .collect();
    std::fs::write(&other, text).unwrap();
    std::fs::rename(&other, day_path(&runs, TODAY)).unwrap();
    answers_as_parsed_whole(&index, &runs, &all, &id("1"));
    whole(&index, parsed);
    assert_eq!(
        run(&index, &runs, &id("1"), NOW).unwrap().run.end,
        Some(End::Red)
    );
    // Longer on the same inode, but no longer holding its last line read where it was.
    let parsed = index.parsed();
    let rewritten = [
        lines[0].clone(),
        lines[1].clone(),
        ended(&lines[2], End::Green, 7.0),
        start("4", "test", TODAY + 4.0),
    ];
    write_day(&runs, TODAY, &rewritten, "");
    answers_as_parsed_whole(&index, &runs, &all, &id("3"));
    whole(&index, parsed);
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["4", "3", "2", "1"]);
    assert_eq!(
        run(&index, &runs, &id("1"), NOW).unwrap().run,
        lines[0],
        "the end line it no longer holds is not answered"
    );
}

#[test]
fn a_line_changed_under_a_kept_offset_is_answered_as_the_file_holds_it_now() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let index = Index::default();
    let all = Filters::default();
    let path = day_path(&runs, TODAY);
    let one = start("1", "test", TODAY + 5.0);
    let two = start("2", "lint", TODAY + 6.0);
    write_day(&runs, TODAY, &[one.clone(), two.clone()], "");
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(names(&page(&index, &runs, &all, NOW)), ["2", "1"]);
    assert!(run(&index, &runs, &id("1"), NOW).is_ok());
    let parsed = index.parsed();
    // The same length and modification time, so the same status, over other lines: run 1's line
    // is now run 3's, and run 2's was asked for a second later.
    write_day(
        &runs,
        TODAY,
        &[
            start("3", "test", TODAY + 5.0),
            RunLine {
                queued: TODAY + 7.0,
                ..two.clone()
            },
        ],
        "",
    );
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(written).unwrap();
    assert_eq!(
        run(&index, &runs, &id("1"), NOW).unwrap_err(),
        format!("no run {} in the last 14 days' files", id("1")),
        "a run whose line is gone from under its offset is none"
    );
    assert_eq!(
        index.parsed() - parsed,
        length(&runs, TODAY),
        "the day is parsed again"
    );
    answers_as_parsed_whole(&index, &runs, &all, &id("3"));
    let listed = page(&index, &runs, &all, NOW);
    assert_eq!(names(&listed), ["2", "3"]);
    assert_eq!(listed.runs[0].queued, TODAY + 7.0);
}

#[test]
fn a_run_opens_on_its_steps_each_after_the_one_it_ran_inside() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let verify = start("a", "verify", TODAY - 300.0);
    let lint = step_of(start("b", "verify", TODAY - 200.0), "a", "lint");
    write_day(&runs, TODAY - DAY, &[verify.clone(), lint.clone()], "");
    let client_lint = step_of(start("c", "verify", TODAY + 10.0), "b", "client-lint");
    let tests = step_of(start("d", "verify", TODAY + 20.0), "a", "tests");
    let failed = |name: &str| FailedTest {
        suite: Some("symphony-server".into()),
        name: name.into(),
        line: "assertion failed".into(),
    };
    let red_tests = RunLine {
        failed: Some(vec![failed("one"), failed("two")]),
        ..ended(&tests, End::Red, 60.0)
    };
    let other = step_of(start("e", "test", TODAY + 15.0), "f", "tests");
    write_day(
        &runs,
        TODAY,
        &[
            client_lint.clone(),
            tests.clone(),
            other,
            ended(&client_lint, End::Green, 3.0),
            ended(&lint, End::Green, 300.0),
            red_tests.clone(),
        ],
        "",
    );
    let opened = run(&Index::default(), &runs, &id("a"), NOW).unwrap();
    assert_eq!(opened.run, verify, "a run still running is its start line");
    assert_eq!(
        opened
            .steps
            .iter()
            .map(|step| step.step.clone().unwrap())
            .collect::<Vec<_>>(),
        ["lint", "client-lint", "tests"],
        "a step's own steps come before its next sibling"
    );
    assert_eq!(opened.steps[0], ended(&lint, End::Green, 300.0));
    assert_eq!(
        opened.steps[2], red_tests,
        "a step that failed two tests names them"
    );
    // A step opens on its own steps, its parent left out.
    let step = run(&Index::default(), &runs, &id("b"), NOW).unwrap();
    assert_eq!(step.run.parent, Some(id("a")));
    assert_eq!(step.steps.len(), 1);
    assert_eq!(
        run(&Index::default(), &runs, &id("7"), NOW).unwrap_err(),
        format!("no run {} in the last 14 days' files", id("7"))
    );
}

#[test]
fn a_loop_of_parents_takes_each_step_once() {
    let one = step_of(start("b", "verify", TODAY), "c", "one");
    let two = step_of(start("c", "verify", TODAY + 1.0), "b", "two");
    let top = step_of(start("d", "verify", TODAY + 2.0), "a", "top");
    let steps = [one.clone(), two.clone(), top.clone()].map(Arc::new);
    assert_eq!(under(&id("a"), &steps), [top]);
    assert_eq!(
        under(&id("b"), &steps),
        [two],
        "the step it started from is not met again"
    );
    assert_eq!(under(&id("c"), &steps), [one]);
}

#[tokio::test]
async fn the_routes_refuse_a_bad_ask_before_any_hub_and_answer_through_one() {
    use axum::http::StatusCode;
    let tree = Tree::new();
    let root = crate::testing::repository(&tree);
    let shared = crate::front::Shared::new(1, 1234, &root, None, crate::testing::open_gate());
    let ask = |uri: String| {
        let shared = shared.clone();
        async move { crate::testing::get_json(&shared, &uri).await }
    };
    let (status, said) = ask("/api/runs?end=running".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    let (status, said) = ask("/api/run".into()).await;
    assert_eq!(
        (status, said),
        (
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "run is required: a run's id"})
        )
    );
    let (status, _) = ask("/api/runs".into()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "no hub yet");

    let runs = tree.path("runs");
    let red = start("d", "test", TODAY + 120.0);
    write_day(
        &runs,
        TODAY,
        &[red.clone(), ended(&red, End::Red, 10.0)],
        "",
    );
    let mut hub = crate::live::Hub::new(&root, &root.join("build/client"));
    hub.clock = Box::new(|| NOW);
    let none = || Err("not this test's".to_string());
    hub.report_folders
        .set(super::super::reports::Folders {
            reports: none(),
            agents: none(),
            health: none(),
            runs: Some(runs.clone()),
        })
        .unwrap();
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let (status, said) = ask("/api/runs?pipeline=test&end=red".into()).await;
    assert_eq!(status, StatusCode::OK);
    let page: RunsPage = serde_json::from_value(said).unwrap();
    assert_eq!(page.runs, [ended(&red, End::Red, 10.0)]);
    let (status, said) = ask("/api/runs?pipeline=lint".into()).await;
    assert_eq!(
        (status, said["runs"].clone()),
        (StatusCode::OK, serde_json::json!([]))
    );
    let (status, said) = ask(format!("/api/run?run={}", id("d"))).await;
    assert_eq!(status, StatusCode::OK);
    let opened: RunPage = serde_json::from_value(said).unwrap();
    assert_eq!(opened.run.end, Some(End::Red));
    let (status, said) = ask("/api/run?run=..%2F..%2Fconfig".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    let (status, said) = ask(format!("/api/run?run={}", id("e"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{said}");
}

#[tokio::test]
async fn a_project_that_keeps_no_runs_where_the_server_reads_says_so() {
    let tree = Tree::new();
    let root = crate::testing::repository(&tree);
    let hub = crate::live::Hub::new(&root, &root.join("build/client"));
    let none = || Err("not this test's".to_string());
    hub.report_folders
        .set(super::super::reports::Folders {
            reports: none(),
            agents: none(),
            health: none(),
            runs: None,
        })
        .unwrap();
    let said = (
        StatusCode::NOT_FOUND,
        "this project keeps no runs where this server reads".to_string(),
    );
    assert_eq!(answer(&hub, &Filters::default()).unwrap_err(), said);
    assert_eq!(answer_run(&hub, &id("a")).unwrap_err(), said);
    assert_eq!(
        answer_run(&hub, "a").unwrap_err().0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        answer_run(&hub, &id("A")).unwrap_err(),
        (
            StatusCode::BAD_REQUEST,
            format!(
                "run is a run's id, 32 lower-case hex digits, not {:?}",
                id("A")
            )
        ),
        "the recorder writes no capitals"
    );
}

#[test]
fn equal_queued_roots_match_the_original_full_sequence_before_pagination() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let mut lines: Vec<_> = (0..RUNS_PAGE + 3)
        .rev()
        .map(|index| start(&format!("{index:x}"), "verify", TODAY + 20.0))
        .collect();
    write_day(&runs, TODAY, &lines, "");
    symphony_board::order::sort_by(&mut lines, &|a, b| {
        b.queued
            .total_cmp(&a.queued)
            .then_with(|| a.run.cmp(&b.run))
    });
    lines.truncate(RUNS_PAGE);
    let found = page(&Index::default(), &runs, &Filters::default(), NOW);
    assert_eq!(found.runs, lines);
    assert!(found.more);
    let before = page(
        &Index::default(),
        &runs,
        &Filters {
            before: Some(TODAY + 20.0),
            ..Filters::default()
        },
        NOW,
    );
    assert_eq!(
        before.runs, lines,
        "the documented before boundary includes equal timestamps"
    );
    assert!(before.more);
    let earlier = page(
        &Index::default(),
        &runs,
        &Filters {
            before: Some(TODAY + 19.0),
            ..Filters::default()
        },
        NOW,
    );
    assert!(earlier.runs.is_empty());
    assert!(!earlier.more);
}

#[test]
fn run_children_keep_depth_first_time_id_order_and_first_equal_duplicate() {
    let late = step_of(start("b", "verify", TODAY + 2.0), "a", "late");
    let duplicate = RunLine {
        label: "duplicate payload".into(),
        ..late.clone()
    };
    let early = step_of(start("c", "verify", TODAY + 1.0), "a", "early");
    let equal = step_of(start("d", "verify", TODAY + 1.0), "a", "same time");
    let nested = step_of(start("e", "verify", TODAY), "c", "nested");
    let cycle = step_of(start("a", "verify", TODAY), "e", "cycle");
    let unrelated = step_of(start("f", "verify", TODAY), "f", "unrelated cycle");
    let steps: Vec<_> = [
        late.clone(),
        early.clone(),
        duplicate,
        equal.clone(),
        nested.clone(),
        cycle,
        unrelated,
    ]
    .into_iter()
    .map(Arc::new)
    .collect();
    assert_eq!(under(&id("a"), &steps), [early, nested, equal, late]);
    let empty: Vec<Arc<RunLine>> = Vec::new();
    assert!(under(&id("a"), &empty).is_empty());
}

#[test]
fn shared_found_order_matches_all_fields_and_reports_comparison_cpu_cost() {
    let mut rows: Vec<_> = (0..512)
        .rev()
        .map(|index| Found {
            run: Arc::from(id(&format!("{index:x}"))),
            day: index % 3,
            key: Key {
                parent: None,
                queued: TODAY + f64::from(u32::try_from(index % 17).unwrap()),
                end: None,
                pipeline: Arc::from("verify"),
                task: Some(Arc::from("tools/task")),
                branch: Some(Arc::from("main")),
                at: u64::try_from(index).unwrap(),
                len: 17,
            },
        })
        .collect();
    let mut expected = rows.clone();
    let by = |a: &Found, b: &Found| {
        b.key
            .queued
            .total_cmp(&a.key.queued)
            .then_with(|| a.run.cmp(&b.run))
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "the standard sort this test holds the shared sorter to"
    )]
    let (_, old_cpu) = test_time::cpu::spent(|| expected.sort_by(by));
    let calls = std::cell::Cell::new(0);
    let (_, new_cpu) = test_time::cpu::spent(|| {
        symphony_board::order::sort_by(&mut rows, &|a, b| {
            calls.set(calls.get() + 1);
            by(a, b)
        })
    });
    let fields = |rows: &[Found]| {
        rows.iter()
            .map(|row| {
                (
                    row.run.clone(),
                    row.day,
                    (
                        row.key.parent.clone(),
                        row.key.queued.to_bits(),
                        row.key.end,
                        row.key.pipeline.clone(),
                        row.key.task.clone(),
                        row.key.branch.clone(),
                        row.key.at,
                        row.key.len,
                    ),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(fields(&rows), fields(&expected));
    assert!(calls.get() < rows.len() * 64);
    eprintln!(
        "Found report sort: n={}, payload_bytes={}, position_bytes={}, comparisons={}, old_thread_cpu={old_cpu:?}, shared_thread_cpu={new_cpu:?}; additional usize-sort scratch, no payload clones inside helper",
        rows.len(),
        std::mem::size_of::<Found>(),
        rows.len() * std::mem::size_of::<usize>(),
        calls.get()
    );
}
