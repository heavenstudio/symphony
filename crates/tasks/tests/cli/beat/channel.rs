//! A launched session's channel, registered by its own heartbeat: `launch` and `restore` leave the
//! registration to the agent, and one that skips `just tasks callback` would sit idle on messages
//! nobody delivers. The hook's beat before a board command registers the session's inbox socket
//! for the handle an open launch records its conversation for.

use super::bash;
use crate::Case;
use serde_json::Value;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use symphony_board::{NewLaunch, NewTask};

/// A board with a launched executor, `handle`, whose conversation is `CONVERSATION`, and a keys'
/// folder of its own.
struct Launched {
    case: Case,
    handle: String,
    folder: PathBuf,
}

const CONVERSATION: &str = "conv-1";

impl Launched {
    fn new(name: &str) -> Launched {
        let folder = super::keys(&format!("channel-{name}"));
        let mut case = Case::new();
        case.env
            .vars
            .insert("TASKS_KEYS".into(), folder.to_string_lossy().into_owned());
        let filer = case.register("worker");
        let handle = case.register("worker");
        let store = case.store();
        store
            .create(&NewTask::new("sim/a", "A task", &filer))
            .unwrap();
        store
            .record_launch(&NewLaunch {
                claude_session: CONVERSATION.into(),
                pane: "t1".into(),
                ..NewLaunch::new("sim/a", &handle, "executor")
            })
            .unwrap();
        Launched {
            case,
            handle,
            folder,
        }
    }

