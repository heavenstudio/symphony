// The board's feed in the browser: a WebSocket on the page's project's feed, each event it hears
// handed on as `Heard`. The server says on it what it says as Server-Sent Events to any other
// request of the feed's (symphony_api's feed contract), each event one text message: its name, a
// line feed, then its data. A browser keeps six HTTP/1.1 connections to a host, and an EventSource
// holds one for as long as it is open, so six tabs following the board would leave a seventh none
// for its reads; it counts its WebSockets apart from them.
//
// A page that goes away closes its stream: left (`pagehide`, which every browser fires as a page
// is left, for good or into its back/forward cache) or frozen (Chrome's `freeze`). A page kept for
// back and forward with its stream open would keep its socket, and the server its subscription,
// for as long as it is kept. The stream ends as `away`, and the page reads again from a fresh
// snapshot once it is back (`pageshow`, `resume`), as after any lost stream. Hidden alone (another
// tab in front) the page keeps its stream.

import { here, project } from "./project";
import { deadline } from "./timing";
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

/** Once the page is shown: at once if it is, else at the first change of visibility that shows
 * it, or once `signal` ends. */
export function whenShown(signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (typeof document === "undefined" || !document.hidden || signal.aborted) return resolve();
    const heard = () => {
      if (!document.hidden) done();
    };
    const done = () => {
      document.removeEventListener("visibilitychange", heard);
      signal.removeEventListener("abort", done);
      resolve();
    };
    document.addEventListener("visibilitychange", heard);
    signal.addEventListener("abort", done);
  });
}

/**
 * Wait until the page is back, if it is away: at the first event that says so (`pageshow`,
 * `resume`), or once `signal` ends. A stream opened while it is away would be held.
 */
export function back(signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (!isAway() || signal?.aborted) return resolve();
    const done = () => {
      targets().forEach((target, at) => {
        target.removeEventListener(RETURNING[at] as string, done);
      });
      signal?.removeEventListener("abort", done);
      resolve();
    };
    targets().forEach((target, at) => {
      target.addEventListener(RETURNING[at] as string, done);
    });
    signal?.addEventListener("abort", done);
  });
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

/** What one message of the socket says: its event's name, a line feed, then the event's data. */
export function message(data: unknown): Heard {
  if (typeof data !== "string") return { kind: "lost" };
  const at = data.indexOf("\n");
  if (at < 0) return { kind: "lost" };
  return heard(data.slice(0, at), data.slice(at + 1));
}

/** The socket's address for `url`, an address of the page's: its `ws:` (or `wss:`) twin. */
export function socketAddress(url: URL): string {
  const socket = new URL(url);
  socket.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return socket.href;
}

/**
 * The stream at `url` (a path under the project), each thing it says handed to `said`; none when
 * it cannot be opened or the page is away. Once lost it says nothing more, and once closed (by the
 * function it gives, or as the page goes away) nothing at all: a socket closed tells of its close
 * later.
 */
export function openFeed(url: string, said: (heard: Heard) => void): (() => void) | null {
  if (isAway()) return null;
  let socket: WebSocket;
  try {
    socket = new WebSocket(socketAddress(new URL(here(project(), url), location.href)));
  } catch {
    return null;
  }
  let ended = false;
  const say = (heard: Heard) => {
    if (ended) return;
    if (heard.kind === "lost") ended = true;
    said(heard);
  };
  const close = () => {
    ended = true;
    socket.close();
    targets().forEach((target, at) => {
      target.removeEventListener(LEAVING[at] as string, leaving);
    });
  };
  const leaving = () => {
    // Before the page is put away, whichever of its listeners runs first.
    away = true;
    if (ended) return;
    close();
    said({ kind: "away" });
  };
  socket.addEventListener("open", () => say(heard("open", undefined)));
  socket.addEventListener("message", (event) => say(message(event.data)));
  // A socket that fails is closed too; the server ending the feed closes it.
  socket.addEventListener("error", () => say({ kind: "lost" }));
  socket.addEventListener("close", () => say({ kind: "lost" }));
  isAway();
  targets().forEach((target, at) => {
    target.addEventListener(LEAVING[at] as string, leaving);
  });
  return close;
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

  /** The next thing said, or `null` when `ms` pass first (the wait's deadline) or `signal`
   * aborts. */
  next(ms: number, signal?: AbortSignal): Promise<T | null> {
    const first = this.queued.shift();
    if (first !== undefined) return Promise.resolve(first);
    return new Promise((resolve) => {
      const done = (value: T | null) => {
        ended();
        signal?.removeEventListener("abort", aborted);
        this.waiting = null;
        resolve(value);
      };
      const aborted = () => done(null);
      const ended = deadline(ms, aborted);
      if (signal?.aborted) return aborted();
      signal?.addEventListener("abort", aborted);
      this.waiting = done;
    });
  }
}
