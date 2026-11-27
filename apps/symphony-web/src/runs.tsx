// The Runs page, `/runs` (docs/workflow/RUNS.md, *The Runs page*): what runs and waits for the project on
// this machine now, the machine's schema updates, then every run newest first with its filters, and
// a run (`/runs?run=ID`) opened on its steps, the tests it failed and its log. Now is the machine
// work the board reads (`/api/machine-work`); the schema updates are SCHEMA_UPDATES_PATH, the
// machine's ledger of them; the list is RUNS_PATH a page at a time, a run RUN_PATH and its log
// RUN_LOG_PATH, which read the project's day files and never the board. The page keeps nothing on
// the device but its filters and opens no feed: while it is open it reads Now every NOW_MS, and the
// schema updates, its list's first page, or a run that has not ended, every LIST_MS or NOW_MS,
// naming the last answer's tag; each read ends with what asked for it.

import {
  type CSSProperties,
  type FormEvent,
  type ReactNode,
  useEffect,
  useRef,
  useState,
} from "react";
import { Button } from "./button";
import { whenShown } from "./feed";
import { taskSlug } from "./links";
import { useMount } from "./mount";
import { href, writer } from "./project";
import { Failure, getIf, getJson, said } from "./request";
import { field } from "./shared";
import { load, save } from "./storage";
// biome-ignore lint/style/noRestrictedImports: no-timed-waits baseline: Runs has no feed of its own: it reads again every 5 and 15 s while shown, where a feed event of a run is the wait
import { sleep } from "./timing";
import { failed as failedRead, ready, Trouble } from "./trouble";
import { Skeleton } from "./viewport";
import {
  type End,
  type FailedTest,
  RUN_LOG_MOST,
  RUN_LOG_PATH,
  RUN_LOG_TAIL,
  RUN_PATH,
  RUNS_DAYS,
  RUNS_PATH,
  type RunLine,
  type RunLog,
  type RunPage,
  type RunsPage as RunsAnswer,
  SCHEMA_UPDATES_PATH,
  type SchemaUpdate,
  type SchemaUpdates,
  type Suite,
  type UpdateState,
  type UpdateTime,
} from "./wire";
import { MONTHS, plural, twoDigits, WEEKDAYS } from "./words";

/** How often an open page reads what runs now, in ms. */
export const NOW_MS = 5_000;
/** How often an open page reads its list's first page again, in ms. */
export const LIST_MS = 15_000;

const ENDS: End[] = ["green", "red", "timed_out", "interrupted", "incomplete"];

/** What a run's line says of it: how it ended, else that it runs now or only that it started. */
export type Result = End | "running" | "started";

/** Each result's mark and word, as a line of the list and a run's head say it. */
export const RESULTS: Record<Result, [string, string]> = {
  green: ["✓", "passed"],
  red: ["✗", "failed"],
  timed_out: ["⏱", "timed out"],
  interrupted: ["⊘", "interrupted"],
  incomplete: ["?", "incomplete"],
  running: ["●", "running"],
  started: ["○", "started"],
};

/**
 * What `line` says of its run: its end, else running while an operation of the machine's work
 * (`running`, their ids) is it, else started: a line with no end, read from the day files alone,
 * says no more (docs/workflow/RUNS.md, *Start and end*).
 */
export function result(line: RunLine, running: ReadonlySet<string>): Result {
  if (line.end !== null) return line.end;
  return running.has(line.run) ? "running" : "started";
}

const isText = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value);
const optional =
  (check: (value: unknown) => boolean) =>
  (value: unknown): boolean =>
    value === undefined || value === null || check(value);
const optionalText = optional(isText);
const optionalNumber = optional(isNumber);

function isSuite(value: unknown): value is Suite {
  return (
    isText(field(value, "name")) &&
    ["passed", "failed", "ignored", "s"].every((key) => isNumber(field(value, key))) &&
    optional((error) => typeof error === "boolean")(field(value, "error"))
  );
}

function isFailedTest(value: unknown): value is FailedTest {
  return (
    optionalText(field(value, "suite")) &&
    isText(field(value, "name")) &&
    isText(field(value, "line"))
  );
}

const listOf = (check: (value: unknown) => boolean) =>
  optional((value) => Array.isArray(value) && value.every(check));

/** Whether `value` is a run's line with every field the page draws, each of its type. */
export function isRunLine(value: unknown): value is RunLine {
  const end = field(value, "end");
  return (
    ["run", "pipeline", "label", "trigger"].every((key) => isText(field(value, key))) &&
    ["parent", "step", "task", "handle", "branch", "commit", "rerun_of", "log"].every((key) =>
      optionalText(field(value, key)),
    ) &&
    isNumber(field(value, "queued")) &&
    ["started", "ended", "exit", "signal", "cpu_s", "log_bytes"].every((key) =>
      optionalNumber(field(value, key)),
    ) &&
    (end === null || ENDS.includes(end as End)) &&
    optional((dirty) => typeof dirty === "boolean")(field(value, "dirty")) &&
    optional((partial) => typeof partial === "boolean")(field(value, "cpu_partial")) &&
    listOf(isSuite)(field(value, "suites")) &&
    listOf(isFailedTest)(field(value, "failed"))
  );
}

const isTexts = (value: unknown): value is string[] => Array.isArray(value) && value.every(isText);

/** A page of the list as the page reads it, or none for a value without what it draws. */
export function readRunsPage(value: unknown): RunsAnswer | null {
  const runs = field(value, "runs");
  const more = field(value, "more");
  const pipelines = field(value, "pipelines");
  const unread = field(value, "unread");
  if (
    !Array.isArray(runs) ||
    !runs.every(isRunLine) ||
    typeof more !== "boolean" ||
    !isTexts(pipelines) ||
    !isTexts(unread)
  ) {
    return null;
  }
  return { runs, more, pipelines, unread };
}

/** A run and its steps as the page reads them, or none for a value without what it draws. */
export function readRunPage(value: unknown): RunPage | null {
  const run = field(value, "run");
  const steps = field(value, "steps");
  const unread = field(value, "unread");
  if (!isRunLine(run) || !Array.isArray(steps) || !steps.every(isRunLine) || !isTexts(unread)) {
    return null;
  }
  return { run, steps, unread };
}

/** Part of a run's log as the page reads it, or none for a value without what it draws. */
export function readRunLog(value: unknown): RunLog | null {
  const text = field(value, "text");
  const numbers = ["size", "from", "to"].map((key) => field(value, key));
  if (!isText(text) || !numbers.every(isNumber)) return null;
  const [size, from, to] = numbers as [number, number, number];
  return {
    run: isText(field(value, "run")) ? (field(value, "run") as string) : "",
    log: isText(field(value, "log")) ? (field(value, "log") as string) : "",
    size,
    from,
    to,
    text,
  };
}

