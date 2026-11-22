#!/usr/bin/env bash
# What the fixture's machine recorded, for the pictures of Reports and Runs
# (tools/product-pictures.sh, which runs this in the fixture project's copy once its board is
# filled): the day files a shared server and the board's recorder would have written in the copy's
# git folder, each at a moment counted back from NOW, so the pages are the same on any day. A
# server of a project's own, as Reports' and Runs' pictures', writes none of them, so they stay as
# written here.
#
#   NOW=SECONDS tools/product-pictures/records.sh
#
# It writes, under `.git/symphony/`: twelve days of the hourly roll-up (`reports/`), of the agents'
# hours (`agents/`) and of the board's health (`health/`), the last three days' runs (`runs/`) with
# the logs of the run the run's picture shows, and on the default branch one commit that changes
# the project's settings, which every chart marks.
#
# A state the pictures lack is added here: a run, a step, a figure of an hour.
set -euo pipefail

: "${NOW:?the moment of the pictures, in seconds}"

state=".git/symphony"
mkdir -p "$state/reports" "$state/agents" "$state/health" "$state/runs"

# The settings change the charts mark, fourteen hours before NOW.
cat >>symphony.toml <<'TOML'

[limits]
machine_agents = 4
TOML
git add symphony.toml
GIT_AUTHOR_DATE="@$((NOW - 14 * 3600)) +0000" GIT_COMMITTER_DATE="@$((NOW - 14 * 3600)) +0000" \
    git -c user.name=Orchard -c user.email=orchard@example.invalid \
    commit --quiet --message "Four agents at once on this machine"

