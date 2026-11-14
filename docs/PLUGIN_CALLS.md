# Plugin calls

A project can have every call into its plugins timed, and sent through a JSON round trip inside
the process: JSON's share of what a plugin protocol's seam costs, measured on a linked plugin. Two
settings turn it on, both off by default; the record they write is one JSON line per call, under
the project's artifacts folder, and a plugin answering from a process of its own
([PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md)) writes its calls there too. The protocol's requests and
answers are the serde types of `symphony_plugin::protocol`; the measure is
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
and fails on one the protocol or the measure lacks. One call is no method: `card_kinds.read`, the
batch a register read asks a plugin's process in one request (`BATCHES` in
`symphony_plugin::protocol`, [PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md#card-kinds-in-one-request)),
recorded as the process's call, `name` its kinds' names; a linked plugin, measured or under the
loopback, is asked each card's calls.

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
| `path` | how the call went: `linked` (timed alone), `loopback`, or `process` (a plugin's own process) |
| `seam`, `call` | the seam and the call, as the table above names them (`card_kinds`, `slot`) |
| `name` | the seam's object: its name, a route's path, a home region's id, a watch's paths |
| `us` | the plugin's own call, in microseconds to the nanosecond; from a process, as the plugin says it (under the SDK its request read, the call and its answer written), and absent when the call had no answer |
| `round_trip_us` | under the loopback, the whole call: the request built, written and read back, the call, its answer written and read back; from a process, the host's whole wait: its line built and written, the answer read back |
| `request_bytes`, `answer_bytes` | under the loopback, the JSON request's and answer's length; from a process, each line's |
| `lost` | under the loopback, what did not survive JSON; absent when everything did |
| `failed` | from a process, why the call had no answer: the process down, no answer in time, a refusal |

A line timed alone:

```json
{"at_ms":1762941600123,"pid":4242,"plugin":"game","path":"linked","seam":"card_kinds","name":"widget","call":"slot","us":0.412}
```

The same call through the loopback:

```json
{"at_ms":1762941600124,"pid":4242,"plugin":"game","path":"loopback","seam":"card_kinds","name":"widget","call":"slot","us":0.398,"round_trip_us":14.215,"request_bytes":612,"answer_bytes":26}
```

And answered by the plugin's own process:

```json
{"at_ms":1762941600125,"pid":4242,"plugin":"game","path":"process","seam":"card_kinds","name":"widget","call":"slot","us":1.2,"round_trip_us":61.5,"request_bytes":631,"answer_bytes":47}
```

## The loopback

Each call goes as a protocol would send it: the request built from the call's arguments, one member
named by its call and holding its fields (`{"card_kinds.slot":{"kind":"widget","card":{…}}}`),
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

What it does not measure, so its numbers are JSON's share and not a protocol's whole cost (a
plugin's own process measures the rest, `path` `process`):

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

## The protocol

The wire the loopback writes is the one a plugin answering from a process of its own speaks: its
envelope, each call's minor, its shape and its versions are [PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md).

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

In a Symphony checkout, what each call of its test plugin with an object at every seam costs, timed
alone and through the loopback, as the median of a thousand rounds: a measurement, not a check,
best taken in a release build on a machine nothing else loads, with its load average beside it:

```sh
uptime
tools/cargo.sh test --locked --release -p symphony-app --test project the_cost_of_each -- --ignored --nocapture
```

The same for a component's and a card kind's calls linked, through the loopback and from the
plugin's own process is [PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md#measured)'s.
