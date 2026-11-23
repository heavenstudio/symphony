# Plugin protocol

A plugin can answer from a process of its own instead of being linked into the host: the host
(the server, the `symphony` command) starts the plugin's binary and speaks with it in JSON lines on
its stdin and stdout. A project chooses per plugin. Replacing such a plugin is replacing its binary,
with no rebuild of the server. The requests and answers are the serde types of
`symphony_plugin::protocol`, which also name each call in the record of a project's plugin calls
([PLUGIN_CALLS](PLUGIN_CALLS.md)). The plugin's side is `symphony_plugin::serve`
(`crates/symphony-plugin/src/serve.rs`) and the host's is `crates/symphony/src/plugins/process.rs`.

This page is the reference. [Writing a plugin and a theme](PLUGIN_WRITING.md) writes a first
plugin from nothing, with its code, its test, its settings and how to see it drawn.

## The kit

A plugin, linked or a process, is built on the plugin kit: three crates that move together, as their
types cross.

- `symphony-plugin`: the seams a plugin implements, the data each call hands it (a page's
  artifacts folder among them, `Page::artifact`), this protocol's requests and answers, `serve`,
  the block a component's problem is shown as (`error`), and the harness its tests ask it through
  (`testing`, below).
- `symphony-blocks`: the block vocabulary a plugin renders to and the data its seams return, as the
  server sends them and the client draws them; `symphony_plugin::vocabulary` re-exports it.
- `symphony-docs`: the doc checks a plugin adds, the docs' syntax they read, and the text and
  markup rules the Symphony reads files by (`text`: Python's whitespace, lines and `strip`, a file
  read with universal newlines and written whole; `markup`: HTML escaping and URL quoting).