# The roll-up, the agents' hours and the board's health: an hour's figures follow from its place in
# the day and its number since 1970, so every run of this writes the same files.
jq -n -r --argjson now "$NOW" '
  def day_name: strftime("%Y-%m-%d");
  # How busy an hour of the day is, 0 to 3: the owner answers by day, the agents work on at night.
  def busy: [1,1,0,0,0,1,1,2,3,3,2,3,2,2,3,3,2,2,1,1,2,1,1,1][(. / 3600 | floor) % 24];
  def number: . / 3600 | floor;
  def tokens($role; $model; $calls):
    {role: $role, round: 0, model: $model, speed: "standard", input: ($calls * 310),
     cache_write_5m: ($calls * 2400), cache_write_1h: 0, cache_read: ($calls * 61000),
     output: ($calls * 720), calls: $calls};
  ["site/recipe-photos", "site/ingredient-list", "ops/nightly-backup", "site/step-timers",
   "docs/kitchen-glossary", "site/serving-sizes", "site/allergen-marks", "ops/image-resize",
   "site/shopping-list", "site/recipe-notes", "site/oven-units", "docs/photo-guide",
   "site/tag-pages", "ops/log-rotation", "site/reading-mode", "site/recipe-ratings",
   "site/season-banner", "docs/voice-and-tone", "site/print-margins", "ops/cache-headers",
   "site/related-recipes", "site/quick-filters", "site/cook-mode", "docs/contributor-notes",
   "site/pantry-list", "ops/uptime-check", "site/video-steps", "site/menu-cards",
   "site/leftover-ideas", "docs/recipe-template", "site/import-recipes", "ops/font-subset",
   "site/metric-toggle", "site/cuisine-index", "site/chef-notes", "ops/sitemap"] as $names
  | ($now - $now % 3600) as $this_hour
  | ($this_hour - 4 * 3600) as $favourites
  | ($now - $now % 86400) as $today
  | [range($today - 11 * 86400; $this_hour + 1; 3600)] as $hours
  | ($hours | map(select(
        . == $favourites
        or (. < $this_hour and . != $favourites - 3600 and . != $favourites + 3600
            and ((busy >= 2 and number % 7 == 3) or number % 19 == 5))))) as $done
  | def landed_slug: . as $hour
      | if $hour == $favourites then "site/favourites"
        else $names[($done | index($hour)) % ($names | length)] end;
    def hour: . as $start | number as $n | busy as $busy
      | ($done | index($start) != null) as $closed
      | (if $closed then 1 else 0 end) as $landed
      | (if $n % 23 == 7 then 1 else 0 end) as $bounced
      | {ready: (2 + $n % 5 % 3), in_progress: (1 + $n % 7 % 2 + (if $busy > 1 then 1 else 0 end)),
         review: ((if $n * 3 % 4 < 2 then 1 else 0 end) + (if $busy > 2 then 1 else 0 end)),
         blocked: 3} as $held
      | ($landed * 2 + (if $n % 6 == 0 then 1 else 0 end)) as $commits
      | {start: $start,
         top: {held: $held, entered: {}, done: $landed, task_hours: ($held | map_values(. * 1.0)),
               unclaimed_hours: 0.5, bounces: $bounced},
         subtasks: {held: {in_progress: 1}, done: 0, task_hours: {in_progress: 1.0}},
         landed: $landed, reds_filed: 0, reds_closed: 0,
         waits: {
           approved_to_landed:
             (if $closed then [{slug: ($start | landed_slug), seconds: (240 + $n % 5 * 60)}] else [] end),
           review_to_reviewer:
             (if $n % 5 == 1 then [{slug: "site/print-view", seconds: (600 + $n % 7 * 300)}] else [] end),
           changes_to_resumed:
             (if $bounced == 1 then [{slug: "site/print-view", seconds: 900}] else [] end),
           changes_to_review:
             (if $n % 23 == 9 then [{slug: "site/print-view", seconds: 5400}] else [] end)},
         decisions: {open: (3 + (if $n % 11 < 4 then 1 else 0 end)),
                     answered: (if $busy >= 3 and $n % 4 == 0 then 1 else 0 end),
                     auto_approved: (if $n % 13 == 2 then 1 else 0 end), withdrawn: 0},
         git: {commits: $commits, merges: $landed, added: ($commits * (40 + $n % 50)),
               removed: ($commits * (12 + $n % 17)), reverts: 0},
         tokens: [tokens("executor"; "claude-opus-5-5"; 14 + $busy * 22 + $n % 9),
                  tokens("reviewer"; "claude-sonnet-5-5"; 4 + $busy * 6 + $n % 5),
                  tokens("orchestrator"; "claude-fable-5-2"; 6 + $n % 4)]};
    def report: . as $day
      | ($hours | map(select(. >= $day and . < $day + 86400))) as $own
      | ($own | map(select(. as $hour | $done | index($hour) != null))) as $closed
      | (if $day < $today then $day + 86412 else $this_hour + 12 end) as $read
      | {file: "reports/\($day | day_name).json",
         line: {schema: 1, project: "orchard", day: ($day | day_name), start: $day, read: $read,
                until: $read, closed: ($day < $today), hours: ($own | map(hour)),
                top: {lead: (if ($closed | length) == 0 then null
                             else {n: ($closed | length),
                                   median: (93600 + ($day / 86400 | floor) % 5 * 7200),
                                   p90: (187200 + ($day / 86400 | floor) % 3 * 21600)} end),
                      longest: []},
                subtasks: {lead: null, longest: []},
                rounds: [{number: 1, changes: (($day / 86400 | floor) % 2), approved: ($closed | length)},
                         {number: 2, changes: 0, approved: (($day / 86400 | floor) % 2)}],
                reds: [], decisions: null, git: null, usage: null,
                landed: ($closed | map(. as $hour
                  | {slug: ($hour | landed_slug), at: ($hour + 1500),
                     spent: [tokens("executor"; "claude-opus-5-5"; 180 + ($hour | number) % 140),
                             (tokens("executor"; "claude-opus-5-5"; ($hour | number) % 3 * 45) | .round = 1),
                             tokens("reviewer"; "claude-sonnet-5-5"; 40 + ($hour | number) % 30)]}))}};
    # An hour the server watched whole: its line is written as the hour ends, so the hour NOW is
    # in has none yet.
    def agents: . as $start | number as $n | busy as $busy
      | (1 + (if $busy > 1 then 1 else 0 end) + $n % 2) as $executors
      | {file: "agents/\($start | day_name).jsonl",
         line: {type: "hour", schema: 1, at: ($start + 3600), pid: 4242,
                generation: ($hours[0] + 12), start: $start, from: $start, to: ($start + 3600),
                observed_s: 3600, samples: 360,
                sessions: [{kind: "worker", model: "claude-opus-5-5",
                            started: (if $n % 4 == 0 then 1 else 0 end),
                            went_stale: (if $n % 9 == 4 then 1 else 0 end),
                            went_lost: (if $n % 37 == 11 then 1 else 0 end),
                            stale_s: (if $n % 9 == 4 then 840 else 0 end), lost_s: 0}],
                running_roles: [
                  {role: "orchestrator", peak: 1, mean: 1},
                  {role: "executor", peak: $executors, mean: ($executors - 0.4)},
                  {role: "reviewer", peak: 1, mean: (if $busy > 1 then 0.7 else 0.2 end)}],
                running_agents: {peak: ($executors + 2),
                                 mean: ($executors + 0.6 + (if $busy > 1 then 0.7 else 0.2 end))},
                load_1m: {peak: (2.4 + $busy * 1.9 + $n % 4 * 0.3), mean: (1.1 + $busy * 0.9)},
                load_15m: {peak: (1.8 + $busy * 1.2), mean: (1.0 + $busy * 0.8)}}};
    def health: . as $start | number as $n | busy as $busy
      | {file: "health/\($start | day_name).jsonl",
         line: {schema: 1, at: ($start + 3600), pid: 4242, generation: ($hours[0] + 12),
                start: $start, from: $start, to: ($start + 3600), partial: false,
                dispatch: {ok: 360, failed: (if $n % 41 == 17 then 1 else 0 end)},
                sweep: {ok: 12, failed: 0},
                requests: {count: (900 + $busy * 700 + $n % 90),
                           slow: (if $busy >= 3 then $n % 3 else 0 end),
                           unanswered: (if $n % 53 == 29 then 1 else 0 end)},
                writes: {count: (40 + $busy * 35), p50_ms: 4, p90_ms: (9 + $busy * 3)}}};
  ([range($today - 11 * 86400; $today + 1; 86400)] | map(report))[],
  ($hours | map(select(. < $this_hour)) | map(agents, health))[]
  | "\(.file)\t\(.line | tojson)"
