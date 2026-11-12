//! `tasks status [--json]`: the board's health as the machine's Symphony server last recorded it
//! ([`crate::board_health`]), read from its record in `symphony/tasks/` and never from the board,
//! so a board too slow to answer, or a server that stopped, is still said. Its warnings are the
//! record's read at this run's time, and that the server stopped recording when it did, and the
//! free disk where the record is kept, read now, when it is under `SYMPHONY_LOW_DISK_GB`
//! ([`board_health::disk`]; a setting that is not a number is said, and the default taken).
//! `--json` prints the record as `/api/health` answers it, beside the file, the disk and those
//! warnings.

use super::{Clock, Env, args::Parsed};
use crate::board_health::{self, Report, disk};
use crate::sys;
use std::io::Write;

pub(super) fn run(parsed: &Parsed, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let read = Clock::of(env).and_then(|clock| {
        let file = board_health::file(env)?;
        Ok((clock.now(), Report::read(&file)?, file))
    });
    let (now, report, file) = match read {
        Ok(read) => read,
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            return 2;
        }
    };
    // A threshold it cannot read is the default's, said, as the server's pages take it: the
    // board's health is still said.
    let threshold = disk::threshold(&env.vars).unwrap_or_else(|why| {
        let _ = writeln!(
            err,
            "· {why}: it warns under {} GB free",
            disk::THRESHOLD_GB
        );
        disk::THRESHOLD_GB
    });
    let mut warnings = match &report {
        Some(report) => report.read_warnings(now, sys::pid_alive),
        None => vec![format!(
            "no board health is recorded in {}: the machine's Symphony server records it while it serves this project (`just symphony`)",
            file.display()
        )],
    };
    let free = disk::free(file.parent().unwrap_or(&file));
    if let Some((at, bytes)) = &free
        && let Some(low) = disk::Low::of(at, *bytes, threshold)
    {
        warnings.push(low.warning());
    }
    let text = if parsed.flag("json") {
        let disk = free.map(|(path, bytes)| {
            serde_json::json!({"path": path, "free_bytes": bytes, "threshold_gb": threshold})
        });
        let json =
            serde_json::json!({"file": file, "report": report, "disk": disk, "warnings": warnings});
        serde_json::to_string_pretty(&json).unwrap_or_default()
    } else {
        match &report {
            Some(report) => report.lines(now, &warnings).join("\n"),
            None => format!("⚠ {}", warnings.join("\n⚠ ")),
        }
    };
    let _ = writeln!(out, "{text}");
    0
}

#[cfg(test)]
mod tests {
    use super::super::{Env, main};
    use crate::board_health::{FILE_VAR, Recorder, Report, disk};
    use std::fs;
    use std::path::{Path, PathBuf};

