# Plugin calls

A project can have every call into its plugins timed. One setting turns it on, off by default;
the record it writes is one JSON line per call, under the project's artifacts folder, and a plugin
answering from a process of its own ([PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md)) writes its calls there
too. Each call is named as the protocol names it (`symphony_plugin::protocol`); the measure is
`crates/symphony/src/plugins/measure.rs`.

## Turning it on

In the project's `symphony.toml`:

```toml
[plugins]
crates = ["the-project-plugin"]
timing = true      # each call's time
```

`timing` records each call's time. The server and the `symphony` command read the file as they
read it everywhere, again whenever it changes, so the next call after an edit is measured and the
one after the setting is removed is not: no restart, and nothing changes in a host or a consuming
project. A plugin's own command (`symphony NAME`) reads the setting alone and quietly before it
runs: with it off it loads no project and says nothing of the file, as before the setting existed;
with it on, the project is loaded as the server loads it, and a key this build does not know is
named on stderr. Leave it uncommitted: every call writes a line.

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
not timed: a plugin's process sends it once, as `plugin.declare`, at its start, which is not
recorded. The Symphony's own `task` component is no plugin's and is not measured. Every method of
`Plugin` and of each seam's trait is a call above or a part of the declaration, and the measure
forwards each: a test reads the traits' methods from their source and fails on one the protocol or
the measure lacks. One call is no method: `card_kinds.read`, the batch a register read asks a
plugin's process in one request (`BATCHES` in `symphony_plugin::protocol`,
[PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md#card-kinds-in-one-request)), recorded as the process's call,
`name` its kinds' names; a linked plugin is asked each card's calls.

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
| `path` | how the call went: `linked` (timed in the process that made it) or `process` (a plugin's own process) |
| `seam`, `call` | the seam and the call, as the table above names them (`card_kinds`, `slot`) |
| `name` | the seam's object: its name, a route's path, a home region's id, a watch's paths |
| `us` | the plugin's own call, in microseconds to the nanosecond; from a process, as the plugin says it (under the SDK its request read, the call and its answer written), and absent when the call had no answer |
| `round_trip_us` | from a process, the host's whole wait: its line built and written, the answer read back |
| `request_bytes`, `answer_bytes` | from a process, each line's length |
| `failed` | from a process, why the call had no answer: the process down, no answer in time, a refusal |

A line timed linked:

```json
{"at_ms":1762941600123,"pid":4242,"plugin":"game","path":"linked","seam":"card_kinds","name":"widget","call":"slot","us":0.412}
```

And answered by the plugin's own process:

```json
{"at_ms":1762941600125,"pid":4242,"plugin":"game","path":"process","seam":"card_kinds","name":"widget","call":"slot","us":1.2,"round_trip_us":61.5,"request_bytes":631,"answer_bytes":47}
```

## Reading it

Each call's count, total and median time:

```sh
jq -s 'group_by(.seam + "." + .call) | map({
  call: (.[0].seam + "." + .[0].call),
  n: length,
  us: (map(.us) | add),
  median_us: (map(.us) | sort | .[length / 2 | floor]),
  round_trip_us: (map(.round_trip_us // .us) | add)
})' artifacts/plugin-calls.jsonl
```

One process's calls, as a single page render or register read in the server, by its `pid` and the
`at_ms` range of the read:

```sh
jq -c 'select(.pid == 4242 and .at_ms >= 1762941600000)' artifacts/plugin-calls.jsonl
```

A rolled record reads the same: name both files, the roll first
(`artifacts/plugin-calls.1.jsonl artifacts/plugin-calls.jsonl`).

In a Symphony checkout, what each call of its test plugin with an object at every seam costs,
timed linked, as the median of a thousand rounds: a measurement, not a check, best taken in a
release build on a machine nothing else loads, with its load average beside it:

```sh
uptime
tools/cargo.sh test --locked --release -p symphony-app --test project the_cost_of_each -- --ignored --nocapture
```

The same for a component's and a card kind's calls linked and from the plugin's own process is
[PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md#measured)'s.