' | while IFS=$'\t' read -r file line; do
    printf '%s\n' "$line" >>"$state/$file"
done

# The UTC day OFFSET seconds before NOW, as a day file is named.
day_of() {
    jq -r -n --argjson at "$((NOW - $1))" '$at | strftime("%Y-%m-%d")'
}

# The runs. `run OFFSET JSON`: a line of the day file of the day its run ended, else started,
# OFFSET seconds before NOW being when it was asked for; the JSON names what differs from a green
# run of the test pipeline asked for by the owner on the default branch, its times (`took`, in
# seconds, none for a run with no end) among it.
run() {
    jq -c -n --argjson now "$NOW" --argjson offset "$1" --argjson said "$2" '
      ($now - $offset) as $queued
      | {schema: 1, parent: null, project: "orchard", machine: null, pipeline: "test", step: null,
         task: null, handle: null, trigger: "owner", branch: "main",
         commit: "3e1f6b0c9a2d47f58b6c1d0e9f3a7b2c4d5e6f70", tree: "9c4b1e7a2f6d4380b5c9e1d27a6f3b8c0d4e5f61",
         dirty: false, queued: $queued, started: ($queued + 0.2),
         ended: (if $said.took == null then null else $queued + 0.2 + $said.took end),
         end: "green", exit: 0, signal: null,
         cpu_s: (if $said.took == null then null else $said.took * 3.4 end), cpu_partial: false,
         rerun_of: null}
      + ($said | del(.took))
      | if .ended == null then .end = null | .exit = null | del(.cpu_partial) else . end' \
        >>"$state/runs/$(day_of "$1").jsonl"
}

