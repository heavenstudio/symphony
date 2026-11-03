//! The leftovers the one server looks for: Symphony processes whose parent is init, which nothing
//! ends on its own (`docs_server_test.py` runs from before 371b52e6 left 1,144 of them, 29.8 GB, on
//! 2026-10-07). They are counted and shown, never killed: that is the owner's call.

use std::path::Path;

/// The pid `home`'s record names, if it names one.
pub fn recorded(home: &Path) -> Option<i64> {
    let text = std::fs::read_to_string(home.join(crate::home::ADDRESS)).ok()?;
    let running: crate::home::Running = serde_json::from_str(&text).ok()?;
    Some(running.pid)
}

/// How many Symphony processes in `ps -axww -o pid=,ppid=,command=` have init as their parent, whose
/// starter is gone: a `docs_server` app (or a test's stand-in for it), and a `symphony-server serve`
/// other than `own`, the server asking, and other than the one server its `--home`'s record names
/// (`recorded`), both detached on purpose. A leftover server's own app has that server as its parent,
/// so it is not seen: the server is the one counted.
pub fn count(ps: &str, own: i64, recorded: impl Fn(&Path) -> Option<i64>) -> usize {
    ps.lines()
        .filter(|line| {
            let Some((pid, rest)) = line.trim().split_once(char::is_whitespace) else {
                return false;
            };
            let Some((parent, command)) = rest.trim().split_once(char::is_whitespace) else {
                return false;
            };
            let Ok(pid) = pid.parse::<i64>() else {
                return false;
            };
            if parent != "1" {
                return false;
            }
            let Some(args) = crate::sys::server_command(command.trim()) else {
                return line.contains("docs_server");
            };
            let home = args.strip_prefix("--home ").and_then(|rest| {
                rest.split_once(" --root ")
                    .or_else(|| rest.split_once(" --app "))
                    .map(|(home, _)| home)
            });
            pid != own && home.is_none_or(|home| recorded(Path::new(home)) != Some(pid))
        })
        .count()
}

/// The count, from this machine's process table now; `None` when `ps` cannot be read.
/// Cancelling the scan kills its owned child; Tokio reaps it without holding server shutdown.
/// The table is written to a file ([`crate::sys::capture`]), never a pipe: a child another thread
/// spawns meanwhile could keep a pipe's writer and hold the round open for its whole life.
pub async fn scan() -> Option<usize> {
    let table = crate::sys::capture().ok()?;
    let status = tokio::process::Command::new("ps")
        .args(["-axww", "-o", "pid=,ppid=,command="])
        .stdin(std::process::Stdio::null())
        .stdout(table.try_clone().ok()?)
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .status()
        .await
        .ok()?;
    let table = crate::sys::captured(table).ok()?;
    status.success().then(|| {
        count(
            &String::from_utf8_lossy(&table),
            i64::from(std::process::id()),
            recorded,
        )
    })
}

/// The line every page shows while there are any.
pub fn banner(count: usize) -> String {
    let (noun, verb, them) = if count == 1 {
        ("process", "is", "it")
    } else {
        ("processes", "are", "them")
    };
    format!(
        "<div class=\"code-banner\" role=\"status\"><p><strong>{count} Symphony {noun} from test runs \
         {verb} running with no parent</strong>: nothing ends {them} on its own, and the Symphony kills \
         none. <code>ps -axo pid,ppid,command | grep -e docs_server -e 'symphony-server serve'</code> lists \
         {them}, with parent 1; stop {them} by pid.</p></div>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // An app is known by `docs_server` on its command line, whatever interpreter runs it (`interp` here).
    const PS: &str = "  101     1 /usr/bin/interp -B tools/docs_server --root /tmp/x --port 5 --host 127.0.0.1\n\
                      \x20 102     1 sh -c t=$1; shift; ... sh 100 interp -B tools/docs_server --root /tmp/y\n\
                      \x20 103   555 /usr/bin/interp -B tools/docs_server --root /repo --port 6\n\
                      \x20 104     1 /Users/o/.git/sf-symphony/bin/symphony-server serve --home /x --app interp tools/docs_server\n\
                      \x20 105     1 /private/tmp/t/deps/symphony-server serve --home /tmp/y/sf-symphony --app stub\n\
                      \x20 106   555 /private/tmp/t/deps/symphony-server serve --root /tmp/z --port 7 --app stub\n\
                      \x20 107     1 /Users/o/.git/sf-symphony/bin/symphony-server --stop\n\
                      \x20 108     1 /usr/sbin/cron\n\
                      garbage line\n";

    /// No home's record names any server.
    fn unrecorded(_: &Path) -> Option<i64> {
        None
    }

    #[test]
    fn a_docs_server_or_a_symphony_server_whose_parent_is_init_is_counted_but_the_one_server_itself()
     {
        // 101 and 102 are apps under init; 105 is a test's server left under init, whose own app has it as
        // its parent and so is not seen; 104 is the one server, which asks.
        assert_eq!(count(PS, 104, unrecorded), 3);
    }

    #[test]
    fn a_server_its_home_does_not_record_counts_whoever_asks() {
        assert_eq!(count(PS, 999, unrecorded), 4);
        let another = |home: &Path| (home == Path::new("/x")).then_some(4242);
        assert_eq!(count(PS, 999, another), 4);
    }

    #[test]
    fn the_one_server_its_home_records_is_no_leftover_to_a_server_of_its_own_port() {
        // 106 asks, a server of its own port; 104 is the machine's one server, named by /x/server.json.
        let one = |home: &Path| (home == Path::new("/x")).then_some(104);
        assert_eq!(count(PS, 106, one), 3);
    }

    #[test]
    fn nothing_is_counted_in_an_empty_table() {
        assert_eq!(count("", 1, unrecorded), 0);
    }

    #[test]
    fn spaces_in_the_executable_and_home_paths_do_not_hide_a_recorded_server() {
        let ps = "101 1 /repo with spaces/.git/sf-symphony/bin/symphony-server serve --home /repo with spaces/.git/sf-symphony --root /repo with spaces --port 8765 --app stub\n\
                  102 1 /repo with spaces/.git/sf-symphony/bin/symphony-server serve --root /own root --port 9000 --app stub\n";
        let recorded = |home: &Path| {
            assert_eq!(home, Path::new("/repo with spaces/.git/sf-symphony"));
            Some(101)
        };
        assert_eq!(count(ps, 999, recorded), 1);
        assert_eq!(count(ps, 999, unrecorded), 2);
    }

    #[test]
    fn a_homes_record_is_read_for_its_pid() {
        let home = std::env::temp_dir().join(format!("orphans-record-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(recorded(&home), None);
        std::fs::write(home.join("server.json"), "{not a record").unwrap();
        assert_eq!(recorded(&home), None);
        std::fs::write(
            home.join("server.json"),
            r#"{"port":5,"pid":104,"root":"/r"}"#,
        )
        .unwrap();
        assert_eq!(recorded(&home), Some(104));
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn the_banner_names_the_count_and_says_nothing_is_killed() {
        let said = banner(3);
        assert!(said.contains("3 "), "{said}");
        assert!(said.contains("class=\"code-banner\""), "{said}");
        assert!(banner(1).contains("1 Symphony process "), "{}", banner(1));
        assert!(said.contains("symphony-server serve"), "{said}");
    }
}
