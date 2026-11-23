//! The task page over a real board: what it sends for a long brief, what it reads from the board to
//! send it, and how the owner reaches the rest of the brief.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "../../../tasks/tests/store/common.rs"]
#[expect(
    dead_code,
    reason = "the page's reads, the agents view's, the activity's and the workflow save's need only the store fixture's server, seed and held flow"
)]
pub(crate) mod store_fixture;

use store_fixture::{Fixture, NOW, Seed, task as row};
use symphony_board::rows::TaskRow;
use symphony_board::server::postgres::SOCKET_FILE;

use super::{BRIEF_LIMIT, brief, page};
use crate::taskview::read_failure;
use crate::test_tree::Tree;

/// A relay in front of the board's PostgreSQL, on a socket of its own in a folder of its own,
/// counting the bytes the server sends back: what a read cost, whatever the rows it read hold.
/// The folder is a short one under `/tmp`, as a socket's path has a length limit, and goes with the
/// relay.
struct Relay {
    folder: PathBuf,
    received: Arc<AtomicUsize>,
}

impl Relay {
    /// A relay to the PostgreSQL whose socket is in `server`, its address `postgresql:` and
    /// [`Relay::folder`].
    fn to(server: &str) -> Relay {
        static RELAYS: AtomicUsize = AtomicUsize::new(0);
        let server = PathBuf::from(server.strip_prefix("postgresql:").unwrap()).join(SOCKET_FILE);
        let folder = PathBuf::from(format!(
            "/tmp/symphony-relay-{}-{}",
            std::process::id(),
            RELAYS.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir(&folder).unwrap();
        let listener = UnixListener::bind(folder.join(SOCKET_FILE)).unwrap();
        let received = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&received);
        std::thread::spawn(move || {
            for caller in listener.incoming().map_while(Result::ok) {
                let upstream = UnixStream::connect(&server).unwrap();
                let (mut asked, mut forwarded) =
                    (caller.try_clone().unwrap(), upstream.try_clone().unwrap());
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut asked, &mut forwarded);
                    let _ = forwarded.shutdown(Shutdown::Write);
                });
                let counted = Arc::clone(&counted);
                std::thread::spawn(move || {
                    let (mut answer, mut caller) = (upstream, caller);
                    let mut buffer = vec![0; 1 << 16];
                    while let Ok(n @ 1..) = answer.read(&mut buffer) {
                        // Counted before it is passed on: once the caller has its reply, it is counted.
                        counted.fetch_add(n, Ordering::SeqCst);
                        if caller.write_all(&buffer[..n]).is_err() {
                            break;
                        }
                    }
                    let _ = caller.shutdown(Shutdown::Write);
                });
            }
        });
        Relay { folder, received }
    }

    /// The relay's address, as a board's is named.
    fn addr(&self) -> String {
        format!("postgresql:{}", self.folder.display())
    }

    fn received(&self) -> usize {
        self.received.load(Ordering::SeqCst)
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// A brief of at least `bytes` bytes, one numbered line after another.
fn numbered(bytes: usize) -> String {
    let mut text = String::new();
    let mut n = 0;
    while text.len() < bytes {
        writeln!(text, "mark-{n:06} of a long brief").unwrap();
        n += 1;
    }
    text
}

/// The numbers of the lines `blocks` show, in the order they show them.
fn marks(blocks: &[symphony_api::Block]) -> Vec<usize> {
    let json = serde_json::to_string(blocks).unwrap();
    json.match_indices("mark-")
        .filter_map(|(at, _)| json.get(at + 5..at + 11)?.parse().ok())
        .collect()
}

fn brief_row(id: i64, slug: &str, body: &str, parent: Option<&str>) -> TaskRow {
    TaskRow {
        id,
        body: body.to_owned(),
        parent: parent.map(str::to_owned),
        ..row(slug, "ready")
    }
}

#[test]
fn a_long_briefs_page_sends_its_start_reads_only_its_rows_and_reaches_the_rest_on_demand() {
    let (short, long) = (numbered(4 * BRIEF_LIMIT), numbered(16 * BRIEF_LIMIT));
    let f = Fixture::new(
        Seed::default()
            .task_row(brief_row(1, "fixture/short", &short, None))
            .task_row(brief_row(2, "fixture/long", &long, None))
            .task_row(brief_row(
                3,
                "fixture/short-step",
                "A step.",
                Some("fixture/short"),
            )),
    );
    let relay = Relay::to(&f.store.database().addr());
    let store = symphony_board::Store::open(symphony_board::rpc::Client::new(
        &relay.addr(),
        "sf",
        f.store.database().database(),
    ))
    .unwrap();
    let tree = Tree::new();
    let (short_task, long_task) = (
        store.get("fixture/short").unwrap(),
        store.get("fixture/long").unwrap(),
    );

    let before = relay.received();
    let short_page = page(&store, &short_task, &tree.root).unwrap();
    let read = relay.received() - before;
    assert_eq!(short_page.children_count, 1);
    assert!(
        read < long.len(),
        "the page of fixture/short read {read} bytes from the board, more than the {} of \
         fixture/long's brief alone: it read rows it does not show",
        long.len()
    );

    let long_page = page(&store, &long_task, &tree.root).unwrap();
    let sent = |page: &symphony_api::TaskPage| serde_json::to_vec(page).unwrap().len();
    assert!(
        (&long_page.blocks, &long_page.task.body) == (&short_page.blocks, &short_page.task.body),
        "a brief four times as long made the page send more of it: {} bytes against {}",
        sent(&long_page),
        sent(&short_page)
    );
    assert!(
        sent(&long_page) < long.len(),
        "the page of a {}-byte brief sent {} bytes",
        long.len(),
        sent(&long_page)
    );
    assert_eq!(
        long_page.brief_omitted + long_page.brief_next,
        long.len(),
        "the page says how much of the brief it left out"
    );

    let mut shown = marks(&long_page.blocks);
    assert_eq!(shown.first(), Some(&0), "the page shows the brief's start");
    let (mut from, mut omitted) = (long_page.brief_next, long_page.brief_omitted);
    while omitted > 0 {
        let chunk = brief(&long_task, from, &tree.root);
        assert!(chunk.next > from, "a chunk from byte {from} read nothing");
        assert_eq!(chunk.revision, long_page.brief_revision);
        shown.extend(marks(&chunk.blocks));
        (from, omitted) = (chunk.next, chunk.omitted);
    }
    let lines = long.lines().count();
    assert_eq!(
        shown,
        (0..lines).collect::<Vec<_>>(),
        "every line of the brief is reached once, in order"
    );
    assert_ne!(short_page.brief_revision, long_page.brief_revision);
}

/// A task page counts the subtasks the board lists under it (`/api/task-board?parent=`): a slice,
/// an errand and an open `/work`, not a finished `/work`, `/review` or `staff-review-N`.
#[test]
fn a_page_counts_slices_errands_and_open_work_but_no_finished_instruction_to_staff_its_task() {
    let mut id = 0;
    let mut child = |slug: &str, parent: &str, status: &str, instruction: bool| {
        id += 1;
        TaskRow {
            id,
            parent: Some(parent.to_owned()).filter(|p| !p.is_empty()),
            instruction,
            ..row(slug, status)
        }
    };
    let seed = Seed::default()
        .task_row(child("feature", "", "in_progress", false))
        .task_row(child("feature/slice", "feature", "done", false))
        .task_row(child("feature/visual", "feature", "done", true))
        .task_row(child("feature/work", "feature", "in_progress", true))
        .task_row(child("feature/review", "feature", "done", true))
        .task_row(child(
            "feature/staff-review-1",
            "feature",
            "cancelled",
            true,
        ))
        .task_row(child("staffed", "", "done", false))
        .task_row(child("staffed/work", "staffed", "done", true))
        .task_row(child("staffed/review", "staffed", "cancelled", true));
    let f = Fixture::new(seed);
    let tree = Tree::new();
    let count = |slug: &str| {
        page(&f.store, &f.store.get(slug).unwrap(), &tree.root)
            .unwrap()
            .children_count
    };
    assert_eq!(
        count("feature"),
        3,
        "the slice, the errand and the open /work"
    );
    assert_eq!(
        count("staffed"),
        0,
        "only finished instructions to staff it"
    );
}

#[test]
fn a_reference_link_keeps_its_target_whichever_chunk_holds_its_definition() {
    let body = format!(
        "Open [the board][board] first.\n\n[agents]: /agents\n\n{}Then see [the agents][agents].\n\n[board]: /tasks\n",
        "padding line\n\n".repeat(BRIEF_LIMIT / 4)
    );
    let f = Fixture::new(Seed::default().task_row(brief_row(1, "fixture/references", &body, None)));
    let record = f.store.get("fixture/references").unwrap();
    let tree = Tree::new();
    let page = page(&f.store, &record, &tree.root).unwrap();
    assert!(page.brief_omitted > 0, "the brief is cut");
    let shown = serde_json::to_string(&page.blocks).unwrap();
    assert!(
        shown.contains(r#""href":"/tasks""#) && !shown.contains("[board]"),
        "a link before the cut lost the target defined after it"
    );
    let mut rest = Vec::new();
    let mut from = page.brief_next;
    while from < record.body.len() {
        let chunk = brief(&record, from, &tree.root);
        assert!(chunk.next > from, "a chunk from byte {from} read nothing");
        rest.extend(chunk.blocks);
        from = chunk.next;
    }
    let rest = serde_json::to_string(&rest).unwrap();
    assert!(
        rest.contains(r#""href":"/agents""#) && !rest.contains("[agents]"),
        "a link after the cut lost the target defined before it"
    );
}

/// Every part of `body`'s brief, the page's and then each `brief` call's, shows the target the
/// whole brief gives `[the board][board]`: `/tasks`, never `/wrong`.
fn assert_the_parts_keep_the_whole_briefs_target(body: &str) {
    let f = Fixture::new(Seed::default().task_row(brief_row(1, "fixture/reference", body, None)));
    let record = f.store.get("fixture/reference").unwrap();
    let tree = Tree::new();
    let whole = serde_json::to_string(&super::reply_blocks(
        &crate::render::Render::new(&tree.root),
        &record.body,
    ))
    .unwrap();
    assert!(
        whole.contains(r#""href":"/tasks""#) && !whole.contains(r#""href":"/wrong""#),
        "the whole brief resolves the reference: {whole}"
    );
    let page = page(&f.store, &record, &tree.root).unwrap();
    assert!(page.brief_omitted > 0, "the brief is cut");
    let mut blocks = page.blocks;
    let mut from = page.brief_next;
    while from < record.body.len() {
        let chunk = brief(&record, from, &tree.root);
        assert_eq!(chunk.revision, page.brief_revision);
        assert!(chunk.next > from, "a chunk from byte {from} read nothing");
        from = chunk.next;
        blocks.extend(chunk.blocks);
    }
    let shown = serde_json::to_string(&blocks).unwrap();
    assert!(
        shown.contains(r#""href":"/tasks""#) && !shown.contains(r#""href":"/wrong""#),
        "the parts lost the target the whole brief gives the reference: {shown}"
    );
}

#[test]
fn a_reference_defined_over_two_lines_keeps_its_target_in_another_part() {
    assert_the_parts_keep_the_whole_briefs_target(&format!(
        "Open [the board][board] first.\n\n{}[board]:\n  /tasks\n",
        "padding line\n\n".repeat(BRIEF_LIMIT / 4)
    ));
}

#[test]
fn a_definition_inside_a_tilde_fence_is_code_in_every_part() {
    assert_the_parts_keep_the_whole_briefs_target(&format!(
        "Open [the board][board] first.\n\n{}~~~\n\n[board]: /wrong\n\n~~~\n\n[board]: /tasks\n",
        "padding line\n\n".repeat(BRIEF_LIMIT / 4)
    ));
}

#[test]
fn a_list_the_page_cuts_is_read_on_from_the_item_the_page_stopped_at() {
    // Longer than the page holds and shorter than one `brief` call does.
    let mut body = String::from("Intro.\n\n");
    let mut items = 0;
    while body.len() < 2 * BRIEF_LIMIT {
        writeln!(body, "- mark-{items:06} of a long brief").unwrap();
        items += 1;
    }
    body.push_str("\nAfter.\n");
    let f = Fixture::new(Seed::default().task_row(brief_row(1, "fixture/list", &body, None)));
    let record = f.store.get("fixture/list").unwrap();
    let tree = Tree::new();
    let page = page(&f.store, &record, &tree.root).unwrap();
    assert!(page.brief_omitted > 0, "the page cuts the list");
    let mut shown = marks(&page.blocks);
    let mut from = page.brief_next;
    while from < record.body.len() {
        let chunk = brief(&record, from, &tree.root);
        assert!(chunk.next > from, "a chunk from byte {from} read nothing");
        shown.extend(marks(&chunk.blocks));
        from = chunk.next;
    }
    assert_eq!(
        shown,
        (0..items).collect::<Vec<_>>(),
        "every item once, in order"
    );
}

#[test]
fn a_bold_paragraph_longer_than_the_page_is_cut_between_its_lines() {
    let body = format!("**{}**\n\nAfter.\n", numbered(16 * BRIEF_LIMIT).trim_end());
    assert!(body.lines().all(|line| line.len() < BRIEF_LIMIT));
    let f = Fixture::new(Seed::default().task_row(brief_row(1, "fixture/bold", &body, None)));
    let record = f.store.get("fixture/bold").unwrap();
    let tree = Tree::new();
    let page = page(&f.store, &record, &tree.root).unwrap();
    let sent = serde_json::to_vec(&page).unwrap().len();
    assert!(page.brief_omitted > 0, "the brief is cut");
    assert!(
        sent < 4 * BRIEF_LIMIT,
        "a {}-byte brief sent {sent} bytes on its page",
        body.len()
    );
    let mut parts = vec![page.blocks];
    let mut from = page.brief_next;
    while from < record.body.len() {
        let chunk = brief(&record, from, &tree.root);
        assert!(chunk.next > from, "a chunk from byte {from} read nothing");
        assert!(
            chunk.next - from <= 100_000,
            "a chunk of {} bytes",
            chunk.next - from
        );
        parts.push(chunk.blocks);
        from = chunk.next;
    }
    for blocks in &parts[..parts.len() - 1] {
        let json = serde_json::to_string(blocks).unwrap();
        assert!(
            json.starts_with(r#"[{"type":"paragraph","spans":[{"type":"strong""#),
            "{json}"
        );
    }
    let shown: Vec<usize> = parts.iter().flat_map(|blocks| marks(blocks)).collect();
    let lines = numbered(16 * BRIEF_LIMIT).lines().count();
    assert_eq!(
        shown,
        (0..lines).collect::<Vec<_>>(),
        "every line once, in order"
    );
}

/// A board nothing serves cannot be reached, 503, and the page says where it was looked for.
#[test]
fn a_page_whose_board_nothing_serves_says_the_board_cannot_be_reached() {
    const UNREACHABLE: &str = "the task board cannot be reached: ";
    let tree = Tree::new();
    let nowhere = tree.path("no-board");
    std::fs::create_dir_all(&nowhere).unwrap();
    let refused = symphony_board::Store::unchecked_on(
        symphony_board::pg::Client::new(&nowhere, "gone").into(),
        Box::new(|| NOW),
        Box::new(|_| true),
        "this-host",
        &symphony_board::Opening::read(),
    );
    let failed = refused.get("fixture/parent").unwrap_err();
    let (status, said) = read_failure(&failed.to_string(), UNREACHABLE);
    assert_eq!(status, 503, "{said}");
    assert!(
        said.starts_with(&format!(
            "{UNREACHABLE}the board's database: the board's PostgreSQL is not answering on {}: ",
            nowhere.join(SOCKET_FILE).display()
        )),
        "{said}"
    );
}
