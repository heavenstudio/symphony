// A board row link can name a stage the phone has never mounted. Resolve only that explicit
// address through paged summaries, without reading a task's brief or the whole board.
import { scoped } from "./project";
import { getJson } from "./request";
import { readListing } from "./task";
import type { Task } from "./wire";

export function linkedSlug(anchor: string): string {
  return anchor.startsWith("row-") ? anchor.slice(4) : "";
}

/** Search may match other tasks' text. Follow its cursors until the exact slug appears. */
export async function linkedTask(
  scope: string,
  slug: string,
  signal: AbortSignal,
): Promise<Task | null> {
  if (slug === "") return null;
  const seen = new Set<string>();
  let cursor = "";
  do {
    const after = cursor === "" ? "" : `&cursor=${encodeURIComponent(cursor)}`;
    const list = readListing(
      await getJson(
        scoped(scope, `/api/task-board?q=${encodeURIComponent(slug)}&older=1&work=0${after}`),
        signal,
      ),
    );
    if (list === null) throw new Error("The linked task's summary could not be read.");
    const task = list.tasks.find((task) => task.slug === slug);
    if (task !== undefined) return task;
    cursor = list.next ?? "";
    if (seen.has(cursor)) break;
    seen.add(cursor);
  } while (cursor !== "" && !signal.aborted);
  return null;
}