const UPDATE_STATES: UpdateState[] = ["open", "served", "left"];

function isUpdateTime(value: unknown): value is UpdateTime {
  return isNumber(field(value, "at")) && isText(field(value, "words"));
}

/** Whether `value` is a schema update with every field the page draws, each of its type. */
export function isSchemaUpdate(value: unknown): value is SchemaUpdate {
  return (
    ["id", "project"].every((key) => isText(field(value, key))) &&
    ["schema", "generation", "failures"].every((key) => isNumber(field(value, key))) &&
    UPDATE_STATES.includes(field(value, "state") as UpdateState) &&
    ["reached", "available", "served"].every((key) => optional(isUpdateTime)(field(value, key))) &&
    ["total_said", "after_available_said", "failure", "task"].every((key) =>
      optionalText(field(value, key)),
    )
  );
}

/** The machine's schema updates as the page reads them, or none for a value without what it draws. */
export function readSchemaUpdates(value: unknown): SchemaUpdates | null {
  const updates = field(value, "updates");
  const unread = field(value, "unread");
  if (!Array.isArray(updates) || !updates.every(isSchemaUpdate) || !optionalText(unread)) {
    return null;
  }
  return { updates, unread: (unread as string | null | undefined) ?? null };
}

/** One operation of what runs now: its machine, its state and what it is. */
export interface NowOperation {
  machine: string;
  id: string | null;
  state: "running" | "queued";
  recipe: string;
  task: string | null;
  handle: string | null;
  /** Since when it has run or waited, by its producer's clock; none when not said. */
  since: number | null;
}

/** One machine of what runs now: its name, how fresh its word is and how long ago it said it. */
export interface NowMachine {
  label: string;
  freshness: string;
  /** How long before the read its last word came, in ms; none when not known. */
  ageMs: number | null;
}

/** What runs and waits now, as the page draws it. */
export interface Now {
  machines: NowMachine[];
  operations: NowOperation[];
}

function nowOperation(machine: string, value: unknown): NowOperation | null {
  const state = field(value, "state");
  const recipe = field(value, "recipe");
  const since = field(value, "phase_since");
  const texts = ["operation_id", "task", "handle"].map((key) => field(value, key));
  if (
    (state !== "running" && state !== "queued") ||
    !isText(recipe) ||
    !optionalNumber(since) ||
    !texts.every(optionalText)
  ) {
    return null;
  }
  const [id, task, handle] = texts.map((text) => (isText(text) ? text : null)) as [
    string | null,
    string | null,
    string | null,
  ];
  return { machine, id, state, recipe, task, handle, since: isNumber(since) ? since : null };
}

/**
 * What runs now out of the machine work (`/api/machine-work`, `MachineWork` on the wire), or none
 * for a value without what the page draws: each machine's name, freshness and age, and each
 * operation of a machine or of none, running or queued.
 */
export function readNow(value: unknown): Now | null {
  const machines = field(value, "machines");
  const unassigned = field(value, "unassigned");
  if (!Array.isArray(machines) || !Array.isArray(unassigned)) return null;
  const now: Now = { machines: [], operations: [] };
  const take = (machine: string, operations: unknown): boolean => {
    if (!Array.isArray(operations)) return false;
    for (const value of operations) {
      const operation = nowOperation(machine, value);
      if (operation === null) return false;
      now.operations.push(operation);
    }
    return true;
  };
  for (const machine of machines) {
    const label = field(machine, "label");
    const freshness = field(machine, "freshness");
    const age = field(machine, "age_ms");
    if (!isText(label) || !isText(freshness) || !optionalNumber(age)) return null;
    now.machines.push({ label, freshness, ageMs: isNumber(age) ? age : null });
    if (!take(label, field(machine, "operations"))) return null;
  }
  return take("", unassigned) ? now : null;
}

/** The ids of what runs now: a run's line with one of them and no end runs. */
export function runningIds(now: Now | null): Set<string> {
  return new Set(
    (now?.operations ?? []).flatMap((operation) =>
      operation.state === "running" && operation.id !== null ? [operation.id] : [],
    ),
  );
}

/** How a machine's freshness reads. */
export function freshness(word: string): string {
  return (
    {
      fresh: "up to date",
      stale: "late",
      disconnected: "not heard from",
      incomplete: "partly read",
      conflict: "conflicting",
    }[word] ?? "not known"
  );
}

