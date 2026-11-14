# The owner's words

The owner's words that the guides and the documents under `docs/` quoted as the basis of a rule or
a choice, kept here word for word since each came to state its rule in plain words (2026-11-14).
Each quote is set apart as it stood, its spelling and marks untouched, under the file and the rule
or section it stood in, with the date it carried (none where it carried none) and what it was said
on. Beside them, the dated lines that said who decided what, and when, without quoting. Kept for the
record, never as reference ([RULES](../../RULES.md) `history-is-not-reference`): a guide or a
document says what the rule is, and the decision register and the task threads keep the rest.

## The guides

### RULES.md

**`workflow/evidence-is-the-gate`**, 2026-11-11, two quotes:

> "Do they need to run a full test suite? Why? If tests are so time consuming why are they being
> required to run every time, instead of running tests more closely associated with the tasks at
> hand, leaving a nighly run for a full verify?"

> "So the executors need to 'prove that the work they did work'. That is the concept of the stamp
> based gating. As long as evidence is good enough, a full verification can be left to be done
> later"

**`workflow/release-reds-first`**, 2026-11-12:

> "also nightly fixes should be prioritized, like bugfixes are since they block releases. Meaning
> express path line followed up by another run"

**`tests/rerun-the-failure-alone`**, 2026-11-11:

> "test that fail should be ran themselves to see if they pass. No need to run entire test suite
> each time."

**`tests/a-red-on-main-has-one-owner`**, 2026-11-11:

> "tests that are failing in main should be handled by an orchestrator as a separate task in order
> for individual tasks not to be blocked by it and have a single dedicated owner to solve it."

**`tests/no-test-fails-on-load`**, its first paragraph, 2026-11-12:

> "A test that fail when machine is under load should be rewritten to account for cpu wall clock
> time usage - not total time spent or be removed from nightly"

and its second, on a test that fails a release run or its rerun because of load, 2026-11-13, which
the rule called the approval to discard such a test without a card:

> "A rerun should not fail or pass based on machine load. If it does, discard the tests and move on"

**`review/auto-approve-what-is-decided`**, 2026-11-11:

> "run some auto triage. Some decisions look like they should had been auto approved"

**`review/decided-is-not-asked`**, 2026-11-12, approving cards a design had added after the owner
approved its first card:

> "A. this should had been auto approve/triaged, its basically what was decided in the other card."

> "A. should also had been auto approved."

then of that first card, which an agent's question on it had put back in front of the owner:

> "this should had been a seaparate card no the same one. I also don't really get why those similar
> questions about things that were already been approved are being asked. They are consequences of
> the previous decision and should be auto-approved"

**`docs/cards-clear-and-concise`**, 2026-11-12, on a card that did not open with what changes for
the owner:

> "I don't fully understand this issue. So pick whatever choice makes sense to you."

**`commits/no-ai-authorship`**, 2026-11-11:

> "no claude code or codex co authorship anywhere"

**`tools/no-new-python`**, 2026-11-13:

> "By the way, usage of python is acceptable for throw away code."

### AGENTS.md

**Landing and pushing main** (*This repository as its own project*), that a land pushes main
without asking each time, 2026-11-11:

> "just land should do the push"

and, before it:

> "do push it yes"

The same paragraph, that an orchestrator lands an approved task without asking the owner, 2026-11-12:

> "asks to lands should be auto approved by orchestrators"

The same paragraph, the release cadence and the refresh, 2026-11-12:

> "It would be nice to release a new version every given number of tasks done, say 20 and refresh
> server/clients"

then:

> "yes, a green release should refresh"

*Agent integration*, that the land recipe needs no permission prompt, 2026-11-12:

> "should not require permission from me"

### apps/symphony-ui/AGENTS.md

Its opening, on the client being one Dioxus app, with no date (the guide carried it since the
workspace was extracted, 2026-11-02):

> "I want to experiment more with dioxus, so let's switch to it"

**Budget**, the first-visit cap of 300 KB, 2026-11-11:

