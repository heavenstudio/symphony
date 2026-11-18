use super::*;
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt as _;

/// Answers each call from a table, keeping what was asked.
struct Table {
    answers: Vec<(&'static str, Result<Value, String>)>,
    asked: RefCell<Vec<(String, Value)>>,
}

impl Caller for Table {
    fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        self.asked.borrow_mut().push((method.into(), args));
        self.answers
            .iter()
            .find(|(name, _)| *name == method)
            .map(|(_, answer)| answer.clone())
            .expect("an answer for the call")
    }
}

fn api(answers: Vec<(&'static str, Result<Value, String>)>) -> Api<Table> {
    Api(Table {
        answers,
        asked: RefCell::default(),
    })
}

fn token() -> Value {
    json!({"compat": 1, "generation": "gen", "id": 4, "version": 2})
}

#[test]
fn each_call_is_the_boards_and_its_answer_is_read() {
    let task = json!({"slug": "a/a", "id": 4, "title": "T", "body": "B", "status": "backlog"});
    let bound = json!({"bound": token(), "title": "T", "body": "B"});
    let board = api(vec![
        (
            "integration_state",
            Ok(json!({"ok": {"path": "/repo/.git/symphony/integrations/github"}})),
        ),
        (
            "tasks",
            Ok(
                json!({"ok": [task, {"slug": "no number"}, {"slug": "b/b", "id": 5, "title": "U"}]}),
            ),
        ),
        ("create", Ok(json!({"ok": task}))),
        ("bound", Ok(json!({"ok": bound}))),
        ("edit_bound", Ok(json!({"ok": bound}))),
        ("say", Ok(json!({"ok": {"id": 1}}))),
    ]);
    assert_eq!(
        board.state_folder().unwrap(),
        PathBuf::from("/repo/.git/symphony/integrations/github")
    );
    let tasks = board.tasks().unwrap();
    assert_eq!(tasks.len(), 2, "a task of another shape is left out");
    assert_eq!(
        (tasks[1].slug.as_str(), tasks[1].body.as_str()),
        ("b/b", "")
    );
    assert_eq!(board.propose("a/a", "T", "B", "amber-fox-1").unwrap().id, 4);
    let read = board.bound("a/a").unwrap();
    assert_eq!((read.token.generation(), read.token.id()), ("gen", 4));
    board
        .edit("a/a", &read.token, "T2", "B2", "amber-fox-1")
        .unwrap();
    board
        .note("a/a", "text", Some("@orchestrator"), "key", "amber-fox-1")
        .unwrap();

    let asked = board.0.asked.borrow();
    assert_eq!(
        asked[0],
        ("integration_state".into(), json!({"name": "github"}))
    );
    assert_eq!(
        asked[2].1,
        json!({"slug": "a/a", "title": "T", "body": "B", "by": "amber-fox-1", "status": "backlog"}),
        "a proposal starts in the backlog"
    );
    assert_eq!(
        asked[4].1,
        json!({"slug": "a/a", "bound": token(), "by": "amber-fox-1", "title": "T2", "body": "B2"}),
        "the token goes back as it was given"
    );
    assert_eq!(
        asked[5].1,
        json!({"slug": "a/a", "sender": "amber-fox-1", "text": "text", "to": "@orchestrator", "key": "key"})
    );
}

#[test]
fn a_refusal_is_told_from_a_failure() {
    let board = api(vec![
        (
            "bound",
            Ok(json!({"refused": "the board is bound to no recovery generation"})),
        ),
        ("tasks", Ok(json!({"error": "no board"}))),
        ("create", Ok(json!({"error": {"code": 1}}))),
        ("integration_state", Err("the program did not start".into())),
        ("edit_bound", Ok(json!({"neither": true}))),
        ("say", Ok(json!({"ok": "not a bound"}))),
    ]);
    assert_eq!(
        board.bound("a/a"),
        Err(BoardError::Refused(
            "the board is bound to no recovery generation".into()
        ))
    );
    let failed = |error: BoardError| match error {
        BoardError::Failed(why) => why,
        BoardError::Refused(why) => panic!("refused: {why}"),
    };
    assert_eq!(
        failed(board.tasks().unwrap_err()),
        "tasks api tasks: no board"
    );
    assert!(failed(board.propose("a/a", "T", "B", "x").unwrap_err()).contains("{\"code\":1}"));
    assert!(failed(board.state_folder().unwrap_err()).contains("the program did not start"));
    let edit = board
        .edit("a/a", &Token(token()), "T", "B", "x")
        .unwrap_err();
    assert!(failed(edit.clone()).contains("no result"));
    assert_eq!(
        edit.said(),
        "tasks api edit_bound: an answer with no result"
    );
    assert!(board.note("a/a", "t", None, "k", "x").is_ok());

    // Answers of another shape fail, never read as something else.
    let board = api(vec![
        ("tasks", Ok(json!({"ok": {}}))),
        ("create", Ok(json!({"ok": {}}))),
        ("bound", Ok(json!({"ok": {"title": "T"}}))),
        ("integration_state", Ok(json!({"ok": {}}))),
    ]);
    assert!(board.tasks().is_err() && board.propose("a/a", "T", "B", "x").is_err());
    assert!(board.bound("a/a").is_err() && board.state_folder().is_err());
    assert_eq!(Token(json!({})).generation(), "");
}

#[test]
fn the_process_carries_one_call_on_the_programs_standard_input() {
    let dir = std::env::temp_dir().join(format!(
        "symphony-github-board-process-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("tasks");
    // Answers with what it was run as and what it read.
    std::fs::write(
        &program,
        "#!/bin/sh\nif [ \"$1\" != api ]; then echo 'not api' >&2; exit 2; fi\nprintf '{\"ok\": {\"cwd\": \"%s\", \"call\": %s}}' \"$(pwd -P)\" \"$(cat)\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let cwd = dir.canonicalize().unwrap();
    let answer = Process::new(&program, &cwd)
        .call("get", json!({"slug": "a/a"}))
        .unwrap();
    assert_eq!(
        answer["ok"]["call"],
        json!({"method": "get", "args": {"slug": "a/a"}})
    );
    assert_eq!(answer["ok"]["cwd"], json!(cwd));

    std::fs::write(
        &program,
        "#!/bin/sh\ncat > /dev/null\necho 'the board is down' >&2\nexit 1\n",
    )
    .unwrap();
    let failed = Process::new(&program, &cwd)
        .call("get", json!({}))
        .unwrap_err();
    assert!(
        failed.contains("gave no answer") && failed.contains("the board is down"),
        "{failed}"
    );
    let absent = Process::new(&dir.join("absent"), &cwd)
        .call("get", json!({}))
        .unwrap_err();
    assert!(absent.contains("did not start"), "{absent}");
    let _ = std::fs::remove_dir_all(&dir);
}
