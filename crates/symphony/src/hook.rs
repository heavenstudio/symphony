//! The Symphony's generic hooks: the silent heartbeat and the memory guard. Project policy hooks
//! remain in the project.

mod event;
mod memory_guard;
mod tasks_beat;

use std::io::{BufRead, Write};

pub(crate) use tasks_beat::live_build;

const USAGE: &str = "usage: symphony hook {tasks-beat,memory-guard}";
/// The hooks, in the usage's order.
const HOOKS: [&str; 2] = ["tasks-beat", "memory-guard"];

/// `symphony hook tasks-beat|memory-guard`, with the complete tool event on stdin. No build or server
/// start. The heartbeat always exits 0; the guard exits 2 with its one line when it refuses the call.
pub fn main(args: &[String], input: &mut dyn BufRead, err: &mut dyn Write) -> i32 {
    let hook = match args {
        [hook] if HOOKS.contains(&hook.as_str()) => hook.as_str(),
        _ => return crate::cli::refuse(err, USAGE, "hook", &misnamed(args)),
    };
    let mut raw = Vec::new();
    if input.read_to_end(&mut raw).is_err() {
        return 0;
    }
    if hook == "tasks-beat" {
        tasks_beat::run(&raw, &std::env::vars().collect());
    } else if memory_guard::refuses(&raw) {
        let _ = writeln!(err, "{}", memory_guard::REASON);
        return 2;
    }
    0
}

/// Why a command line naming no one hook is refused, after its usage: no hook, one nobody has (with
/// the nearest), or words after it.
fn misnamed(args: &[String]) -> String {
    match args {
        [] => "the following arguments are required: hook".into(),
        [hook, ..] if !HOOKS.contains(&hook.as_str()) => {
            let near = match symphony_args::nearest(hook, HOOKS).as_slice() {
                [one] => format!("; did you mean '{one}'?"),
                _ => String::new(),
            };
            format!(
                "argument hook: invalid choice: {hook:?} (choose from 'tasks-beat', \
                 'memory-guard'){near}"
            )
        }
        [_, extra @ ..] => format!("unrecognized arguments: {}", extra.join(" ")),
    }
}

#[cfg(test)]
mod paths_tests;

#[cfg(test)]
mod tests {
    use super::*;

    struct Unreadable;
    impl std::io::Read for Unreadable {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("fixture read failure"))
        }
    }

    #[test]
    fn unknown_hooks_refuse_before_reading_and_unreadable_events_pass_silently() {
        for (args, problem) in [
            (
                &["unknown"][..],
                "argument hook: invalid choice: \"unknown\" (choose from 'tasks-beat', \
                 'memory-guard')",
            ),
            (
                &["memory-gaurd"],
                "argument hook: invalid choice: \"memory-gaurd\" (choose from 'tasks-beat', \
                 'memory-guard'); did you mean 'memory-guard'?",
            ),
            (&[], "the following arguments are required: hook"),
            (
                &["memory-guard", "tasks-beat"],
                "unrecognized arguments: tasks-beat",
            ),
        ] {
            let args: Vec<String> = args.iter().map(ToString::to_string).collect();
            let mut input = std::io::Cursor::new(b"untouched");
            let mut err = Vec::new();
            assert_eq!(main(&args, &mut input, &mut err), 2);
            assert_eq!(input.position(), 0);
            assert_eq!(
                String::from_utf8(err).unwrap(),
                format!(
                    "usage: symphony hook {{tasks-beat,memory-guard}}\nsymphony hook: error: \
                     {problem}\n"
                ),
                "{args:?}"
            );
        }
        for hook in ["tasks-beat", "memory-guard"] {
            let mut err = Vec::new();
            assert_eq!(
                main(
                    &[hook.into()],
                    &mut std::io::BufReader::new(Unreadable),
                    &mut err
                ),
                0
            );
            assert!(err.is_empty(), "unexpected hook diagnostics: {err:?}");
        }
    }

    #[test]
    fn the_memory_guard_refuses_with_its_one_line_and_lets_anything_else_through_silently() {
        let run = |event: serde_json::Value| {
            let mut err = Vec::new();
            let code = main(
                &["memory-guard".into()],
                &mut std::io::Cursor::new(event.to_string()),
                &mut err,
            );
            (code, String::from_utf8(err).unwrap())
        };
        let memory = "/Users/someone/.claude/projects/-Users-someone-repo/memory/MEMORY.md";
        let write = serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": memory}});
        assert_eq!(run(write), (2, format!("{}\n", memory_guard::REASON)));
        let read = serde_json::json!({"tool_name": "Read", "tool_input": {"file_path": memory}});
        assert_eq!(run(read), (0, String::new()));
    }
}