    const T0: f64 = 1_800_000_000.0;
    /// A board's server that is not there: the command never asks it.
    const GONE: &str = "127.0.0.1:1";

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("symphony-status-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run(args: &[&str], vars: &[(&str, &str)], cwd: &Path) -> (i32, String, String) {
        let env = Env {
            vars: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            cwd: cwd.to_path_buf(),
        };
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(&args, &env, &mut std::io::empty(), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// The record of a server whose dispatch failed at `T0 + 1` and `T0 + 2`, made at `T0 + 5`.
    fn recorded(path: &Path) -> Report {
        let recorder = Recorder::default();
        recorder.dispatched(T0, Ok(()));
        recorder.dispatched(T0 + 1.0, Err("the board did not answer"));
        recorder.dispatched(T0 + 2.0, Err("the board did not answer"));
        let report = recorder.report(T0 + 5.0);
        report.write(path).unwrap();
        report
    }

    #[test]
    fn status_prints_the_record_and_its_warnings_at_its_own_time_without_the_board() {
        let dir = scratch("record");
        let path = dir.join("health.json");
        let report = recorded(&path);
        assert_eq!(
            report.warnings,
            [
                "callback dispatch failing for 4 s (2 of 3 passes in the last hour failed): the board did not answer"
            ]
        );
        let file = path.to_str().unwrap();
        // The disk is never low here: the test is of the record's own warnings.
        let vars = [
            (FILE_VAR, file),
            ("TASKS_SERVER", GONE),
            ("TASKS_NOW", "1800000010"),
            (disk::VAR, "0"),
        ];
        let (code, out, err) = run(&["status"], &vars, &dir);
        let pid = std::process::id();
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (
                0,
                format!(
                    "board health, as the Symphony server (pid {pid}) recorded it 5 s ago, over the last hour:\n  \
                     callback dispatch: 1 ok, 2 failed; the last failed 8 s ago: the board did not answer\n  \
                     idle sweep: 0 ok, 0 failed; none recorded\n  \
                     board requests: 0, 0 slow (1 s or more), 0 failed\n  \
                     writes: 0\n\
                     ⚠ callback dispatch failing for 9 s (2 of 3 passes in the last hour failed): the board did not answer\n"
                )
                .as_str(),
                ""
            )
        );

        // Twelve minutes on, with no record since: the failure is too old to warn of, and the
        // server, this test's own process, is running and stopped recording.
        let late = [
            (FILE_VAR, file),
            ("TASKS_SERVER", GONE),
            ("TASKS_NOW", "1800000725"),
            (disk::VAR, "0"),
        ];
        let (code, out, _) = run(&["status", "--json"], &late, &dir);
        assert_eq!(code, 0);
        let said: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(said["file"], file);
        assert_eq!(
            serde_json::from_value::<Report>(said["report"].clone()).unwrap(),
            report,
            "the record as /api/health answers it"
        );
        assert_eq!(
            said["warnings"],
            serde_json::json!([format!(
                "the Symphony server (pid {pid}) last recorded the board's health 12 min ago: no callback dispatch pass is recorded since"
            )])
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn status_without_a_record_says_where_it_would_be() {
        let dir = scratch("none");
        let path = dir.join("health.json");
        let vars = [
            (FILE_VAR, path.to_str().unwrap()),
            ("TASKS_SERVER", GONE),
            (disk::VAR, "0"),
        ];
        let missing = format!(
            "no board health is recorded in {}: the machine's Symphony server records it while it serves this project (`just symphony`)",
            path.display()
        );
        assert_eq!(
            run(&["status"], &vars, &dir),
            (0, format!("⚠ {missing}\n"), String::new())
        );
        let (code, out, _) = run(&["status", "--json"], &vars, &dir);
        let said: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            (code, &said["report"], &said["warnings"]),
            (0, &serde_json::Value::Null, &serde_json::json!([missing]))
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn status_refuses_what_it_cannot_read() {
        let dir = scratch("refused");
        assert_eq!(
            run(&["status"], &[("TASKS_SERVER", GONE)], &dir),
            (
                2,
                String::new(),
                "✖ a board TASKS_SERVER names keeps no health record unless TASKS_HEALTH names one\n"
                    .into()
            )
        );
        let path = dir.join("health.json");
        fs::write(&path, "{").unwrap();
        let file = path.to_str().unwrap();
        let (code, out, err) = run(&["status"], &[(FILE_VAR, file)], &dir);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.starts_with(&format!("✖ {file}: ")), "{err}");
        assert_eq!(
            run(
                &["status"],
                &[(FILE_VAR, file), ("TASKS_NOW", "soon")],
                &dir
            ),
            (
                2,
                String::new(),
                "✖ TASKS_NOW=soon is not a time in seconds\n".into()
            )
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn status_warns_of_a_disk_under_the_threshold_where_the_record_is_kept() {
        let dir = scratch("disk");
        let path = dir.join("health.json");
        recorded(&path);
        let file = path.to_str().unwrap();
        let at = |threshold: &'static str| {
            [
                (FILE_VAR, file),
                ("TASKS_SERVER", GONE),
                ("TASKS_NOW", "1800000010"),
                (disk::VAR, threshold),
            ]
        };
        let dispatch = "⚠ callback dispatch failing for 9 s (2 of 3 passes in the last hour failed): the board did not answer";

        // Under a threshold over any disk there is, the disk is low: said after the record's own
        // warnings, with its free gigabytes.
        let (code, out, err) = run(&["status"], &at("1000000000000"), &dir);
        assert_eq!((code, err.as_str()), (0, ""));
        let lines: Vec<&str> = out.lines().collect();
        let (said, before) = lines.split_last().unwrap();
        assert_eq!(before.last(), Some(&dispatch), "{out}");
        let head = format!("⚠ the disk holding {} has ", dir.display());
        let tail = " GB free, under the 1000000000000 GB the board warns at (SYMPHONY_LOW_DISK_GB): a full disk fails every build on the machine and can corrupt the board's database; `just tasks sweep` clears the build output of finished worktrees";
        assert!(said.starts_with(&head) && said.ends_with(tail), "{said}");
        let gb: f64 = said[head.len()..said.len() - tail.len()].parse().unwrap();
        assert!(gb > 0.0, "{said}");

        let (code, out, _) = run(&["status", "--json"], &at("1000000000000"), &dir);
        assert_eq!(code, 0);
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["disk"]["path"], dir.to_str().unwrap());
        assert_eq!(json["disk"]["threshold_gb"], 1e12);
        assert!(json["disk"]["free_bytes"].as_u64().unwrap() > 0, "{json}");
        let warnings = json["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 2, "{json}");
        assert!(warnings[1].as_str().unwrap().ends_with(tail), "{json}");

        // At 0 it never warns, and the JSON still says the disk.
        let (code, out, _) = run(&["status"], &at("0"), &dir);
        assert_eq!((code, out.lines().last()), (0, Some(dispatch)), "{out}");
        let (_, out, _) = run(&["status", "--json"], &at("0"), &dir);
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            (
                json["disk"]["threshold_gb"].as_f64(),
                json["warnings"].as_array().map(Vec::len)
            ),
            (Some(0.0), Some(1))
        );

        // A setting that is not a number is the default's, said; the health is still said.
        for args in [&["status"][..], &["status", "--json"]] {
            let (code, out, err) = run(args, &at("lots"), &dir);
            assert_eq!(
                (code, err.as_str()),
                (
                    0,
                    "· SYMPHONY_LOW_DISK_GB=\"lots\" is not a number of gigabytes: it warns under 20 GB free\n"
                ),
                "{args:?}"
            );
            assert!(out.contains("callback dispatch failing"), "{out}");
        }
        let (_, out, _) = run(&["status", "--json"], &at("lots"), &dir);
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            json["disk"]["threshold_gb"].as_f64(),
            Some(disk::THRESHOLD_GB)
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
