//! The Symphony's silent heartbeat hook. Project policy hooks remain in the project.

mod event;
mod tasks_beat;

use std::io::{BufRead, Write};

/// `symphony hook tasks-beat`, with the complete tool event on stdin. No build or server start.
pub fn main(args: &[String], input: &mut dyn BufRead, err: &mut dyn Write) -> i32 {
    if args != ["tasks-beat"] {
        let _ = writeln!(err, "usage: symphony hook tasks-beat");
        return 2;
    }
    let mut raw = Vec::new();
    if input.read_to_end(&mut raw).is_ok() {
        tasks_beat::run(&raw, &std::env::vars().collect());
    }
    0
}

#[cfg(test)]
mod paths_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_hooks_refuse_before_reading_and_unreadable_events_pass_silently() {
        let mut input = std::io::Cursor::new(b"untouched");
        let mut err = Vec::new();
        assert_eq!(main(&["unknown".into()], &mut input, &mut err), 2);
        assert_eq!(input.position(), 0);
        assert_eq!(err, b"usage: symphony hook tasks-beat\n");
        struct Unreadable;
        impl std::io::Read for Unreadable {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("fixture read failure"))
            }
        }
        err.clear();
        assert_eq!(
            main(
                &["tasks-beat".into()],
                &mut std::io::BufReader::new(Unreadable),
                &mut err
            ),
            0
        );
        assert!(err.is_empty());
    }
}