minute=60
hour=3600
day=86400
search='"task":"site/recipe-search","handle":"amber-finch","trigger":"amber-finch","branch":"site/recipe-search","commit":"b72d9e4f1a6c4053982e7d1f0c5a3b6e8d9f2a14","tree":"4a8e2c6f0b1d4937a5c3e9f17b2d6a0c8e4f5b23"'
print='"task":"site/print-view","branch":"site/print-view","commit":"c58a1f3e7b2d4690a4d8e2c6f1b0a9d7e5c3f481","tree":"1f7c3a9e5b2d4816c0e4a8f26d1b7c3e9a5f0d42"'
favourites='"task":"site/favourites","handle":"amber-finch","trigger":"amber-finch","branch":"site/favourites","commit":"e04c8a2f6b1d4375b9e3c7a1f5d0b2e6a8c4f937","tree":"7b3f9a1e5c2d4068a4e0c6f28d1b5a7e3c9f1d60"'

# The run the run's picture shows: a check of three steps, its tests red.
check="5f0c2a9e41d7438b9c1e6a07d2b8e314"
tests="8a21d4c7e3f0496b85a2c9e17f3b6d05"
logs="$state/runs/logs/$(day_of $((130 * minute)))"
mkdir -p "$logs"
cat >"$logs/$tests.log" <<'LOG'
   Compiling orchard-site v0.4.0 (/orchard/site)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 41.80s
     Running unittests src/lib.rs (target/debug/deps/orchard_site-5c1e0a9b7d3f2e46)

running 58 tests
test result: ok. 58 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.92s

     Running tests/search.rs (target/debug/deps/search-9a4e7c2b1f0d6e83)

running 12 tests
test a_search_is_not_case_sensitive ... ok
test a_search_for_two_ingredients_ranks_the_recipe_with_both_first ... FAILED
test a_search_with_no_match_lists_nothing ... ok

failures:

---- a_search_for_two_ingredients_ranks_the_recipe_with_both_first stdout ----

thread 'a_search_for_two_ingredients_ranks_the_recipe_with_both_first' panicked at site/tests/search.rs:88:5:
assertion `left == right` failed: leek potato
  left: "Potato gratin"
 right: "Leek and potato soup"

failures:
    a_search_for_two_ingredients_ranks_the_recipe_with_both_first

test result: FAILED. 11 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.37s

error: test failed, to rerun pass `-p orchard-site --test search`
LOG
{
    printf '%s\n' "cargo clippy --workspace -- -D warnings" \
        "    Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 44.10s" \
        "cargo build --release" \
        "    Finished \`release\` profile [optimized] target(s) in 1m 34s" \
        "cargo test -p orchard-site"
    cat "$logs/$tests.log"
    printf '%s\n' "error: Recipe \`check\` failed on line 12 with exit code 101"
} >"$logs/$check.log"
size() { wc -c <"$1" | tr -d ' '; }

