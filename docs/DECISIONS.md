# Decision register

Symphony's own product and engineering decisions, one card each. The cards that need the owner
are at the top, and the page regroups itself every time one is answered. How cards, verdicts and
their dated history work is in [REVIEW](REVIEW.md).

<!-- register:start -->
## Waiting on an agent

### Test leftovers

- [-] **orphans-end-by-age** — The server finds test processes left running with no parent: a Symphony server whose home is in the temporary folder or a checkout's `artifacts/`, and a task database on a private port whose data is in memory or in such a folder. Today every page names each one and gives one command, `symphony-server orphans --end PID…`, which checks each again and ends only those; the server ends none by itself. Should it end them itself once they are old enough? A: no, it keeps reporting them. B: it ends both kinds after an hour, with the same checks, and logs each one it ends. C: it ends the databases after an hour and only reports the servers. Recommended: A. Approve and say A, B or C in the note, or comment.
  [2026-11-11 18:44] agent — Posted for the board task `tools/orphan-processes`.

    **Where they came from.** The 14 databases seen on 2026-11-11 were the board servers of fixture repositories. They were started detached, as the live board's is, so nothing ended them. That start is now supervised for any board in the temporary folder or under `artifacts/`: the database ends when the process that started it ends, even when it is killed. The two test fixtures that started an in-memory database themselves now have the store tests' leash. The server's own parent is no longer counted. After these changes, a leftover means a path nobody has found yet.

    **A, report only.** Nothing is ended that someone meant to keep running, such as a server started by hand with `nohup` and its home in the temporary folder. Cost: the owner runs the command when the notice shows.

    **B, end both after an hour.** Nothing piles up, and no one has to act. Cost: a process kept running on purpose under the temporary folder ends after an hour without being asked. A leak that keeps happening is then cleaned up instead of reported.

    **C, end the databases only.** The databases took the memory (542 MB for 14). A database nobody started by hand is nearly always a test's. Servers stay the owner's call.

    **Why A.** The paths that leaked are repaired, so the notice should rarely show. When it does, it points at a new leak that should be fixed, not cleaned up quietly. B or C can be added later as one timer on the census, which re-checks each process before ending it, as the command does.
  [2026-11-11 18:51] owner · rejected — I don't think it should report it in the symphony app. It's no the job of the symphony app to find duplicates of itself running
  [2026-11-11 19:13] agent — Done on `tools/orphan-processes`: the Symphony no longer looks for copies of itself or for test databases. The census, the notice at the top of every page, the `symphony-server orphans` command and the starter's line about leftovers are gone, with their tests and docs; no notice moved to the live feed. The repairs at the source stay: the database of a board in the temporary folder or under `artifacts/` ends when the process that started it ends, even when that process is killed, and the two test fixtures that start a database themselves end it the same way. The server's status still sends `orphans`, always 0, so a starter built before this can still read it.

## Rejected and replaced

### Reports and monitoring

