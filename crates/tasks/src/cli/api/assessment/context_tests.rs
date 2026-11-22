//! The api's entries with no context given, and frozen invocation/stream connection state. These
//! controls do not supply publication evidence; native producer tests cover target admission on
//! real boards.

use super::super as api;
use serde_json::{Value, json};
use std::cell::Cell;
use std::io::Cursor;

#[test]
fn connect_only_valid_v2_calls_remain_pending_before_any_connection_in_every_input_mode() {
    for protocol in ["assessment/1", "assessment/2"] {
        let mut call = json!({"method": "record_waits", "args": {
            "protocol": protocol, "slug": "task", "record": format!("{}: changes", "a".repeat(40)),
            "assessment": "occ-event", "actor": "worker",
            "expected": {"slug": "task", "status": "ready", "version": 0},
        }});
        if protocol == "assessment/2" {
            call["args"]["publication"] = json!({
                "id": "occ-source", "branch": "tools/task", "tip": "a".repeat(40),
                "verification": null,
            });
        }
        for mode in ["single", "list", "lines"] {
            let input = if mode == "list" {
                json!([call]).to_string()
            } else {
                format!("{call}\n")
            };
            let mut input = Cursor::new(input);
            let mut out = Vec::new();
            let connections = Cell::new(0);
            let connect = || {
                connections.set(connections.get() + 1);
                Err("v1 connection selected".into())
            };
            let code = if mode == "lines" {
                api::serve_in(&mut input, &mut out, connect, None)
            } else {
                api::run_in(&mut input, &mut out, connect, None)
            };
            assert_eq!(code, 0, "{mode}: {call}: {out:?}");
            let response: Value = serde_json::from_slice(&out).unwrap();
            let response = if mode == "list" {
                &response[0]
            } else {
                &response
            };
            let (error, count) = if protocol == "assessment/2" {
                (
                    "assessment/2 requires explicit effective CLI context; evidence remains pending",
                    0,
                )
            } else {
                ("v1 connection selected", 1)
            };
            assert_eq!(*response, json!({"error": error}), "{mode}: {call}");
            assert_eq!(connections.get(), count, "{mode}: {call}: {response}");
        }
    }
}

#[test]
fn legacy_wrappers_keep_one_lazy_connection_for_lists_and_lines() {
    let call = json!({"method": "assessment_state", "args": {"slug": "task"}});
    for streaming in [false, true] {
        let input = if streaming {
            format!("{call}\n{call}\n")
        } else {
            json!([call, call]).to_string()
        };
        let mut input = Cursor::new(input);
        let mut out = Vec::new();
        let connections = Cell::new(0);
        let connect = || {
            connections.set(connections.get() + 1);
            Err("v1 connection selected".into())
        };
        let code = if streaming {
            api::serve_in(&mut input, &mut out, connect, None)
        } else {
            api::run_in(&mut input, &mut out, connect, None)
        };
        assert_eq!(code, 0, "streaming={streaming}: {out:?}");
        let replies = if streaming {
            String::from_utf8(out)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>()
        } else {
            serde_json::from_slice::<Vec<Value>>(&out).unwrap()
        };
        assert_eq!(
            replies,
            vec![json!({"error": "v1 connection selected"}); 2],
            "streaming={streaming}"
        );
        assert_eq!(connections.get(), 1, "streaming={streaming}");
    }
}

#[test]
fn invalid_arguments_and_protocols_never_open_the_selected_connection() {
    let calls = [
        json!({"method": "assessment_state", "args": {"slug": "task", "path": "/forbidden"}}),
        json!({"method": "record_waits", "args": {
            "protocol": "assessment/99", "slug": "task", "record": "aaa: changes",
            "assessment": "occ-event", "actor": "worker",
            "expected": {"slug": "task", "status": "ready", "version": 0},
        }}),
    ];
    for streaming in [false, true] {
        let input = if streaming {
            format!("{}\n{}\n", calls[0], calls[1])
        } else {
            json!(calls).to_string()
        };
        let mut input = Cursor::new(input);
        let mut out = Vec::new();
        let connections = Cell::new(0);
        let connect = || {
            connections.set(connections.get() + 1);
            Err("must not connect".into())
        };
        let context = crate::cli::Env::default();
        let code = if streaming {
            api::serve_in(&mut input, &mut out, connect, Some(&context))
        } else {
            api::run_in(&mut input, &mut out, connect, Some(&context))
        };
        assert_eq!(code, 0, "streaming={streaming}: {out:?}");
        let replies = if streaming {
            String::from_utf8(out)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>()
        } else {
            serde_json::from_slice::<Vec<Value>>(&out).unwrap()
        };
        assert!(
            replies[0]["error"].as_str().unwrap().contains("path"),
            "{replies:?}"
        );
        assert!(
            replies[1]["error"]
                .as_str()
                .unwrap()
                .contains("unsupported assessment protocol"),
            "{replies:?}"
        );
        assert_eq!(
            connections.get(),
            0,
            "streaming={streaming}: {calls:?}: {replies:?}"
        );
    }
}

#[test]
fn a_connect_only_entry_names_no_integration_state_and_reaches_no_board_for_it() {
    let mut input =
        Cursor::new(json!({"method": "integration_state", "args": {"name": "github"}}).to_string());
    let mut out = Vec::new();
    let connections = Cell::new(0);
    let connect = || {
        connections.set(connections.get() + 1);
        Err("never reached".into())
    };
    assert_eq!(api::run_in(&mut input, &mut out, connect, None), 0);
    let answer: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        answer,
        json!({"error": "integration_state: this entry was given no folder to find the project from"})
    );
    assert_eq!(connections.get(), 0);
}
