# Plugin protocol

A plugin can answer from a process of its own instead of being linked into the host: the host
(the server, the `symphony` command) starts the plugin's binary and speaks with it in JSON lines on
its stdin and stdout. A project chooses per plugin. Replacing such a plugin is replacing its binary,
with no rebuild of the server. The requests and answers are the serde types of
`symphony_plugin::protocol`, the ones the loopback sends inside the process
([PLUGIN_CALLS](PLUGIN_CALLS.md)). The plugin's side is `symphony_plugin::serve`
(`crates/symphony-plugin/src/serve.rs`) and the host's is `crates/symphony/src/plugins/process.rs`.

## Turning it on

In the project's `symphony.toml`, name the plugin in `[plugins] crates` as a linked one is named,
and give the command that starts it in `[plugins.process]`:

```toml
[plugins]
crates = ["the-project-plugin"]

[plugins.process]
the-project-plugin = ["bin/the-project-plugin", "--quiet"]
```

The first word is the program. A relative path holding a `/` is under the project's checkout, its
`.` parts folded; a bare name is looked up on `PATH`. A name `[plugins] crates` does not list, or a
command with no program, is refused by name. A plugin with no command here is linked, as before.
`symphony-status --lint` names a plugin whose process does not start or does not declare.

A plugin's binary is its `main` calling the SDK with the plugin it links:

```rust
fn main() -> std::process::ExitCode {
    symphony_plugin::serve(&the_project_plugin::PLUGIN)
}
```

What answers from a process here is a plugin's components and its card kinds. Its other seams
(renderers, routes, home regions, board actions and fields, gates, watches, doc checks, prose rules,
generators, commands, transports, navigation and theme) are not asked of a process: a plugin with
any of them stays linked.

## Its process

The host starts a plugin's process at the first call made of it: in the project's checkout, without
the session that started the host, its stdin and stdout the protocol's, its stderr appended to its
log, `plugins/NAME.log` in the project's artifacts folder (`artifacts/` unless `[project] artifacts`
names another). The host writes a line there, stamped, for each start (`started PID: COMMAND`), each
end (`its process PID exited with code 3`, `… was killed by signal 9`), each call that had no answer
and each line of the plugin's that answers no call. One host process keeps one process for each
plugin of each project, for its life; the plugin's stdin ends when the host does, and the SDK's
`serve` returns then.

The first request is the declaration (below). A call then waits 10 s for its answer; one past it is
given up, sent a `cancel`, and shown as failed. A process that ends fails every call waiting on it
with how it ended, and the first call 1 s after starts it again: at most 5 starts in 180 s, after
which it stays down until the oldest start leaves that window. What it declared at its first start
stands for it until the host ends: a declaration that changed is noted in the log.

A call that has no answer (the process down or restarting, no answer in time, a refusal, an answer
that does not read) is shown where its answer would go: a component's in its place on the page, as
`⚠ plugin NAME: its process 4242 was killed by signal 9; it starts again in a moment`, and a card
kind's as the kind's default, as if the plugin said nothing of the card.

A plugin's command (`symphony NAME ARGS…`) does not go on the pipe: the plugin's binary runs it as
`PROGRAM command NAME ARGS…`, with streams of its own and the command's exit code as its own (its
low byte, as an exit status holds it). `commands.run` is the loopback's alone.

## The line

Each line is one JSON object, the envelope: an `id`, and exactly one of

| Member | From | What it holds |
|---|---|---|
| `request` | the host | a call: one member named by the call and holding its fields (`{"card_kinds.slot":{"kind":"hull","card":{…}}}`) |
| `cancel` | the host | `{}`: the call of that id is given up and its answer, if one comes, is dropped |
| `answered` | the plugin | the answer to the call of that id: `{"answer":…}` or `{"refused":"…"}` |

beside an `answered`, `us` is the plugin's own time for the call, in microseconds: under the SDK its
request read, the call made and its answer written. A member the reader does not know is ignored. A line with none of the three, or with two, is refused by its id; a
line that is not an envelope is skipped. The host reads `answered` raw until its `id` names the call,
and reads it as that call's answer then, so an answer is never read as another call's.

```json
{"id":7,"request":{"components.render":{"component":"hull","page":{"root":"/p","doc_path":"docs/A.md","render":3},"argument":"A-1","option":null}}}
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
(`{"protocol":{"major":0,"minor":1}}`), and the plugin its own in its declaration: its name, each
seam object's name and what it says of itself, its navigation and theme. They speak the lower minor
of their major. A plugin of another major is refused by name, saying to install a Symphony that
speaks the plugin's major or to rebuild the plugin with a `symphony-plugin` of the host's, and its
process is left to end. The protocol is 0.1.

Each call names the minor it came in (`since` in `symphony_plugin::protocol`, listed in `MINORS`).
The host never sends a call newer than the minor the two agreed: the seam's own default answers it,
as for a plugin that does not implement that method, and a call the seam requires (a component's
`render`) is shown failed, naming the protocol the plugin speaks. Every call is 0.1's.

- A field the reader does not know is ignored, wherever it stands: in a request's fields, in a card,
  in an answer, in the declaration. A newer peer's additions reach an older one unread.
- A field missing is refused, unless its type gives it a default (a gate's `by` or `at`, a viewer's
  `views`, a page's `render`). A new field comes with a default, or with a new major.
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
its value the number's text, which `crates/symphony-api/src/field_value.rs` reads by that name. A
`serde_json` that renamed the key would break the reading, and that file's tests would say so.

Three types of the block vocabulary are read through that buffer, as they are tagged among their
fields: `Block` and `Span` by `type`, `Media` by `kind`. They read back in any order because they
hold no fractional number, only integers, which the buffer keeps; each derives `Eq`, which no float
field can, so a fractional field added to one stops its build before it reaches the wire, and a test
holds both.

## Measured

Under `[plugins] timing` or `loopback` ([PLUGIN_CALLS](PLUGIN_CALLS.md)) each call of a plugin's
process writes its line to the project's record, `path` `process`: `us` the plugin's own time as it
says it (its JSON both ways and the call), `round_trip_us` the host's whole wait (its line built and written, the answer read back),
the two lines' bytes, and `failed` with why when the call had no answer. The declaration is the
start's and is not recorded.

In a Symphony checkout, what a call costs linked, through the loopback and from a process, for its
test plugin's component and card kind, as the median of a thousand rounds, and a page of ten
components drawn whole: a measurement, not a check, best taken in a release build with the load
average beside it:

```sh
uptime
tools/cargo.sh test --locked --release -p symphony --test project the_cost_of_a_plugins_calls -- --ignored --nocapture
```
