# A desktop Symphony app

What a desktop app for the Symphony would give, how it could be built, and how it would connect to
more than one server, as proposed on the desktop-client card in [DECISIONS](../DECISIONS.md). Each
fact about the code names the file and function it was read in. "Read 2026-11-11" marks the state
of this machine that day.

The owner, 2026-11-11: "I also want to push for a desktop client app. Would likely help improve the
experience even more. Specially if it can connect to different servers".

## What the app gives

Ranked by what the owner gains, against a browser tab on the same Mac:

| | What | Today in the browser | First release |
|---|---|---|---|
| 1 | **Notifications with no Symphony tab open**, for every server and project: more cards wait on you, an agent asks you something | A notice fires only from a project's home page, while its tab is hidden and once notifications are allowed (`apps/symphony-ui/src/live.rs`, the `page-review` branch; `state.rs`, `apply`) | yes |
| 2 | **A count of what waits on you** in the dock and the menu bar, summed over every server and project | Each project's home shows its own count; the project list at a server's `/` shows each project's | yes |
| 3 | **Stays connected with its window closed**: the menu-bar item keeps following each project's feed | Nothing follows once the tab is closed | yes: 1 and 2 rest on it |
| 4 | **No address or port to remember**: this Mac's server is found by itself, others are saved by name | A bookmark per server | yes |
| 5 | **A global shortcut** that brings the app forward at the next card | None | second release |
| 6 | **Focus a local agent's pane** | Planned for the browser too: the server focuses the pane on its own machine when the request comes from that machine (`tools/go-to-agent-session`) | comes with that task, in both |
| 7 | **Open a file in the editor** | None | later, done by the server on its own machine, the same way as 6 |