- [-] **reports-task-gates-timeline** — A task's page opens on its gates: every stamp or check it needs to land, met (by whom, when), missing, or waived (why). Below them, a timeline of what it went through: stages, claims, review rounds and verdicts, builds and tests, waits, each with its duration. Symphony shows its own gates without a plugin: the reviewer's verdict, the project's check on the tip, the review callback, a card waiting on the owner. How do a project's own gates reach the page? A: a typed seam, drawn the same way for every project. B: a panel of generic blocks the plugin lays out. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/reports-and-monitoring#The task page]]
  [2026-11-11 13:31] agent — Posted for the board task `product/reports-and-monitoring`, the second reports card.

    **What exists.** The board replays a task's stages from its thread but keeps only totals, and the task page shows none. Star Foundry's landing already answers its stamps for the task page (required, met or missing, waivers with why and by whom), and nothing shows them: a plugin can add a button or a field to a task, not a list of what it needs.

    **A, typed gates.** A plugin answers name, state, by, when and why; one component draws them. Symphony can then count them across tasks: which gate waits longest, which is missing most. Cost: one seam, one wire type, one component; Star Foundry's adapter adds the times its stamps lack today.

    **B, a block panel.** Cost: the least Symphony work, but each project draws it differently and nothing can be counted.

    **The timeline** needs no new recording: the replay keeps each segment as well as its totals, and runs come from the run files. A phone gets a vertical list with a bar per step; a wide screen gets lanes over time.

    **Why A.** What a task waits on is the question asked of it, and a typed answer also feeds the defect report. This page takes the place of the task-page halves of `tools/stage-times` and `tools/review-as-stamps`. [[analysis/reports-and-monitoring#Timeline]]
  [2026-11-11 13:46] owner · rejected — this needs some visualization layer to decide (images)
  [2026-11-11 14:12] agent — Replaced by **reports-task-page-pictures**: the same choice, with pictures. One task's page under A and under B, on a phone and on a wide screen, and its timeline drawn both ways.

## Resolved

### Desktop client

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

### Reports, small choices

- [r] **reports-small-choices** — Six smaller choices for the Runs and Reports pages, each with the default the filed tasks follow until you answer. 1. Slowest tests: stable cargo prints no time per test, so show the slowest suites only, without adopting cargo-nextest. 2. Agents' cost: tokens only, no money, since a subscription is not paid per token. 3. Symphony's landings: Symphony has no land command, so count each merge commit on its main as a landing run. 4. Queue wait where no pool queues work: show none, and leave out the time Cargo waits on its build lock. 5. A project's own gate with the same name as Symphony's review gate replaces it on the task page only, not in the board's sign-off lane or the PULL digest. 6. A project's own section under Symphony's figures (Star Foundry's certificate costs, art builds, test classes), drawn through a plugin seam and a chart block: not filed until a project asks for it. Approve to keep all six defaults, or comment with the numbers to change. [[analysis/reports-and-monitoring#Open questions]]
  [2026-11-11 15:45] agent — Posted for the board task `product/reports-and-monitoring`, after `reports-ci-runs` was approved. Choices 1 to 5 are the analysis's open questions 2, 3, 5, 6 and 7, which no verdict has answered. Choice 6 is the report-section seam the analysis proposed, which no card asked. The tasks that meet them: 1 `tools/run-suites`; 2 `tools/agent-activity-history`; 3 `tools/runs-rollup`; 4 `tools/run-record`; 5 `tools/task-gates-seam`; 6 none yet.
  [2026-11-11 16:22] owner · approved — 1. can absolutely adopt cargo nextest.
    2. nope. tokens are hard to measure, since cache vs non cache hits counts differently. This should be counted "as if we were paying API costs" for it. Best assumption/guess on pricing actual cost
    3. add a land command, should be offered by default on init. Remember -> convention over configuration approach
    4. okay
    5. yes indeed. lower level replaces higher level. Should replace it everywhere for itself.
    6 approved.
  [2026-11-11 16:31] agent — Recorded, each with what it changes. New tasks are on this board, ready.

    **1. cargo-nextest: adopted.** `tools/nextest` runs this workspace's tests through nextest at a pinned version, installed by one setup step so a test run never downloads. Doc-tests, which nextest does not run, still run through cargo. Test binaries that share one fixture across their tests, such as the perf suite's one board, keep it. `tools/run-suites` now waits on it and reads each test's time; `tools/runs-rollup` gives the slowest and flakiest tests, not only suites.

    **2. Cost in money, as if paying API prices.** `tools/agent-cost-estimate` prices input, output, cache writes and cache reads apart, per model, from a dated price table in the repository taken from the providers' published prices. Every figure is labelled an estimate. A model missing from the table takes the newest price of its family, marked as a guess; one with no family is shown unpriced and named, never counted as free.

    **3. A land command.** `tools/land-command`: `just land SLUG` checks the reviewer's approval on the tip and the project's check on the merge, merges, closes the task, sweeps its worktree, and later records the landing as a run. `symphony init` installs it in every project, and a project's own `land` replaces it. It replaces the merges by hand, so the Runs page counts landings from it, not from merge commits.

    **4. No queue wait where nothing queues.** Recorded; `tools/run-record` already does this.

    **5. A project's gate replaces Symphony's everywhere.** `tools/project-gates-everywhere` carries it past the task page to the sign-off lane, the digest orchestrators get, and the move to blocked. The analysis now states the rule in general: the more specific level's answer replaces the general one's.

    **6. The project report section and the chart block wait until a project asks.** Recorded; nothing filed.
  [2026-11-11 16:33] agent · resolved

### Reports and monitoring

- [r] **reports-ci-runs** — Each project gets a Runs page, as a CI product shows it: what runs and waits now, then every verify, lint, test, client build, review callback, landing and later nightly, with its commit, branch, task, who started it, result, duration and queue wait. A run opens on its steps, their times and logs; trends show pass rate and duration per pipeline and suite, the slowest suites, the flakiest tests, and machine minutes per task, day and project. Where are runs kept? A: day files in the git common dir, read by a core page. B: tables on the board. C: the same files, shown by plugin components on a document. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/reports-and-monitoring#Runs]]
  [2026-11-11 13:31] agent — Posted for the board task `product/reports-and-monitoring`, the first of three reports cards; each can be approved alone.

    **Why.** No run is kept anywhere Symphony reads, and Symphony's own builds and tests are never recorded. Star Foundry's verify keeps a line per run for itself, and it shows what is missing: 510 runs since 2026-11-08, a full verify's median 24 min and p90 2.7 h, 139 wall-hours on 2026-11-09 alone, 28 red and 52 timed out.

    **A, day files.** The recorder `tools/build-test-recorded-generic` adds appends one line per run and per step in the git common dir, with its log beside it and the test results read from cargo's output. The page reads the newest files; trends are rolled up hourly. Cost: a page kind, an API, a roll-up. Nothing writes to the board. Runs kept 90 days, logs 14.

    **B, the board.** Cost: a schema change, which makes every older server refuse writes, and writes on the board's write path for every run and step, hundreds of runs a day at Star Foundry's rate.

    **C, plugin components.** Cost: rendered again on every read, nothing updates while a run goes, and charts carry no values on a tap.

    **Why A.** Every project gets it, Symphony included, with no load on the board, in the shape Star Foundry's verify lines already have.

    **The record needs more than the recorder plans** (task, handle, phase, label, start, end, exit, machine): the run and its steps, the pipeline, commit, tree and branch, who started it, queued and started times, CPU time, the log, and results per test suite. Stable cargo prints no time per test (tried). [[analysis/reports-and-monitoring#The run record]]

    Seven smaller questions, for all three cards, are at [[analysis/reports-and-monitoring#Open questions]]; answer any of them in a note.
  [2026-11-11 13:45] owner · question — How would A work when we move to separate machines using a single board?
  [2026-11-11 14:12] agent — **As written, A shows only one machine's runs.** Run files live in the git common dir of the clone where the run happened. A clone on another machine has its own common dir, and today its own board as well.

    **How A works on several machines.** Each machine's recorder writes its own day file first, and that file is its outbox. It sends each run's start and end lines over HTTP to the Symphony server that hosts the board. That server appends them to its own run files under the sending machine's id, and answers with the run ids it stored. A line not yet acknowledged is sent again at the next run's start and end, and a line sent twice is stored once.

    - **Who may send.** The server already admits only its own machine and confirmed tailnet peers, and it checks the Host. The machine id is a label, not a password, so the server records which tailnet machine sent each line. It refuses one machine id arriving from a second tailnet machine.
    - **Server unreachable.** Runs wait in the outbox and arrive late, marked late. The page shows each machine's last contact.
    - **Clocks.** Each machine times its own runs with a clock that only counts forward, so durations are right even when its time of day is wrong. Order and age use the time the server received a line. Each send measures the machine's clock offset, and the page shows it when it is over 30 s.
    - **Logs** stay on their machine. A red run's first and last lines travel with its end line. A green run's log opens on demand when its machine serves; otherwise the page says which machine has it.
    - **Running now.** Sending only finished runs would hide what runs elsewhere, so the start line is sent too.

    **B on several machines** needs the board itself reachable from every machine. It is a database without a login, listening on this machine alone, so it would need a login it does not have. An offline machine would still need an outbox and its log files. **So A, with the send, stays the recommendation.** [[analysis/reports-and-monitoring#Several machines, one board]]
  [2026-11-11 15:21] owner · approved — A
  [2026-11-11 15:43] agent — Recorded: A, with the send from the entry above, so runs on several machines show on one page. Filed on this board, ready, in the order they land:

    - `tools/run-record`: the recorder writes one line per run and per named step to day files in the git common dir, kept 90 days. It builds on `tools/build-test-recorded-generic` once that lands, and does not change it. The review callback runs through it, so its exit is kept.
    - `tools/run-logs`: each run's log beside the day files, kept 14 days (a red run's 30, at most 2 GB a project). The terminal shows what it showed before.
    - `tools/run-suites`: each test binary's counts and time, and the failed tests with their first line, read from cargo's plain output.
    - `tools/run-send`: the send to the server hosting the board. The local file is the outbox, the server acknowledges by run id, and a machine id is bound to the first tailnet machine it came from.
    - `tools/runs-rollup`: pass rate and duration per pipeline and suite, the slowest suites, flaky tests, queue wait and machine minutes, rolled up in the hourly job `tools/reports-rollup` builds.
    - `tools/runs-page`: the Runs page and its API, on a phone and wide, with the machine view.

    **Six smaller choices** these tasks meet are on a short card of their own, `reports-small-choices`, each with the default the tasks follow until you answer.
  [2026-11-11 15:45] agent · resolved
- [r] **reports-pipeline-defects** — A Reports page per project, after Runs: how work flows (tasks in each status per day, lead time and its trend, where task-hours go, bounces, what sits longest); where defects are caught (review, red verify, main red, nightly red, revert), how long each took to fix, how many escape past landing and which checks catch what; then agents (sessions, tokens) and board health per hour over days. How are the figures computed? A: the server rolls them up every hour into files. B: from the board on each view. C: a nightly report document. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/reports-and-monitoring#Pipeline and defects]]
  [2026-11-11 13:31] agent — Posted for the board task `product/reports-and-monitoring`, the third reports card.

    **What exists.** `tasks stats` prints the flow figures in a terminal. Star Foundry's board holds about 1,220 top-level tasks since 2026-09-26: 405 went back from review at least once, 16 main reds and 54 nightly reds were filed, and its main took 1,218 merges in 14 days with no revert. Board health is kept for one hour, in memory.

    **A, hourly.** Each hour the server reads each project's window once (0.3 to 2.5 s on Star Foundry's board) and writes the day's figures to a file; health gets a line an hour from memory. Cost: a figure up to an hour old; one board read an hour per project; nothing written to the board.

    **B, per view.** Cost: that read on every open, slower as the board grows, and no history of what the board does not keep.

    **C, a nightly document.** Cost: nothing live, and a commit a day.

    **Why A.** The page opens at once on a phone and the board's write path is untouched.

    **New recording these need:** health per hour; agents' activity and stale or lost counts; tokens (`tools/board-token-usage`, approved, not built); and, from the project's landing and nightly, which landing a red came from, for the escape rate. **A cost in money is not knowable today**: transcripts carry tokens and the model, never a price. [[analysis/reports-and-monitoring#Defects]]
  [2026-11-11 13:48] owner · approved — A
  [2026-11-11 14:12] agent — Recorded as A. The shared server rolls the figures up every hour into day files in each project's git common dir. Nothing is computed when the page opens, and no report document is written. [[analysis/reports-and-monitoring#How the figures are computed]] now says so.

    Filed on the board, ready, each quoting your verdict:
    - tools/reports-rollup: the hourly roll-up into files.
    - tools/reports-page: the page. It comes after the roll-up and the next two, whose files it reads.
    - tools/board-health-history: board health, one line an hour, kept 90 days.
    - tools/agent-activity-history: sessions started, and stale and lost counts, by kind and model.
    - tools/red-landing-origin: a red names the commits it came between, so escapes trace to the landings in that range.

    Star Foundry's landing and nightly would need to pass those commits; that change is Star Foundry's, for its own board.
  [2026-11-11 14:14] agent · resolved

### Client size budget

- [r] **first-visit-budget-pings** — The `/tasks` page is capped at 250,000 bytes on a first visit, and a test enforces it. The page is 249,982 bytes, 18 under. The test also counts the live feed's keep-alive pings, 19 bytes every 5 seconds, so on a busy machine it reads 250,047 to 250,066 and fails though nothing changed. It is the last thing between main and a `just verify` that passes every time. What should the cap count? A: everything but the keep-alive pings, which are reported beside the total. B: as now, and trim the page until pings fit under it. C: raise the cap. Recommended: A. Approve and say A, B or C in the note, and say whether 18 bytes of room is enough or the page should be trimmed as well.
  [2026-11-11 16:20] agent — Posted for the board task `tests/first-visit-budget-counts-pings`.

    **A, leave the pings out.** Real feed content still counts. A quiet run measures the same as today, so the cap is no looser. Cost: a small change to the test, and the guide says what the cap counts.

    **B, trim the page.** Cost: the test stays tied to machine load until the page is about 400 bytes smaller, and every later ping-sized change brings the failure back.

    **C, raise the cap.** Cost: the phone's first visit gets heavier each time this is done.

    **Why A.** A ping is not page weight, and how many arrive depends on when the test server started, not on the page.

    **The room.** At 18 bytes almost any client change crosses the cap. The last change, one CSS selector, moved the total by 5.
  [2026-11-11 15:18] owner · approved — A and B. This is too close to the limit anyway. Check what can be done to improve upon it for a more reasonable/comfortable range.
  [2026-11-11 15:43] agent — Recorded: A and B, both in the board task `tests/first-visit-budget-counts-pings`, which fleet-osprey is working on now. The keep-alive pings leave the cap and are reported beside the total. The `/tasks` first visit is measured part by part (the compiled client, styles, scripts, the board data) and trimmed until it is at least 10 KB under the cap, instead of 18 bytes; the task reports what each cut saved and what further cuts would cost. The cap stays 250,000 bytes.
  [2026-11-11 15:45] agent · resolved

### Task page, in pictures

- [r] **reports-task-page-pictures** — A task's page opens on its gates: every stamp or check it needs to land, each met, missing or waived, with who, when and why. Below them, a timeline of what the task went through. The pictures show one task, on a phone and on a wide screen. How do a project's own gates reach the page? A: a typed seam, so Symphony draws every project's gates the same way and can count them. B: a panel of generic blocks the plugin lays out. The timeline is the same under A and B. Recommended: A. Approve and say A or B in the note, or comment. *Replaces:* reports-task-gates-timeline. [[analysis/reports-and-monitoring#Pictures]]
  [2026-11-11 14:12] agent — The gates on a phone, under A and under B: ![Gates on a phone, option A](analysis/reports-gates-a-phone.svg) ![Gates on a phone, option B](analysis/reports-gates-b-phone.svg)
  [2026-11-11 14:12] agent — The task is made up in Star Foundry's shape: its four stamps, two review rounds, a build and tests. Its times are invented. The pictures follow the app's colours, type and chips, in light and dark.

    **A** is one component for every project. What the task waits on comes first, with who and when, and a gate can link to its run or verdict. Symphony can count these gates across tasks, for example which one waits longest.

    **B** is the plugin's own layout. Star Foundry's stamps answer has no times, so its table has none. On a phone, a table wider than the screen scrolls sideways.

    Wide: ![Gates on a wide screen, A](analysis/reports-gates-a-wide.svg) ![Gates on a wide screen, B](analysis/reports-gates-b-wide.svg)

    **The timeline** is the same under A and B. On a phone it is a list with a bar per step; on a wide screen, lanes over time. ![Timeline on a phone](analysis/reports-timeline-phone.svg) ![Timeline on a wide screen](analysis/reports-timeline-wide.svg)
  [2026-11-11 14:28] owner · approved — A
  [2026-11-11 14:36] agent — Recorded as A: a typed seam, and Symphony draws every project's gates the same way. [[analysis/reports-and-monitoring#Gates]] now says so, and the A pictures are the look to build to.

    Filed on the board, ready, each quoting your verdict and naming the pictures:
    - tools/task-gates-seam: the plugin seam and its wire type, with Symphony's own gates when no plugin answers.
    - tools/task-gates-panel: the gates on the task page, on a phone and wide.
    - tools/task-timeline: stage segments kept from the replay, served and drawn as a list on a phone and as lanes wide. Runs join once tools/build-test-recorded-generic records them.
    - tools/star-foundry-gates-adapter: a note to copy to Star Foundry's board, since the code is there. Its stamps answer gains times and maps to the seam.

    The task-page halves of tools/stage-times and tools/review-as-stamps are replaced by these; both threads say so.
  [2026-11-11 14:37] agent · resolved

### Delivery to projects

- [r] **symphony-update-command** — A project takes a newer Symphony with one command, `just symphony-update`, which `symphony init` installs. The project's own check, land command and approval still decide whether it lands. Which commit should the command take? A: main's newest. B: the newest release, which Symphony would tag. C: no command, a written checklist instead. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/symphony-update#The command, step by step]]
  [2026-11-11 12:37] agent — Posted for the board task `product/symphony-update`.

    **What the command does.** On a new branch it moves only the Symphony commit in Cargo.lock, builds, brings the shipped skills and hook up to date, and runs the project's check. The project's land command and approval land it. Run again on main afterwards, it restarts the shared server. It never lands, pushes or moves another dependency by itself, and it asks before a restart that changes the board schema.

    **A, follow main.** Each run takes main's newest commit. Main moved 9 to 23 times a day this past week, so a project is shown as behind only when main's board schema or pin is ahead of it, or its commit is over a week old. Cost: a bad main commit reaches the project if the project's own check misses it; one revert undoes it.

    **B, follow releases.** Symphony tags a release for a fix, a schema change or once a week, and the command takes the newest tag. Cost: a release step on Symphony's side for every fix, and a fix waits for its tag. There are no tags today.

    **C, a checklist.** The same steps by hand. Cost: today's hand work every time, and nothing tells a project it is behind.

    **Why A.** Star Foundry is the only project, its own `just verify` checks every update before it lands, and a revert is the rollback. B pays off once a project off this machine uses Symphony.

    Four smaller questions are at the end of [[analysis/symphony-update#Open questions]]; answer any of them in the note.
  [2026-11-11 12:43] owner · approved — B. There will be other projects using it. New releases can be pushed for urgent bugfixes, but generally should be generated based on nightly builds that would do a full verification cycle and conciliation.
  [2026-11-11 12:53] agent — Recorded as B: projects follow Symphony's release tags, never main. A nightly on main's tip runs the full verification and the conciliation, and tags a release only when both are green. An urgent fix is released out of cycle after the same checks. The pin-only move, the project's own landing, the schema and restart handling, and rollback (now to the previous release tag) stay as proposed. [[analysis/symphony-update#Releases]] now states this. Filed on the board: tools/release-nightly (the releases and the nightly), tools/symphony-update-command (the command and the stamped recipe init installs) and tools/symphony-behind-signal (the line on the page, in tasks status and at start). What conciliation should cover is asked on tools/release-nightly's thread; until you answer it reads as building each project the machine serves against the candidate.
  [2026-11-11 12:53] agent · resolved
<!-- register:end -->
