# apps/symphony-desktop/

The workspace, the cargo wrapper and the toolchain pin: [the Rust section of the root
guide](../../AGENTS.md#rust). What the app is for and the choices behind it:
[the desktop client analysis](../../docs/analysis/desktop-client.md) and its card
(`desktop-client`, [DECISIONS](../../docs/DECISIONS.md)).

`symphony-desktop` is the app for the Mac: a thin shell whose windows show each server's own
pages, and which follows every server it knows with its windows closed. The library holds every
decision and is tested on the host; the binary (`src/main.rs`, `src/app.rs`, macOS alone) only
draws them, and no test opens a window. `publish = false`.

## Where each job lives

- `src/address.rs`: a server's address and the rule for one (`Server::parse`): `http://` alone,
  to loopback, the tailnet's addresses (100.64.0.0/10, fd7a:115c:a1e0::/48) or a `*.ts.net`
  name, no credentials; kept as its origin. Its default name: "This Mac" for loopback, else the
  first label of its name.
- `src/navigation.rs`: where a server's window may go. Its own origin (and a frame's own
  `about:blank`, `about:srcdoc` or `blob:` of it) stays in the web view; any other `http` or
  `https` address, every new window and every download open in the system browser; any other
  scheme is refused. The app never runs a command, opens a local file or an application, or
  writes a file but its own list, on a server's word.
- `src/read.rs`: one read of a server's `GET /api/projects` (`symphony_api::ProjectList`), over
  a plain socket: listed, older (a 404: a server with no list) or lost, with why.
- `src/servers.rs`: the list of added servers (`servers.json` in the app's folder, written whole
  through a file beside it and renamed; a file that does not read is kept as it is and never
  written over), this Mac's server (the port of the machine folder's `server.json`, only read),
  the folders (`SYMPHONY_DESKTOP_DIR`, else `~/Library/Application Support/Symphony Desktop`;
  `SYMPHONY_MACHINE_DIR`, else `~/Library/Application Support/Symphony`), and adding one: saved
  only once its list answers as a Symphony's, and refused when its installation id is one listed
  already or this Mac's, so a server reached two ways is listed once.
- `src/follow.rs`: the follower. What waits on the owner is a project's cards plus the agents'
  questions to `owner` on its tasks; each installation is counted once. A notice comes when a
  project's cards or questions rise, and when a newer reviewer's verdict comes (which waits on no
  one); the first list read tells nothing. A notice names the server, and the project, only when
  there is more than one, and opens a page of that server: the next card, the project, the task.
  It reads each server every 2 s, a lost one after 1 s doubling to 30 s, one with no list every
  30 s, and makes the menu's lines (greyed, with the time last reached, while a server does not
  answer). It reads the list rather than the feed: the list holds every project's counts and asks
  in one small answer the server keeps in memory, where the feed is one stream per project.
- `src/cli.rs`: the command line, `symphony-desktop` (the app) and `symphony-desktop add URL`.
- `src/notification.js`: the one script the app puts in a server's pages, before their own:
  notifications read as denied, so a page never notifies beside the app. Nothing else is
  injected, and no page calls the app.
- `src/app.rs`: one tao window and wry web view per server, hidden on close and kept alive; a
  reader thread per server; the dock badge and the menu-bar item (tray-icon's) with the total, its
  menu each server and project. Notices are a line on stderr until the app posts them.
- `Info.plist`, which `build.rs` puts in the binary's `__TEXT,__info_plist`: the bundle's
  identifier and plain HTTP to local addresses and `*.ts.net` (App Transport Security).

Only the machine's one server (`serve --home`) answers `/api/projects`: a server of its own (`just
symphony --port 0`) has none, and reads as an older one.

## Running and testing

`tools/rs-bin.sh symphony-desktop` opens the app on the servers it knows;
`tools/rs-bin.sh symphony-desktop add URL` adds one. An agent runs neither against a person's
screen or folders: the app opens windows and sets the dock badge.

`just test-rs -p symphony-desktop` runs the library's tests and `tests/real_server.rs`: the one
server on a private port over a temporary machine folder and project, its board a SurrealDB server
of the test's own in that folder (`symphony_board::server::Home`, ending with the test), followed
as the app follows it. It builds `symphony-server` through `tools/rs-bin.sh --build`, or takes it
from the folder `SYMPHONY_TEST_BINARIES` names, and runs the pinned SurrealDB as the board's tests
do (`SYMPHONY_SURREAL`, else `tools/surreal.sh`). The stub's test is the browser target's `desktop_stub`
(`apps/symphony-ui/tests/browser/desktop_stub.rs`): home behind another tab, its count rising,
notifies under a browser's granted permission and neither throws nor notifies under the stub.

tao, wry, tray-icon and the objc2 crates are this package's alone, at the versions Dioxus desktop
0.7.10 builds on, and macOS-only: `tools/cargo.sh tree -p symphony-app`, `-p symphony-server` and
`-p symphony-board` name none of them.

## Not in it yet

The app's own screens (the server list, adding a server, a server that does not answer), renaming
and removing a server, ⌘1 to ⌘9, notices through the notification centre, and an install recipe
that builds the app at a release tag and signs it ad hoc.
