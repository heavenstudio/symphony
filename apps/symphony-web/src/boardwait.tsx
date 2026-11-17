// The approved waiting board's presentation. The server supplies complete counts and pages;
// these helpers place and order live changes by the same waiting-column contract.
import { useEffect } from "react";
import { href } from "./project";
import { slugOrder } from "./task";
import { markTasksReady } from "./taskcard";
import type { MachineWork, Orchestrator, Task } from "./wire";
import { taskStage } from "./work";

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
export const STAGES = ["concept", "design", "ready", "in_progress", "review", "done"];
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

export function WaitCard({ task, now }: { task: Task; now: number }) {
  useEffect(markTasksReady);
  const kind = waitKind(task);
  const finished = ["done", "cancelled"].includes(task.status);
  const since = finished ? task.updated : waitSince(task);
  const old = now - since >= 7 * 86400 && !["queued", "parked"].includes(kind) && !finished;
  const line = task.waits_on;
  const stale = task.roles.some(
    (role) =>
      role.health === "stale" &&
      (line?.who.includes(role.handle) || line?.why.includes(role.handle)),
  );
  return (
    <div
      className={`taskcard waiting-card p${task.priority} k-${kind}${blocked(task) ? " is-blocked" : ""}`}
      data-slug={task.slug}
    >
      <a className="taskcard-title" href={href(`/task/${task.slug}`)}>
        {blocked(task) && <span className="blocked-mark">blocked</span>}
        {task.title}
      </a>
      <div className="waiting-slug">
        <code>{task.slug}</code> <span className="prio">{`p${task.priority}`}</span>
      </div>
      <div className="task-wait">
        <span className="on" title={waitLabel(task)}>
          {finished ? (task.status === "done" ? "Done" : "Cancelled") : waitLabel(task)}
        </span>
        <span className={old ? "age old" : "age"}>{waitAge(since, now)}</span>
        <span className="why" title={line?.why}>
          {line?.why ?? ""}
          {stale && !line?.why.includes("(stale)") ? " (stale)" : ""}
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
  ready: number;
}) {
  if (unstarted === 0 || orchestrators.some((one) => one.health === "live")) return null;
  const stale = orchestrators.some((one) => one.health === "stale");
  const holders = orchestrators
    .map((one) => `${one.handle}${one.health === "stale" ? " is stale" : ` ${one.health}`}`)
    .join(", ");
  return (
    <p className={`orchestrator-banner${stale ? " warn" : ""}`}>
      <strong>No live orchestrator.</strong>
      {holders && ` ${holders}`} · {unstarted} of the {ready} ready tasks wait to start
    </p>
  );
}
