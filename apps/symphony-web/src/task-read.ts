// What a task's page reads, as this client reads it: the page (`/api/task-page`), its thread's pages
// (`/api/task-history`), a message the feed brings, the rest of a long brief (`/api/task-brief`),
// the landing it offers (`/api/task-offer`) and the ways into its holders' sessions (a session's
// screen is read by session.tsx). Each guard takes what serde would take of the same JSON, and is
// none for what it would refuse: a field it defaults when absent defaults here too.

import { readBlocks } from "./blocks-read";
import { readGatesSeen } from "./gates-model";
import { readOwner } from "./owner";
import { field } from "./shared";
import { readTask } from "./task";
import type {
  Message,
  SessionGo,
  Task,
  TaskBrief,
  TaskEntry,
  TaskHistory,
  TaskOffer,
  TaskPage,
} from "./wire";
import { NO_WORK, readWork } from "./work";

const isText = (value: unknown): value is string => typeof value === "string";
/** A count as serde reads a `usize`: a whole number, none below zero. */
const isCount = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
/** As serde reads an `Option`: absent and `null` are none. */
const absent = (value: unknown) => value === undefined || value === null;
/** A field serde reads as an `Option<String>`: none, or text. */
const maybeText = (value: unknown) => absent(value) || isText(value);
/** A whole number serde reads as an `i64`. */
const isId = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value);

/** A message of a task's thread, or none: its id, task, sender, kind, text and time, and whether
 * a person a board named wrote it (none from an older server, which names nobody). */
export function readMessage(value: unknown): Message | null {
  const recipient = field(value, "recipient");
  return isId(field(value, "id")) &&
    ["slug", "sender", "kind", "text"].every((key) => isText(field(value, key))) &&
    maybeText(recipient) &&
    typeof field(value, "ts") === "number"
    ? {
        id: field(value, "id") as number,
        slug: field(value, "slug") as string,
        sender: field(value, "sender") as string,
        recipient: (recipient as string | undefined) ?? null,
        kind: field(value, "kind") as string,
        text: field(value, "text") as string,
        ts: field(value, "ts") as number,
        person: field(value, "person") === true,
      }
    : null;
}

/** A message as the thread draws it: the message, its time as said, and its blocks. */
export function readEntry(value: unknown): TaskEntry | null {
  const message = readMessage(field(value, "message"));
  const blocks = readBlocks(field(value, "blocks"));
  const stamp = field(value, "stamp");
  return message !== null && blocks !== null && isText(stamp) ? { message, stamp, blocks } : null;
}

/** A page of a task's thread, or none: its messages, and the cursors before and after it. */
export function readHistory(value: unknown): TaskHistory | null {
  const entries = field(value, "entries");
  if (!Array.isArray(entries)) return null;
  const read = entries.map(readEntry);
  const before = field(value, "before");
  const after = field(value, "after");
  if (read.some((entry) => entry === null)) return null;
  if (!(absent(before) || isId(before)) || !(absent(after) || isId(after))) return null;
  return {
    entries: read as TaskEntry[],
    before: (before as number | undefined) ?? null,
    after: (after as number | undefined) ?? null,
  };
}

/** The message the feed's `message` region brings: its record, with the thread's stamp and blocks
 * when it sends them (none sent are none to draw), or none when it is no message. */
export function heardEntry(value: unknown): TaskEntry | null {
  const message = readMessage(value);
  if (message === null) return null;
  const stamp = field(value, "stamp");
  const sent = field(value, "blocks");
  const blocks = sent === undefined ? [] : readBlocks(sent);
  if (blocks === null) return null;
  return { message, stamp: isText(stamp) ? stamp : "", blocks };
}

/** How a session is gone to: whose session, its web address and its pane, each when known. */
export function readSessionGo(value: unknown): SessionGo | null {
  const url = field(value, "url");
  const pane = field(value, "pane");
  return isText(field(value, "handle")) && maybeText(url) && maybeText(pane)
    ? {
        handle: field(value, "handle") as string,
        url: (url as string | undefined) ?? null,
        pane: (pane as string | undefined) ?? null,
      }
    : null;
}

/** The sessions of a page's roles, by handle: none sent is none known. */
function readSessions(value: unknown): Record<string, SessionGo> | null {
  if (value === undefined) return {};
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const read: Record<string, SessionGo> = {};
  for (const [handle, go] of Object.entries(value)) {
    const session = readSessionGo(go);
    if (session === null) return null;
    read[handle] = session;
  }
  return read;
}

/**
 * A task's page, or none: its task, brief and subtasks, and what serde defaults when absent (the
 * machines' work, the brief's cut, the sessions, whether the page was asked on the server's own
 * machine, the gates the server last answered, and how many execution attempts it has had).
 */
export function readTaskPage(value: unknown): TaskPage | null {
  const task = readTask(field(value, "task"));
  const blocks = readBlocks(field(value, "blocks"));
  const children = field(value, "children");
  const listed = Array.isArray(children) ? children.map(readTask) : null;
  const work = field(value, "machine_work");
  const machineWork = work === undefined ? NO_WORK : readWork(work);
  const sessions = readSessions(field(value, "sessions"));
  const gates = field(value, "gates");
  const seen = absent(gates) ? null : readGatesSeen(gates);
  const count = (key: string) => {
    const said = field(value, key);
    return said === undefined ? 0 : isCount(said) ? said : null;
  };
  const omitted = count("brief_omitted");
  const next = count("brief_next");
  const attempts = count("attempts");
  const revision = field(value, "brief_revision") ?? "";
  const local = field(value, "local") ?? false;
  const said = field(value, "owner");
  const owner = absent(said) ? null : readOwner(said);
  if (
    task === null ||
    blocks === null ||
    listed === null ||
    listed.some((child) => child === null) ||
    !isCount(field(value, "children_count")) ||
    machineWork === null ||
    sessions === null ||
    (!absent(gates) && seen === null) ||
    omitted === null ||
    next === null ||
    attempts === null ||
    !isText(revision) ||
    typeof local !== "boolean" ||
    (!absent(said) && owner === null)
  ) {
    return null;
  }
  return {
    machine_work: machineWork,
    task,
    blocks,
    children: listed as Task[],
    children_count: field(value, "children_count") as number,
    brief_omitted: omitted,
    brief_next: next,
    brief_revision: revision,
    sessions,
    local,
    gates: seen,
    // Left out at none, as the server leaves it out, so a page read is the page it sent.
    ...(attempts > 0 && { attempts }),
    ...(owner !== null && { owner }),
  };
}

/** A chunk of a long brief, or none: its blocks, where the next starts, what is left and the
 * revision it was cut from (none sent is the empty one). */
export function readBrief(value: unknown): TaskBrief | null {
  const blocks = readBlocks(field(value, "blocks"));
  const revision = field(value, "revision") ?? "";
  return blocks !== null &&
    isCount(field(value, "next")) &&
    isCount(field(value, "omitted")) &&
    isText(revision)
    ? {
        blocks,
        next: field(value, "next") as number,
        omitted: field(value, "omitted") as number,
        revision,
      }
    : null;
}

/** The landing a task offers, or none: its action, label, branch and payload. */
export function readOffer(value: unknown): TaskOffer | null {
  const payload = field(value, "payload");
  return ["action", "label", "branch"].every((key) => isText(field(value, key))) &&
    maybeText(payload)
    ? {
        action: field(value, "action") as string,
        label: field(value, "label") as string,
        branch: field(value, "branch") as string,
        payload: (payload as string | undefined) ?? null,
      }
    : null;
}