run $((3 * minute)) "{\"run\":\"d19e6b3a7c2f4058a1d4e8c2b6f09a73\",\"label\":\"cargo test -p orchard-site --test search\",$search,\"took\":null}"
run $((130 * minute)) "{\"run\":\"$check\",\"pipeline\":\"check\",\"label\":\"just check\",$search,\"took\":312,\"end\":\"red\",\"exit\":101,\"log\":\"logs/$(basename "$logs")/$check.log\",\"log_bytes\":$(size "$logs/$check.log")}"
run $((130 * minute)) "{\"run\":\"2c7f1a5e9b3d4682a0e6c4d8f1b7a395\",\"parent\":\"$check\",\"pipeline\":\"check\",\"step\":\"lint\",\"label\":\"cargo clippy --workspace -- -D warnings\",$search,\"started\":$((NOW - 130 * minute + 1)),\"ended\":$((NOW - 130 * minute + 47)),\"cpu_s\":118.2,\"took\":0}"
run $((130 * minute)) "{\"run\":\"6e3b9d1f5a7c4204b8e2a6c0d4f18b57\",\"parent\":\"$check\",\"pipeline\":\"check\",\"step\":\"build\",\"label\":\"cargo build --release\",$search,\"started\":$((NOW - 130 * minute + 48)),\"ended\":$((NOW - 130 * minute + 144)),\"cpu_s\":402.9,\"took\":0}"
run $((130 * minute)) "{\"run\":\"$tests\",\"parent\":\"$check\",\"pipeline\":\"check\",\"step\":\"tests\",\"label\":\"cargo test -p orchard-site\",$search,\"started\":$((NOW - 130 * minute + 145)),\"ended\":$((NOW - 130 * minute + 311)),\"cpu_s\":231.4,\"took\":0,\"end\":\"red\",\"exit\":101,\"log\":\"logs/$(basename "$logs")/$tests.log\",\"log_bytes\":$(size "$logs/$tests.log"),\"suites\":[{\"name\":\"unittests src/lib.rs (orchard_site)\",\"passed\":58,\"failed\":0,\"ignored\":0,\"s\":1.92},{\"name\":\"tests/search.rs (search)\",\"passed\":11,\"failed\":1,\"ignored\":0,\"s\":0.37}],\"failed\":[{\"suite\":\"tests/search.rs (search)\",\"name\":\"a_search_for_two_ingredients_ranks_the_recipe_with_both_first\",\"line\":\"assertion \`left == right\` failed: leek potato\"}]}"
run $((6 * hour + 2 * minute)) "{\"run\":\"91a5c3e7f2b04d68a3c7e1f5b9d20c46\",\"pipeline\":\"review-callback\",\"label\":\"just review-check\",$print,\"trigger\":\"on_review\",\"took\":72}"
run $((6 * hour + 30 * minute)) "{\"run\":\"b4d8f2a6c0e14739951c5e9a3f7b1d82\",\"pipeline\":\"lint\",\"label\":\"cargo clippy --workspace -- -D warnings\",$print,\"handle\":\"lucid-otter\",\"trigger\":\"lucid-otter\",\"took\":44}"
run $((12 * hour + 30 * minute)) "{\"run\":\"f7c1a5e9d3b24806b2e6a0c4d8f15937\",\"label\":\"cargo test -p orchard-site\",$favourites,\"took\":96}"
run $((13 * hour)) "{\"run\":\"3a9e5c1f7b2d4640a8c2e6f0b4d79153\",\"pipeline\":\"check\",\"label\":\"just check\",\"took\":287}"
run $((26 * hour)) "{\"run\":\"c6f0b4d8a2e14973b5d9e3a7c1f52b08\",\"label\":\"cargo test -p orchard-backup\",\"task\":\"ops/photo-backup\",\"handle\":\"lucid-otter\",\"trigger\":\"lucid-otter\",\"branch\":\"ops/photo-backup\",\"took\":600,\"end\":\"timed_out\",\"exit\":124}"
run $((29 * hour)) "{\"run\":\"e2a6c0d4f8b14315a7e1c5f9d3b06a24\",\"pipeline\":\"build\",\"label\":\"cargo build --release\",\"took\":118,\"end\":\"interrupted\",\"exit\":null,\"signal\":2}"
run $((2 * day + hour)) "{\"run\":\"0d4f8b2e6a1c4759c3f7a1e5b9d24c60\",\"label\":\"cargo test -p orchard-site\",$favourites,\"rerun_of\":\"7b1d5f9a3c2e4086e0a4c8d2f6b19e35\",\"took\":94}"
run $((2 * day + 2 * hour)) "{\"run\":\"7b1d5f9a3c2e4086e0a4c8d2f6b19e35\",\"label\":\"cargo test -p orchard-site\",$favourites,\"took\":101,\"end\":\"red\",\"exit\":101,\"failed\":[{\"suite\":\"tests/favourites.rs (favourites)\",\"name\":\"a_favourite_is_kept_for_the_next_visit\",\"line\":\"assertion failed: page.favourites().contains(&tart)\"}]}"
run $((2 * day + 9 * hour)) "{\"run\":\"a8c2e6f0b4d74197d1f5a9c3e7b08d46\",\"pipeline\":\"check\",\"label\":\"just check\",\"took\":301}"
