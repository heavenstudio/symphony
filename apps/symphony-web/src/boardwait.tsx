// The approved waiting board's presentation. The server supplies complete counts and pages;
// these helpers place and order live changes by the same waiting-column contract.
import { useEffect, useState } from "react";
import { useCardCost } from "./card-costs";
import { href } from "./project";
import { slugOrder } from "./task";
import { markTasksReady, statusWords } from "./taskcard";
import type { MachineWork, Orchestrator, Task } from "./wire";
import { PIPELINE, taskStage } from "./work";

export const WAIT_KINDS = ["decision", "nobody", "check", "task", "working", "queued", "parked"];
export const WAIT_LABELS = [
  "Your decisions",
  "On nobody",
  "On checks",
  "On tasks",
  "Working",
  "Queued",
  "Parked",
];
export const STAGES: string[] = [...PIPELINE];
export const PHONE_STAGES = [
  "review",
  "in_progress",
  "ready",
  "done",
  "design",
  "concept",
  "backlog",
];
export const STAGE_KEY = "board-waiting-stage";

/**
 * What a custom workflow's IDs read as, once its definition is (`flow.tsx`): a column's key
 * (`phase:ID`) its phase's label, a wait facet (`state:ID`) its state's, and a task's place
 * (`PHASE/STATE`) both. Empty on a board its own writers keep.
 */
export const NAMES = new Map<string, string>();

/** The phone's stage `value` names, when `tabs` has it; else the first of them. */
export function savedStage(value: string | null, tabs = PHONE_STAGES): string {
  return value !== null && tabs.includes(value) ? value : (tabs[0] as string);
}

export function waitKind(task: Task): string {
  if (task.status === "blocked" && !blocked(task)) return "nobody";
  const kind = task.waits_on?.kind ?? "nobody";
  return WAIT_KINDS.includes(kind) ? kind : "nobody";
}

export function blocked(task: Task): boolean {
  const line = task.waits_on;
  return (
    task.status === "blocked" &&
    line != null &&
    (line.kind === "parked" ||
      (["decision", "check", "task", "nobody"].includes(line.kind) &&
        line.who.trim() !== "" &&
        line.why !== "no reason recorded"))
  );
}

export function waitingStage(work: MachineWork, task: Task, connected: boolean): string {
  const stage = taskStage(work, task, connected);
  // A task a custom workflow holds goes in its phase's column, blocked or not, unless a machine
  // builds or tests it.
  const place = task.workflow;
  if (place) return stage === "build" || stage === "test" ? stage : `phase:${place.phase}`;
  if (task.status !== "blocked") return stage;
  return ["concept", "design", "ready", "in_progress", "review", "backlog"].includes(
    task.from ?? "",
  )
    ? (task.from as string)
    : "ready";
}

export function waitSince(task: Task): number {
  const since = task.waits_on?.since;
  return since != null && Number.isFinite(since) ? since : task.updated;
}

export function waitingOrder(a: Task, b: Task): number {
  const bucket = (task: Task) => (waitKind(task) === "parked" ? 2 : blocked(task) ? 0 : 1);
  const rank = (task: Task) =>
    blocked(task) && waitKind(task) !== "parked" ? WAIT_KINDS.indexOf(waitKind(task)) : 0;
  const since = (task: Task) =>
    ["done", "cancelled"].includes(task.status) ? -task.updated : waitSince(task);
  return (
    bucket(a) - bucket(b) || rank(a) - rank(b) || since(a) - since(b) || slugOrder(a.slug, b.slug)
  );
}

export function waitAge(since: number, now: number): string {
  const hours = Math.floor(Math.max(0, now - since) / 3600);
  return hours < 48 ? `${hours} h` : `${Math.floor(hours / 24)} d`;
}

export function waitLabel(task: Task): string {
  const who = task.waits_on?.who ?? "";
  switch (waitKind(task)) {
    case "decision":
      return `On your decision ${who}`;
    case "check":
    case "task":
      return `On ${who}`;
    case "working":
      return "Working";
    case "queued":
      return "Queued";
    case "parked":
      return "Parked";
    default:
      return "On nobody";
  }
}

/**
 * `text` as a cut line shows it, its start and the end it keeps: texts that differ only at their
 * end (a slug's last word, a wait's task, a reason's last words) stay told apart when the line is
 * too narrow for them. The end starts at the first space, hyphen or slash among the last 16
 * characters that leaves it ten or more, else ten characters from the end, never inside a
 * character; a text of 16 or fewer is all start.
 */
export function ends(text: string): [string, string] {
  const at = text.length > 16 ? text.search(/[ /-].{1,15}$|.{10}$/su) : -1;
  return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at)];
}