/** `seconds` as a duration reads: `42 s`, `3 min 20 s`, `24 min`, `1 h 05 min`. */
export function duration(seconds: number): string {
  const whole = Math.max(Math.round(seconds), 0);
  if (whole < 60) return `${whole} s`;
  const minutes = Math.floor(whole / 60);
  if (minutes < 10) return whole % 60 === 0 ? `${minutes} min` : `${minutes} min ${whole % 60} s`;
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${twoDigits(minutes % 60)} min`;
}

/** How long `line`'s run took, or has run by `now`; none for one that never started. */
export function took(line: RunLine, now: number): number | null {
  if (line.started === null) return null;
  return Math.max((line.ended ?? now) - line.started, 0);
}

/** Its CPU as a run's facts say it, `at least` where the recorder knows it is short. */
export function cpu(line: RunLine): string | null {
  if (line.cpu_s === null) return null;
  return `${line.cpu_partial === true ? "at least " : ""}${duration(line.cpu_s)} of CPU`;
}

/** `at`, seconds since the epoch, in UTC: `Sun 15 Nov, 02:47 UTC`. */
export function when(at: number): string {
  const date = new Date(at * 1000);
  return (
    `${WEEKDAYS[date.getUTCDay()]} ${date.getUTCDate()} ${MONTHS[date.getUTCMonth()]}, ` +
    `${twoDigits(date.getUTCHours())}:${twoDigits(date.getUTCMinutes())} UTC`
  );
}

/** A commit's short name. */
export function shortCommit(commit: string | null): string | null {
  return commit === null ? null : commit.slice(0, 7);
}

/** What the list is filtered by: each matched whole; an empty one is none. */
export interface Filters {
  pipeline: string;
  end: string;
  task: string;
  branch: string;
}

export const NO_FILTERS: Filters = { pipeline: "", end: "", task: "", branch: "" };

const FILTER_KEYS = ["pipeline", "end", "task", "branch"] as const;

/** The filters as a query: `pipeline=test&end=red`, the empty ones left out. */
export function filterQuery(filters: Filters): string {
  const query = new URLSearchParams();
  for (const key of FILTER_KEYS) {
    const value = filters[key].trim();
    if (value !== "") query.set(key, value);
  }
  return query.toString();
}

/** The filters a query names, as filterQuery writes them. */
export function parseFilters(text: string): Filters {
  const query = new URLSearchParams(text);
  return {
    pipeline: query.get("pipeline") ?? "",
    end: query.get("end") ?? "",
    task: query.get("task") ?? "",
    branch: query.get("branch") ?? "",
  };
}

/** Where this browser keeps the filters it chose, as a query (storage.ts). */
export const FILTERS_KEY = "runs-filters";

/** The read of the page of runs `filters` take, asked for before `before` when one is given. */
export function listUrl(filters: Filters, before: number | null = null): string {
  const query = new URLSearchParams(filterQuery(filters));
  if (before !== null) query.set("before", String(before));
  const text = query.toString();
  return text === "" ? RUNS_PATH : `${RUNS_PATH}?${text}`;
}

/** A run's page, under the project's base. */
export function runHref(run: string): string {
  return href(`/runs?run=${encodeURIComponent(run)}`);
}

/**
 * The list once its first page is read again: that page, then the runs already shown from its
 * last's time on down, those queued in the same second as its last among them (a tie the page cut
 * through), so a reader's Load more stays; a fresh page with none is the list.
 */
export function merged(fresh: RunLine[], shown: RunLine[]): RunLine[] {
  const last = fresh.at(-1);
  if (last === undefined) return fresh;
  const listed = new Set(fresh.map((line) => line.run));
  return [...fresh, ...shown.filter((line) => line.queued <= last.queued && !listed.has(line.run))];
}

/** A step drawn on its run's span: how deep it ran, where its bar starts and how wide it is. */
export interface StepBar {
  line: RunLine;
  depth: number;
  left: number;
  width: number;
}

/**
 * Each step of `run` as a bar on the run's span, from its start to its end (or `now` while it runs),
 * in percent, and its depth below the run; a step with no time of its own is a sliver at its place.
 */
export function stepBars(run: RunLine, steps: RunLine[], now: number): StepBar[] {
  const start = run.started ?? run.queued;
  const end = run.ended ?? Math.max(now, start);
  const span = Math.max(end - start, 1);
  const depths = new Map<string, number>([[run.run, 0]]);
  const percent = (at: number) => Math.min(Math.max(((at - start) / span) * 100, 0), 100);
  return steps.map((line) => {
    const depth = (depths.get(line.parent ?? "") ?? 0) + 1;
    depths.set(line.run, depth);
    const from = line.started ?? line.queued;
    const to = line.ended ?? (line.end === null ? Math.max(now, from) : from);
    const left = percent(from);
    return { line, depth, left, width: Math.max(percent(to) - left, 0.5) };
  });
}

/** What a run's log shows: the part read of it, its bytes `from` to `to` of `size`. */
export type ShownLog = Pick<RunLog, "size" | "from" | "to" | "text">;

/** The read that brings `log`'s next part: the bytes written after what is shown. */
export function nextLogUrl(run: string, log: ShownLog | null): string {
  const id = encodeURIComponent(run);
  return log === null
    ? `${RUN_LOG_PATH}?run=${id}&tail=${RUN_LOG_TAIL}`
    : `${RUN_LOG_PATH}?run=${id}&from=${log.to}`;
}

/** The read that brings the part of `log` before what is shown, at most RUN_LOG_MOST bytes. */
export function earlierLogUrl(run: string, log: ShownLog): string {
  const from = Math.max(log.from - RUN_LOG_MOST, 0);
  return `${RUN_LOG_PATH}?run=${encodeURIComponent(run)}&from=${from}&to=${log.from}`;
}

/**
 * `log` with `read` after it: the bytes written since, taken on; a log now shorter than what is
 * shown was written again from the start, and is `read` alone.
 */
export function appended(log: ShownLog | null, read: RunLog): ShownLog {
  if (log === null || read.size < log.to || read.from !== log.to) return read;
  return { size: read.size, from: log.from, to: read.to, text: log.text + read.text };
}

/** `log` with `read`, the part just before it, in front. */
export function prepended(log: ShownLog, read: RunLog): ShownLog {
  if (read.to !== log.from) return log;
  return {
    size: Math.max(log.size, read.size),
    from: read.from,
    to: log.to,
    text: read.text + log.text,
  };
}

/**
 * What `lines` carry under `take` (their failed tests, their suites), in order, each with a key of
 * its line's run and its place there, so two steps' tests of one name are two.
 */
export function carried<T>(
  lines: RunLine[],
  take: (line: RunLine) => T[] | null | undefined,
): [string, T][] {
  return lines.flatMap((line) =>
    (take(line) ?? []).map((item, place): [string, T] => [`${line.run}/${place}`, item]),
  );
}

/**
 * What of `log` is drawn: its text, less the line its first byte may cut short where the log goes
 * on before it (the output before it, once read, brings that line whole).
 */
export function wholeLines(log: ShownLog): string {
  if (log.from === 0) return log.text;
  const cut = log.text.indexOf("\n");
  return cut < 0 ? log.text : log.text.slice(cut + 1);
}

/** `bytes` as a size reads: `812 B`, `4.2 KB`, `1.3 MB`. */
export function bytesWord(bytes: number): string {
  if (bytes < 1_000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${(bytes / 1_000).toFixed(1)} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

/** The address's `run`, the run the page shows; none for the list. */
export function shownRun(search: string): string | null {
  const run = new URLSearchParams(search).get("run");
  return run === null || run === "" ? null : run;
}

/** The address's query, read again each time the shell says the page moved. */
function useSearch(): string {
  const [search, setSearch] = useState(() => location.search);
  useEffect(() => {
    const moved = () => setSearch(location.search);
    addEventListener("review:navigate", moved);
    return () => removeEventListener("review:navigate", moved);
  }, []);
  return search;
}

/** A style that sets custom properties. */
function css(properties: Record<string, string>): CSSProperties {
  return properties as CSSProperties;
}

/** A run's result, its mark and its word, the mark alone hidden from a screen reader. */
function Mark({ shown }: { shown: Result }) {
  const [mark, word] = RESULTS[shown];
  return (
    <span className="run-result" data-result={shown}>
      <span className="run-mark" aria-hidden="true">
        {mark}
      </span>
      <span className="run-word">{word}</span>
    </span>
  );
}

export interface NowViewProps {
  now: Now | null;
  error: string | null;
  /** The time of the read, in seconds. */
  at: number;
}

/** What runs and waits now: each machine's word, then each operation, the running first. */
export function NowView({ now, error, at }: NowViewProps) {
  const operations = [...(now?.operations ?? [])].sort(
    (a, b) => Number(b.state === "running") - Number(a.state === "running"),
  );
  return (
    <section className="runs-now" aria-labelledby="runs-now-title">
      <h2 id="runs-now-title">Now</h2>
      {error !== null && (
        <p className="runs-error" role="alert">
          {`What runs now could not be read: ${error}`}
        </p>
      )}
      {now === null ? (
        error === null && <p className="runs-quiet">Reading what runs now…</p>
      ) : (
        <>
          <ul className="runs-machines">
            {now.machines.map((machine) => (
              <li key={machine.label}>
                <strong>{machine.label}</strong>
                {` · ${freshness(machine.freshness)}`}
                {machine.ageMs !== null && ` · last word ${duration(machine.ageMs / 1000)} ago`}
              </li>
            ))}
          </ul>
          {operations.length === 0 ? (
            <p className="runs-quiet">Nothing runs or waits for this project now.</p>
          ) : (
            <ul className="runs-ops">
              {operations.map((operation, place) => (
                <li
                  key={operation.id ?? `at-${place}`}
                  className="runs-op"
                  data-state={operation.state}
                >
                  <span className="run-result" data-result={operation.state}>
                    <span className="run-mark" aria-hidden="true">
                      {operation.state === "running" ? "●" : "◌"}
                    </span>
                    <span className="run-word">{operation.state}</span>
                  </span>
                  <span className="run-label">{operation.recipe}</span>
                  <span className="run-where">
                    {[operation.task, operation.handle].filter((text) => text !== null).join(" · ")}
                  </span>
                  <span className="run-time">
                    {operation.since === null ? "" : duration(at - operation.since)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}

/** One run of the list: a link to its page, saying how it went, where, who started it and for how long. */
function RunRow({ line, shown, at }: { line: RunLine; shown: Result; at: number }) {
  const time = took(line, at);
  const failed = line.failed?.length ?? 0;
  const where = [line.task ?? line.branch, shortCommit(line.commit)].filter(
    (text) => text !== null && text !== undefined,
  );
  return (
    <li>
      <a className="run-row" href={runHref(line.run)} data-run={line.run} data-result={shown}>
        <Mark shown={shown} />
        <span className="run-pipeline">{line.pipeline}</span>
        <span className="run-label" title={line.label}>
          {line.label}
        </span>
        <span className="run-where">{where.join(" · ")}</span>
        <span className="run-who">{writer(line.trigger).text}</span>
        <span className="run-time">{time === null ? "" : duration(time)}</span>
        {failed > 0 && <span className="run-note">{`${failed} failed`}</span>}
      </a>
    </li>
  );
}

export interface FiltersViewProps {
  filters: Filters;
  pipelines: string[];
  onAsk: (filters: Filters) => void;
}

/** The list's filters: a pipeline and a result, chosen at once, and a task and a branch, typed. */
export function FiltersView({ filters, pipelines, onAsk }: FiltersViewProps) {
  const [task, setTask] = useState(filters.task);
  const [branch, setBranch] = useState(filters.branch);
  const offered = pipelines.includes(filters.pipeline) || filters.pipeline === "";
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onAsk({ ...filters, task, branch });
  };
  return (
    <form className="runs-filters" aria-label="Filter the runs" onSubmit={submit}>
      <label>
        <span>Pipeline</span>
        <select
          name="pipeline"
          value={filters.pipeline}
          onChange={(event) => onAsk({ ...filters, task, branch, pipeline: event.target.value })}
        >
          <option value="">Every pipeline</option>
          {!offered && <option value={filters.pipeline}>{filters.pipeline}</option>}
          {pipelines.map((pipeline) => (
            <option key={pipeline} value={pipeline}>
              {pipeline}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>Result</span>
        <select
          name="end"
          value={filters.end}
          onChange={(event) => onAsk({ ...filters, task, branch, end: event.target.value })}
        >
          <option value="">Every result</option>
          {[...ENDS, "started" as const].map((end) => (
            <option key={end} value={end}>
              {end === "started" ? "not ended" : RESULTS[end][1]}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>Task</span>
        <input
          name="task"
          type="search"
          value={task}
          placeholder="area/slug"
          onChange={(event) => setTask(event.target.value)}
        />
      </label>
      <label>
        <span>Branch</span>
        <input
          name="branch"
          type="search"
          value={branch}
          placeholder="main"
          onChange={(event) => setBranch(event.target.value)}
        />
      </label>
      <div className="runs-filter-actions">
        <Button type="submit" size="sm">
          Show
        </Button>
        {filterQuery(filters) !== "" && (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => {
              setTask("");
              setBranch("");
              onAsk(NO_FILTERS);
            }}
          >
            Clear
          </Button>
        )}
      </div>
    </form>
  );
}

/** The list as it is drawn: its runs, whether more match, what was not read. */
export interface ListState {
  runs: RunLine[];
  more: boolean;
  pipelines: string[];
  unread: string[];
}

export interface ListViewProps {
  list: ListState | null;
  error: string | null;
  running: ReadonlySet<string>;
  at: number;
  loadingMore: boolean;
  onMore: () => void;
}

/** The runs, newest first, a line each, with Load more while more match. */
export function ListView({ list, error, running, at, loadingMore, onMore }: ListViewProps) {
  if (list === null) {
    return error === null ? <Skeleton lines={6} label="Loading the runs…" /> : null;
  }
  return (
    <>
      {list.unread.length > 0 && (
        <details className="runs-unread">
          <summary>{`${list.unread.length} day ${plural(list.unread.length, "file")} not read: their runs are missing`}</summary>
          <ul>
            {list.unread.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </details>
      )}
      {list.runs.length === 0 ? (
        <p className="runs-quiet">{`No run in the last ${RUNS_DAYS} days matches.`}</p>
      ) : (
        <ol className="runs-rows">
          {list.runs.map((line) => (
            <RunRow key={line.run} line={line} shown={result(line, running)} at={at} />
          ))}
        </ol>
      )}
      {list.more && (
        <Button type="button" className="runs-more" disabled={loadingMore} onClick={onMore}>
          {loadingMore ? "Loading…" : "Load more"}
        </Button>
      )}
    </>
  );
}

/** Each update's state, its mark and its word, as its row says it. */
export const UPDATE_WORDS: Record<UpdateState, [string, string]> = {
  open: ["◷", "waiting"],
  served: ["✓", "served"],
  left: ["↩", "went back"],
};

/** The updates as the section lists them: the open ones first, each part newest first. */
export function updateRows(updates: SchemaUpdate[]): SchemaUpdate[] {
  return [
    ...updates.filter((update) => update.state === "open"),
    ...updates.filter((update) => update.state !== "open"),
  ];
}

/** One of an update's times as its row says it: its name, when, and its record's words. */
export interface ShownTime {
  name: string;
  said: string;
  words: string;
}

/**
 * When an update's times came, each named and said in UTC, a time on the day of the one before it
 * by its hour alone, with the record's whole words, which its row shows on hover.
 */
export function updateTimes(update: SchemaUpdate): ShownTime[] {
  const named: [string, UpdateTime | null][] = [
    ["reached", update.reached],
    ["release installed", update.available],
    ["served", update.served],
  ];
  let before: number | null = null;
  const times: ShownTime[] = [];
  for (const [name, time] of named) {
    if (time === null) continue;
    const day = (at: number) => new Date(at * 1000).toISOString().slice(0, 10);
    const said =
      before !== null && day(before) === day(time.at)
        ? `${new Date(time.at * 1000).toISOString().slice(11, 16)} UTC`
        : when(time.at);
    times.push({ name, said: `${name} ${said}`, words: time.words });
    before = time.at;
  }
  return times;
}

/**
 * One update: its board and schema, how long it took and since the release, when (each time's
 * record on hover), and its failures and task.
 */
function UpdateRow({ update }: { update: SchemaUpdate }) {
  const [mark, word] = UPDATE_WORDS[update.state];
  const times = updateTimes(update);
  const generation = update.generation > 1 ? `, generation ${update.generation}` : "";
  const slug = update.task === null ? null : taskSlug(update.task);
  return (
    <li className="runs-op update-row" data-update={update.id} data-state={update.state}>
      <span className="run-result" data-result={update.state === "open" ? "waiting" : update.state}>
        <span className="run-mark" aria-hidden="true">
          {mark}
        </span>
        <span className="run-word">{word}</span>
      </span>
      <span className="run-label">{`${update.project} board · schema ${update.schema}${generation}`}</span>
      <span
        className="run-time"
        title="From its board reaching the schema to the server writing it"
      >
        {update.total_said ?? ""}
      </span>
      <span className="run-where">
        {times.map((time, at) => (
          <span key={time.name} title={time.words}>
            {at > 0 && " · "}
            {time.said}
          </span>
        ))}
      </span>
      <span className="run-who">
        {update.after_available_said === null
          ? ""
          : `${update.after_available_said} after its release`}
      </span>
      {(update.failures > 0 || slug !== null) && (
        <span className="run-note">
          {update.failures > 0 &&
            `${update.failures} failed ${update.failures === 1 ? "refresh" : "refreshes"}${update.failure === null ? "" : `, the last: ${update.failure}`}`}
          {slug !== null && update.task !== null && (
            <>
              {update.failures > 0 && " · "}
              {"task "}
              <a href={update.task}>{slug}</a>
            </>
          )}
        </span>
      )}
    </li>
  );
}

export interface UpdatesViewProps {
  updates: SchemaUpdates | null;
  error: string | null;
}

/**
 * The machine's schema updates: each registered project's board reaching a schema, and how long
 * until this machine's server wrote it, the open ones first. A task is the server's whole path,
 * never the project's base, as the server names it under the project whose board holds it.
 */
export function UpdatesView({ updates, error }: UpdatesViewProps) {
  const rows = updateRows(updates?.updates ?? []);
  const unread = error ?? updates?.unread ?? null;
  return (
    <section className="runs-updates" aria-labelledby="runs-updates-title">
      <h2 id="runs-updates-title">Schema updates</h2>
      {unread !== null && (
        <p className="runs-error" role="alert">
          {`The schema updates could not be read: ${unread}`}
        </p>
      )}
      {updates === null ? (
        error === null && <p className="runs-quiet">Reading the schema updates…</p>
      ) : rows.length === 0 ? (
        updates.unread === null && <p className="runs-quiet">No schema update recorded yet.</p>
      ) : (
        <ol className="runs-ops">
          {rows.map((update) => (
            <UpdateRow key={update.id} update={update} />
          ))}
        </ol>
      )}
    </section>
  );
}

/** A wait between two reads: `pause`, or a test's own. */
export type Wait = (ms: number, signal: AbortSignal) => Promise<void>;

/**
 * The wait between two reads: `ms`, then on only once the page is shown, so a tab behind another
 * reads nothing and reads at once when it is in front again; at once when `signal` ends.
 */
export async function pause(ms: number, signal: AbortSignal): Promise<void> {
  await sleep(ms, signal);
  await whenShown(signal);
}

/** What a read that keeps reading tells its page. */
export interface Told<T> {
  /** It answered what it read; none for a `304` of the copy shown. */
  answered(read: T | null): void;
  /** It failed, said for the page; `missing` when the server has no such thing (a `404`). */
  failed(why: string, missing: boolean): void;
}

/**
 * The failure a read ended with, said for the page; `trying` when the read goes on by itself, which
 * a 404 (said as missing) and a read asked by a click do not say.
 */
function failure(error: unknown, trying = false): string {
  return failedRead(said(error), trying);
}

const isMissing = (error: unknown) => error instanceof Failure && error.kind === "missing";

/** The machine work the board reads (`MachineWork` on the wire), what runs now. */
export const MACHINE_WORK_PATH = "/api/machine-work";

/**
 * Reads what runs now at once and then every NOW_MS while the page is shown (through `wait`),
 * until `signal` ends it or the server has no such thing (a `404`), which no later read brings.
 */
export async function followNow(
  signal: AbortSignal,
  told: Told<Now>,
  wait: Wait = pause,
): Promise<void> {
  while (!signal.aborted) {
    try {
      const read = readNow(await getJson(MACHINE_WORK_PATH, signal));
      if (signal.aborted) return;
      if (read === null) throw new Error("The machine work's answer is not machine work");
      told.answered(read);
    } catch (error) {
      if (signal.aborted) return;
      told.failed(failure(error, !isMissing(error)), isMissing(error));
      if (isMissing(error)) return;
    }
    await wait(NOW_MS, signal);
  }
}

/**
 * Reads `url` at once and then every `every` ms while the page is shown (through `wait`), naming
 * the last answer's tag: an answer the same is told as none, a new one as `read` reads it, one
 * `read` refuses (`unreadable`) or a failure as failed. It ends once `signal` ends it, the server
 * has no such thing (a `404`), or `after`, asked after each read with the last answer read, says
 * so.
 */
async function followTagged<T>(
  url: string,
  read: (value: unknown) => T | null,
  unreadable: string,
  every: number,
  signal: AbortSignal,
  told: Told<T>,
  wait: Wait,
  after?: (last: T | null) => Promise<boolean>,
): Promise<void> {
  let etag = "";
  let last: T | null = null;
  while (!signal.aborted) {
    try {
      const answer = await getIf(url, etag, signal);
      if (signal.aborted) return;
      if (answer.same) {
        told.answered(null);
      } else {
        // An answer it cannot read leaves the last one it read standing.
        const value = read(answer.value);
        if (value === null) throw new Error(unreadable);
        last = value;
        etag = answer.etag;
        told.answered(value);
      }
    } catch (error) {
      if (signal.aborted) return;
      told.failed(failure(error, !isMissing(error)), isMissing(error));
      if (isMissing(error)) return;
    }
    if (await after?.(last)) return;
    await wait(every, signal);
  }
}

/**
 * Reads the first page of the runs `filters` take at once and then every LIST_MS while the page is
 * shown (through `wait`), naming the last answer's tag so a list no day file changed under answers
 * `304`, until `signal` ends it or the server has no such thing (a `404`).
 */
export async function followList(
  filters: Filters,
  signal: AbortSignal,
  told: Told<RunsAnswer>,
  wait: Wait = pause,
): Promise<void> {
  return followTagged(
    listUrl(filters),
    readRunsPage,
    "The runs' answer is not a page of runs",
    LIST_MS,
    signal,
    told,
    wait,
  );
}

/**
 * Reads the machine's schema updates at once and then every LIST_MS while the page is shown
 * (through `wait`), naming the last answer's tag so a ledger no refresh changed answers `304`, until
 * `signal` ends it or the server has no such thing (a `404`).
 */
export async function followUpdates(
  signal: AbortSignal,
  told: Told<SchemaUpdates>,
  wait: Wait = pause,
): Promise<void> {
  return followTagged(
    SCHEMA_UPDATES_PATH,
    readSchemaUpdates,
    "The schema updates' answer is not schema updates",
    LIST_MS,
    signal,
    told,
    wait,
  );
}

/** The page of the runs `filters` take asked for before `before`, read under `signal`. */
export async function readOlder(
  filters: Filters,
  before: number,
  signal: AbortSignal,
): Promise<RunsAnswer> {
  const read = readRunsPage(await getJson(listUrl(filters, before), signal));
  if (read === null) throw new Error("The runs' answer is not a page of runs");
  return read;
}

const union = (...lists: string[][]) => [...new Set(lists.flat())].sort();

/**
 * The list once its first page is read again (`merged`): the older runs a reader loaded kept, and
 * with them whether more match and the files their read left out; with none kept, the page.
 */
export function refreshed(shown: ListState | null, page: RunsAnswer): ListState {
  const runs = merged(page.runs, shown?.runs ?? []);
  const older = shown !== null && runs.length > page.runs.length;
  return {
    runs,
    more: older ? shown.more : page.more,
    pipelines: union(page.pipelines, older ? shown.pipelines : []),
    unread: union(page.unread, older ? shown.unread : []),
  };
}

/** The list with `page`, read below its last run, after it: each run once. */
export function extended(shown: ListState, page: RunsAnswer): ListState {
  const listed = new Set(shown.runs.map((line) => line.run));
  return {
    runs: [...shown.runs, ...page.runs.filter((line) => !listed.has(line.run))],
    more: page.more,
    pipelines: union(shown.pipelines, page.pipelines),
    unread: union(shown.unread, page.unread),
  };
}

/**
 * The list and what runs now: its filters (this browser's last, kept as a query), its first page
 * read at once and every LIST_MS, and Load more read under the mount's signal.
 */
function RunsHome({ now, nowError, at }: { now: Now | null; nowError: string | null; at: number }) {
  const mount = useMount();
  const [filters, setFilters] = useState(() => parseFilters(load(FILTERS_KEY) ?? ""));
  const [list, setList] = useState<ListState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [updates, setUpdates] = useState<SchemaUpdates | null>(null);
  const [updatesError, setUpdatesError] = useState<string | null>(null);
  // What is shown, for the reads that build on it; a new choice of filters starts it again.
  const shown = useRef<ListState | null>(null);
  const asked = useRef(filters);
  useEffect(() => {
    const controller = new AbortController();
    asked.current = filters;
    void followList(filters, controller.signal, {
      answered(read) {
        if (read !== null) {
          shown.current = refreshed(shown.current, read);
          setList(shown.current);
        }
        setError(null);
        setMissing(false);
      },
      failed(why, gone) {
        setError(why);
        setMissing(gone);
      },
    });
    return () => controller.abort();
  }, [filters]);
  useEffect(() => {
    const controller = new AbortController();
    void followUpdates(controller.signal, {
      answered(read) {
        if (read !== null) setUpdates(read);
        setUpdatesError(null);
      },
      failed(why) {
        setUpdatesError(why);
      },
    });
    return () => controller.abort();
  }, []);
  const ask = (next: Filters) => {
    save(FILTERS_KEY, filterQuery(next));
    shown.current = null;
    setList(null);
    setError(null);
    setFilters(next);
  };
  const more = () => {
    const last = shown.current?.runs.at(-1);
    if (last === undefined) return;
    const choice = filters;
    setLoadingMore(true);
    readOlder(choice, last.queued, mount).then(
      (page) => {
        if (mount.aborted || asked.current !== choice || shown.current === null) return;
        shown.current = extended(shown.current, page);
        setList(shown.current);
        setLoadingMore(false);
      },
      (failed) => {
        if (mount.aborted) return;
        setError(failure(failed));
        setLoadingMore(false);
      },
    );
  };
  const operations = now?.operations ?? [];
  const runningCount = operations.filter((operation) => operation.state === "running").length;
  return (
    <div
      className="runs"
      data-ready={list !== null ? "runs" : ready(missing, error !== null)}
      // Busy while the first page is read, and no longer once its read failed: the page then says
      // so, drawn, and a region busy on the screen would hold `symphony shot` to its timeout.
      aria-busy={list === null && error === null}
    >
      <header className="runs-head">
        <div>
          <h1>Runs</h1>
          <p className="runs-unit">Times in UTC</p>
        </div>
        {now !== null && (
          <p className="runs-count">{`${runningCount} running · ${operations.length - runningCount} queued`}</p>
        )}
      </header>
      <NowView now={now} error={nowError} at={at} />
      <UpdatesView updates={updates} error={updatesError} />
      <section className="runs-list" aria-labelledby="runs-list-title">
        <h2 id="runs-list-title">Every run</h2>
        <FiltersView
          key={filterQuery(filters)}
          filters={filters}
          pipelines={list?.pipelines ?? []}
          onAsk={ask}
        />
        {error !== null && (
          <p className="runs-error" role="alert">
            {missing ? error : `The runs could not be read: ${error}`}
          </p>
        )}
        <ListView
          list={list}
          error={error}
          running={runningIds(now)}
          at={at}
          loadingMore={loadingMore}
          onMore={more}
        />
      </section>
    </div>
  );
}

export interface RunViewProps {
  page: RunPage;
  log: ShownLog | null;
  logError: string | null;
  /** The server keeps no log of the run (its read answered 404). */
  logMissing: boolean;
  running: ReadonlySet<string>;
  at: number;
  loadingEarlier: boolean;
  onEarlier: () => void;
}

/** A fact of a run: its name and its value, left out where it has none. */
function Fact({ name, value }: { name: string; value: string | null | undefined }) {
  if (value === null || value === undefined || value === "") return null;
  return (
    <div>
      <dt>{name}</dt>
      <dd>{value}</dd>
    </div>
  );
}

/**
 * The log as it stands: none kept, kept no more (or not yet written), not read, nothing yet, or its
 * text with what came before.
 */
function LogView({
  page,
  log,
  logError,
  logMissing,
  loadingEarlier,
  onEarlier,
}: Pick<
  RunViewProps,
  "page" | "log" | "logError" | "logMissing" | "loadingEarlier" | "onEarlier"
>) {
  let body: ReactNode;
  if (page.run.log === undefined || page.run.log === null) {
    body = <p className="runs-quiet">No log was kept.</p>;
  } else if (log === null && logMissing) {
    body = (
      <p className="runs-quiet">
        {ended(page) ? "This run's log is no longer kept." : "Its log is not written yet."}
      </p>
    );
  } else if (log === null) {
    body =
      logError === null ? (
        <p className="runs-quiet">Reading the log…</p>
      ) : (
        <p className="runs-error" role="alert">{`The log could not be read: ${logError}`}</p>
      );
  } else if (log.size === 0) {
    body = <p className="runs-quiet">Nothing written to the log yet.</p>;
  } else {
    body = (
      <>
        <p className="run-log-part">
          {log.from === 0
            ? `The whole log, ${bytesWord(log.size)}`
            : `The last ${bytesWord(log.to - log.from)} of ${bytesWord(log.size)}`}
        </p>
        {log.from > 0 && (
          <Button type="button" size="sm" disabled={loadingEarlier} onClick={onEarlier}>
            {loadingEarlier ? "Loading…" : "Show earlier output"}
          </Button>
        )}
        <pre className="run-log-text">{wholeLines(log)}</pre>
      </>
    );
  }
  return (
    <section className="run-log" aria-labelledby="run-log-title">
      <h2 id="run-log-title">Log</h2>
      {body}
    </section>
  );
}

/** A run's page: its head and facts, its steps as bars on its span, the tests it failed, its log. */
export function RunView({
  page,
  log,
  logError,
  logMissing,
  running,
  at,
  loadingEarlier,
  onEarlier,
}: RunViewProps) {
  const { run } = page;
  const shown = result(run, running);
  const time = took(run, at);
  const bars = stepBars(run, page.steps, at);
  const failed = carried([run, ...page.steps], (line) => line.failed);
  const suites = carried(run.suites == null ? page.steps : [run], (line) => line.suites);
  return (
    <div className="runs run-page" data-ready="run" data-result={shown}>
      <a
        className="run-back"
        href={href(run.parent === null ? "/runs" : `/runs?run=${run.parent}`)}
      >
        <span aria-hidden="true">← </span>
        {run.parent === null ? "Every run" : "The run it is a step of"}
      </a>
      <header className="runs-head run-head">
        <div>
          <h1>
            <Mark shown={shown} />
            {` ${run.step ?? run.pipeline}`}
          </h1>
          <p className="runs-label run-label">{run.label}</p>
        </div>
      </header>
      <dl className="run-facts">
        <Fact name="Pipeline" value={run.pipeline} />
        <Fact name="Task" value={run.task} />
        <Fact name="Branch" value={run.branch} />
        <Fact
          name="Commit"
          value={
            run.commit === null
              ? null
              : `${shortCommit(run.commit)}${run.dirty === true ? ", with changes not committed" : ""}`
          }
        />
        <Fact
          name="Started by"
          value={
            run.handle !== null && run.handle !== run.trigger
              ? `${writer(run.trigger).text} for ${run.handle}`
              : writer(run.trigger).text
          }
        />
        <Fact name="Asked for" value={when(run.queued)} />
        {/* A line with no end that the machine does not run says when it started, not a run. */}
        <Fact
          name={run.ended !== null ? "Took" : shown === "running" ? "Running for" : "Started"}
          value={
            time === null
              ? null
              : `${duration(time)}${shown === "started" ? " ago, no end written" : ""}`
          }
        />
        <Fact name="CPU" value={cpu(run)} />
        <Fact
          name="Exit"
          value={
            run.signal !== null
              ? `signal ${run.signal}`
              : run.exit === null
                ? null
                : String(run.exit)
          }
        />
      </dl>
      {run.rerun_of !== null && (
        <p className="run-rerun">
          A rerun of <a href={runHref(run.rerun_of)}>the same check on the same tree</a>.
        </p>
      )}
      {bars.length > 0 && (
        <section className="run-steps" aria-labelledby="run-steps-title">
          <h2 id="run-steps-title">Steps</h2>
          <ol>
            {bars.map((bar) => {
              const stepTime = took(bar.line, at);
              const stepResult = result(bar.line, running);
              return (
                <li
                  key={bar.line.run}
                  className="run-step"
                  style={css({ "--depth": String(bar.depth - 1) })}
                >
                  <a href={runHref(bar.line.run)} data-result={stepResult}>
                    <Mark shown={stepResult} />
                    <span className="run-step-name">{bar.line.step ?? bar.line.label}</span>
                  </a>
                  <span className="run-step-track" aria-hidden="true">
                    <span
                      className="run-step-bar"
                      data-result={stepResult}
                      style={{ left: `${bar.left.toFixed(2)}%`, width: `${bar.width.toFixed(2)}%` }}
                    />
                  </span>
                  <span className="run-time">{stepTime === null ? "" : duration(stepTime)}</span>
                </li>
              );
            })}
          </ol>
        </section>
      )}
      {failed.length > 0 && (
        <section className="run-failed" aria-labelledby="run-failed-title">
          <h2 id="run-failed-title">{`${failed.length} failed ${plural(failed.length, "test")}`}</h2>
          <ul>
            {failed.map(([key, test]) => (
              <li key={key} className="run-failed-test">
                <code>{test.name}</code>
                {test.suite !== null && <span className="run-suite">{` in ${test.suite}`}</span>}
                <p>{test.line}</p>
              </li>
            ))}
          </ul>
        </section>
      )}
      {suites.length > 0 && (
        <section className="run-suites" aria-labelledby="run-suites-title">
          <h2 id="run-suites-title">Test binaries</h2>
          <table>
            <thead>
              <tr>
                <th scope="col">Binary</th>
                <th scope="col">Passed</th>
                <th scope="col">Failed</th>
                <th scope="col">Ignored</th>
                <th scope="col">Time</th>
              </tr>
            </thead>
            <tbody>
              {suites.map(([key, suite]) => (
                <tr key={key}>
                  <th scope="row">
                    {suite.name}
                    {suite.error === true && (
                      <span className="run-suite-error"> ended with no result</span>
                    )}
                  </th>
                  <td data-label="Passed">{suite.passed}</td>
                  <td data-label="Failed">{suite.failed}</td>
                  <td data-label="Ignored">{suite.ignored}</td>
                  <td data-label="Time">{duration(suite.s)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
      <LogView
        page={page}
        log={log}
        logError={logError}
        logMissing={logMissing}
        loadingEarlier={loadingEarlier}
        onEarlier={onEarlier}
      />
    </div>
  );
}

/** What a run's read tells its page, beside its page's answers. */
export interface RunTold extends Told<RunPage> {
  /** Its log as it now stands, the part just read taken on. */
  log(log: ShownLog): void;
  /** Its log's read failed, said for the page; the run's own reads go on. */
  logFailed(why: string): void;
  /** The server keeps no log of it (a 404): gone with its days, or not written yet. */
  logMissing(): void;
}

/** Whether `page`'s run and every step of it have ended: nothing it shows changes after. */
export function ended(page: RunPage): boolean {
  return [page.run, ...page.steps].every((line) => line.end !== null);
}

/**
 * Reads the run `id` at once and then every NOW_MS while the page is shown (through `wait`),
 * naming the last answer's tag, and its log on from what is shown (`shown`) after each read; it
 * stops once the run and its steps have ended and their log is read or the server keeps none, once
 * the server has no such run, or once `signal` ends it. A log now shorter than what is shown,
 * written again, is read again from its tail.
 */
export async function followRun(
  id: string,
  shown: () => ShownLog | null,
  signal: AbortSignal,
  told: RunTold,
  wait: Wait = pause,
): Promise<void> {
  const url = `${RUN_PATH}?run=${encodeURIComponent(id)}`;
  const readLog = async (last: RunPage | null): Promise<boolean> => {
    let logRead = true;
    if (last?.run.log != null) {
      try {
        let log = shown();
        let part = readRunLog(await getJson(nextLogUrl(id, log), signal));
        if (part !== null && log !== null && part.size < log.to) {
          log = null;
          part = readRunLog(await getJson(nextLogUrl(id, log), signal));
        }
        if (signal.aborted) return true;
        if (part === null) throw new Error("The log's answer is not a log");
        told.log(appended(log, part));
      } catch (error) {
        if (signal.aborted) return true;
        // A log the server keeps no more is read to its end: an ended run's follow stops on it.
        if (isMissing(error)) {
          told.logMissing();
        } else {
          told.logFailed(failure(error, true));
          logRead = false;
        }
      }
    }
    return last !== null && ended(last) && logRead;
  };
  return followTagged(
    url,
    readRunPage,
    "The run's answer is not a run",
    NOW_MS,
    signal,
    told,
    wait,
    readLog,
  );
}

/** The part of the log before what `log` shows, at most RUN_LOG_MOST bytes, read under `signal`. */
export async function readEarlier(id: string, log: ShownLog, signal: AbortSignal): Promise<RunLog> {
  const read = readRunLog(await getJson(earlierLogUrl(id, log), signal));
  if (read === null) throw new Error("The log's answer is not a log");
  return read;
}

/**
 * A run's page and its log: read at once, and again every NOW_MS until the run and its steps have
 * ended, its log read on from where it stopped; earlier output read under the mount's signal.
 */
/**
 * A run's page before its first answer: its ghost while it is read, busy, or why it could not be
 * read, with Retry. That one is drawn and not busy: a region busy on the screen would hold
 * `symphony shot` to its timeout, picturing nothing of what the page says.
 */
export function RunWaiting({ error, onRetry }: { error: string | null; onRetry: () => void }) {
  return (
    <div
      className="runs run-page"
      data-ready={ready(false, error !== null)}
      aria-busy={error === null}
    >
      <a className="run-back" href={href("/runs")}>
        <span aria-hidden="true">← </span>
        Every run
      </a>
      {error === null ? (
        <Skeleton lines={8} label="Loading the run…" />
      ) : (
        <Trouble title="The run could not be read" text={error} retry={onRetry} />
      )}
    </div>
  );
}

function RunScreen({ id, running, at }: { id: string; running: ReadonlySet<string>; at: number }) {
  const mount = useMount();
  const [page, setPage] = useState<RunPage | null>(null);
  const [missing, setMissing] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [log, setLog] = useState<ShownLog | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const [logMissing, setLogMissing] = useState(false);
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const shownLog = useRef<ShownLog | null>(null);
  const [asked, setAsked] = useState(0);
  useEffect(() => {
    void asked;
    const controller = new AbortController();
    void followRun(id, () => shownLog.current, controller.signal, {
      answered(read) {
        if (read !== null) setPage(read);
        setError(null);
      },
      failed(why, gone) {
        if (gone) setMissing(why);
        else setError(why);
      },
      log(next) {
        shownLog.current = next;
        setLog(next);
        setLogError(null);
        setLogMissing(false);
      },
      logFailed: setLogError,
      logMissing() {
        setLogError(null);
        setLogMissing(true);
      },
    });
    return () => controller.abort();
  }, [id, asked]);
  const earlier = () => {
    const kept = shownLog.current;
    if (kept === null || kept.from === 0) return;
    setLoadingEarlier(true);
    readEarlier(id, kept, mount).then(
      (part) => {
        if (mount.aborted) return;
        // A poll that took the log on meanwhile keeps its part: the earlier one goes in front.
        if (shownLog.current !== null) {
          shownLog.current = prepended(shownLog.current, part);
          setLog(shownLog.current);
        }
        setLoadingEarlier(false);
      },
      (failed) => {
        if (mount.aborted) return;
        setLogError(failure(failed));
        setLoadingEarlier(false);
      },
    );
  };
  if (missing !== null) {
    return (
      <div className="runs run-page" data-ready="missing">
        <Trouble
          title="No such run"
          text={`${missing} A run is kept ${RUNS_DAYS} days on this page.`}
          back={["/runs", "Every run"]}
        />
      </div>
    );
  }
  if (page === null) {
    return <RunWaiting error={error} onRetry={() => setAsked((n) => n + 1)} />;
  }
  return (
    <RunView
      page={page}
      log={log}
      logError={logError}
      logMissing={logMissing}
      running={running}
      at={at}
      loadingEarlier={loadingEarlier}
      onEarlier={earlier}
    />
  );
}

/**
 * The page: what runs now, read every NOW_MS whichever view shows, then the list, or the run the
 * address names (`?run=ID`), each a view of its own that the shell's moves switch between.
 */
export function RunsPage() {
  const search = useSearch();
  const run = shownRun(search);
  const [now, setNow] = useState<Now | null>(null);
  const [nowError, setNowError] = useState<string | null>(null);
  const [at, setAt] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const controller = new AbortController();
    void followNow(controller.signal, {
      answered(read) {
        if (read !== null) setNow(read);
        setNowError(null);
        setAt(Date.now() / 1000);
      },
      failed(why) {
        setNowError(why);
        setAt(Date.now() / 1000);
      },
    });
    return () => controller.abort();
  }, []);
  return (
    <main className="runs-page">
      {run === null ? (
        <RunsHome now={now} nowError={nowError} at={at} />
      ) : (
        <RunScreen key={run} id={run} running={runningIds(now)} at={at} />
      )}
    </main>
  );
}
