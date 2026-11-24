//! The slots on files of a test's own folder, locked as a server's start locks them, with no
//! server: taken lowest first, waited for on the locks, refused at the backstop naming their
//! holders, held by a child they are handed to; and the room the machine's segments leave, read
//! from what `ipcs` says.

use super::*;
use std::sync::mpsc;

/// A wait that only a stall reaches.
const STALL: Duration = Duration::from_secs(120);

/// A folder of the test `name`'s own under the temporary folder, removed when it goes.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let path = std::env::temp_dir().join(format!("slots-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Folder(path)
    }

    fn slots(&self, count: usize) -> Slots {
        Slots::at(&self.0, count).backstop(STALL)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn quiet(_: &str) {}

#[test]
fn slots_are_taken_lowest_first_each_naming_its_holder_and_a_let_go_one_is_taken_again() {
    let folder = Folder::new("lowest");
    let slots = folder.slots(3);
    let first = slots.take(quiet).unwrap();
    let second = slots.take(quiet).unwrap();
    assert_eq!((first.index(), second.index()), (0, 1));
    let mode = fs::metadata(&folder.0).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700, "the folder is this user's alone");
    assert_eq!(
        fs::read_to_string(folder.0.join("slot-1")).unwrap(),
        format!("{}\n", std::process::id()),
        "a slot's file names its holder"
    );
    drop(first);
    assert_eq!(slots.take(quiet).unwrap().index(), 0, "let go, taken again");
    assert!(
        !folder.0.join("slot-2").exists(),
        "a slot never taken is never made"
    );
}

#[test]
fn a_start_with_every_slot_held_says_so_once_and_takes_the_first_let_go() {
    let folder = Folder::new("waits");
    let slots = folder.slots(2);
    let held = [slots.take(quiet).unwrap(), slots.take(quiet).unwrap()];
    let (said, heard) = mpsc::channel();
    let waiting = slots.clone();
    let third = std::thread::spawn(move || {
        waiting.take(move |line| {
            let _ = said.send(line.to_string());
        })
    });
    let told = heard.recv_timeout(STALL).expect("the start says it waits");
    assert!(
        told.starts_with(&format!(
            "· 2 of the machine's slots for scratch boards' servers are held ({}), and its shared \
             memory has room for ",
            folder.0.display()
        )) && told.ends_with(": this start waits for one to come free"),
        "{told}"
    );
    let [first, second] = held;
    drop(second);
    let taken = third.join().unwrap().unwrap();
    assert_eq!(taken.index(), 1, "the one let go");
    assert!(heard.try_recv().is_err(), "said once");
    drop(first);
}

#[test]
fn a_wait_past_its_backstop_is_refused_naming_each_slot_and_the_pid_that_holds_it() {
    let folder = Folder::new("backstop");
    let held = folder.slots(1).take(quiet).unwrap();
    let refused = folder
        .slots(1)
        .backstop(Duration::from_millis(100))
        .take(quiet)
        .err()
        .expect("no slot came free");
    let ServerError::NotUp(said) = refused else {
        panic!("{refused:?}");
    };
    assert!(
        said.starts_with(
            "no slot for a scratch board's server came free in the 0.1 s this start waited for \
             one (load average "
        ),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "they are held by slot 0 by pid {}. Nothing was started: run it again once one of \
             them has ended",
            std::process::id()
        )),
        "{said}"
    );
    drop(held);
    assert_eq!(folder.slots(1).take(quiet).unwrap().index(), 0);
}

#[test]
fn a_holder_that_ended_or_names_none_is_said_so() {
    let folder = Folder::new("holders");
    let slots = folder.slots(2);
    let room = Room::of(2, None, None, &[]);
    let said = slots.stalled(&[(0, Some(i64::from(i32::MAX))), (1, None)], &room);
    assert!(
        said.contains(&format!(
            "they are held by slot 0 by pid {}, which has ended, slot 1 by a holder that names \
             none.",
            i32::MAX
        )),
        "{said}"
    );
    assert!(
        said.contains("room for 2 (its segments in use unread)"),
        "{said}"
    );
}

#[test]
fn a_handed_slot_is_held_by_the_child_alone_until_it_ends() {
    let folder = Folder::new("handed");
    let slot = folder.slots(1).take(quiet).unwrap();
    let passed = slot.descriptor().unwrap();
    assert!(passed.as_raw_fd() >= HANDED_FROM, "{passed:?}");
    let (input, writer) = crate::sys::pipe().unwrap();
    let mut child = command("/bin/sh");
    child
        .args(["-c", "read -r line || exit 0"])
        .stdin(input)
        .pass_fd(passed.as_raw_fd());
    let mut child = child.spawn().unwrap();
    let handed = slot.handed();
    drop(passed);
    // A pid no process can have, so the line it is named in never depends on the machine.
    let server = i64::from(i32::MAX);
    handed.serving(server);
    drop(handed);
    let refused = folder
        .slots(1)
        .backstop(Duration::from_millis(100))
        .take(quiet)
        .err()
        .expect("the child holds it");
    assert!(
        refused
            .to_string()
            .contains(&format!("slot 0 by pid {server}, which has ended")),
        "the file names the server it was handed to: {refused}"
    );
    drop(writer);
    assert!(child.wait().unwrap().success());
    assert_eq!(
        folder.slots(1).take(quiet).unwrap().index(),
        0,
        "let go with the child"
    );
}

