//! A launched session's channel, registered by its own heartbeat: `launch` and `restore` leave the
//! registration to the agent, and one that skips `just tasks callback` would sit idle on messages
//! nobody delivers. The hook's beat before a board command registers the session's inbox socket
//! for the handle an open launch records its conversation for. Run as the `tasks` binary, as the
//! hook runs it, the beat leaves the delivery to the channel it registers to a detached dispatcher.

use super::bash;
use crate::Case;
use crate::detached::{self, Listener};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::Duration;
use symphony_board::cli::{Env, main_detaching};
use symphony_board::{NewLaunch, NewTask};

/// A board with a launched executor, `handle`, on a task `filer` filed, whose conversation is
/// `CONVERSATION`, and a keys' folder of its own. It removes the folder and every socket it bound
/// when it goes, whatever a case asserted.
struct Launched {
    case: Case,
    filer: String,
    handle: String,
    folder: PathBuf,
    sockets: RefCell<Vec<PathBuf>>,
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
            filer,
            handle,
            folder,
            sockets: RefCell::default(),
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
        self.sockets.borrow_mut().push(path.clone());
        (path, listener)
    }

    /// The environment of the launched session whose inbox is `socket`, in terminal `t1`.
    fn session(&self, socket: &Path) -> Env {
        let mut env = self.case.env.clone();
        env.vars.insert(
            "CLAUDE_CODE_MESSAGING_SOCKET".into(),
            socket.to_string_lossy().into_owned(),
        );
        env.vars.insert("HC_TERMINAL_ID".into(), "t1".into());
        env
    }

    /// The hook's beat of `event` in a session whose inbox is `socket`.
    fn beat(&self, socket: &Path, event: &Value) {
        super::beat(&self.session(socket), event);
    }

    /// [`Launched::beat`] as the `tasks` binary runs it (`cli::main_detaching`), which starts a
    /// write's dispatcher detached: in a folder that is there, the event's, where that dispatcher
    /// starts.
    fn beat_as_the_binary(&self, socket: &Path, event: &Value) {
        let mut event = event.clone();
        event["cwd"] = json!(std::env::temp_dir());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main_detaching(
            &["beat".to_string()],
            &self.session(socket),
            &mut event.to_string().as_bytes(),
            &mut out,
            &mut err,
            Path::new(env!("CARGO_BIN_EXE_tasks")),
        );
        let out = String::from_utf8(out).unwrap();
        let err = String::from_utf8(err).unwrap();
        assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));
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
        for socket in self.sockets.get_mut().drain(..) {
            let _ = std::fs::remove_file(socket);
        }
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
}

#[test]
fn the_hooks_beat_leaves_the_delivery_to_the_channel_it_registers_to_a_detached_dispatcher() {
    let l = Launched::new("handed");
    let listener = Listener::new("beat-handed");
    // A message for the handle, which waits for the channel its session never registered.
    let id = (l.case.store())
        .say("sim/a", &l.filer, "over to the channel", Some(&l.handle))
        .unwrap()
        .id;
    // A dispatcher that died holding this host's lease: the one the beat starts asks it for a pass
    // and waits.
    detached::held_by_the_dead(&l.case);
    l.beat_as_the_binary(&listener.path, &l.inbox());
    assert_eq!(l.address(), uds(&listener.path));
    // The beat queued the message for the channel it registered, and delivered nothing itself.
    let store = l.case.store();
    let line = (store.raw()).row("callback_event", json!([l.handle, format!("message-{id}")]));
    assert!(
        line.exists().unwrap(),
        "the message is queued for the channel"
    );
    assert_ne!(
        line.field("delivered").unwrap(),
        json!(true),
        "delivered by the beat itself"
    );
    detached::until(
        || detached::lease(&l.case)["asked"] == json!(1),
        "the beat's dispatcher's ask",
    );
    detached::lapse(&l.case);
    let lines = listener.until(&format!("MSG #{id} "), Duration::from_secs(60));
    let delivered = (lines.iter())
        .filter(|line| line.contains("over to the channel"))
        .count();
    assert_eq!(delivered, 1, "{lines:?}");
    detached::until(
        || {
            let lease = detached::lease(&l.case);
            lease["until"] == json!(0) && lease["served"] == json!(1)
        },
        "the taken lease's release",
    );
}
