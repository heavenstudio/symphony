//! The program's commands: `key` makes the pair and prints its line; `serve` says what it lacks
//! before it listens; `--help` says both; anything else is refused with the usage.

use super::*;
use crate::stand_in::{folder, write};

/// What the program does with `args` in `machine`: its exit and what it printed on each stream.
fn run(args: &[&str], machine: Result<PathBuf, String>) -> (ExitCode, String, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main(&args, machine, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn key_makes_the_pair_and_prints_the_line_sign_in_toml_takes() {
    let machine = folder("main-key");
    let (code, out, err) = run(&["key"], Ok(machine.to_path_buf()));
    assert_eq!((code, err.as_str()), (ExitCode::SUCCESS, ""));
    let public = settings::Key::read(&machine).unwrap().public();
    assert_eq!(out, format!("key = \"{public}\"\n"));
    let (code, out, err) = run(&["key"], Ok(machine.to_path_buf()));
    assert_eq!((code, out.as_str()), (ExitCode::FAILURE, ""));
    assert!(
        err.starts_with("symphony-sign-in: ") && err.contains("is there already"),
        "{err}"
    );
}

#[test]
fn serve_says_what_it_lacks_before_it_listens() {
    let machine = folder("main-serve");
    let (code, _, err) = run(&["serve"], Ok(machine.to_path_buf()));
    assert_eq!(code, ExitCode::FAILURE);
    assert!(err.contains("sign-in-service.toml is not there"), "{err}");
    write(
        &machine,
        settings::SETTINGS,
        "listen = \"127.0.0.1:0\"\nupstream = \"127.0.0.1:1\"\norigin = \"https://board.example.com\"\n\
         [google]\nclient-id = \"id\"\nclient-secret = \"secret\"\n",
        0o600,
    );
    let (code, _, err) = run(&["serve"], Ok(machine.to_path_buf()));
    assert_eq!(code, ExitCode::FAILURE);
    assert!(err.contains("make it with `symphony-sign-in key`"), "{err}");
    let (code, line, _) = run(&["key"], Ok(machine.to_path_buf()));
    assert_eq!(code, ExitCode::SUCCESS);
    write(
        &machine,
        settings::SERVER,
        &format!("{line}origin = \"https://other.example.com\"\n"),
        0o644,
    );
    let (code, _, err) = run(&["serve"], Ok(machine.to_path_buf()));
    assert_eq!(code, ExitCode::FAILURE);
    assert!(err.contains("names another origin"), "{err}");
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = taken.local_addr().unwrap();
    write(
        &machine,
        settings::SETTINGS,
        &format!(
            "listen = \"{listen}\"\nupstream = \"127.0.0.1:1\"\norigin = \"https://other.example.com\"\n\
             [google]\nclient-id = \"id\"\nclient-secret = \"secret\"\n"
        ),
        0o600,
    );
    let (code, _, err) = run(&["serve"], Ok(machine.to_path_buf()));
    assert_eq!(code, ExitCode::FAILURE);
    assert!(
        err.starts_with(&format!("symphony-sign-in: listen {listen}: ")),
        "{err}"
    );
    let (code, _, err) = run(&["serve"], Err("the machine folder needs HOME".into()));
    assert_eq!(
        (code, err.as_str()),
        (
            ExitCode::FAILURE,
            "symphony-sign-in: the machine folder needs HOME\n"
        )
    );
}

/// A stream that takes nothing.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// What the program could not print is a failure, whatever it did.
#[test]
fn a_line_the_program_could_not_print_is_a_failure() {
    let help = main(
        &["--help".into()],
        Err("unused".into()),
        &mut Closed,
        &mut Vec::new(),
    );
    assert_eq!(help, ExitCode::FAILURE);
    let machine = folder("main-closed");
    let key = main(
        &["key".into()],
        Ok(machine.to_path_buf()),
        &mut Closed,
        &mut Vec::new(),
    );
    assert_eq!(key, ExitCode::FAILURE);
}

#[test]
fn help_says_both_commands_and_anything_else_is_refused_with_the_usage() {
    let (code, out, _) = run(&["--help"], Err("unused".into()));
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.contains("key ") && out.contains("serve "), "{out}");
    for args in [&[][..], &["sign"], &["key", "extra"]] {
        let (code, out, err) = run(args, Err("unused".into()));
        assert_eq!((code, out.as_str()), (ExitCode::from(2), ""), "{args:?}");
        assert!(
            err.starts_with("usage: symphony-sign-in"),
            "{args:?}: {err}"
        );
    }
}
