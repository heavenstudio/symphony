// The approved waiting board's presentation. The server supplies complete counts and pages;
// these helpers place and order live changes by the same waiting-column contract.
import { useEffect, useState } from "react";
import { href } from "./project";
import { slugOrder } from "./task";
import { markTasksReady } from "./taskcard";
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

export function savedStage(value: string | null): string {
  return value !== null && PHONE_STAGES.includes(value) ? value : "review";
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
  if (task.status !== "blocked") return taskStage(work, task, connected);
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
 * A task's card on the waiting board. Closed, it keeps its fixed box (`ROW`, board.tsx); its one
 * button opens it in place, by key or touch, to the whole of its title, slug, wait and reason.
 */
export function WaitCard({ task, now }: { task: Task; now: number }) {
  useEffect(markTasksReady);
  const [open, setOpen] = useState(false);
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
  return (
    <div
      className={`taskcard waiting-card p${task.priority} k-${kind}${blocked(task) ? " is-blocked" : ""}`}
      data-slug={task.slug}
      data-open={open ? "true" : undefined}
    >
      <a className="taskcard-title" href={href(`/task/${task.slug}`)}>
        {blocked(task) && <span className="blocked-mark">blocked</span>}
        {(task.review_rounds ?? 0) > 0 && (
          <span className="review-mark">
            {task.review_rounds} {task.review_rounds === 1 ? "review" : "reviews"}
          </span>
        )}
        {task.title}
      </a>
      <div className="waiting-slug">
        <code>
          <Ends text={task.slug} />
        </code>
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
          <Ends
            text={finished ? (task.status === "done" ? "Done" : "Cancelled") : waitLabel(task)}
          />
        </span>
        <span className={old ? "age old" : "age"}>{waitAge(since, now)}</span>
        <span className="why" title={why}>
          <Ends text={why} />
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
