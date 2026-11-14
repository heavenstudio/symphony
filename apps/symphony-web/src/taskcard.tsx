// A task's card, as the board's columns draw it, and the agents page and a task's page will: its
// slug and priority, its status, its title (the card's one link to its task), its build and test
// phases, the task it is a subtask of, the documents it links and who holds its roles.

import { useEffect } from "react";
import { linkName } from "./cardheight";
import { href } from "./project";
import type { MachineWork, Role, Task } from "./wire";
import { NO_WORK, phaseLabel, shownPhases } from "./work";

/** The performance mark set once a task's card is first drawn. */
const READY_MARK = "tasks-ready";
let marked = false;

/** Mark, once per page load, that a task's card is drawn. */
export function markTasksReady(): void {
  if (marked) return;
  marked = true;
  performance.mark(READY_MARK);
}

/** A status as the board says it: `in_progress` reads "in progress". */
export function statusWords(status: string): string {
  return status.replaceAll("_", " ");
}

/** Who holds a task's roles, each chip in the health of its holder's session. */
export function Roles({ roles }: { roles: Role[] }) {
  return (
    <span className="chips">
      {roles.map((role) => (
        <span
          key={`${role.role}:${role.handle}`}
          className={`chip health-${role.health}`}
          title={`${role.role} is ${role.health}`}
        >
          {`${role.role} ${role.handle}`}
        </span>
      ))}
    </span>
  );
}

/**
 * The builds and tests of `slug`'s task, a chip each: live while a fresh machine runs it, else
 * stale, each saying how sure it is; nothing for a task with none.
 */
export function WorkPhases({
  work,
  slug,
  connected,
}: {
  work: MachineWork;
  slug: string;
  connected: boolean;
}) {
  const rows = shownPhases(work, slug, connected);
  if (rows.length === 0) return null;
  return (
    // A group, so the label is read: a plain box's is not.
    // biome-ignore lint/a11y/useSemanticElements: a row of chips, laid out as the card's others are, not a form's fieldset
    <div className="work-phases chips" role="group" aria-label="Build and test work">
      {rows.map((row, at) => (
        <span
          // biome-ignore lint/suspicious/noArrayIndexKey: a phase is its place in the machines' report
          key={at}
          className={
            row.freshness === "fresh" && row.operation.state === "running"
              ? "chip work-phase health-live"
              : "chip work-phase health-stale"
          }
          data-work-kind={row.operation.kind === "build" ? "build" : "test"}
          title={`${row.operation.recipe} · ${row.operation.resource} · ${row.operation.reason}`}
        >
          {phaseLabel(row)}
        </span>
      ))}
    </div>
  );
}

export interface TaskCardProps {
  task: Task;
  work?: MachineWork;
  /** Whether the feed is open: a machine's phases read disconnected while it is not. */
  connected?: boolean;
}

/** A task's card. The title is its one link to the task; the slug above it is text to copy. */
export function TaskCard({ task, work = NO_WORK, connected = true }: TaskCardProps) {
  useEffect(markTasksReady);
  return (
    <div className={`taskcard p${task.priority}`} data-slug={task.slug}>
      <code className="taskcard-slug">{task.slug}</code>{" "}
      <span className="prio">{`p${task.priority}`}</span>
      <span className={`task-status st-${task.status}`}>{statusWords(task.status)}</span>
      <a className="taskcard-title" href={href(`/task/${task.slug}`)}>
        {task.title}
      </a>
      <WorkPhases work={work} slug={task.slug} connected={connected} />
      {task.parent !== null && task.parent !== undefined && (
        <div className="subtask-parent">
          {"Subtask of "}
          <a href={href(`/task/${task.parent}`)}>{task.parent}</a>
        </div>
      )}
      <div className="chips">
        {task.links.map((link, at) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: a task may link a document twice
          <a key={at} className="docchip" href={href(`/${link}`)}>
            {linkName(link)}
          </a>
        ))}
      </div>
      <Roles roles={task.roles} />
    </div>
  );
}
