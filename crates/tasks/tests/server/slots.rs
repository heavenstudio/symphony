//! The slots scratch boards' servers take, on real servers, each test's slots a folder of its own
//! apart from the machine's: more boards than slots start in turn, a server killed with -9 leaves
//! no segment once its supervisor has ended but one its record no longer names as its own, a slot
//! is free again however its server ended, and a project's own board starts with every slot taken.

use crate::postgres::{Board, GONE, folder, programs, signalled};
use std::path::Path;
use std::sync::mpsc;
use symphony_board::server::Home;
use symphony_board::server::postgres::Slots;
use symphony_board::sys::{self, command};

/// `count` slots in a folder of the test `name`'s own, removed when they go.
struct OwnSlots(Slots);

impl OwnSlots {
    fn new(name: &str, count: usize) -> OwnSlots {
        let dir = folder(&format!("{name}-slots"));
        let _ = std::fs::remove_dir_all(&dir);
        OwnSlots(Slots::at(&dir, count).backstop(GONE))
    }

    /// A board of the test's own whose servers take these slots, its home as `home` makes it.
    fn board(&self, name: &str, home: impl FnOnce(Home) -> Home) -> Board {
        let mut board = Board::new(name);
        board.pg = home(board.home().telling(|_| {}).slots(self.0.clone())).postgresql(&programs());
        board
    }

    /// What slot `index`'s file says of its holder.
    fn named(&self, index: usize) -> String {
        std::fs::read_to_string(self.0.folder().join(format!("slot-{index}"))).unwrap()
    }
}

impl Drop for OwnSlots {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.folder());
    }
}

/// The id of the segment the server running on `board` made, as its record's seventh line says.
fn segment_of(board: &Board) -> String {
    recorded_segment(board).1
}

/// The key and the id of the segment the server running on `board` made, as its record's seventh
/// line says them: the key a 32-bit `key_t` the record writes sign-extended, as its low 32 bits.
fn recorded_segment(board: &Board) -> (u32, String) {
    let record = std::fs::read_to_string(board.pg.data().join("postmaster.pid")).unwrap();
    let line = record.lines().nth(6).unwrap_or_else(|| panic!("{record}"));
    match line.split_whitespace().collect::<Vec<_>>()[..] {
        [key, id] => {
            let key: u64 = key.parse().unwrap_or_else(|_| panic!("{line}"));
            (key as u32, id.to_string())
        }
        _ => panic!("{record}"),
    }
}

/// The key the segment `id` carries, as `ipcs -m` lists it on this machine: 0 once it was removed
/// while a process is still attached to it (which leaves its id and creator listed until the last
/// detaches), and none once it is gone.
fn key_of(id: &str) -> Option<u32> {
    let said = sys::output(command("ipcs").arg("-m")).unwrap();
    assert!(said.status.success(), "{said:?}");
    key_in(&String::from_utf8_lossy(&said.stdout), id)
}

/// The key of the segment `id` in `ipcs -m`'s listing, in macOS's layout (`m ID KEY …`) or
/// Linux's (`KEY SHMID …`).
fn key_in(listing: &str, id: &str) -> Option<u32> {
    listing.lines().find_map(|line| {
        let key = match line.split_whitespace().collect::<Vec<_>>()[..] {
            ["m", shmid, key, ..] if shmid == id => key,
            [key, shmid, ..] if shmid == id && key.starts_with("0x") => key,
            _ => return None,
        };
        u32::from_str_radix(key.trim_start_matches("0x"), 16).ok()
    })
}