#[test]
fn a_folder_another_could_reach_is_refused_and_a_folder_with_no_parent_cannot_be_made() {
    let folder = Folder::new("open");
    fs::create_dir(&folder.0).unwrap();
    fs::set_permissions(&folder.0, fs::Permissions::from_mode(0o755)).unwrap();
    let refused = folder.slots(1).take(quiet).err().expect("refused");
    assert!(
        refused
            .to_string()
            .ends_with("is there and is not a folder this user alone reads: a scratch board's server takes its slot nowhere else"),
        "{refused}"
    );
    let orphan = Slots::at(&folder.0.join("gone/slots"), 1);
    assert!(
        orphan
            .take(quiet)
            .err()
            .expect("no parent")
            .to_string()
            .starts_with("could not make "),
    );
}

#[test]
fn only_the_first_count_slots_are_counted_and_zero_is_one() {
    let folder = Folder::new("count");
    assert_eq!(Slots::at(&folder.0, 0).count(), 1);
    let slots = folder.slots(1);
    fs::create_dir(&folder.0).unwrap();
    fs::set_permissions(&folder.0, fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["slot-7", "slot-x", "taking", "other"] {
        fs::write(folder.0.join(name), "").unwrap();
    }
    assert_eq!(slots.there().unwrap(), BTreeSet::new());
    let _taken = slots.take(quiet).unwrap();
    assert_eq!(slots.there().unwrap(), BTreeSet::from([0]));
    assert_eq!(slots.folder(), folder.0);
}

#[test]
fn the_machines_slots_are_half_its_limit_in_a_folder_of_this_users_under_tmp() {
    let machine = Slots::machine();
    // SAFETY: geteuid reads nothing of ours.
    let user = unsafe { libc::geteuid() };
    assert_eq!(
        machine.folder(),
        Path::new(&format!("/tmp/symphony-pg-slots-{user}"))
    );
    assert_eq!(machine.count(), share(limit()));
    assert_eq!(machine.backstop, BACKSTOP);
    assert!(limit().is_some(), "this machine says its limit");
    assert_eq!(share(Some(32)), 16);
    assert_eq!(share(Some(4096)), 2048);
    assert_eq!(share(Some(1)), 1, "one at least");
    assert_eq!(share(None), UNREAD);
}

#[test]
fn the_room_is_the_count_or_what_the_segments_others_hold_leave() {
    let room = Room::of(16, Some(32), Some(&[1, 2, 3, 10, 11][..]), &[10, 11, 12]);
    assert_eq!(room.cap, 16, "3 held by others leave 29");
    assert_eq!(
        room.said(16),
        "16 (at most 16; 3 of its 32 segments are held by servers that hold no slot)"
    );
    let leaked: Vec<i64> = (100..120).collect();
    let room = Room::of(16, Some(32), Some(&leaked[..]), &[]);
    assert_eq!(room.cap, 12, "20 leaked leave 12");
    let full: Vec<i64> = (100..140).collect();
    let room = Room::of(16, Some(32), Some(&full[..]), &[]);
    assert_eq!(room.cap, 0, "none, never fewer");
    assert_eq!(
        room.none(Path::new("/tmp/slots")),
        "the machine's shared memory has no room for a scratch board's server: 40 of its 32 \
         segments are held by servers that hold no slot (/tmp/slots), and none holds one to wait \
         for (`ipcs -m` lists them). Nothing was started"
    );
    for (limit, creators) in [(None, Some(&full[..])), (Some(32), None), (None, None)] {
        assert_eq!(
            Room::of(16, limit, creators, &[]),
            Room {
                cap: 16,
                others: None
            }
        );
    }
}

#[test]
fn the_creators_are_read_from_ipcs_on_macos_and_on_linux() {
    let macos = "IPC status from <running system> as of Tue Nov 24 15:52:41 -03 2026\n\
                 T     ID     KEY        MODE       OWNER    GROUP  CPID  LPID\n\
                 Shared Memory:\n\
                 m 227737600 0x853d1ded --rw-------  stefano    staff  56107  56107\n\
                 m 562692102 0x9381229f --rw-------  stefano    staff  37354      0\n\n";
    assert_eq!(creators(macos), [56107, 37354]);
    let linux = "\n------ Shared Memory Creator/Last-op PIDs --------\n\
                 shmid      owner      cpid       lpid      \n\
                 32768      postgres   1234       5678      \n\
                 65537      1001       2345       0         \n";
    assert_eq!(creators(linux), [1234, 2345]);
    assert_eq!(
        creators("m x 0x1 --rw------- o g 12 13\nm 1 0x1 --rw------- o g x 13\n"),
        Vec::<i64>::new()
    );
}

#[test]
fn this_machines_segments_are_read() {
    assert!(segments().is_some(), "ipcs runs here");
}

#[test]
fn a_start_whose_folder_lock_stays_held_past_the_backstop_is_refused_saying_so() {
    let folder = Folder::new("taking");
    let slots = folder.slots(1);
    drop(slots.take(quiet).unwrap());
    let other = File::open(folder.0.join(TAKING)).unwrap();
    other.lock().unwrap();
    let refused = slots
        .clone()
        .backstop(Duration::from_millis(100))
        .take(quiet)
        .err()
        .expect("the folder's lock is held");
    assert!(
        refused.to_string().starts_with(&format!(
            "{} was held for the 0.1 s this start waited for it, by another start counting the \
             slots",
            folder.0.join(TAKING).display()
        )),
        "{refused}"
    );
    other.unlock().unwrap();
    assert_eq!(slots.take(quiet).unwrap().index(), 0);
}
