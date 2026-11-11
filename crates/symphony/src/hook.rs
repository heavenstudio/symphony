//! The Symphony's generic hooks: the silent heartbeat and the memory guard. Project policy hooks
//! remain in the project.

mod event;
mod memory_guard;
mod tasks_beat;

use std::io::{BufRead, Write};

pub(crate) use tasks_beat::live_build;

/// `symphony hook tasks-beat|memory-guard`, with the complete tool event on stdin. No build or server
/// start. The heartbeat always exits 0; the guard exits 2 with its one line when it refuses the call.
pub fn main(args: &[String], input: &mut dyn BufRead, err: &mut dyn Write) -> i32 {
    let hook = match args {
        [hook] if hook == "tasks-beat" || hook == "memory-guard" => hook.as_str(),
        _ => {
            let _ = writeln!(err, "usage: symphony hook {{tasks-beat,memory-guard}}");
            return 2;
        }
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
        for args in [&["unknown"][..], &[], &["memory-guard", "tasks-beat"]] {
            let args: Vec<String> = args.iter().map(ToString::to_string).collect();
            let mut input = std::io::Cursor::new(b"untouched");
            let mut err = Vec::new();
            assert_eq!(main(&args, &mut input, &mut err), 2);
            assert_eq!(input.position(), 0);
            assert_eq!(err, b"usage: symphony hook {tasks-beat,memory-guard}\n");
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
