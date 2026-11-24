# apps/symphony-desktop/

The workspace, the cargo wrapper and the toolchain pin: [the Rust section of the root
guide](../../AGENTS.md#rust). What the app is for and the choices behind it:
[the desktop client analysis](../../docs/product/analysis/desktop-client.md) and
[its card](../../docs/product/analysis/desktop-client.md#desktop-client).

`symphony-desktop` is the app for the Mac: a thin shell whose windows show each server's own
pages, and which follows every server it knows with its windows closed. The library holds every
decision and is tested on the host; the binary (`src/bin/symphony-desktop.rs` and its `app.rs`,
macOS alone) only draws them, and no test opens a window. `publish = false`.

## Where each job lives

Each module's doc says what it holds and why.

- `src/address.rs`: a server's address and the rule that admits one (`Server::parse`).
- `src/navigation.rs`: where a server's window may go.
- `src/read.rs`: one read of a server's `GET /api/projects`, which only the machine's one server
  answers; a server of its own (`just symphony --port 0`) reads as an older one.
- `src/servers.rs`: the list of added servers, this Mac's server and the app's folders
  (`SYMPHONY_DESKTOP_DIR`, `SYMPHONY_MACHINE_DIR`).
- `src/follow.rs`: the follower: the count of what waits on the owner, the notices, the menu's
  lines and when each server is read next.
- `src/cli.rs` the command line, `src/main_menu.rs` the menu bar,
  `src/bin/symphony-desktop/app.rs` the windows, the dock badge and the menu-bar item.
- `src/notification.ts`: the one script the app puts in a server's pages.
- `Info.plist`, which `build.rs` puts in the binary: the bundle's identifier and plain HTTP to
  local addresses and `*.ts.net`.

## Rules a change keeps

- A server is admitted only as `http://` to loopback, the tailnet's addresses or a `*.ts.net`
  name, with no credentials.
- On a server's word the app never runs a command, opens a local file or an application, or
  writes a file but its own list: its own origin stays in the web view, any other `http` or
  `https` address, every new window and every download open in the system browser, and any other
  scheme is refused.
- The machine folder is only read. The list file is written whole through a file beside it and
  renamed, and one that does not read is kept as it is, never written over.
- The app injects nothing in a page but `notification.ts`, and no page calls the app.
- Hand-written code here is Rust, and TypeScript for what a page runs, never JavaScript
  ([the client's guide](../symphony-web/AGENTS.md#the-tools)). `notification.ts` goes in the page
  as it is written, so it has no syntax of its own, every type inferred; the web client's `tsc`
  checks it and `src/notification-stub.test.ts` there holds it plain.
- tao, wry, tray-icon and the objc2 crates are this package's alone and macOS-only:
  `tools/cargo.sh tree -p symphony-app`, `-p symphony-server` and `-p symphony-board` name none.

## Running and testing

`tools/rs-bin.sh symphony-desktop` opens the app and `tools/rs-bin.sh symphony-desktop add URL`
adds a server. An agent runs neither against a person's screen or folders: the app opens windows
and sets the dock badge.

`just test-rs -p symphony-desktop` runs the library's tests and `tests/real_server.rs`: the one
server on a private port over a temporary machine folder and project, followed as the app follows
it. It builds `symphony-server` through `tools/rs-bin.sh --build`, or takes it from the folder
`SYMPHONY_TEST_BINARIES` names, and makes its board on the machine's PostgreSQL as the board's
tests do. The stub's test is the browser target's `desktop_stub`
(`apps/symphony-web-tests/tests/browser/desktop_stub.rs`).

## Not in it yet

The app's own screens (the server list, adding a server, a server that does not answer), renaming
and removing a server, ⌘1 to ⌘9, notices through the notification centre (a line on stderr until
then), and an install recipe that builds the app at a release tag and signs it ad hoc. Known
limits: a cross-origin frame in a page opens the system browser, as wry names no frame to the
navigation rule; the menu-bar item's menu is made again on every read; a `*.ts.net` name is
trusted by its name alone.