> "lets increase the budget to 300"

The same bullet said that what a visit leaves out (document-content images, browser-cache hits and
the feed's keep-alive pings) was "the owner's, 2026-11-11".

### crates/tasks/AGENTS.md

That a role on open work keeps it until its agent has sat idle an hour, 2026-11-08:

> "Only sessions doing active work or that will soon (less than one hour) have active work should
> be kept open"

## The documents

### docs/RELEASES.md

**Packages on crates.io**, 2026-11-13, after the verdict on the consumer contract's card:

> "I would like to publish crates (MIT license) to crates.io as the pacakge manager"

The same section called the renamed packages "the owner's choice (2026-11-13, on the board task
`tools/publish-to-crates-io`)", and *The refresh* called `symphony-app` "the owner's names,
2026-11-13".

### docs/analysis/attached-artifacts.md

Its opening and *Chosen: F, a split*, then *The implementation tasks*, 2026-11-12, on the card:

> "agreed with F"

*The implementation tasks* also said "The owner chose F on 2026-11-12", and the note it drafts for
the consuming project's board opened "Symphony's owner chose F on 2026-11-12".

Its opening, on where a swept worktree's artifacts go, 2026-11-12:

> "images should be kept, build should not. Images and other artifacts can be pushed into a main
> tree. However there should be a cap to the size of the folder, after which it should ask/warn
> agents to manually check and remove stale images and artifacts not in use. Maybe 5 GB limit?"

then:

> "Instead of directly commiting, artifacts could be attached some with some different process.
> Either nodes, or git files? Forgot the name but I think there was something for this type of
> challenge in particular. But if there's something that still actively references the artifact,
> then it should be kept until space used becomes a problems requiring it to be remove."

and on the card, later the same day:

> "A git clone should be able to see the images, specially the ones that are referenced in the docs
> and including for previous decisions as well. I believe this excludes E and A correct? Assuming
> the above, what is the recommmendation? If B, can you not push override objects to delete them? I
> doubt there is no way to remove them?"

