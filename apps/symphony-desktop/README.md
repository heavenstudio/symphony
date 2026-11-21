# Symphony for the Mac

`symphony-desktop` is a small Mac app that keeps the Symphony servers you follow one click away:
a window for each server, showing that server's own pages, and a count of what waits on you in
the dock and the menu bar while its windows are closed.

## What it does

- **A window per server.** The window shows the server's pages as a browser would. A link that
  leaves the server opens in your browser.
- **What waits on you, counted.** For every project of every server it follows: the decision
  cards waiting on you plus the agents' questions to you on its tasks. The total is the dock's
  badge and the menu-bar item, whose menu lists each server and project and opens it.
- **Several servers.** This Mac's server is followed without being added. Another machine's is
  added by its address, over your tailnet. A server that does not answer is greyed in the menu
  with the time it was last reached, and is tried again by itself.

It follows only a machine's one server (the one `just symphony` starts), which lists its
projects. A private server (`just symphony --port 0`) has no such list and shows as a server with
none.

## Getting it

It is not published or packaged yet: build it from a clone of this repository, on a Mac, with the
Rust toolchain `rust-toolchain.toml` names.

```sh
cargo build --locked --release -p symphony-desktop
.target/release/symphony-desktop                          # open the app
.target/release/symphony-desktop add http://NAME.ts.net:8765/   # follow another machine's server
```

A server's address is `http://` to this Mac, to a tailnet address or to a `*.ts.net` name, with
no credentials in it. A server is saved once it answers as a Symphony, and a server already
listed, reached another way, is refused rather than listed twice.

## Where it keeps things

The list of added servers is `servers.json` in `~/Library/Application Support/Symphony Desktop`
(`SYMPHONY_DESKTOP_DIR` names another folder). It reads this Mac's server from the machine folder,
`~/Library/Application Support/Symphony`, and writes nothing there.

## Not in it yet

Notifications through the notification centre (a notice is a line on the app's standard error for
now), screens of its own for the server list and for adding, renaming or removing a server, and an
installer that builds and signs the app at a release.

How it is built is [its guide](AGENTS.md); why it is a thin shell is
[the analysis](../../docs/analysis/desktop-client.md).
