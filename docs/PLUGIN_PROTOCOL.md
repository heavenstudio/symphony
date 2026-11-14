# Plugin protocol

A plugin can answer from a process of its own instead of being linked into the host: the host
(the server, the `symphony` command) starts the plugin's binary and speaks with it in JSON lines on
its stdin and stdout. A project chooses per plugin. Replacing such a plugin is replacing its binary,
with no rebuild of the server. The requests and answers are the serde types of
`symphony_plugin::protocol`, the ones the loopback sends inside the process
([PLUGIN_CALLS](PLUGIN_CALLS.md)). The plugin's side is `symphony_plugin::serve`
(`crates/symphony-plugin/src/serve.rs`) and the host's is `crates/symphony/src/plugins/process.rs`.

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
on Unix (its device, inode and change time). They carry one version, 1.0.0 now, and are made to be
published on crates.io ([RELEASES](RELEASES.md#packages-on-cratesio)), so they export what a plugin
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

Card kinds from a process cost about eight round trips per card for each read of the register (one
for each call a card kind answers of a card), until the register's reads are batched into one request
a read. No consumer turns `[plugins.process]` on for a plugin with card kinds before then;
components cost one round trip each.

A plugin's binary is its `main` calling the SDK with the plugin it links:

```rust
fn main() -> std::process::ExitCode {
    symphony_plugin::serve(&the_project_plugin::PLUGIN)
}
```

What answers from a process here is a plugin's components and its card kinds. Its other seams
(renderers, routes, home regions, board actions and fields, gates, watches, doc checks, prose rules,
generators, commands, transports, navigation and theme) are not asked of a process: a plugin that
declares any of them is refused under `[plugins.process]`, naming them, rather than losing them
without a word, and stays a linked plugin's to answer. So is a plugin that declares another name
than the one `[plugins.process]` gives it.

## Its process

The host starts a plugin's process at the first call made of it: in the project's checkout, without
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

The first request is the declaration (below). A call then waits 10 s for its answer, the writing of
its request included: the host's lines go to a writer thread through a queue of 256, so a plugin that
stops reading holds no call past its wait. A call whose answer does not come in the wait is given
up, sent a `cancel`, and shown as failed; a call whose request the plugin did not even read in the
wait ends the process. The host ends a process (closes its stdin and kills it) when:

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
`⚠ plugin NAME: its process 4242 was killed by signal 9; it starts again in a moment`, and a card
kind's as the kind's default, as if the plugin said nothing of the card.

A plugin's commands do not go on the pipe, and the host does not run them: `symphony NAME ARGS…`
runs a linked plugin's command alone, so a plugin with commands is refused under
`[plugins.process]`. The SDK's binary still runs one for whoever starts it as `PROGRAM command NAME
ARGS…`, with streams of its own and the command's exit code as its own (its low byte, as an exit
status holds it). `commands.run` is the loopback's alone.

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
(`{"protocol":{"major":0,"minor":2}}`), and the plugin its own in its declaration: its name, each
seam object's name and what it says of itself, its navigation and theme. They speak the lower minor
of their major. A plugin of another major is refused by name, saying to install a Symphony that
speaks the plugin's major or to rebuild the plugin with a `symphony-plugin` of the host's, and its
process is killed and not started again until its program changes. The protocol is 0.2.

Each call names the minor it came in (`since` in `symphony_plugin::protocol`, listed in `MINORS`).
The host never sends a call newer than the minor the two agreed: the seam's own default answers it,
as for a plugin that does not implement that method, and a call the seam requires (a component's
`render`) is shown failed, naming the protocol the plugin speaks. Every call is 0.1's. 0.2 hands a
page its project's artifacts folder (`artifacts` in its page, the checkout's `artifacts` unless
`[project] artifacts` names another), which a plugin of 0.1 reads past and one of 0.2 reads as
`artifacts` from a host of 0.1.

- A field the reader does not know is ignored, wherever it stands: in a request's fields, in a card,
  in an answer, in the declaration. A newer peer's additions reach an older one unread.
- A field missing is refused, unless its type gives it a default (a gate's `by` or `at`, a viewer's
  `views`, a page's `render` or `artifacts`). A new field comes with a default, or with a new major.
  So that such a field is a new minor of the kit's crates too, every struct of
  `symphony_plugin::protocol` is `#[non_exhaustive]` and is made by its constructors, never by its
  fields: a request by its `new`; a page, a card, a task, a file or a problem by its `of`; a
  declaration, and what it lists, by its `of` from a plugin (or from its `Default`, its fields set
  after); a version by its `new`; a line by `Envelope::asking`, `cancelling` or `answering`. A
  request's `new` takes every field its call has, so a field added to a call's own changes it: no
  call has a field a later minor added yet, and the first gives `calls!` a group of fields after
  the call's own that `new` leaves out and fills with their defaults, set on the request after.
  `Request` is `#[non_exhaustive]` as well, as a new call is a minor of the protocol and a new
  variant of it, and a match on it outside the kit ends with `_`. The protocol's other enums stay
  exhaustive: a variant of `Answered` or `Member` is a line an older peer cannot read, a new major
  of the protocol, and `Lined` never goes on the wire. The seam types and the vocabulary (`Card`,
  `Picture`, `TaskOffer`, `Submission`, `Theme`, `Block`…) are not marked, as a plugin and its
  tests build them by their fields: a field or a variant added to one is a new major of the kit,
  whichever minor of the protocol carries it.
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

Under `[plugins] timing` or `loopback` ([PLUGIN_CALLS](PLUGIN_CALLS.md)) each call of a plugin's
process writes its line to the project's record, `path` `process`: `us` the plugin's own time as it
says it (its JSON both ways and the call), `round_trip_us` the host's whole wait (its line built
and written, the answer read back), the two lines' bytes, and `failed` with why when the call had
no answer. The declaration is the start's and is not recorded.

In a Symphony checkout, what a call costs linked, through the loopback and from a process, for its
test plugin's components and card kind, as the median of a thousand interleaved rounds: each call's
row, components.render among them, and the range of what a call costs from the process more than
linked over them all. A call of a known time (the test plugin's `slow`, spending 500 µs in the
plugin) shows each meter moves by it, and the test fails if one does not. Then a card's eight
card-kind calls summed, from which a register read is an extrapolation and not a measured read, and
a page of ten components drawn whole, linked and from a process, neither page recorded. It is a
measurement, not a check: a process call waits on two wake-ups across threads and processes and a
linked one on none, so a loaded machine inflates the difference by an amount nobody knows. Take it on
a quiet machine, in a release build, twice, with the load average before and after each run:

```sh
uptime
just test-rs --release -p symphony-app --test project the_cost_of -- --ignored --nocapture --test-threads=1
uptime
just test-rs --release -p symphony-app --test project the_cost_of -- --ignored --nocapture --test-threads=1
uptime
```