Parts of these stood again: the requirement in its opening ("A git clone should be able to see the
images, …"), *Removing what reaches GitHub* ("If B, can you not push override objects to delete
them? …"), the sweep's rule under *The rule* ("if there's something that still actively references
the artifact, …"), *The check* ("build should not") and *Why F* ("Instead of directly commiting").

### docs/analysis/board-design.md

The ask, 2026-11-11:

> "The design of the board isn't great."

### docs/analysis/client-cache.md

**Read ahead**, the rulings on the build's thread, with no date:

> "eager loading of content of both tasks and docs of related / adjacent info based on links present
> on a page"

> "Large documents should not be eager loaded… a given amount of its content could be pre-loaded
> (equivalent to a screen render or a bit more of it)."

### docs/analysis/client-shared-runtime.md

Its opening, 2026-11-13, on the Reports page costing 557 KB on a first visit, choosing the option
"Share the runtime between modules. One common chunk that every page module loads, so a page's own
module is only its own code":

> "seems like the obious choice. I'm okay with keeping it t 557 for now, and a follow up to
> investigate this option"

and, of the first-visit caps on `/` and `/tasks`, 2026-11-11, as in the client's guide:

> "lets increase the budget to 300"

*The choice* said "the owner chose A, 2026-11-13".

### docs/analysis/consumer-contract.md

**Open choices**, where versions live, "by the owner's word after B, 2026-11-13":

> "I would like to publish crates (MIT license) to crates.io as the pacakge manager"

*The options* (its table's Distribution row) said "the owner chose crates.io", and B's option "The
owner chose one all the same, after B", of a registry.

### docs/analysis/desktop-client.md

Its opening, 2026-11-11:

> "I also want to push for a desktop client app. Would likely help improve the experience even more.
> Specially if it can connect to different servers"

It said "**Decided: A, a thin native shell** (the owner, 2026-11-11)", and *Why A* "**Decided: A**
(the owner, 2026-11-11)". Under *Open questions*: "The owner answered all five on 2026-11-11
(15:26), quoted after each", then asked how an iPhone app would shape the choice "then approved A
(16:24)". The five answers, each also quoted in the body where it applied (*What notifies*, *A: a
thin shell*, *Several servers*, *Packaging and delivery*, and the table's global shortcut, "skipped
for now (the owner)"):

> "Yes. We should also have different users in the future, so that asks / notifications could be
> targetted to specific people."

> "For now I think the model should be indeed two separate boards. Albeit we will soon move to a
> "central board" with "multiple machines connected""

> "Yeah, why not dioxus."

> "once it does."

> "can be skipped for now."

### docs/analysis/own-names.md

Its opening, "**Decided: B, at once**", 2026-11-11:

> "just remove the bandaid quickly. Break things then fix them ASAP"

It went on: "the owner then asked, on 2026-11-11, for every direct reference to the consuming
project to be removed from Symphony's app folder (the owner's exact words: board post #2869)".

### docs/analysis/owner-asks.md

Its opening, 2026-11-11, in four messages:

> "answering chat is a task, but kind of not really the same type of task. Feels like it should be
> called something else and not be considered when we look at task counts"

> "it also has no status like the other things would"

> "it should also generally be prioritized to be answered, and be routed to a proper agent that has
> context if one is running"

*The design* said the owner approved the card on 2026-11-11, before the pictures were added.

### docs/analysis/plugin-delivery.md

Its opening, 2026-11-12, after the consuming project's pin had to be moved and its host rebuilt to
bring the app up to date:

> "seeing how problematic the current approach that requires building client's board often for them
> to even work is, there needs to be design a better approach for more automatic type of updates, by
> keeping client specific code separate from the board's one, in order full rebuilds to be avoided
> when binaries are updated?"

> "I feel that this is more of a problem around "rust's crate and build archiectural design issue"
> around server and client architecture with custom plugin capabilities."

later the same morning:

> "It feels to me we should really have separate crates that have lower binary sizes, in order to be
> able to distribute things quickly and with smaller builds, so that only things that needs to be
> rebuilt are rebuilt and distributed. And that is rust's crate design architecture pains that we
> need to really solve well. 30MB is still really big for a binary"

**How a release gets there**, on hosted builds, 2026-11-12:

> "We will move to B in the future. Just not yet"

**The chosen path: B, measured first**, 2026-11-12:

> "Let's go with B first, measure results against the current approach for a post change
> comparison."

Its opening also said "The owner chose the path on 2026-11-12, on three cards"; *One update
command*, "On 2026-11-11 the owner chose a per-project command that moves a project's pin to the
newest release tag"; and *The board*, that a board moves when every writer is on the release while
there is no dedicated build service "(the owner, 2026-11-12)".

### docs/analysis/relax-build-trial.md

The ask, 2026-11-11:

> "https://makepad.github.io/relax/#architecture evaluate if adopting relax could help us with how
> symphony build process works"

> "(this should be empirical test, if better numbers should be adopted)"

then:

> "meaning, if there's proof this is better no need to ask me to decide"

### docs/analysis/release-candidate.md

Its opening, 2026-11-12, in the order the board records them (the express-path task, filed at
18:05, quoted the first; its scope note at 18:08 said the release candidate question came next):

> "also nightly fixes should be prioritized, like bugfixes are since they block releases. Meaning
> express path line followed up by another run. Nightly can also use the stamp based approach for
> things that did not change so it's faster"

> "release candidate branch process? bugfix - rerun failures then release once green. Then update
> main with bugfixed changes. Open to suggestions on how"

> "A test that fail when machine is under load should be rewritten to account for cpu wall clock
> time usage - not total time spent or be removed from nightly"

> “we can also move from calling this "nightly" to release candidate”

> "just release ? (would do tests and release, if fails then won't do release and agents has to
> fix?)"

Parts of these stood again under *The first run, tonight* ("Rerun failures"), *A. The fix's own
tip is the candidate* ("rewritten to account for cpu wall clock time usage - not total time
spent", "removed from nightly"), *The trades* ("Nightly can also use the stamp based approach for
things that did not change so it's faster"), *C* ("bugfix - rerun failures then release once
green. Then update main with bugfixed changes"), *Why A* ("update main with bugfixed changes",
"Then", "Open to suggestions on how") and *The vocabulary*.

**The rulings**, the owner's earlier rulings the options were judged against: no lock or queue on a
land (2026-11-12, 06:42):

> "A solution like a lock or queue would directly bottleneck the pipeline which isn’t acceptable
> either."

a release only after a full run that is green (2026-11-12, 06:42; *The trades* quoted it as "runs
all tests and publishes when green"):

> "a nightly or more often run that then runs all tests and publishes when green is the only
> acceptable choice I can think of."

releases built from full runs, urgent fixes out of cycle (2026-11-11), the words under
symphony-update.md below; and fixes to reds first (2026-11-12), the first quote's "Meaning express
path line followed up by another run."

**What keeps working during the change**, the last rename, of the consuming project's names
(2026-11-11):

> "B - just remove the bandaid quickly. Break things then fix them ASAP"

Its opening also said that the owner chose A on its card on 2026-11-12 at 19:05, and that the
review's corrections were "auto-approved on its card at 20:35 the same day".

### docs/analysis/reports-and-monitoring.md

**The ask**, 2026-11-11, on the board task `product/reports-and-monitoring`, where the owner first
said a report and monitoring bucket was missing from the consuming project, then corrected that to
Symphony (both on board post #2869):

> "considering how symphony now behaves like CI/CD, then info on the CI/CD aspects is what I mean
> is missing."

> "As well as general pipeline and defect pipeline information. We don't have stamp info in the UI
> for tasks, no 'graph' of the steps that tasks did and how much time each took, etc"

**Tests from cargo's output**, "**Decided: cargo-nextest**", 2026-11-11, and *Open questions*' answer
to question 2:

> "can absolutely adopt cargo nextest"

**Gates**, the rule that the more specific level's answer replaces the general one's, 2026-11-11:

> "lower level replaces higher level. Should replace it everywhere for itself."

**Agents**, agents at work per hour, 2026-11-12:

> "Agents per hour is absolutely something we need. [...] adding to that number of agents,
> orchestrators, reviewers (possibly two charts?) so that it becomes easier to understand how
> healthy a pipeline is"

and money estimated at API prices, 2026-11-11:

> "tokens are hard to measure, since cache vs non cache hits counts differently. This should be
> counted "as if we were paying API costs" for it. Best assumption/guess on pricing actual cost"

**Open questions**, answered on 2026-11-11 (16:22): question 3, whether to show tokens only:

> "nope"

and question 5, whether to count each merge commit on main as a landing run:

> "add a land command, should be offered by default on init. Remember -> convention over
> configuration approach"

Dated lines without a quote: *What exists today* said "the owner approved option B of" the token
recording card "on 2026-11-08"; *Where runs are kept*, "**Decided: A, run files in the common dir,
a core page** (the owner, 2026-11-11)"; *Tests from cargo's output*, that no queue wait is shown
where nothing queues work, "Decided (the owner, 2026-11-11)"; *Gates*, "**Decided: A, a typed gates
seam** (the owner, 2026-11-11, after the pictures below)"; *How the figures are computed*,
"**Decided: A, an hourly roll-up into files** (the owner, 2026-11-11)"; *Core page or plugin
blocks*, that the report-section seam and the chart block wait for a project's own section "(the
owner, 2026-11-11)"; *Several machines, one board*, "**Decided: A, with the send** (the owner
approved A after this answer, 2026-11-11)".

### docs/analysis/symphony-update.md

**Releases**, 2026-11-11:

> "B. There will be other projects using it. New releases can be pushed for urgent bugfixes, but
> generally should be generated based on nightly builds that would do a full verification cycle and
> conciliation."
