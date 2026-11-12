# Plugin calls

A project can have every call into its plugins timed, and sent through a JSON round trip inside
the process: JSON's share of what a plugin protocol's seam would cost, measured on today's linked
plugins before any process boundary exists. Two settings turn it on, both off by default; the
record they write is one JSON line per call, under the project's artifacts folder. The protocol's
requests and answers are the serde types of `symphony_plugin::protocol`; the measure is
`crates/symphony/src/plugins/measure.rs`.

## Turning it on

In the project's `symphony.toml`:

```toml
[plugins]
crates = ["the-project-plugin"]
timing = true      # each call's time
loopback = true    # each call through JSON as well, both times recorded
```

`timing` records each call's time. `loopback` sends each call through JSON and records both times;
it records whether `timing` is on or not. The server and the `symphony` command read the file as
they read it everywhere, again whenever it changes, so the next call after an edit is measured and
the one after the setting is removed is not: no restart, and nothing changes in a host or a
consuming project. A plugin's own command (`symphony NAME`) reads the two settings alone and
quietly before it runs: with both off it loads no project and says nothing of the file, as before
either setting existed; with one on, the project is loaded as the server loads it, and a key this
build does not know is named on stderr. Leave neither setting committed: every call writes a line.

What is measured is every call into a seam's object made through a project's plugins
(`Plugins::of`, `Plugins::at`):

| Seam | Calls |
|---|---|
| `components` | `render`, `resolve`, `cache_version` |
| `card_kinds` | `recognises`, `pictures`, `slot`, `triage_category`, `auto_approvable`, `gates`, `approval_needs_agent`, `chosen` |
| `renderers` | `draw` |
| `routes` | `serve` |
| `home_regions` | `region` |
| `board_actions` | `offer`, `submit` |
| `board_fields` | `validate` |
| `gates` | `answer`, `kept_while` |
| `watches` | `changed`, `forget` |
| `doc_checks` | `check`, `components` |
| `prose_rules` | `check` |
| `commands` | `run`, measured as the project at the folder the `symphony` command runs in says |
| `transports` | `register`, `deliver`, `alive` |

What a plugin declares (its objects' names, a card kind's markers, a renderer's extensions, a
route's path, a watch's paths, its navigation, generators and theme) is read from the plugin and
not timed: a protocol sends it once, as `plugin.declare`, and the loopback records that once for
each plugin, record and setting in a process. The Symphony's own `task` component is no plugin's
and is not measured. Every method of `Plugin` and of each seam's trait is a call above or a part of
the declaration, and the measure forwards each: a test reads the traits' methods from their source
and fails on one the protocol or the measure lacks.

## The record

`plugin-calls.jsonl` in the project's artifacts folder (`artifacts/` unless `[project] artifacts`
names another), appended to by every process that measures, the server and the `symphony` command
alike, each line in one write. When it reaches 32 MiB it is rolled: renamed to
`plugin-calls.1.jsonl`, replacing the one rolled before, and begun again, so the two hold 64 MiB at
most. Each write goes to the file the path names at that moment: removing the record starts a new
one, and a record another process removed and made again, or rolled, is the one written to, not the
file this process held.

| Field | What it holds |
|---|---|
| `at_ms` | when the line was written, in milliseconds since the epoch |
| `pid` | the process that made the call |
| `plugin` | the plugin's name, as `[plugins] crates` names it |
| `seam`, `call` | the seam and the call, as the table above names them (`card_kinds`, `slot`) |
| `name` | the seam's object: its name, a route's path, a home region's id, a watch's paths |
| `us` | the plugin's own call, in microseconds to the nanosecond |
| `round_trip_us` | under the loopback, the whole call: the request built, written and read back, the call, its answer written and read back |
| `request_bytes`, `answer_bytes` | under the loopback, the JSON request's and answer's length |
| `lost` | under the loopback, what did not survive JSON; absent when everything did |

A line timed alone:

```json
{"at_ms":1762941600123,"pid":4242,"plugin":"game","seam":"card_kinds","name":"hull","call":"slot","us":0.412}
```

The same call through the loopback:

```json
{"at_ms":1762941600124,"pid":4242,"plugin":"game","seam":"card_kinds","name":"hull","call":"slot","us":0.398,"round_trip_us":14.215,"request_bytes":612,"answer_bytes":26}
```

## The loopback

Each call goes as a protocol would send it: the request built from the call's arguments, one member
named by its call and holding its fields (`{"card_kinds.slot":{"kind":"hull","card":{…}}}`),
written as JSON and read back, the plugin called with what was read, and its answer written and read
back as `{"answer":…}`. What came back is the answer the seam gives, so the server runs on what
crossed JSON. `round_trip_us` less `us` is the JSON's cost; the comparisons that tell a value lost
are left out of both times.