#[test]
fn a_segments_key_is_read_from_ipcs_on_macos_and_on_linux_and_is_0_once_removed() {
    let macos = "IPC status from <running system> as of Tue Nov 24 17:00:00 -03 2026\n\
                 T     ID     KEY        MODE       OWNER    GROUP\n\
                 Shared Memory:\n\
                 m 287178755 0x8aa149cf --rw-------  stefano    staff\n\
                 m 588447752 0x00000000 --rw-------  stefano    staff\n";
    assert_eq!(key_in(macos, "287178755"), Some(0x8aa1_49cf));
    assert_eq!(key_in(macos, "588447752"), Some(0));
    assert_eq!(key_in(macos, "28717875"), None);
    let linux = "\n------ Shared Memory Segments --------\n\
                 key        shmid      owner      perms      bytes      nattch     status\n\
                 0x0052e2c1 32768      postgres   600        56         6\n\
                 0x00000000 32769      postgres   600        56         2          dest\n";
    assert_eq!(key_in(linux, "32768"), Some(0x0052_e2c1));
    assert_eq!(key_in(linux, "32769"), Some(0));
    assert_eq!(key_in(linux, "600"), None);
}

/// The creator of the segment `id`, as `ipcs -m -p` lists it on macOS or Linux: none once it is
/// gone.
fn creator(id: &str) -> Option<i64> {
    created(id).unwrap_or_else(|said| panic!("{said}"))
}

/// The creator of the segment `id`, or what `ipcs` said when it could not list the segments.
fn created(id: &str) -> Result<Option<i64>, String> {
    let said = sys::output(command("ipcs").args(["-m", "-p"])).map_err(|why| why.to_string())?;
    if !said.status.success() {
        return Err(format!("{said:?}"));
    }
    Ok(String::from_utf8_lossy(&said.stdout)
        .lines()
        .find_map(
            |line| match line.split_whitespace().collect::<Vec<_>>()[..] {
                ["m", shmid, _, _, _, _, cpid, _] | [shmid, _, cpid, _] if shmid == id => {
                    cpid.parse().ok()
                }
                _ => None,
            },
        ))
}

/// More scratch boards asked for than there are slots: the two first start, the third says it
/// waits and starts once the first's server has stopped, and none is refused for want of a
/// segment. Each slot's file names the server holding it.
#[test]
fn more_scratch_boards_than_slots_start_in_turn_and_none_is_refused() {
    let slots = OwnSlots::new("in-turn", 2);
    let first = slots.board("in-turn-1", |home| home);
    let second = slots.board("in-turn-2", |home| home);
    let third = slots.board("in-turn-3", |home| home);
    let one = first.pg.ensure().unwrap();
    let two = second.pg.ensure().unwrap();
    let (said, heard) = mpsc::channel();
    let pg = third
        .home()
        .slots(slots.0.clone())
        .telling(move |line| {
            let _ = said.send(line.to_string());
        })
        .postgresql(&programs());
    let starting = std::thread::spawn(move || pg.ensure());
    let told = heard.recv_timeout(GONE).expect("the third says it waits");
    assert!(
        told.starts_with(&format!(
            "· 2 of the machine's slots for scratch boards' servers are held ({}), and its shared \
             memory has room for 2 (at most 2; ",
            slots.0.folder().display()
        )) && told.ends_with(": this start waits for one to come free"),
        "{told}"
    );
    assert_eq!(first.pg.stop().unwrap().map(|s| s.pid), Some(one.pid));
    let three = starting.join().unwrap().unwrap();
    assert_eq!(third.psql(&three, "SELECT 1"), "1");
    assert_eq!(second.psql(&two, "SELECT 1"), "1");
    assert_eq!(
        (slots.named(0), slots.named(1)),
        (format!("{}\n", three.pid), format!("{}\n", two.pid)),
        "the third took the first's slot"
    );
}

/// A scratch server killed with -9 leaves its record and its segment, which nothing else would
/// take back: its supervisor removes the segment before it lets the slot go.
#[test]
fn a_scratch_server_killed_with_kill_9_leaves_no_segment_once_its_supervisor_has_ended() {
    let slots = OwnSlots::new("killed", 1);
    let board = slots.board("killed", |home| home);
    let serving = board.pg.ensure().unwrap();
    let id = segment_of(&board);
    assert_eq!(creator(&id), Some(serving.pid), "the server made it");
    signalled(serving.pid, "KILL");
    let free = slots.0.take(|_| {}).unwrap();
    assert!(
        board.pg.data().join("postmaster.pid").exists(),
        "killed, it left its record"
    );
    assert_eq!(creator(&id), None, "its supervisor removed segment {id}");
    drop(free);
    board.none_left();
}

