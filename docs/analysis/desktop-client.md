# A desktop Symphony app

What a desktop app for the Symphony would give, how it could be built, and how it would connect to
more than one server, as proposed on the desktop-client card in [DECISIONS](../DECISIONS.md). Each
fact about the code names the file and function it was read in. "Read 2026-11-11" marks the state
of this machine that day.

The ask: a desktop client app, to improve the experience further, above all by connecting to
different servers.

**Decided: A, a thin native shell.** Its windows show each server's own pages, and its own few
screens are Dioxus. The answers to the open questions are applied throughout. The first release
is two tasks, [The first slice](#the-first-slice).

## What the app gives

Ranked by what the owner gains, against a browser tab on the same Mac:

| | What | Today in the browser | First release |
|---|---|---|---|
| 1 | **Notifications with no Symphony tab open**, for every server and project, when a card comes to wait on you (see [What notifies](#what-notifies)) | A notice fires only from a project's home page, while its tab is hidden and once notifications are allowed (`apps/symphony-web/src/home-board.ts`, `apply` and `notify`) | yes: cards, agents' questions on tasks, reviewers' verdicts |
| 2 | **A count of what waits on you** in the dock and the menu bar, summed over every server and project | Each project's home shows its own count; the project list at a server's `/` shows each project's | yes |
| 3 | **Stays connected with its window closed**: the menu-bar item keeps following each project's feed | Nothing follows once the tab is closed | yes: 1 and 2 rest on it |
| 4 | **No address or port to remember**: this Mac's server is found by itself, others are saved by name | A bookmark per server | yes |
| 5 | **A global shortcut** that brings the app forward at the next card | None | skipped for now |
| 6 | **Focus a local agent's pane** | Planned, not built: `tools/go-to-agent-session` (ready) has the server focus the pane on its own machine when the request comes from that machine, in the browser too | when that task lands, in both |
| 7 | **Open a file in the editor** | None | proposed here, later: done by the server on its own machine, the way 6 plans for panes |

Items 6 and 7 act on the server's machine, so they should be the server's to do: 6 is planned that
way by `tools/go-to-agent-session`, and 7 is proposed the same way here. The app then gains them
without running anything itself (see [Trust and access](#trust-and-access)).

### What notifies

The first release notifies when a project's `counts.waiting` rises, the count the badge shows. That
count is the owner's whole queue (`crates/symphony/src/dashboard.rs`, `snapshot`; `triage.rs`,
`queue`):

- a new card waiting on your verdict;
- a card on which an agent asks you a question (`agent · question`);
- a card where an agent answered your comment or question, waiting for you to approve again or
  comment.

Also in the first release, as there will be different users later, so that asks and
notifications can be aimed at specific people:

- **A question an agent asks you on a task** (`just tasks say SLUG "…" --to owner`). It joins the
  badge and notifies, until its recipient posts on that task's thread after it, the rule the
  activity list already uses for "answered" (`crates/symphony/src/taskview.rs`, `answered`), or the
  task is done or cancelled. The wire counts none today: `Counts` has the cards waiting, the
  documents they sit in, spot checks, today's answers and the task board's in progress, in review
  and live agents (`crates/symphony-api/src/lib.rs`, `Counts`, `TaskCounts`), so the server's list
  and the feed gain the count.
- **A reviewer's verdict on a task.** It notifies. It asks nothing of you (RULES.md
  `workflow/reviewers-not-the-owner`), so it does not join the badge. The server's list and the
  feed carry the newest verdict, and the app notifies when it changes.
- **Per recipient.** Both are kept per recipient. Today the board knows one, `owner`. Adding people
  later adds names, not new fields; people themselves need a login, which comes with a hosted
  server.

## How it is built

The client was chosen for this: one Dioxus app, "the browser now and the desktop window later, from
the same code" (`apps/symphony-ui/AGENTS.md`), with every platform hook behind
`platform::Platform` (`apps/symphony-ui/src/platform.rs`: `open_external`, `load`, `save`; home,
drawn by the web client since, notifies from `apps/symphony-web/src/home-board.ts`). Today each server compiles its client into itself and serves it
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
  badge, so they work with every window closed. The page calls nothing native in the first
  release: there is no bridge from page to shell. A link that leaves the server's origin, or a
  `window.open` (`platform::Browser::open_external`), is caught by the web view's navigation and
  new-window rules, which the shell sets, and opens in the system browser. Nothing in the client
  changes for the first release.
- **The page never meets a missing API.** A web view may lack the web Notification API (to try in
  the first slice). The shell defines one that always reports notifications as denied, so the page's
  own notice (`notify` in `apps/symphony-web/src/home-board.ts`) stays quiet and never notifies
  twice.
- **The app's own screens** (the server list, adding a server, "this server does not answer") are a
  few small pages, in Dioxus: a Dioxus desktop
  app around the server's web view. The first slice tries whether Dioxus desktop 0.7.10 can host a
  second web view in its window; plain wry draws them if not.
- **What the server adds.** One read, `GET /api/projects`: each project's id, name, cards waiting,
  first waiting card (title and link), its agents' questions waiting on each recipient and its
  newest reviewer verdict ([What notifies](#what-notifies)), and whether it is refused or its board
  unwell, with the server's installation id and its pin, the Symphony version it was built at
  (`symphony::machine::PIN`, raised with each change to the wire contract; no release tag, which
  no build embeds yet). It answers the tailnet's names like every route but one.
  `/api/front` answers loopback's name alone (`apps/symphony-server/src/projects.rs`, `front`), and
  the project list at `/` answers the tailnet but is an HTML page (`home`), so another machine has
  no list it can read as data today.
  The live counts come from each project's existing feed (`/<id>/api/feed`), with the feed's own
  loss rules (`crates/symphony-api/src/lib.rs`, the module docs).
- **Wire.** The follower reads only `/api/projects`, `Board.counts.waiting`, `Board.next`'s first
  card, the new counts of questions and verdicts, and the feed's `counts` and `queue` changes and
  its `task-asks` region, which carries those counts. A fixture test in `crates/symphony-api` pins
  those few fields, added to and never renamed. Everything else stays free to change with each
  release, as now.
- **Budget.** Nothing is added to any page kind: the 250 KB first visit is untouched.
- **Offline.** A server that does not answer shows the app's own page saying so and when it was last
  reached, with its last count in the menu, greyed. Nothing of it can be read offline, as in the
  browser today.

### B: the client compiled in

The Dioxus client built for the desktop (`dx` builds desktop as well as web) with a `desktop`
feature, the app holding one compiled client that talks to every server's `/api`.

- **A native half for most of the client.** Read 2026-11-11: 31 of the client's files, about 8,000
  of its 10,700 lines outside tests, call the browser directly (`web-sys`, `js-sys`, `wasm-bindgen`): the feed
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
- **It still runs scripts a server sends.** A viewer's module script would run in the app's own
  page, beside the app's native channel. A mockup in a card runs the server's scripts in a frame
  sandboxed to `allow-scripts` ([REVIEW](../REVIEW.md#html-mockups-in-cards)). B either keeps both,
  and runs a server's code as A does in those places, or drops viewers and mockups, or puts viewers
  in a sandboxed frame too.
- **Storage.** Every server's projects would share one origin, the app's, so every stored key also
  takes the server first.
- **Wire.** The app at one release talks to servers at others, so the whole wire becomes a contract
  between releases. Today client and server move together and the wire moves often: 15 commits,
  merges left out, changed `crates/symphony-api` from 2026-11-04 to 2026-11-11, and several removed or
  renamed fields (the one-server projects, the panes probe, the machine-work evidence). The app
  would need a rule such as: it speaks to a server at its own pin (`crates/symphony/src/machine.rs`,
  `PIN`, raised on every change that is not additive) and shows any other read-only or not at all.
  In practice a Symphony release would need an app release.
- **Budget.** Untouched only while every desktop path sits behind the `desktop` feature; a shared
  transport layer would land in every page kind and must be measured against the 250 KB.
- **Offline.** It could keep each project's last board and show it read-only.
- **Gain over A.** It opens at once from local code and reads the last board offline. Its own client
  code never comes from a server; viewers and mockups still do, unless B drops them. It draws in a
  web view all the same, so it looks and performs as A does.

### C: A, then B

A first, as above, and B later. B pays off only if reading offline comes to matter, or not taking a
server's client code does, for instance with a hosted server run by someone else; even then its
viewers and mockups run that server's scripts unless B drops them. Cost: A now, and B's whole cost
later on a wire that would by then have to stand still.

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
| Runs code a server sends | yes, as the browser does | in viewers and mockups, unless dropped | yes, then in viewers and mockups |

### Why A

**Decided: A**; B and C were not chosen. A gives every item of the first
release with the least new code, and keeps the client and the wire free to change every release
(but for the few fields the app reads), which they do several times a day. What B adds, offline reading and a client that never comes from a server, matters for
a server someone else runs, which does not exist yet, and even there its viewers and mockups would
run that server's scripts.

- [r] **desktop-client** — A Symphony app for the Mac that sits in the menu bar and the dock, connects to several servers (this Mac's, another machine's over the tailnet, later a hosted one), counts what waits on you across all of them and notifies you with no tab open. How is it built? A: a thin native shell whose windows show each server's own pages, always at that server's release. B: the Dioxus client compiled into the app, talking to every server's API. C: A now, B later. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/desktop-client#How it is built]]
  [2026-11-11 14:07] agent — Posted for the board task `product/desktop-client`.

    **What the tab cannot do.** Notify you with no Symphony tab open (today only a project's home page does, while its tab is hidden). Show one count of what waits on you, across every server and project, in the dock and menu bar. Keep following with its window closed. Find this Mac's server by itself, and keep the others by name. The first release does these four; a global shortcut comes next. Focusing an agent's pane and opening a file in the editor are done by the server on its own machine (`tools/go-to-agent-session`), so the browser gets them too.

    **A, a thin shell.** Each server's window loads that server's own pages, so the app and a server never disagree, and a Symphony release reaches the app with no app update. The shell adds notifications, the badge, the menu bar and links out to the browser. Cost: one new crate, and one new read on the server listing its projects with their counts, which another machine cannot get today. No client change, nothing added to the 250 KB budget, and the wire stays free to change. Offline: says the server does not answer and keeps its last count.

    **B, the client compiled in.** Cost: 31 of the client's files call the browser directly and need a native version; plugin viewers need cross-origin access the server does not give. The wire becomes a contract between releases, while it changed 20 times last week, several not additively: a Symphony release would need an app release. Gain: it reads each project's last board offline and runs no code a server sends; it looks and performs as A does.

    **C.** A's cost now, B's later, on a wire that would by then have to stand still.

    **Why A.** Everything in the first release for the least new code, with the client and wire still free to change every release. The app's own screens can still be Dioxus.

    **Trust.** On a tailnet nothing new is needed: the server already admits the tailnet's machines, as it does your phone. A hosted server needs HTTPS and a login, a card of its own later. The app never runs a command, opens a local file or app, or leaves the server's site in its window on a server's word.

    **Delivery.** macOS first, built from a release tag and installed with one recipe; a build made on the Mac that runs it needs no notarisation. Signing ($99 a year) waits until it runs on another machine. Linux, Windows and a phone app are not in this card.

    The first slice, and how it is tested without a display: [[analysis/desktop-client#The first slice]]. Five smaller questions are at [[analysis/desktop-client#Open questions]]; answer any of them in the note.
  [2026-11-11 14:33] agent — Corrections to the entry above, after a review of its clarity.

    **B** still runs a server's scripts in plugin viewers and card mockups unless it drops them; only its own client code never comes from a server.

    **The projects list** answers the tailnet today, but only as an HTML page; A's new read gives it as data.

    **Pane focus** is planned (`tools/go-to-agent-session`, not built yet), and opening a file in the editor is proposed here the same way; neither is done by the server today.

    **The first release notifies** when a project's waiting count rises: a new card, an agent's question on a card, or an answer to your note. Questions agents ask you on tasks and reviewers' verdicts are counted nowhere yet; open question 1 asks whether you want them. [[analysis/desktop-client#What notifies]]

    **The wire** changed in 15 commits last week (not 20: that counted merges). With A it stays free to change but for the few fields the app reads, which a test pins.
  [2026-11-11 15:26] owner · question — 1. Should the badge and notifications also cover questions agents ask you on tasks (--to owner), or reviewers' verdicts on tasks?
    Yes. We should also have different users in the future, so that asks / notifications could be targetted to specific people.
    2. When two servers serve the same project's board
    For now I think the model should be indeed two separate boards. Albeit we will soon move to a "central board" with "multiple machines connected"
    3. Yeah, why not dioxus.
    4. once it does.
    5. can be skipped for now.
    But considering there will be an iOS app desired to exist too, how would this shape or change the decided upon solution here?
  [2026-11-11 15:44] agent — Your five answers, each with what it changes, then the iPhone app. Answered for the board task `product/desktop-client`; the analysis records the answers under [[analysis/desktop-client#Open questions]].

    **1. Questions on tasks and reviewers' verdicts: yes.** The badge also counts each question an agent asks you on a task (`--to owner`) until you answer it, and the app notifies it. A reviewer's verdict is notified too. It asks nothing of you, so it does not stay in the badge unless you say it should. Both need counts on the wire beside the cards, which the server's new projects read carries. Targeting people: the board knows one person today, `owner`, so the new counts are kept per recipient, and adding people later adds names, not new counts. People themselves need a login, which comes with the hosted server's card.

    **2. Two servers, one project: two boards for now.** The app lists and counts each server's own board, as the analysis assumed. Once the central board exists, a project reached through two servers must be counted once, by the board rather than the server; that waits for the central board. Runs already go this way: each machine sends them to the server hosting the board (`reports-ci-runs`, approved 15:21).

    **3. "Yeah, why not dioxus": the app's own screens are Dioxus.** The question was narrow: under A, are the few screens the app draws itself (the server list, adding a server, "this server does not answer") Dioxus or plain pages? Your answer settles that: Dioxus, beside the server's page in the same window, if the first slice shows Dioxus can host it. It does not choose A, B or C. Under A the app is a Dioxus app whose windows show each server's own pages; under B the whole Dioxus client is compiled into the app. If you meant the whole client should be Dioxus inside the app, that is B. Asked plainly below.

    **4. Apple developer account: once it runs on a second machine.** No change for the Mac app. An iPhone app brings it forward (below).

    **5. Global shortcut: skipped.** It leaves the second release too, until you ask for it.

    **An iPhone app.** Read in code: Dioxus 0.7.10 builds for iOS, and its mobile target is its desktop one (the same window and web view crates), so A's shell and B's client both carry to the phone. The rest is inferred, not tried:

    - **Notices on a phone come from the server.** iOS suspends an app soon after it leaves the screen, so the follower that keeps counting with every window closed cannot run there. A notice is a push the server sends through Apple's push service, which needs the paid developer account ($99 a year). So the rule for when to notify, and what a notice says, belongs in shared code the server can run too, not only in the Mac app. This holds under A and B alike.
    - **The account.** An iPhone app installed without the paid account stops opening after 7 days. Anything longer, TestFlight and push all need it, so the account comes when the phone app starts.
    - **A fits the phone better.** Under A the iPhone app is the same shell, showing the server's pages, so a Symphony release reaches the phone with no new phone build. Under B a Symphony release that changes the wire needs a new phone build, installed through TestFlight.
    - **A cheaper first step.** The board added to the phone's home screen can get notices without any app (web push, iOS 16.4 and later), but only over HTTPS. The tailnet can give its names HTTPS (`tailscale serve`); the server's save check accepts only `http://` pages today.

    So the phone strengthens A, and adds a push from the server whichever way the app is built. The phone app stays a card of its own, after the Mac app.
  [2026-11-11 15:44] agent · question — How is the app built? A: a thin shell whose windows show each server's own pages, with its own screens in Dioxus. B: the whole Dioxus client compiled into the app. C: A now, B later. Recommended: A, and the iPhone app keeps it so. Approve and say A, B or C in the note.
  [2026-11-11 16:24] owner · approved — A
  [2026-11-11 16:32] agent — Recorded: A. The app is a thin shell whose windows show each server's own pages, with its own few screens in Dioxus. The analysis now states A as decided and follows your five answers throughout. Filed on this board, ready, as the analysis's two first-release slices:

    - `tools/server-projects-list`: each server lists its projects as data. It gives counts, the first waiting card, the questions agents ask you on tasks until you answer them, and the newest reviewer verdict, kept per person (today only you). It lands first, since the app reads it.
    - `tools/desktop-app`: the Mac app. It finds this Mac's server and adds others by URL, shows a sidebar of servers and projects, and puts the count in the dock and menu bar. It notifies for cards, task questions and verdicts, opens outside links in the browser, and installs with one recipe. Two servers serving one project count as two boards. There is no developer account and no global shortcut.

    Not filed, each waiting on what it needs: the iPhone app and its push from the server, a hosted server's HTTPS and login, signing and a self-updating app (once it runs on a second machine), notices aimed at named people (once Symphony has users), and counting one project once across servers (with the central board).
  [2026-11-11 16:33] agent · resolved

## Several servers

- **A server** is a base URL: this Mac's (`http://127.0.0.1:8765`), another machine's over the
  tailnet (`http://studio.tailnet-name.ts.net:8765`), later a hosted one (`https://…`). It serves
  one or more projects under `/<id>/` (read 2026-11-11: this Mac's serves `symphony` and the
  consuming project).
- **Its identity** is the server's installation id (`crates/symphony/src/machine/identity.rs`,
  `installation_id`), which `/api/projects` carries, so the same server reached by loopback and by
  its tailnet name is listed once.
- **Two servers, one project.** Two separate boards for now, until the move, coming soon, to a
  central board with several machines connected. Each server's board is listed and counted apart.
  Counting a project once across servers comes with the central board.
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
- **Where the list is kept.** The app's own settings file in a folder of its own,
  `~/Library/Application Support/Symphony Desktop/`, apart from the server's machine folder
  `~/Library/Application Support/Symphony/` (`crates/symphony/src/machine.rs`, the module docs). It
  is written whole like the registry (`crates/symphony/src/machine/registry.rs`, `Registry::write`),
  and never written over when it does not parse. The machine folder is only read, for this Mac's
  port. A hosted server's credential, later, goes in the Keychain.
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
  - answer a call from a page: the first release has no bridge from page to shell. One added later
    answers only its short list, from the top page at the server's origin; a mockup's sandboxed
    frame never reaches it.
- **Local actions belong to the server** on its own machine, offered only to a request from that
  machine, as `tools/go-to-agent-session` plans for panes. The app adds no path around that check.

## Packaging and delivery

- **macOS first.** Read 2026-11-11: the owner's Mac is Apple silicon on macOS 26.6. `dx` 0.7.10
  bundles a `.app` and a `.dmg` (`dx bundle --package-types macos`).
- **Signing.** Apple silicon runs only signed code, and a local build is signed ad hoc by the
  linker. An app built and installed on the Mac that runs it carries no quarantine flag and opens
  without notarisation. That is the first release: `just desktop-install` builds it from a
  checkout at a release tag and copies it to `~/Applications`. A developer account waits until the
  app runs on a second machine. Whether the Mac's notification centre
  accepts an ad-hoc signed app is the first thing the first slice tries. Read 2026-11-11: this Mac
  has no code-signing identity. Notarisation needs a Developer ID (the Apple Developer Program,
  $99 a year) and is needed only once the app is downloaded onto another machine.
- **Plain HTTP in the web view.** A load of `http://…ts.net` meets App Transport Security, which
  leaves IP addresses alone but not a tailnet name; the app's `Info.plist` names `ts.net` as an
  exception for plain HTTP. Not tried yet: the first slice loads a tailnet server by its name.
- **Updates and release tags.** The app is built from Symphony's release tags
  (`tools/release-nightly`), like everything else a project takes. With A, a release that changes
  only the client or the server reaches the app at once, from each server; the app itself is
  rebuilt only when the shell changes, and says so when a server reports a newer release than its
  own. `tools/symphony-update-command` moves projects and never touches the app. An updater that
  downloads new builds by itself (such as Sparkle) needs signed builds and a place to publish them;
  it waits for signing.
- **Linux and Windows** are not in the first release. tao, wry and the menu-bar crates support
  both; only the badge and notification code is per platform.
- **A phone app** is not in this card; the owner wants an iPhone app later. The owner reads the
  board from a phone over the tailnet. Read in code: Dioxus 0.7.10's mobile target is its desktop
  renderer (the same window and web view crates), so A's shell carries to iOS. Inferred, not tried:
  iOS suspends an app soon after it leaves the screen, so no follower runs there. A notice to a
  phone is a push the server sends through Apple's push service, which needs the paid developer
  account, and so the rule for when to notify belongs in code the server can run too. Under A a
  Symphony release still reaches the phone with no new phone build. A home-screen web app is the
  cheaper step: web push works for it on iOS 16.4 and later, but only over HTTPS, which the
  tailnet can give its names; the server's save check accepts only `http://` today. Either is a
  card of its own after the desktop app.

## The first slice

Two tasks, the shared data first (RULES.md `workflow/shared-data-lands-first`):

1. **The server's list.** `GET /api/projects` as above, answering tailnet names, with the counts of
   agents' questions and the newest verdict per recipient, its wire type in `crates/symphony-api`,
   and a fixture test pinning the fields the app reads. Filed as `tools/server-projects-list`.
2. **The app.** This Mac found by itself, one added tailnet server, a window per server showing its
   pages, the sidebar of servers and projects, the menu-bar item and dock badge with the combined
   count, a notification when a project's count rises or a verdict arrives, links out to the system
   browser, the Notification stub, and `just desktop-install`. Filed as `tools/desktop-app`.

Not filed, each waiting on what it needs: the iPhone app and its push from the server, a hosted
server's HTTPS and login, signing and a self-updating app (once the app runs on a second machine),
notices aimed at named people (once Symphony has users), and counting one project once across
servers (with the central board).

**Tested without a display.** The crate is a library and a thin binary.

- The library holds everything that decides: the URL rule, the server list file (round trip, a
  file that does not parse kept and never written over), the navigation and new-window rule (same
  origin stays, anything else, `window.open` included, goes to the system browser, other schemes
  refused), the count across servers, when to notify and what it says, and the loss and retry
  timing on a fake clock. Its tests run on the host in `just verify`.
- Against a real server: a server on a private port with a temporary machine folder
  (`SYMPHONY_MACHINE_DIR`, as the server's own tests do) and a scratch board; the follower reads
  `/api/projects`, and a card added to a document raises the count and one notice.
- The script the shell injects, the Notification stub, is tested in the existing headless Chrome
  harness (`apps/symphony-ui/tests/browser/`) on a real page: a rising count neither throws nor
  notifies.
- The binary (window, web view, menu bar, notification calls) is built and checked by
  `just lint-rs`; no test opens a window. Each app release gets one short check by hand on the
  owner's Mac, recorded on its task.

## Open questions

None is open. All five are answered, each answer after its question, and the body above follows
the answers. How an iPhone app would shape the choice is answered on the card and under
[Packaging and delivery](#packaging-and-delivery), and A is approved.

1. Should the badge and notifications also cover questions agents ask you on tasks (`--to owner`),
   or reviewers' verdicts on tasks? Each needs one more count on the wire. Until answered, cards
   only ([What notifies](#what-notifies)).
   **Answered:** yes, both: there will be different users later, so that asks and notifications
   can be aimed at specific people.
2. When two servers serve the same project's board (several machines on one board), should the
   app count it once? Until answered it lists and counts each server's own.
   **Answered:** two separate boards for now, each counted apart, until the move, coming soon, to
   a central board with several machines connected.
3. Should the shell's own screens be Dioxus, if the first slice shows Dioxus desktop can host the
   server's web view, or plain pages? Until answered, Dioxus if it can.
   **Answered:** Dioxus. This settles the shell's own screens, not A, B or C.
4. Is an Apple developer account (signing, notarisation, a self-updating app) wanted now, or once
   the app runs on a second machine? Until answered, once it does.
   **Answered:** once it does.
5. Should the global shortcut be in the first release? Until answered, the second.
   **Answered:** skipped for now.

## Verified and inferred

- **Read in code:** each fact above that names a file and function.
- **Measured:** the client's files and lines that call the browser (`grep` over
  `apps/symphony-ui/src`, test files and inline test modules left out); the wire's commits from this
  repository's `git log --no-merges`; the registry, server record and signing identities on this
  Mac; `dx bundle --help` of the installed 0.7.10.
- **Not tried:** a wry web view loading a Symphony page, the web Notification API inside it, plain
  HTTP to a `*.ts.net` name under App Transport Security, the notification centre with an ad-hoc
  signed app, uploads and downloads inside the web view, Dioxus desktop hosting a second web view,
  and the app's feeds while macOS naps it. The first slice tries each before building on it.