A value that does not survive is named in `lost`, and once on stderr for each call, plugin, record
and setting in a process. A request that does not read back leaves that call linked, with no round
trip timed; an answer that does not read back leaves the plugin's own answer; one that reads back
otherwise is answered as it came back; when both the request and the answer lose something, both
are named. JSON has no infinity and no NaN: such a number is written as `null`, so a board field
valued at one does not read back, and a gate's time that is one reads back as no time. A path that
is not UTF-8 cannot be written.

What it does not measure, so its numbers are JSON's share and not a protocol's whole cost:

- the pipe: no write to another process, no context switch, no wait for a worker of the plugin's
  pool, no request id;
- a plugin command's start: under a protocol `symphony NAME` runs the plugin's binary, whose start
  is the cost; the loopback sends the command's arguments and exit code through JSON and passes its
  streams through as they are;
- a plugin's own reading: the values a page's components load once for the render (`Page::once`)
  and the reading of `docs/` a doc check is handed stay the server's on the plugin's side, where a
  plugin's process would load and read its own. A request names them by the page's root and
  document and by the project's root.

What it measures more than the design's protocol would: each card kind's call sends the whole card
again, its text and its thread (about 320 bytes for the test plugin's card, sent with each of a
card's eight calls), where the protocol sends a register read's changed cards once and has each
answered whole.

## The protocol's shape

The wire's forms are chosen so nothing depends on the order of an object's members, which JSON
leaves free and a writer that sorts its keys (`jq -S`, a map in another language, a replay file
edited by hand) changes. The server's JSON keeps numbers exact (the symphony library's `serde_json`
feature `arbitrary_precision`), and so cannot read a fractional number back through serde's
buffered content, which holds a member until the one telling its type is read. So a request is one
member, its call, and its answer one member, `answer` or `refused`; a board field's value goes
tagged by its kind (`{"Number":3.5}`, `symphony_plugin::protocol::FieldData`) where the board's own
untagged value would not read `3.5` back; and nothing uses serde's `flatten`, which buffers too. A
test writes every call's request and answer with its members sorted and reversed, at every depth,
and reads each back the same.

Three types of the block vocabulary are read through that buffer, as they are tagged among their
fields: `Block` and `Span` by `type`, `Media` by `kind`. They read back in any order because they
hold no fractional number, only integers, which the buffer keeps; each derives `Eq`, which no float
field can, so a fractional field added to one stops its build before it reaches the wire, and a test
holds both.

Versions and unknowns:

- The host names its protocol in the first request, `plugin.declare` (`{"protocol":{"major":0,"minor":1}}`),
  and the plugin its own in its declaration. They speak the lower minor of their major; a plugin of
  another major is refused by name, saying to install a Symphony that speaks the plugin's major or
  to rebuild the plugin with a `symphony-plugin` of the host's. The protocol is 0.1 until a process
  speaks it.
- A field the reader does not know is ignored, wherever it stands: in a request's fields, in a card,
  in an answer, in the declaration. A newer peer's additions reach an older one unread.
- A field missing is refused, unless its type gives it a default (a gate's `by` or `at`, a viewer's
  `views`). A new field comes with a default, or with a new major.
- A call the plugin does not know, a request missing a field, or a second member beside the call is
  answered `{"refused":"…"}`, naming what could not be read, and the host shows that in the call's
  place.
- An answer the host cannot read, such as one holding a block type it does not know, is refused
  whole, and the host shows the error in the call's place: a component's in its place in the page.
- A request names no plugin: one process speaks for one plugin, and a project whose
  `[plugins] crates` lists several has a process for each.

## Reading it

Each call's count, total and median time, linked and through the loopback:

```sh
jq -s 'group_by(.seam + "." + .call) | map({
  call: (.[0].seam + "." + .[0].call),
  n: length,
  us: (map(.us) | add),
  median_us: (map(.us) | sort | .[length / 2 | floor]),
  round_trip_us: (map(.round_trip_us // .us) | add)
})' artifacts/plugin-calls.jsonl
```

The calls whose values did not survive JSON:

```sh
jq -c 'select(.lost) | {plugin, seam, name, call, lost}' artifacts/plugin-calls.jsonl
```

One process's calls, as a single page render or register read in the server, by its `pid` and the
`at_ms` range of the read:

```sh
jq -c 'select(.pid == 4242 and .at_ms >= 1762941600000)' artifacts/plugin-calls.jsonl
```

A rolled record reads the same: name both files, the roll first
(`artifacts/plugin-calls.1.jsonl artifacts/plugin-calls.jsonl`).

What each call of a test plugin with an object at every seam costs, timed alone and through the
loopback, as the median of a thousand rounds: a measurement, not a check, best taken in a release
build on a machine nothing else loads, with its load average beside it:

```sh
uptime
just test-rs --release -p symphony --test project the_cost_of_each -- --ignored --nocapture
```
