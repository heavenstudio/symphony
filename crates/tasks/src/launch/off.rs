//! What keeps an agent from starting: its switch turned off (`[agents.launch.on]`,
//! [`Defaults::off`]), or its program missing from this machine. A launch, a restore of a closed
//! pane (and a reviewer's resume, which is one) and the staffing pass ask [`refusing`] before
//! anything starts, so no start of either kind passes it; an agent already running is never
//! stopped by it. Its program is looked for as the Settings page looks for it: by its name on the
//! caller's search path ([`Machine::path`](super::Machine)), whatever the switch says.

use super::Defaults;
use crate::builds;
use std::ffi::OsStr;

/// Why `agent` (`claude` or `codex`) may not start, under the settings in force (`defaults`) on a
/// machine whose search path is `path`: turned off, naming where, and its program missing, each
/// that holds; none when it may.
pub fn refusing(defaults: &Defaults, path: Option<&OsStr>, agent: &str) -> Option<String> {
    let mut why = Vec::new();
    if let Some(switch) = defaults.off(agent) {
        why.push(format!(
            "{agent} is turned off ({}): the board starts none while it is, and turning it on \
             again is that switch alone",
            switch.said()
        ));
    }
    if builds::on_path(agent, path).is_none() {
        why.push(format!(
            "this machine has no {agent} program on its search path, so it cannot start here"
        ));
    }
    (!why.is_empty()).then(|| why.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::defaults::Switch;
    use crate::limits::Written;
    use std::ffi::OsString;

    /// A folder of the test `name`'s holding a runnable program for each of `agents`: its search
    /// path, and the folder.
    fn path_with(name: &str, agents: &[&str]) -> (OsString, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("launch-off-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for agent in agents {
            let program = dir.join(agent);
            std::fs::write(&program, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir.clone().into_os_string(), dir)
    }

    fn off(agent: usize, at: &str, line: Option<usize>) -> Defaults {
        let mut defaults = Defaults {
            agent: Some("claude"),
            ..Defaults::default()
        };
        defaults.on[agent] = Some(Switch {
            on: false,
            at: Written {
                at: at.into(),
                line,
            },
        });
        defaults
    }

    #[test]
    fn an_agent_on_with_its_program_starts() {
        let (path, dir) = path_with("on", &["claude", "codex"]);
        let defaults = Defaults::default();
        for agent in ["claude", "codex"] {
            assert_eq!(refusing(&defaults, Some(&path), agent), None, "{agent}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_agent_turned_off_is_refused_naming_where_from_each_place() {
        let (path, dir) = path_with("each", &["claude", "codex"]);
        for (at, line, said) in [
            (
                "[agents.launch.on] codex",
                Some(9),
                "symphony.toml [agents.launch.on] codex, line 9",
            ),
            (
                "the board's store [agents.launch.on] codex",
                None,
                "the board's store [agents.launch.on] codex",
            ),
            (
                "symphony.local.toml [agents.launch.on] codex",
                Some(2),
                "symphony.local.toml [agents.launch.on] codex, line 2",
            ),
        ] {
            assert_eq!(
                refusing(&off(0, at, line), Some(&path), "codex"),
                Some(format!(
                    "codex is turned off ({said}): the board starts none while it is, and \
                     turning it on again is that switch alone"
                )),
                "{at}"
            );
            // The other agent is not touched.
            assert_eq!(refusing(&off(0, at, line), Some(&path), "claude"), None);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_launch_default_starts_whatever_its_switch_says() {
        let (path, dir) = path_with("default", &["claude", "codex"]);
        // A switch turning off claude while claude is the default does not hold.
        let defaults = off(1, "the board's store [agents.launch.on] claude", None);
        assert_eq!(refusing(&defaults, Some(&path), "claude"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_agent_whose_program_is_missing_is_refused_whatever_its_switch_says() {
        let (path, dir) = path_with("missing", &["claude"]);
        let missing =
            "this machine has no codex program on its search path, so it cannot start here";
        assert_eq!(
            refusing(&Defaults::default(), Some(&path), "codex"),
            Some(missing.into())
        );
        assert_eq!(
            refusing(&Defaults::default(), None, "claude"),
            Some(
                "this machine has no claude program on its search path, so it cannot start here"
                    .into()
            ),
            "no search path finds nothing"
        );
        // Off and missing: both are said.
        let both = refusing(
            &off(0, "[agents.launch.on] codex", Some(3)),
            Some(&path),
            "codex",
        );
        assert_eq!(
            both,
            Some(format!(
                "codex is turned off (symphony.toml [agents.launch.on] codex, line 3): the board \
                 starts none while it is, and turning it on again is that switch alone; {missing}"
            ))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