The kit's crates depend on no other workspace crate but each other, and build wherever Rust's
standard library does: only a file's stamp, which tells `read_text` a file changed, reads more of it
on Unix (its device, inode and change time). They carry one version, 1.2.0 now, and are made to be
published on crates.io ([RELEASES](../workflow/RELEASES.md#packages-on-cratesio)), so they export what a plugin
uses, and beside it the file's stamp (`text::FileStamp`, its fields private), which `read_text` and
the library's board cache share, and no more: the text rules only the library uses (a one-sided
strip, a path as `pathlib` prints it) are the library's own. The app's own wire types, which move with the server and the
client, are `symphony-api`'s, which re-exports the vocabulary under the same names; the library
re-exports the text, markup and `error` under its old names.

Beside the kit, a plugin takes what it wants of the helpers, each on a version line of its own:
`process-cwd` (children started without a fork, and without the session or the marks of the
process that starts them, `process_cwd::session`), `symphony-schema` (JSON Schema checks, and
`repr` and `os_error`, a value and an OS error printed as Python prints them), `symphony-args` (a
command line's flags, and `refuse`, its usage and error), `symphony-env` and `repository-read`. A
plugin built on these alone links no Symphony library.

A plugin's tests ask its components and card kinds as a host does, through the protocol's requests
and answers, with no host linked: `symphony_plugin::testing::Host` hands each request to the plugin
as `serve` would, and its `render` of a `Page` the test makes shows a refusal as the host does, as
`error`'s block. A test makes each other request by its `new` (`Recognises::new(kind, card)`), and a
card's data by `CardData::of` from the `Card` it writes.

## Turning it on

In the project's `symphony.toml`, name the plugin in `[plugins] crates` as a linked one is named,
and give the command that starts it in `[plugins.process]`:

```toml
[plugins]
crates = ["the-project-plugin"]

[plugins.process]
the-project-plugin = ["bin/the-project-plugin", "--quiet"]
```

The first word is the program, and what may be run is exactly one of three: an absolute path, a
bare name looked up on the host's `PATH`, or a relative path holding a `/`, which is a path inside
the project's checkout, its `.` parts folded; a relative path with a `..` part is refused. The host
runs it as the owner's own code, with the host's user and environment less the session's, so a
project names only programs its owner would run by hand. A name `[plugins] crates` does not list,
or a command with no program, is refused by name. A plugin with no command here is linked.
`symphony-status --lint` names a plugin whose process does not start, does not declare or is
refused.

A branch's checkout (a linked worktree) starts no process of its own: its pages are answered by the
process its main checkout's `[plugins.process]` starts for that plugin, run in the main checkout,
each call naming the branch's checkout, so the plugin reads the branch's files. Where the main
checkout's file names no command for the plugin, the branch's page has the linked plugin, or with
none linked, none. A command a branch writes in its own `[plugins.process]` takes effect once it
lands.

A component's call costs one round trip, and a register read one request for all its cards
([below](#card-kinds-in-one-request)), not a round trip for each call of each card.

A plugin's binary is its `main` calling the SDK with the plugin it links:

```rust
fn main() -> std::process::ExitCode {
    symphony_plugin::serve(&the_project_plugin::PLUGIN)
}
```

What answers from a process here is a plugin's components, card kinds, renderers, routes, home
regions, board actions, gates ([below](#gates)), watches and doc checks, each call a request on the
pipe; its commands, each
run as its program ([below](#its-commands)); and its navigation and theme, which its declaration
holds and which cost no call. A project that loads starts its plugins' processes for their
declarations, as its navigation is among the routes an entrypoint's files must not meet. An
entrypoint's `types` may name a renderer of a plugin's process; a project whose entrypoint names a
type only a process could handle, and whose process cannot say its renderers, is refused, naming
why and the plugin's log. A document's route needs no declaration, as none changes an entrypoint's
URL or folder: `symphony reply` names the document it wrote by its route with no process started,
while no entrypoint names such a type. A server serves a project refused with the defaults, and
every page of it says so and why until it loads. While that refusal is a process that has not
declared (its program missing, or slow to declare when the server starts), the project is read
again a second after each read that failed (the process's pause), one read at a time on a thread of
its own: while it is read again, every reader keeps the defaults and none waits on the process. The
first read of the project, and the first after its file changes, are no retry: whoever makes it
waits for the declaration, ten seconds at most, the server's live loop as much as a request. Once
the process declares, the project is served as its file says, with no change to the file. A command
of such a project says why it is not known ([below](#its-commands)). Its other seams (board fields,
prose rules, generators and transports) are not asked of a process: a plugin that declares
any of them is refused under `[plugins.process]`, naming them, rather than losing them without a
word, and stays a linked plugin's to answer. The refusal is what holds: the protocol has their
calls and the SDK's `serve` answers them, but no host asks one of a process. So is a plugin
refused that declares another name than the one `[plugins.process]` gives it.

## The host

Any host serves a plugin that answers from its process, whatever it was built with: the rule that
judges a host for a project (`crates/symphony/src/machine/host.rs`) counts a plugin the project's
main checkout gives a `[plugins.process]` command as served, read from its file each time it
judges. A linked plugin is served only by a build that compiles it, a project's own. The server of
a release installed on the machine compiles none, so it serves a project only when every plugin
the project names answers from a process; a project naming a linked one is registered and refused
there, its pages naming the command that would let the release serve it. While one does, the
machine's server stays a project's build ([the switch](../workflow/RELEASES.md#the-switch)), and every project
naming a linked plugin says so on its pages.

## Its process

The host starts a plugin's process when it first needs what the plugin declared (a project's load
reads its navigation) or a call of it: in the project's checkout, without
the session that started the host, its stdin and stdout the protocol's, each line of its stderr
appended to its log, `plugins/NAME.log` in the project's artifacts folder (`artifacts/` unless
`[project] artifacts` names another); a stderr line longer than 4096 bytes goes in pieces. The host
writes a line there, stamped, for each start (`started PID: COMMAND`), each end (`its process PID
exited with code 3`, `… was killed by signal 9`, `… was ended: WHY` when the host ended it), each
call that had no answer, and each line of the plugin's that answers no call: one that is no
envelope, an answer to an id no call waits for, and an answer that came after its call was given
up. Such a line is shown by its first 256 bytes, control characters replaced, and then dropped. The
log is rolled at 4 MiB to `NAME.1.log`, replacing the one before, so the two hold about 8 MiB.

A plugin writes nothing but the protocol's lines on its stdout: a stray line is logged and skipped,
and its diagnostics go to stderr. One host process keeps one process for each plugin of each project,
for its life. The plugin's stdin ends when the host does: a plugin ends then, and the SDK's `serve`
exits at most 1 s after (`symphony_plugin::ENDING`), with code 1 if a call still runs, whatever that
call is doing. A plugin written without the SDK must end when its stdin does, or it outlives its host.

The first request is the declaration (below). A call then waits 10 s for its answer (a gates answer
a minute, [below](#gates)), the writing of its request included: the host's lines go to a writer
thread through a queue of 256, so a plugin that stops reading holds no call past its wait. A call
whose answer does not come in the wait is given up, sent a `cancel`, and shown as failed; a call
whose request the plugin did not even read in the wait ends the process. The host ends a process
(closes its stdin and kills it) when:

- it is refused at its declaration (another major, another name, a seam a process does not answer);
- its declaration has no answer in 10 s;
- a call it was sent a `cancel` for still has no answer 10 s after the cancel;
- a request of the host's was not taken from its stdin in a call's wait, or the queue of 256 is full;
- its stdout fails, or holds a line longer than 16 MiB.

A process that ends, or is ended, fails every call waiting on it with why, and the first call 1 s
after starts it again: at most 5 starts in 180 s, after which it stays down until the oldest start
leaves that window. A refused plugin is not started again while its program is the file it was (its
length and modification time, on `PATH` for a bare name) and the project's settings name the same
command: a new build of its program, or a changed command, is started afresh. What it declared at
its first start stands for it until the host ends: a declaration that changed is noted in the log.

A call that has no answer (the process down or restarting, no answer in time, a refusal, an answer
that does not read) is shown where its answer would go: a component's in its place on the page, as
`⚠ plugin NAME: its process 4242 was killed by signal 9; it starts again in a moment`, a
renderer's drawing the same way in the file's place, and a card kind's as the kind's default, as if
the plugin said nothing of the card. A route answers not found and a home region shows nothing, why
in the plugin's log. A board action's offer or submission is its error, naming why, as a linked
action's error is shown, and a submission refused moves nothing. A gates answerer's gates read
unknown, why as their detail ([below](#gates)), never met. A watch that did not warm says so
in the server's log, and one told to forget says nothing: a process that ends keeps nothing. A
describer's entry on the Settings page says why in its place, as unknown. A doc
check that did not run is a problem of the project file naming the plugin and the check, so
`symphony status --lint` fails rather than passes, and names no component. A call that had no
answer fails alone: the calls after it start the process again once its pause is over.

## Its commands

A plugin's commands do not go on the pipe. `symphony NAME ARGS…`, run in a project whose plugin
answers from a process and declared the command `NAME`, runs the plugin's program alone as `PROGRAM
… command NAME ARGS…`, the words `[plugins.process]` gives it first: in the folder `symphony` runs
in, without the session, with `symphony`'s own stdin, stdout and stderr, which the program writes to
itself once what `symphony` wrote before is flushed, and its exit code `symphony`'s, which the SDK's
`serve` gives as the command's (its low byte, as an exit status holds it). A program killed exits
128 and the signal's number, and one that no longer starts (its file gone since it declared) exits
1, each saying so on stderr (`plugin NAME: its command C was killed by signal 9`). Running the
command is no call: it waits on no process of the pipe's, copies no stream and is not measured.
`symphony help` lists the commands the project's processes declared, each once, before those of the
plugins linked in the binary; a linked plugin that the project's file gives a process adds none,
even while that process does not start, so a copy built into the binary never stands in for the
project's own. The project is found and its plugins started only when its file names a
`[plugins.process]`, read alone and quietly first; in a project with none, `symphony NAME` runs a
linked plugin's command with no project loaded or checked. `commands.run` goes on no pipe: it names
a linked command's run in the record ([PLUGIN_CALLS](PLUGIN_CALLS.md)).

A command is known by the plugin's declaration, so `symphony NAME` and `symphony help` start each
process of the project for it. While one cannot start or declare, its commands are not known:
`symphony NAME` of a name nothing else adds says on stderr which plugin's process failed, why and
where its log is, then that the command is unknown, and exits 1 (2 when no process failed):

```text
symphony: the plugin shelf's process failed, so its commands are not known: /p/bin/shelf-plugin cannot start: No such file or directory (os error 2) (its log: /p/artifacts/plugins/shelf.log)
symphony: unknown command "render-models" (`just symphony-cli help` lists them)
```

`symphony help` lists the commands there are and then the same line for each such plugin, and
`symphony help NAME` refuses such a name as `symphony NAME` does. A project that does not load
because of such a process (an entrypoint whose renderer it was to declare) has the defaults and so
no plugin: its first line is `the project does not load, so its plugins' commands are not known:`
and why, its log named, and a linked copy of a plugin its `[plugins.process]` names still answers
for none of its commands.

## Card kinds in one request

A card kind answers eight calls of a card, and a read of the registers asks them of every card: the
board's snapshot, `symphony status`, triage and the postponed queue. From a process, a read asks
them in one request, `card_kinds.read` (0.3): the read's cards, each as the read asks it (with its
document, and with none, as a card's slot, choice and thread are asked), and the answer, for each
card, each kind's eight answers in the plugin's order (`KindAnswers`), the pictures read in the
checkout the request names. The cards go in as many requests as keep each line within the 16 MiB
bound, a card too long for a line alone asked card by card. The read's answers stand for that read
alone: a card it did not hold, a request that had no answer, an answer that does not hold one entry
for each card, and a plugin of 0.2, which has no batch, are asked card by card. A panic in any
kind's call refuses the request whole, and its cards are asked card by card.

What it saves, counted: card by card, a read asks each of the eight calls of each card as a round
trip of its own; in one request, a register of 500 cards is one request of 1,000 entries (each card
with its document and with none), 206 KB asked and 410 KB answered, well within a line. What the
three reads cost in time is the second test of [Measured](#measured), taken as that section says.

## Gates

A plugin's gates answerers (`Gates`, declared in `gates` by their names) answer from its process as
linked ones do: the same reads ask them, each answer kept by the task's status, its branch's tip
and what the answerer says it reads besides, so the plugin is asked once a tip
(`crates/symphony/src/gates.rs`). A read asks `gates.kept_while` with the project's checkout and the
task (its slug, status and branch), and, when nothing kept holds, `gates.answer` with those and the
branch's tip, none when the repository holds no such branch. A branch's checkout is answered by its
main checkout's process, each call naming the branch's checkout. The plugin's own error is kept as
it said it, and asked again after a while, as a linked answerer's is.

An answer may start a landing binary, so it is waited for as long as a linked one may run: a minute,
the gates' ceiling (`gates::CEILING`), not a call's 10 s. A read waits 10 s for it, as for a linked
answer, and shows its gates unknown meanwhile; the answer comes on and is kept for the next read.
The host asks a process three answers at once (`GATES_AT_ONCE`), one fewer than the SDK's four
workers, so slow answers never hold the worker its other calls are answered on (a component's
render, an authority's attestation, what an answer is kept while); another answer waits in the host
for one of the three to end, a minute at most, and then has its own minute. An answer with none in
its minute is given up and sent a `cancel` like any call, and a process that has not answered it
10 s later is ended.

`gates.kept_while` is a call's round trip on every read, the sign-off lane's on the server's live
loop among them, where a linked answerer's is cheap and never blocks: a process that stops
answering holds such a read for a call's wait, and the reads after it until the host ends it 10 s
after the cancel, about twenty seconds once.

A call that had no answer (the process down, or ended while it answered; a refusal, a panic in the
plugin among them; no answer in its wait) is the answerer's error, `plugin NAME: WHY`. In
`gates.answer` it is the answer, kept as an error is. `gates.kept_while` has no error to return, so
its failure is the read's answer at once, and nothing kept is used past it. Either way each gate the
answerer answered before reads unknown, why as its detail, in place of the Symphony's of its name,
and never as met. Both calls are 0.1's, so a plugin of any minor is asked them.

## Approval authorities

A project whose workflow guards an edge commits its approval authority's key
([PROJECT_FILE](../operations/PROJECT_FILE.md)), and a plugin may answer for that authority, by the adapter its
guards name (`Authority`, declared in `authorities`). When the owner takes a guarded edge from the
app with no attestation given, the host asks the authority of each guard's adapter,
`authorities.attest` (0.4), for one guard of the operation: the project's committed id, the task,
the edge, the guard, the place it is taken from, the input revisions and the principal
(`Operation`), and the time it has (`within_ms`, ten seconds, a call's wait). Its answer is the
authority's signed statement as it signed it (`Signed`), or why it cannot say. The host hands what
it answered to the board unchanged, which verifies it as one pushed to it
([WORKFLOW](../workflow/WORKFLOW.md#guards)); an error, a panic or no answer in its time refuses the operation
as `guard_unavailable`, naming the guard and why. Nothing is kept: the next operation asks again.
A plugin linked or answering from its process answers alike.

## Settings describers

A plugin may describe its own entries on the project's Settings page
([PROJECT_FILE](../operations/PROJECT_FILE.md#the-settings-page)), each by a describer
(`Describer`, declared in `describers` by its id). The page's read asks each describer of each
plugin the project selects, `describers.describe` (0.5), with the project's checkout, and its
answer is one entry in the page's own shape (`SettingsEntry`, `symphony_plugin::vocabulary`): its
name, what it is for, its state and why, what it needs and whether this machine has it, and its
settings, each with its key, value, source, scopes and class. It is drawn after the Symphony's own
entries in the integrations, by the component that draws theirs, its id the host's
(`plugin-NAME-ID`, counted on from `-2` where an entry before took it, as two describers of one
id do, or plugin `x`'s describer `y-z` beside plugin `x-y`'s `z`), and a plugin the project does
not select adds none. An error, a panic or no answer in a call's wait is said in the entry's
place, the page drawn all the same.

A description declares and runs nothing: it is what the page shows, and the host acts on none of
it. Its class is not the describer's to lower: a setting whose key the local settings file refuses
(the tables and keys of `PROTECTED` and `PROTECTED_KEYS` in `crates/tasks/src/local.rs`, which
decide what runs and what lands) is shown reviewed, written in the project's scope alone, whatever
its describer calls it, and a describer may mark a setting of its own reviewed. Nor is the board's
scope its describer's to take: the board's store keeps the Symphony's own preferences alone, so a
description's setting is shown without it, among the scopes it may be written in and as its
value's source alike, and the page draws no control for one. A plugin built before the seam
declares none and is asked nothing.

## The line

Each line is one JSON object, the envelope: an `id`, and exactly one of

| Member | From | What it holds |
|---|---|---|
| `request` | the host | a call: one member named by the call and holding its fields (`{"card_kinds.slot":{"kind":"widget","card":{…}}}`) |
| `cancel` | the host | `{}`: the call of that id is given up and its answer, if one comes, is dropped |
| `answered` | the plugin | the answer to the call of that id: `{"answer":…}` or `{"refused":"…"}` |

beside an `answered`, `us` is the plugin's own time for the call, in microseconds: under the SDK its
request read, the call made and its answer written. A member the reader does not know is ignored. A
line with none of the three, or with two, is refused by its id; a line that is not an envelope (not
JSON, or not UTF-8) is skipped, and logged by the host. The host reads `answered` raw until its `id`
names the call, and reads it as that call's answer then, so an answer is never read as another
call's.

A line is at most 16 MiB, its newline aside (`symphony_plugin::protocol::LINE_MAX`), read as bytes
up to that bound and never past it. Neither side writes a longer one: the host refuses such a
request in the call's place, naming its length, and sends nothing; the SDK answers such an answer
`{"refused":"its answer is a line of N bytes, longer than the protocol's 16777216"}`. A longer line
read ends the process that wrote it: the host ends a plugin's process for it, and the SDK's `serve`
stops reading and says so on stderr.

```json
{"id":7,"request":{"components.render":{"component":"report","page":{"root":"/p","doc_path":"docs/A.md","render":3,"artifacts":"artifacts"},"argument":"A-1","option":null}}}
{"id":7,"answered":{"answer":{"Ok":[{"type":"paragraph","spans":[{"type":"text","text":"A-1"}]}]}},"us":41.5}
```

The SDK answers on 4 workers, so a slow call holds one and the others answer meanwhile, each
answer written when it is ready, in any order: the host hands each to its call by its id. A panic in
a call is answered `{"refused":"NAME panicked: …"}` and the process goes on. A `cancel` reaches the
SDK and changes nothing there: the call runs to its end and its answer is dropped by the host.

A page's components share what they load once for a render (`Page::once`) in the plugin as they do
linked: every request one `Page` makes names its render (`render` in its page), and the SDK keeps
the loaded values of the 32 newest renders, so an older one asked again loads again. A render of 0
is a page of its own.

## Versions

The host names its protocol in the first request, `plugin.declare`
(`{"protocol":{"major":0,"minor":5}}`), and the plugin its own in its declaration: its name, each
seam object's name and what it says of itself, its navigation and theme. They speak the lower minor
of their major. A plugin of another major is refused by name, saying to install a Symphony that
speaks the plugin's major or to rebuild the plugin with a `symphony-plugin` of the host's, and its
process is killed and not started again until its program changes. The protocol is 0.5.

Each call names the minor it came in (`since` in `symphony_plugin::protocol`, listed in `MINORS`).
The host never sends a call newer than the minor the two agreed: the seam's own default answers it,
as for a plugin that does not implement that method, and a call the seam requires (a component's
`render`) is shown failed, naming the protocol the plugin speaks. Every call but two is 0.1's. 0.2
hands a page its project's artifacts folder (`artifacts` in its page, the checkout's `artifacts`
unless `[project] artifacts` names another), which a plugin of 0.1 reads past and one of 0.2 reads
as `artifacts` from a host of 0.1. 0.3 adds `card_kinds.read`, a register read's card kinds in one
request: a plugin of 0.2 is asked them card by card, and answers the same. 0.4 adds
`authorities.attest` ([below](#approval-authorities)) and the declaration's `authorities`, which a
plugin of 0.3 does not write and a host reads as none: no authority is asked of it. 0.5 adds
`describers.describe` ([above](#settings-describers)) and the declaration's `describers`, which a
plugin of 0.4 does not write and a host reads as none: no entry is asked of it.

- A field the reader does not know is ignored, wherever it stands: in a request's fields, in a card,
  in an answer, in the declaration. A newer peer's additions reach an older one unread.
- A field missing is refused, unless its type gives it a default (a gate's `by` or `at`, a viewer's
  `views`, a page's `render` or `artifacts`). A new field comes with a default, or with a new major.
  So that such a field is a new minor of the kit's crates too, every struct of
  `symphony_plugin::protocol` is `#[non_exhaustive]` and is made by its constructors, never by its
  fields: a request by its `new`; a page, a card, a task, a file or a problem by its `of`; a kind's
  answers of a card (`KindAnswers`) by its `of` from the kind; a declaration, and what it lists, by
  its `of` from a plugin (or from its `Default`, its fields set after); a version by its `new`; a
  line by `Envelope::asking`, `cancelling` or `answering`. A
  request's `new` takes every field its call has, so a field added to a call's own changes it: no
  call has a field a later minor added yet, and the first gives `calls!` a group of fields after
  the call's own that `new` leaves out and fills with their defaults, set on the request after.
  `Request` is `#[non_exhaustive]` as well, as a new call is a minor of the protocol and a new
  variant of it, and a match on it outside the kit ends with `_`. The protocol's other enums stay
  exhaustive: a variant of `Answered` or `Member` is a line an older peer cannot read, a new major
  of the protocol, and `Lined` never goes on the wire. The seam types' and the vocabulary's structs
  (`Card`, `Picture`, `TaskOffer`, `Submission`, `Theme`, `Image`, `Viewer`, `SettingsEntry`,
  `Setting`…) and a variant's own
  fields are not marked, as a plugin and its tests build them by their fields: a field added to one
  is a new major of the kit, whichever minor of the protocol carries it.
- **The vocabulary grows in a minor** ([its card](../workflow/RELEASES.md#kit-new-block-version)).
  A new kind of block, text span or media is a minor of the kit (1.1.0), never a new major: a
  plugin makes blocks and never has to list them, so one written for 1.0.0 keeps building. `Block`,
  `Span` and `Media` are `#[non_exhaustive]`, and so is every other list of the kit a minor may
  add to: the words a block carries (`CalloutKind`, `FigureSize`, `ContainerKind`, `InlineKind`,
  `VectorKind`, `CardState`, `CardGroup`, `ThreadAuthor`), a gate's `GateState`, a board field's
  `FieldKind`, a renderer's `Drawing` and a Settings entry's `SettingScope`, `SettingClass` and
  `SettingKind`. A match on one outside the kit ends with `_`, a plugin's too, and a variant is
  made as before. Two stay exhaustive: `FieldValue`, a number or text as JSON has them, which a
  field's `validate` tells apart, and `Reply`, which a consuming project's route test matches
  whole, so that a new kind of reply waits for a consumer release plan. The kit's own test holds
  the marks and names those two with their reasons (`crates/symphony-plugin/tests/vocabulary.rs`,
  a crate that links the kit alone and matches each list with `_`).

  The compiler does not refuse a build of this workspace in which a kind has no arm, as each
  match outside the kit ends with `_`, so tests do. `crates/symphony-api/tests/vocabulary_drawn.rs`
  reads each list from the kit's source and fails on a variant that a match on it in the server
  or the wire types does not name. The client is held by its types, written from the same lists:
  its switches end in a value of type `never` and its tables name every kind, so a kind it does
  not draw fails its type check, and `blocks.test.tsx` fails until the sample the wire types'
  example writes holds the kind and it is drawn. A kind a client does not know, sent by a newer
  server, is not a failed read: the React client lets it through with its fields unread and says
  it where it stands (`Unsupported document content: NAME`, `Unsupported NAME` in a line,
  `Unsupported media: NAME`), drawing the rest of the page. A word it does not know (a size, a
  tag, a shape) still fails the read, which the page says and tries again.

  A host reading a process plugin's answer also keeps the surrounding content: a block or span
  whose kind it does not know becomes the existing `Unknown` block or span, with the kind as its
  `tag` and empty `text`. Unknown media becomes that unsupported node in its containing block or
  span, tagged `media: NAME`. The unknown node's fields are unread; known nodes still require
  valid fields, and a missing or non-text kind, a duplicate known field or a malformed answer is
  refused. This applies to components' blocks and renderers' block drawings, including their
  nested content, without adding a wire type.
- A call the plugin does not know, a request missing a field, or a second member beside the call is
  answered `{"refused":"…"}`, naming what could not be read, and the host shows that in the call's
  place.
- An answer the host cannot read, such as one holding a block type it does not know, is refused
  whole, and the host shows the error in the call's place: a component's in its place in the page.
- A request names no plugin: one process speaks for one plugin, and a project whose
  `[plugins] crates` lists several has a process for each.

## The shape

The wire's forms are chosen so nothing depends on the order of an object's members, which JSON
leaves free and a writer that sorts its keys (`jq -S`, a map in another language, a replay file
edited by hand) changes. The server's JSON keeps numbers exact (the symphony library's `serde_json`
feature `arbitrary_precision`), and so cannot read a fractional number back through serde's
buffered content, which holds a member until the one telling its type is read. So the envelope's
members are optional fields, never a `flatten`, an adjacent or an internal tag; a request is one
member, its call, and its answer one member, `answer` or `refused`; nothing uses serde's `flatten`,
which buffers too. A test writes every call's request and answer, alone and in its envelope, with
its members sorted and reversed, at every depth, and reads each back the same.

A board field's value goes bare (`3.5` or `"high"`), as the board's own `FieldValue`, whose reader
takes a number however serde hands it, that buffer's included. That leans on `serde_json`'s private
form of a number under `arbitrary_precision`: a map whose one key is `$serde_json::private::Number`,
its value the number's text, which `crates/symphony-blocks/src/field_value.rs` reads by that name. A
`serde_json` that renamed the key would break the reading, and that file's tests would say so.

Three types of the block vocabulary are read through that buffer, as they are tagged among their
fields: `Block` and `Span` by `type`, `Media` by `kind`. They read back in any order because they
hold no fractional number, only integers, which the buffer keeps; each derives `Eq`, which no float
field can, so a fractional field added to one stops its build before it reaches the wire, and a test
holds both.

## Measured

Under `[plugins] timing` ([PLUGIN_CALLS](PLUGIN_CALLS.md)) each call of a plugin's process writes
its line to the project's record, `path` `process`: `us` the plugin's own time as it says it (its
JSON both ways and the call), `round_trip_us` the host's whole wait (its line built and written,
the answer read back), the two lines' bytes, and `failed` with why when the call had no answer. A
register read's batch is one line for each request, `card_kinds` `read`, its `name` the kinds'
names. The declaration is the start's and is not recorded.

In a Symphony checkout, what a call costs linked and from a process, for its test plugin's
components and card kind, as the median of a thousand interleaved rounds: each call's row,
components.render among them, and the range of what a call costs from the process more than linked
over them all. A call of a known time (the test plugin's `slow`, spending 500 µs in the
plugin) shows each meter moves by it, and the test fails if one does not. Then a card's eight
card-kind calls summed, and a page of ten components drawn whole, linked and from a process, neither
page recorded. A second test reads registers of 50 and 500 cards linked, from a process card by
card and from a process in one request, each read checked against the linked one, and prints the
batch's lines ([above](#card-kinds-in-one-request)). Each is a measurement, not a check: a process
call waits on two wake-ups across threads and processes and a linked one on none, so a loaded
machine inflates the difference by an amount nobody knows. Take it on a quiet machine, in a release
build, twice, with the load average before and after each run:

```sh
uptime
just test-rs --release -p symphony-app --test project the_cost_of -- --ignored --nocapture --test-threads=1
uptime
just test-rs --release -p symphony-app --test project the_cost_of -- --ignored --nocapture --test-threads=1
uptime
```