Items 6 and 7 act on the server's machine, so the server does them, as `tools/go-to-agent-session`
already plans for panes. The app gains them without running anything itself (see
[Trust and access](#trust-and-access)).

**Task questions are not counted anywhere yet.** The wire's `Counts` has the cards waiting on the
owner, the documents they sit in, spot checks and today's answers, and the task board's in progress,
in review and live agents (`crates/symphony-api/src/lib.rs`, `Counts`, `TaskCounts`). A question an
agent asks the owner on a task (`just tasks say SLUG "…" --to owner`) is in none of them. A card
question is: it sits in *Needs your decision*. Item 1 covers cards from the first release; task
questions need one more count on the wire, which the first slice adds if the owner wants them in the
badge (see [Open questions](#open-questions)).

## How it is built

The client was chosen for this: one Dioxus app, "the browser now and the desktop window later, from
the same code" (`apps/symphony-ui/AGENTS.md`), with every platform hook behind
`platform::Platform` (`apps/symphony-ui/src/platform.rs`: `notify`, `open_external`, `load`,
`save`). Today each server compiles its client into itself and serves it
(`apps/symphony-server/src/client.rs`, `select`), and a page reloads itself when the server names
another client build (`Board::build`; `live.rs`, `reload_if_redeployed`). Client and server are
always the same release.

### A: a thin shell

A native Mac app whose windows show the pages each server already serves, one web view per server.

- **What is new.** One crate, `apps/symphony-desktop`, built on tao and wry (the window and web view
  crates beneath both Dioxus desktop and Tauri), the menu-bar and menu crates from the same family,
  and the Mac's notification centre. It holds the server list, a follower per project that reads
  that project's board and feed, the badge, the notifications and the window.
- **The pages are the server's.** A web view at `http://<server>/<id>/` loads the shell, the module
  scripts and the API from that server, exactly as the browser does: same origin, so the save
  checks pass unchanged (`apps/symphony-server/src/write.rs`, `allowed`), and the browser's storage
  stays per server and per project as it is today (`apps/symphony-ui/src/project.rs`, `stored`).
  When the server updates, the page reloads itself as in the browser. No app update follows a
  Symphony release unless the shell itself changed.
- **Native parts stay in the shell.** The follower, not the page, posts notifications and sets the
  badge, so they work with every window closed. A link that leaves the server's origin, or a
  `window.open` (`platform::Browser::open_external`), opens in the system browser. Nothing in the
  client changes for the first release.
- **The page never meets a missing API.** A web view may lack the web Notification API (to try in
  the first slice). The shell defines one that always reports notifications as denied, so the page's
  own notice (`platform::Browser::notify`) stays quiet and never notifies twice.
- **The app's own screens** (the server list, adding a server, "this server does not answer") are a
  few small pages. They can be a Dioxus desktop app around the server's web view, which keeps the
  owner's choice of Dioxus on the desktop. The first slice tries whether Dioxus desktop 0.7.10 can
  host a second web view in its window; plain wry draws them if not.
- **What the server adds.** One read, `GET /api/projects`: each project's id, name, cards waiting,
  first waiting card (title and link), and whether it is refused or its board unwell, with the
  server's installation id and release. It answers the tailnet's names like every route but one.
  `/api/front` answers loopback's name alone (`apps/symphony-server/src/projects.rs`, `front`), and
  the project list at `/` is an HTML page (`home`), so another machine has no list to read today.
  The live counts come from each project's existing feed (`/<id>/api/feed`), with the feed's own
  loss rules (`crates/symphony-api/src/lib.rs`, the module docs).
- **Wire.** The follower reads only `/api/projects`, `Board.counts.waiting`, `Board.next`'s first
  card and the feed's `counts` and `queue` changes. A fixture test in `crates/symphony-api` pins
  those few fields, added to and never renamed. Everything else stays free to change with each
  release, as now.
- **Budget.** Nothing is added to any page kind: the 250 KB first visit is untouched.
- **Offline.** A server that does not answer shows the app's own page saying so and when it was last
  reached, with its last count in the menu, greyed. Nothing of it can be read offline, as in the
  browser today.

### B: the client compiled in

The Dioxus client built for the desktop (`dx` builds desktop as well as web) with a `desktop`
feature, the app holding one compiled client that talks to every server's `/api`.

- **A native half for most of the client.** Read 2026-11-11: 31 of the client's files, 8,700 of its
  11,800 non-test lines, call the browser directly (`web-sys`, `js-sys`, `wasm-bindgen`): the feed
  (`EventSource`), every request, the visibility latch (`IntersectionObserver`), attachments (paste,
  drop, the file picker), the media runtime, navigation and the page-kind loader (`assets/shell.js`,
  295 lines). On the desktop, Dioxus runs the app natively and draws into a web view, so each of
  these needs a native or script-bridge version.
- **What the server inlines must be fetched.** The project's element and its theme are inlined in
  each page the server serves (`symphony_api::PROJECT_ELEMENT`, `/<id>/theme.css`); the app would
  read them over the API.
- **Cross-origin.** A plugin viewer's module script is loaded from the server that names it
  (`apps/symphony-ui/AGENTS.md`, Media). From the app's own origin that needs CORS, which the server
  does not send today. The save check refuses a write whose Origin is not the server's own
  (`write.rs`, `allowed`), so every write goes through a native HTTP client, which sends none.
- **Storage.** Every server's projects would share one origin, the app's, so every stored key also
  takes the server first.
- **Wire.** The app at one release talks to servers at others, so the whole wire becomes a contract
  between releases. Today client and server move together and the wire moves often: 20 commits
  changed `crates/symphony-api` from 2026-11-04 to 2026-11-11, and several of them removed or
  renamed fields (the one-server projects, the panes probe, the machine-work evidence). The app
  would need a rule such as: it speaks to a server at its own pin (`crates/symphony/src/machine.rs`,
  `PIN`, raised on every change that is not additive) and shows any other read-only or not at all.
  In practice a Symphony release would need an app release.
- **Budget.** Untouched only while every desktop path sits behind the `desktop` feature; a shared
  transport layer would land in every page kind and must be measured against the 250 KB.
- **Offline.** It could keep each project's last board and show it read-only.
- **Gain over A.** It opens at once from local code, it reads the last board offline, and it never
  runs code a server sends. It draws in a web view all the same, so it looks and performs as A does.

### C: A, then B

A first, as above, and B later. B pays off only if reading offline or not running a server's code
comes to matter, for instance with a hosted server run by someone else. Cost: A now, and B's whole
cost later on a wire that would by then have to stand still.

### Compared

| | A: thin shell | B: client compiled in | C: A, then B |
|---|---|---|---|
| New code | one crate; one server read | a native half for 31 client files; CORS; compatibility rule | A now, B later |
| Client change | none | most of it | none, then most of it |
| A Symphony release reaches the app | at once, from each server | with an app release | at once, then with an app release |
| Servers at other releases | each shows its own pages | needs the compatibility rule | as A, then as B |
| 250 KB budget | untouched | to measure | untouched, then to measure |
| Wire | a few fields pinned | the whole wire becomes a contract | a few fields, then all |
| Offline | says so, last count | last board, read-only | as A, then as B |
| Runs code a server sends | yes, as the browser does | no | yes, then no |

### Why A

It gives every item of the first release with the least new code, and keeps the client and the
wire free to change every release, which they do several times a day. What B adds, offline reading
and not running a server's code, matters for a server someone else runs, which does not exist yet.

## Several servers

- **A server** is a base URL: this Mac's (`http://127.0.0.1:8765`), another machine's over the
  tailnet (`http://studio.tailnet-name.ts.net:8765`), later a hosted one (`https://…`). It serves
  one or more projects under `/<id>/` (read 2026-11-11: this Mac's serves `symphony` and
  `star-foundry`).
- **Its identity** is the server's installation id (`crates/symphony/src/machine/identity.rs`,
  `installation_id`), which `/api/projects` carries, so the same server reached by loopback and by
  its tailnet name is listed once.
- **Adding.** This Mac's server is found by itself, from the machine folder's `server.json` (its
  port). Others are added by pasting a URL: the app reads `/api/projects` and saves the server only
  if it answers as a Symphony.
- **Naming.** "This Mac" for loopback; otherwise the first label of its tailnet name (`studio`).
  The owner can rename any of them.
- **Switching.** A sidebar lists each server and its projects, each with its waiting count;
  ⌘1 to ⌘9 pick a project. One web view per server stays alive while hidden, so drafts, scroll and
  open panels survive a switch.
- **Removing.** Takes it off the list and deletes what its pages stored in the app. Nothing on the
  server changes.
- **What shows across servers.** One combined count in the dock badge and the menu-bar item; the
  menu lists each server and project with its own. Pages show one project at a time. A notification
  names the server and the project when there is more than one, and opens the card's page in that
  server's window.
- **Where the list is kept.** The app's own settings file in its Application Support folder,
  written whole like the registry (`crates/symphony/src/machine/registry.rs`, `Registry::write`),
  and never written over when it does not parse. The server's machine folder is only read, for this
  Mac's port. A hosted server's credential, later, goes in the Keychain.
- **Unreachable.** The follower uses the feed's loss rule: lost on an error or after two and a half
  pings of silence, then retried after 1 s, doubling to 30 s. The badge sums what reachable servers
  say now; an unreachable one shows its last count greyed, with the time it was last reached.
- **Another release.** Each server shows its own pages, so a page never meets a server of another
  release. A server too old to have `/api/projects` still opens, with no count and a line saying
  it needs a newer Symphony. A server newer than the app says so, and the app offers its update.

## Trust and access

- **On a tailnet, nothing new.** The server answers loopback, and a tailnet address only when
  `tailscale whois` names a node of this tailnet; it checks that the Host names this machine, and
  that a save's Origin is the page's own (`apps/symphony-server/src/gate.rs`, `allows` and
  `allows_host`; `write.rs`, `allowed`). The app on another machine of the tailnet passes the same
  gate the owner's phone does.
- **Anywhere else, not yet.** The servers have no login and speak plain HTTP; on a tailnet the
  tunnel encrypts. A hosted server needs HTTPS and a login (a token traded for a session cookie,
  the token kept in the Keychain), and the save check accepts only an `http://` Origin today. That
  is its own card when a hosted server exists. Until then the app accepts `http://` only for
  loopback, the tailnet's addresses (100.64.0.0/10, fd7a:115c:a1e0::/48) and `*.ts.net` names, and
  refuses any other URL.
- **Never on a server's word**, whatever a page or a feed says:
  - run a command, or open a local file, folder or application;
  - read or write any file but its own settings;
  - open a URL that is not `http` or `https` (no `file:`, no custom schemes that start an app);
  - navigate a server's window to another origin (that opens in the system browser);
  - answer a bridge call that is not on its short list (the first release has none), or one from a
    frame: a mockup's sandboxed frame never reaches it.
- **Local actions belong to the server** on its own machine, offered only to a request from that
  machine, as `tools/go-to-agent-session` plans for panes. The app adds no path around that check.

## Packaging and delivery

- **macOS first.** Read 2026-11-11: the owner's Mac is Apple silicon on macOS 26.6. `dx` 0.7.10
  bundles a `.app` and a `.dmg` (`dx bundle --package-types macos`).
- **Signing.** Apple silicon runs only signed code, and a local build is signed ad hoc by the
  linker. An app built and installed on the Mac that runs it carries no quarantine flag and opens
  without notarisation. That is the first release: `just desktop-install` builds it from a
  checkout at a release tag and copies it to `~/Applications`. Whether the Mac's notification centre
  accepts an ad-hoc signed app is the first thing the first slice tries. Read 2026-11-11: this Mac
  has no code-signing identity. Notarisation needs a Developer ID (the Apple Developer Program,
  $99 a year) and is needed only once the app is downloaded onto another machine.
- **Updates and release tags.** The app is built from Symphony's release tags
  (`tools/release-nightly`), like everything else a project takes. With A, a release that changes
  only the client or the server reaches the app at once, from each server; the app itself is
  rebuilt only when the shell changes, and says so when a server reports a newer release than its
  own. `tools/symphony-update-command` moves projects and never touches the app. An updater that
  downloads new builds by itself (such as Sparkle) needs signed builds and a place to publish them;
  it waits for signing.
- **Linux and Windows** are not in the first release. tao, wry and the menu-bar crates support
  both; only the badge and notification code is per platform.
- **A phone app** is not in this card. The owner reads the board from a phone over the tailnet.
  A's shape carries to iOS (a web view per server plus the same follower), but a notification to a
  phone needs Apple's push service, a developer account and a relay the server can reach. A
  home-screen web app (a web manifest) is the cheaper step: full screen with no address bar, but no
  push over plain HTTP. Either is a card of its own after the desktop app.

## The first slice

Two tasks, the shared data first (RULES.md `workflow/shared-data-lands-first`):

1. **The server's list.** `GET /api/projects` as above, answering tailnet names, with its wire type
   in `crates/symphony-api` and a fixture test pinning the fields the app reads.
2. **The app.** This Mac found by itself, one added tailnet server, a window per server showing its
   pages, the sidebar of servers and projects, the menu-bar item and dock badge with the combined
   count, a notification when a project's count rises, links out to the system browser, the
   Notification stub, and `just desktop-install`.

**Tested without a display.** The crate is a library and a thin binary.

- The library holds everything that decides: the URL rule, the server list file (round trip, a
  file that does not parse kept and never written over), the navigation rule (same origin stays,
  anything else goes to the system browser, other schemes refused), the bridge's list and its frame
  and origin check, the count across servers, when to notify and what it says, and the loss and
  retry timing on a fake clock. Its tests run on the host in `just verify`.
- Against a real server: a server on a private port with a temporary machine folder
  (`SYMPHONY_MACHINE_DIR`, as the server's own tests do) and a scratch board; the follower reads
  `/api/projects`, and a card added to a document raises the count and one notice.
- The script the shell injects is tested in the existing headless Chrome harness
  (`apps/symphony-ui/tests/browser/`) on a real page: a rising count does not throw and does not
  notify, and `window.open` reaches the stubbed bridge.
- The binary (window, web view, menu bar, notification calls) is built and checked by
  `just lint-rs`; no test opens a window. Each app release gets one short check by hand on the
  owner's Mac, recorded on its task.

## Open questions

1. Should the badge also count questions agents ask on tasks (`--to owner`)? That needs one more
   count on the wire. Until answered, the badge counts cards.
2. When two servers serve the same project's board (several machines on one board), should the
   app count it once? Until answered it lists and counts each server's own.
3. Should the shell's own screens be Dioxus, if the first slice shows Dioxus desktop can host the
   server's web view, or plain pages? Until answered, Dioxus if it can.
4. Is an Apple developer account (signing, notarisation, a self-updating app) wanted now, or once
   the app runs on a second machine? Until answered, once it does.
5. Should the global shortcut be in the first release? Until answered, the second.

## Verified and inferred

- **Read in code:** each fact above that names a file and function.
- **Measured:** the client's files and lines that call the browser (`grep` over
  `apps/symphony-ui/src`, tests left out); the wire's commits from this repository's `git log`; the
  registry, server record and signing identities on this Mac; `dx bundle --help` of the installed
  0.7.10.
- **Not tried:** a wry web view loading a Symphony page, the web Notification API inside it, the
  notification centre with an ad-hoc signed app, uploads and downloads inside the web view, Dioxus
  desktop hosting a second web view, and the app's feeds while macOS naps it. The first slice tries
  each before building on it.