/// A slot is free again once its server has ended, whatever ended it: a fast, a smart or an
/// immediate shutdown's signal, a -9, or a -9 to its supervisor's shells and the server at once,
/// whose backends end on the server's death. The last leaves its segment, as no supervisor is
/// left to remove it, and the test removes its own.
#[test]
fn a_slot_is_free_again_once_its_server_has_ended_by_any_signal() {
    let slots = OwnSlots::new("signals", 1);
    let board = slots.board("signals", |home| home);
    for signal in ["INT", "TERM", "QUIT", "KILL"] {
        let serving = board.pg.ensure().unwrap();
        signalled(serving.pid, signal);
        drop(
            slots
                .0
                .take(|_| {})
                .unwrap_or_else(|why| panic!("{signal}: {why}")),
        );
        board.none_left();
    }
    let serving = board.pg.ensure().unwrap();
    let _left = Left::by(&board, serving.pid);
    let all = all_killed_at_once(&board);
    assert!(
        all.len() > 1,
        "the server and its supervisor's shells: {all:?}"
    );
    drop(slots.0.take(|_| {}).unwrap());
    board.none_left();
}

/// Every process naming `board`'s data held stopped, then each killed with -9, their pids
/// returned: all stopped first, so none acts on another's end in between (the supervisor's shell
/// ending its watchdog, or starting its cleanup) and every kill finds the process it names.
fn all_killed_at_once(board: &Board) -> Vec<i64> {
    let all: Vec<i64> = board
        .processes()
        .iter()
        .map(|line| line.split_whitespace().next().unwrap().parse().unwrap())
        .collect();
    for &pid in &all {
        signalled(pid, "STOP");
    }
    for &pid in &all {
        signalled(pid, "KILL");
    }
    all
}

/// The segment a test's own server made, which a kill its supervisor cannot clean up after leaves:
/// removed as this goes, however the test ends, once its server has ended and while that server
/// is still its creator, so a failed test leaks none. Made after the board it is of, so it goes
/// first, and a server still running is left to the board to stop.
struct Left {
    id: String,
    server: i64,
}

impl Left {
    fn by(board: &Board, server: i64) -> Left {
        Left {
            id: segment_of(board),
            server,
        }
    }
}

impl Drop for Left {
    fn drop(&mut self) {
        let ended = sys::output(command("kill").args(["-0", &self.server.to_string()]))
            .is_ok_and(|said| !said.status.success());
        if !ended || created(&self.id) != Ok(Some(self.server)) {
            return;
        }
        let removed = sys::output(command("ipcrm").args(["-m", &self.id]));
        if !std::thread::panicking() {
            let removed = removed.unwrap();
            assert!(removed.status.success(), "{removed:?}");
        }
    }
}

/// The server on `board` held stopped, line `index` of its record written as `line`, then killed
/// with -9: held stopped first, so it cannot read its record in between and end on its own.
fn killed_with_its_record_rewritten(board: &Board, pid: i64, index: usize, line: &str) {
    signalled(pid, "STOP");
    let path = board.pg.data().join("postmaster.pid");
    let record = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<&str> = record.lines().collect();
    assert!(lines.len() > index, "{record}");
    lines[index] = line;
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
    signalled(pid, "KILL");
}

/// A supervisor whose server's record names another server by then, as one a later start wrote,
/// removes no segment: the killed server's is left, and the test removes its own.
#[test]
fn a_killed_servers_supervisor_leaves_the_segment_once_its_record_names_another_server() {
    let slots = OwnSlots::new("record-rewritten", 1);
    let board = slots.board("record-rewritten", |home| home);
    let serving = board.pg.ensure().unwrap();
    let left = Left::by(&board, serving.pid);
    killed_with_its_record_rewritten(&board, serving.pid, 0, &i32::MAX.to_string());
    drop(slots.0.take(|_| {}).unwrap());
    board.none_left();
    assert_eq!(
        creator(&left.id),
        Some(serving.pid),
        "segment {} is left",
        left.id
    );
}