/**
 * `text` on one line, as a closed card draws it: each run of white space, a line break or a tab
 * among it, is one space. A card's lines keep their spaces as written (`white-space: pre`), so the
 * end a cut line keeps shows its leading space, and a reason written on two lines would draw on
 * two and be clipped by the card's fixed box.
 */
export function fold(text: string): string {
  return text.replace(/\s+/gu, " ");
}

/** A line of text that keeps its end when cut: the ellipsis falls between its two parts. */
export function Ends({ text }: { text: string }) {
  const [head, tail] = ends(text);
  return (
    <>
      <span className="cut-head">{head}</span>
      {tail !== "" && <span className="cut-tail">{tail}</span>}
    </>
  );
}

/**
 * A task's card on the waiting board. Closed, it keeps its fixed box (`ROW`, board.tsx), each of
 * its lines folded onto one (`fold`); its one button opens it in place, by key or touch, to the
 * whole of its title, slug, wait and reason, each as written.
 */
export function WaitCard({ task, now }: { task: Task; now: number }) {
  useEffect(markTasksReady);
  const [open, setOpen] = useState(false);
  const cost = useCardCost(task.slug, task.updated);
  const kind = waitKind(task);
  const finished = ["done", "cancelled"].includes(task.status);
  const since = finished ? task.updated : waitSince(task);
  const old = now - since >= 7 * 86400 && !["queued", "parked"].includes(kind) && !finished;
  const line = task.waits_on;
  const stale =
    kind === "working" &&
    task.roles.some(
      (role) =>
        role.health === "stale" &&
        (line?.who.includes(role.handle) || line?.why.includes(role.handle)),
    );
  const reason =
    kind === "working" && line?.who && !line.why.includes(line.who)
      ? `${line.why} ${line.who}`.trim()
      : (line?.why ?? "");
  const why = stale && !reason.includes("(stale)") ? `${reason} (stale)` : reason;
  const shown = (text: string) => <Ends text={open ? text : fold(text)} />;
  return (
    <div
      className={`taskcard waiting-card p${task.priority} k-${kind}${blocked(task) ? " is-blocked" : ""}`}
      data-slug={task.slug}
      data-open={open ? "true" : undefined}
    >
      <a className="taskcard-title" href={href(`/task/${task.slug}`)}>
        {blocked(task) && <span className="blocked-mark">blocked</span>}
        {task.workflow && (
          <span className="state-mark">
            {NAMES.get(`${task.workflow.phase}/${task.workflow.state}`) ??
              statusWords(task.workflow.state)}
          </span>
        )}
        {(task.review_rounds ?? 0) > 0 && (
          <span className="review-mark">
            {task.review_rounds} {task.review_rounds === 1 ? "review" : "reviews"}
          </span>
        )}
        {task.title}
      </a>
      <div className="waiting-slug">
        <code>{shown(task.slug)}</code>
        <span className="prio">{`p${task.priority}`}</span>
        <button
          type="button"
          className="card-more"
          aria-expanded={open}
          aria-label={`Show all of ${task.title}`}
          onClick={() => setOpen(!open)}
        >
          ▾
        </button>
      </div>
      <div className="task-wait">
        <span className="on" title={waitLabel(task)}>
          {shown(finished ? (task.status === "done" ? "Done" : "Cancelled") : waitLabel(task))}
        </span>
        <span className={old ? "age old" : "age"}>
          {/* The task's estimated cost, where it took tokens, before its time. */}
          {cost !== undefined && (
            <>
              <span className="card-cost" title={cost.title}>
                {cost.said}
              </span>
              <span className="card-cost-dot" aria-hidden="true">
                ·
              </span>
            </>
          )}
          {waitAge(since, now)}
        </span>
        <span className="why" title={why}>
          {shown(why)}
        </span>
      </div>
    </div>
  );
}

export function OrchestratorBanner({
  orchestrators,
  unstarted,
  ready,
}: {
  orchestrators: Orchestrator[];
  unstarted: number;
  ready?: number | undefined;
}) {
  if (unstarted === 0 || orchestrators.some((one) => one.health === "live")) return null;
  const stale = orchestrators.some((one) => one.health === "stale");
  const holders = orchestrators
    .map((one) => `${one.handle}${one.health === "stale" ? " is stale" : ` ${one.health}`}`)
    .join(", ");
  return (
    <p className={`orchestrator-banner${stale ? " warn" : ""}`}>
      <strong>No live orchestrator.</strong>
      {holders && ` ${holders}`} · {unstarted}
      {ready === undefined ? "" : ` of the ${ready}`} ready tasks wait to start
    </p>
  );
}
