// A task's card, as the board's columns and the agents page draw it: its slug and priority, its
// status, its title (the card's one link to its task), its build and test phases, the task it is a
// subtask of, the documents it links and who holds its roles. A status's words and the phases'
// chips are work-phases.tsx's, which a task's page draws without the card, and are named here too.

import { memo, useEffect } from "react";
import { href } from "./project";
import type { MachineWork, Role, Task } from "./wire";
import { NO_WORK, samePhases, shownPhases } from "./work";
import { statusWords, WorkPhases } from "./work-phases";

export { PhaseChips, statusWords, WorkPhases } from "./work-phases";

/** The last part of a link's path: what its chip says. */
export function linkName(link: string): string {
  return link.slice(link.lastIndexOf("/") + 1);
}

/** The performance mark set once a task's card is first drawn. */
const READY_MARK = "tasks-ready";
let marked = false;

/** Mark, once per page load, that a task's card is drawn. */
export function markTasksReady(): void {
  if (marked) return;
  marked = true;
  performance.mark(READY_MARK);
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

export interface TaskCardProps {
  task: Task;
  work?: MachineWork;
  /** Whether the feed is open: a machine's phases read disconnected while it is not. */
  connected?: boolean;
}

/**
 * Whether `a` and `b` draw the same card: the same task (a page keeps the object of a task that did
 * not change), the feed alike, and the same build and test phases, worked out only when the
 * machines' work changed. A list drawn again draws only the cards that changed.
 */
export function sameCard(a: TaskCardProps, b: TaskCardProps): boolean {
  const connected = a.connected ?? true;
  if (a.task !== b.task || connected !== (b.connected ?? true)) return false;
  const before = a.work ?? NO_WORK;
  const after = b.work ?? NO_WORK;
  return (
    before === after ||
    samePhases(
      shownPhases(before, a.task.slug, connected),
      shownPhases(after, b.task.slug, connected),
    )
  );
}

/** A task's card. The title is its one link to the task; the slug above it is text to copy. It is
 * drawn again only when what it shows changed (`sameCard`). */
export const TaskCard = memo(function TaskCard({
  task,
  work = NO_WORK,
  connected = true,
}: TaskCardProps) {
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
}, sameCard);