/// A supervisor whose killed server's record names a segment another server made removes nothing:
/// that server's segment stays and it still serves, and the killed one's is left.
#[test]
fn a_killed_servers_supervisor_leaves_a_segment_another_server_made() {
    let first_slots = OwnSlots::new("other-creator-1", 1);
    let second_slots = OwnSlots::new("other-creator-2", 1);
    let first = first_slots.board("other-creator-1", |home| home);
    let second = second_slots.board("other-creator-2", |home| home);
    let killed = first.pg.ensure().unwrap();
    let serving = second.pg.ensure().unwrap();
    let own = Left::by(&first, killed.pid);
    let (key, other) = recorded_segment(&second);
    assert_eq!(creator(&other), Some(serving.pid));
    assert_eq!(key_of(&other), Some(key));
    killed_with_its_record_rewritten(&first, killed.pid, 6, &format!("0 {other}"));
    drop(first_slots.0.take(|_| {}).unwrap());
    first.none_left();
    assert_eq!(
        creator(&other),
        Some(serving.pid),
        "segment {other} is left"
    );
    // Removed while the second server is attached, the segment would still be listed by its id
    // and creator, but its key would read 0.
    assert_eq!(
        key_of(&other),
        Some(key),
        "segment {other} still carries its key, so it was not removed"
    );
    assert_eq!(second.psql(&serving, "SELECT 1"), "1");
    assert_eq!(
        creator(&own.id),
        Some(killed.pid),
        "segment {} is left",
        own.id
    );
}

/// A project's own board, whose server outlives its starter, starts while every slot is held and
/// takes none: the slot still names its holder.
#[test]
fn a_projects_own_board_starts_with_every_slot_taken_and_takes_none() {
    let slots = OwnSlots::new("own", 1);
    let held = slots.0.take(|_| {}).unwrap();
    let board = slots.board("own", |home| home.supervised(false));
    let serving = board.pg.ensure().unwrap();
    assert_eq!(board.psql(&serving, "SELECT 1"), "1");
    assert_eq!(
        slots.named(held.index()),
        format!("{}\n", std::process::id())
    );
    assert_eq!(board.pg.stop().unwrap().map(|s| s.pid), Some(serving.pid));
    board.none_left();
    assert!(!Path::new(&board.pg.data().join("postmaster.pid")).exists());
}

/// The server holds its slot itself, not only its supervisor: with the supervisor's shells killed,
/// the slot stays held while the server runs, and is free once the server has stopped.
#[test]
fn a_servers_slot_is_held_by_the_server_itself_once_its_supervisor_is_gone() {
    let slots = OwnSlots::new("server-holds", 1);
    let board = slots.board("server-holds", |home| home);
    let serving = board.pg.ensure().unwrap();
    for line in board.processes() {
        let pid: i64 = line.split_whitespace().next().unwrap().parse().unwrap();
        if pid != serving.pid {
            signalled(pid, "KILL");
        }
    }
    test_time::until_true("the supervisor's shells gone", GONE, || {
        board.processes().len() == 1
    })
    .unwrap_or_else(|stall| panic!("{stall}: {:?}", board.processes()));
    let refused = slots
        .0
        .clone()
        .backstop(std::time::Duration::from_millis(100))
        .take(|_| {})
        .err()
        .expect("the server holds it");
    assert!(
        refused
            .to_string()
            .contains(&format!("slot 0 by pid {}.", serving.pid)),
        "{refused}"
    );
    assert_eq!(board.pg.stop().unwrap().map(|s| s.pid), Some(serving.pid));
    drop(slots.0.take(|_| {}).unwrap());
    board.none_left();
}