    /// A socket somebody listens on, as a Claude Code session's inbox is.
    fn socket(&self, name: &str) -> (PathBuf, UnixListener) {
        let path = PathBuf::from(format!(
            "/tmp/symphony-channel-{}-{name}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        (path, listener)
    }

    /// The hook's beat of `event` in a session whose inbox is `socket`.
    fn beat(&self, socket: &std::path::Path, event: &Value) {
        let mut env = self.case.env.clone();
        env.vars.insert(
            "CLAUDE_CODE_MESSAGING_SOCKET".into(),
            socket.to_string_lossy().into_owned(),
        );
        env.vars.insert("HC_TERMINAL_ID".into(), "t1".into());
        super::beat(&env, event);
    }

    /// A board command of the launched session's own, about to run.
    fn inbox(&self) -> Value {
        bash(&format!("just tasks inbox --as {}", self.handle))
            .session(CONVERSATION)
            .0
    }

    /// The address the handle's channel is registered at, if it has one.
    fn address(&self) -> Option<String> {
        let held = self.case.store().callbacks().unwrap();
        held.get(&self.handle).map(|c| c.address.clone())
    }
}

impl Drop for Launched {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn uds(path: &std::path::Path) -> Option<String> {
    Some(format!("uds:{}", path.display()))
}

#[test]
fn a_launched_sessions_board_command_registers_the_channel_it_never_registered() {
    let l = Launched::new("first");
    let (path, _listener) = l.socket("first");
    assert_eq!(l.address(), None);
    l.beat(&path, &l.inbox());
    assert_eq!(l.address(), uds(&path));
    let _ = std::fs::remove_file(path);
}

#[test]
fn only_the_conversation_a_launch_records_is_connected() {
    let l = Launched::new("other");
    let (path, _listener) = l.socket("other");
    // Another session acting as the handle is not the one launched for it.
    let other = bash(&format!("just tasks inbox --as {}", l.handle))
        .session("conv-2")
        .0;
    l.beat(&path, &other);
    assert_eq!(l.address(), None);
    // Nor is a session of a handle nobody launched.
    let free = l.case.register("worker");
    let event = bash(&format!("just tasks inbox --as {free}"))
        .session("conv-3")
        .0;
    l.beat(&path, &event);
    assert!(!l.case.store().callbacks().unwrap().contains_key(&free));
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_subagent_and_a_beat_an_agent_wrote_register_nothing() {
    let l = Launched::new("guarded");
    let (path, _listener) = l.socket("guarded");
    // A subagent never registers its parent's channel.
    let inside = bash(&format!("just tasks inbox --as {}", l.handle))
        .session(CONVERSATION)
        .agent("agent-1")
        .0;
    l.beat(&path, &inside);
    assert_eq!(l.address(), None);
    // `just tasks beat` reads an event an agent wrote, not the session's own tool call.
    let mut marked = super::marked(&l.case.env);
    marked.vars.insert(
        "CLAUDE_CODE_MESSAGING_SOCKET".into(),
        path.to_string_lossy().into_owned(),
    );
    super::beat(&marked, &l.inbox());
    assert_eq!(l.address(), None);
    // A call that is no board command of the handle's connects nothing either.
    l.beat(&path, &bash("cargo test tasks").session(CONVERSATION).0);
    assert_eq!(l.address(), None);
    // Nor does the board command's return: only the hook's event before it runs.
    let returned = bash(&format!("just tasks inbox --as {}", l.handle))
        .session(CONVERSATION)
        .ended()
        .0;
    l.beat(&path, &returned);
    assert_eq!(l.address(), None);
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_launch_already_closed_connects_nothing() {
    let l = Launched::new("closed");
    let (path, _listener) = l.socket("closed");
    l.case
        .store()
        .close_task_session("sim/a", &l.handle, "its pane is gone", "restore it")
        .unwrap();
    l.beat(&path, &l.inbox());
    assert_eq!(l.address(), None);
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_socket_nobody_listens_on_yet_is_tried_again_at_the_next_board_command() {
    let l = Launched::new("deaf");
    let (path, listener) = l.socket("deaf");
    drop(listener);
    // The board refuses a socket it cannot reach: nothing is registered, and nothing remembered.
    l.beat(&path, &l.inbox());
    assert_eq!(l.address(), None);
    assert!(!l.folder.join(format!("{CONVERSATION}.channel")).exists());
    let (path, _listener) = l.socket("deaf");
    l.beat(&path, &l.inbox());
    assert_eq!(l.address(), uds(&path));
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_socket_remembered_for_a_session_goes_with_its_binding() {
    let l = Launched::new("unbound");
    let (path, _listener) = l.socket("unbound");
    l.beat(&path, &l.inbox());
    let remembered = l.folder.join(format!("{CONVERSATION}.channel"));
    assert_eq!(
        std::fs::read_to_string(&remembered).unwrap(),
        path.to_string_lossy()
    );
    // The handle leaves: the session's next beat finds it bound to nobody.
    super::ok(&l.case.env, &["--as", &l.handle, "leave"]);
    let read = serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "Read",
                                  "session_id": CONVERSATION});
    super::beat(&l.case.env, &read);
    assert!(!l.folder.join(CONVERSATION).exists() && !remembered.exists());
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_channel_turned_off_stays_off_until_the_conversation_runs_on_a_new_socket() {
    let l = Launched::new("off");
    let (first, _a) = l.socket("off-a");
    l.beat(&first, &l.inbox());
    assert_eq!(l.address(), uds(&first));
    // The session turns its channel off: its later board commands leave it off.
    l.case
        .store()
        .callback(&l.handle, None, false, false)
        .unwrap();
    l.beat(&first, &l.inbox());
    assert_eq!(l.address(), None);
    // Restored, the conversation is a new process on a new socket, and is connected again.
    let (second, _b) = l.socket("off-b");
    l.beat(&second, &l.inbox());
    assert_eq!(l.address(), uds(&second));
    for path in [first, second] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn a_restored_conversation_moves_the_channel_its_last_process_left() {
    let l = Launched::new("moved");
    let (first, a) = l.socket("moved-a");
    l.beat(&first, &l.inbox());
    drop(a);
    let _ = std::fs::remove_file(&first);
    let (second, _b) = l.socket("moved-b");
    l.beat(&second, &l.inbox());
    assert_eq!(l.address(), uds(&second));
    let _ = std::fs::remove_file(second);
}

#[test]
fn a_channel_that_holds_the_chat_seat_is_left_where_it_is() {
    let l = Launched::new("chat");
    let (seat, _a) = l.socket("chat-a");
    let held = format!("uds:{}", seat.display());
    l.case
        .store()
        .callback(&l.handle, Some(&held), true, false)
        .unwrap();
    let (other, _b) = l.socket("chat-b");
    l.beat(&other, &l.inbox());
    assert_eq!(l.address(), Some(held));
    for path in [seat, other] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn a_board_command_before_its_launch_is_recorded_is_connected_at_the_next_one() {
    let l = Launched::new("early");
    let (path, _listener) = l.socket("early");
    // The session's first board command runs before its restore records the conversation.
    let early = || {
        bash(&format!("just tasks inbox --as {}", l.handle))
            .session("conv-early")
            .0
    };
    l.beat(&path, &early());
    assert_eq!(l.address(), None);
    assert!(!l.folder.join("conv-early.channel").exists());
    // Recorded since: its next board command registers the socket, and remembers it.
    let store = l.case.store();
    store
        .create(&NewTask::new("sim/b", "B task", &l.handle))
        .unwrap();
    store
        .record_launch(&NewLaunch {
            claude_session: "conv-early".into(),
            pane: "t2".into(),
            ..NewLaunch::new("sim/b", &l.handle, "executor")
        })
        .unwrap();
    l.beat(&path, &early());
    assert_eq!(l.address(), uds(&path));
    assert_eq!(
        std::fs::read_to_string(l.folder.join("conv-early.channel")).unwrap(),
        path.to_string_lossy()
    );
    let _ = std::fs::remove_file(path);
}
