// The board's feed in the browser: an EventSource on the page's project's feed, each event it hears
// handed on as `Heard`.
//
// A page that goes away closes its stream: left (`pagehide`, which every browser fires as a page
// is left, for good or into its back/forward cache) or frozen (Chrome's `freeze`). A page kept for
// back and forward with its stream open would hold one of the browser's six connections to the
// server for as long as it is kept. The stream ends as `away`, and the page reads again from a
// fresh snapshot once it is back (`pageshow`, `resume`), as after any lost stream. Hidden alone
// (another tab in front) the page keeps its stream.

import { here, project } from "./project";
import { FEED_EVENT, PING_EVENT, SERVER_EVENT } from "./wire";

/** What the feed's stream said. */
export type Heard =
  /** The stream is open: the board is fresh. */
  | { kind: "open" }
  /** The server is there. */
  | { kind: "ping" }
  /** One event's data, parsed. */
  | { kind: "delta"; value: unknown }
  | { kind: "server"; message: string }
  /** The stream failed, the server ended it, or it sent what this client cannot read. */
  | { kind: "lost" }
  /** The page went away (left or frozen) and closed the stream. */
  | { kind: "away" };

/** The events that say the page went away, each on its target: the window's, then the document's. */
const LEAVING = ["pagehide", "freeze"] as const;
/** The events that say the page is back, as LEAVING. */
const RETURNING = ["pageshow", "resume"] as const;

const targets = (): [EventTarget, EventTarget] => [window, document];

let away = false;
let watched = false;

/** Whether the page is away now: left (kept for back and forward, or unloading) or frozen. */
export function isAway(): boolean {
  if (!watched) {
    watched = true;
    for (const [names, leaving] of [
      [LEAVING, true],
      [RETURNING, false],
    ] as const) {
      targets().forEach((target, at) => {
        target.addEventListener(names[at] as string, () => {
          away = leaving;
        });
      });
    }
  }
  return away;
}

/** `ms` later, or at once when `signal` aborts. */
export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal?.aborted) return resolve();
    const done = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", done);
      resolve();
    };
    const timer = setTimeout(done, ms);
    signal?.addEventListener("abort", done);
  });
}

/** Wait until the page is back, if it is away: a stream opened while it is away would be held. */
export async function back(signal?: AbortSignal): Promise<void> {
  while (isAway() && !signal?.aborted) await sleep(100, signal);
}

/** What one event of the stream says. */
export function heard(type: string, data: unknown): Heard {
  if (type === "open") return { kind: "open" };
  if (type === PING_EVENT) return { kind: "ping" };
  if (type !== FEED_EVENT && type !== SERVER_EVENT) return { kind: "lost" };
  let value: unknown;
  try {
    value = JSON.parse(String(data));
  } catch {
    return { kind: "lost" };
  }
  if (type === FEED_EVENT) return { kind: "delta", value };
  return typeof value === "string" ? { kind: "server", message: value } : { kind: "lost" };
}

/**
 * The stream at `url` (a path under the project), each thing it says handed to `said`; none when
 * it cannot be opened or the page is away. The function it gives closes it.
 */
export function openFeed(url: string, said: (heard: Heard) => void): (() => void) | null {
  if (isAway()) return null;
  let source: EventSource;
  try {
    source = new EventSource(new URL(here(project(), url), location.href).href);
  } catch {
    return null;
  }
  const listener = (event: Event) =>
    said(heard(event.type, event instanceof MessageEvent ? event.data : undefined));
  const leaving = () => {
    // Before the page is put away, whichever of its listeners runs first.
    away = true;
    source.close();
    said({ kind: "away" });
  };
  for (const name of ["open", "error", PING_EVENT, FEED_EVENT, SERVER_EVENT]) {
    source.addEventListener(name, listener);
  }
  isAway();
  targets().forEach((target, at) => {
    target.addEventListener(LEAVING[at] as string, leaving);
  });
  return () => {
    source.close();
    targets().forEach((target, at) => {
      target.removeEventListener(LEAVING[at] as string, leaving);
    });
  };
}

/** What a stream said, waited for one at a time, each wait bounded. */
export class Inbox<T> {
  private queued: T[] = [];
  private waiting: ((value: T | null) => void) | null = null;

  push(value: T): void {
    if (this.waiting) {
      const wake = this.waiting;
      this.waiting = null;
      wake(value);
    } else {
      this.queued.push(value);
    }
  }

  /** The next thing said, or `null` when `ms` pass first or `signal` aborts. */
  next(ms: number, signal?: AbortSignal): Promise<T | null> {
    const first = this.queued.shift();
    if (first !== undefined) return Promise.resolve(first);
    return new Promise((resolve) => {
      const done = (value: T | null) => {
        clearTimeout(timer);
        signal?.removeEventListener("abort", aborted);
        this.waiting = null;
        resolve(value);
      };
      const aborted = () => done(null);
      const timer = setTimeout(aborted, ms);
      if (signal?.aborted) return aborted();
      signal?.addEventListener("abort", aborted);
      this.waiting = done;
    });
  }
}
