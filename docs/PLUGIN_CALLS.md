# Plugin calls

A project can have every call into its plugins timed, and sent through a JSON round trip inside
the process: what a plugin protocol's seam would cost, measured on today's linked plugins before
any process boundary exists. Two settings turn it on, both off by default; the record they write
is one JSON line per call, under the project's artifacts folder. The protocol's requests and
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
consuming project. Leave neither setting committed: the record grows with every call.

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
not timed: a protocol sends it once, as `plugin.declare`, and the loopback records that once per
process. The Symphony's own `task` component is no plugin's and is not measured.

## The record

`plugin-calls.jsonl` in the project's artifacts folder (`artifacts/` unless `[project] artifacts`
names another), appended to by every process that measures, the server and the `symphony` command
alike, each line in one write. Removing it starts a new record: a process opens it again when it is
gone.

| Field | What it holds |
|---|---|
| `at_ms` | when the line was written, in milliseconds since the epoch |
| `pid` | the process that made the call |
| `plugin` | the plugin's name, as `[plugins] crates` names it |
| `seam`, `call` | the seam and the call, as the two tables above name them (`card_kinds`, `slot`) |
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
{"at_ms":1762941600124,"pid":4242,"plugin":"game","seam":"card_kinds","name":"hull","call":"slot","us":0.398,"round_trip_us":14.215,"request_bytes":612,"answer_bytes":15}
```

## The loopback

Each call goes as a protocol would send it: the request built from the call's arguments
(`{"call":"card_kinds.slot","args":{"kind":"hull","card":{…}}}`), written as JSON and read back,
the plugin called with what was read, and its answer written and read back. What came back is the
answer the seam gives, so the server runs on what crossed JSON. `round_trip_us` less `us` is the
JSON's cost; the comparisons that tell a value lost are left out of both times.

A value that does not survive is named in `lost`, and once a process on stderr for each call. A
request that does not read back leaves that call linked, with no round trip timed; an answer that
does not read back leaves the plugin's own answer; one that reads back otherwise is answered as it
came back. JSON has no infinity and no NaN: such a number is written as `null`, so a board field
valued at one does not read back, and a gate's time that is one reads back as no time. A path that
is not UTF-8 cannot be written.

The server's JSON keeps numbers exact (the symphony library's `serde_json` feature
`arbitrary_precision`), and so cannot read a fractional number back through serde's buffered
content, which an untagged or internally tagged type is read through. A request therefore names its
call beside its `args`, the call first, and a board field's value goes tagged by its kind
(`{"Number":3.5}`, `symphony_plugin::protocol::FieldData`) where the board's own untagged value
would not read `3.5` back.

Two things a page or a check shares with the server stay the server's on the plugin's side, as a
protocol's plugin keeps its own for one render or one check: the values a page's components load
once for the render (`Page::once`), and the reading of `docs/` a doc check is handed. A request
names them by the page's root and document and by the project's root.

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
