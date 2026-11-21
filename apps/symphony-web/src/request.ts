// The browser's JSON transport, under the page's project's base: reads, reads of what this device
// kept (naming its tag), and posts. A read's failure is what a page says in place of what it could
// not show (trouble.tsx): the server's own error with its status, that the server could not be
// reached, or that it did not answer in time. A post's keeps the server's words alone, beside the
// draft it kept.

import { here, project } from "./project";
import { deadline } from "./timing";
import { NOT_IN_TIME, answered as refusedRead, UNREACHABLE } from "./trouble";
import { VERSION_HEADER } from "./wire";

/** How long a request waits for the server's response, and then again for its body. */
export const ANSWER_MS = 15_000;
/** What a post fails with when no answer came in time: the server may have acted on it. A read
 * fails with trouble.tsx's NOT_IN_TIME. */
export const NO_ANSWER = "✖ no answer from the server; check before sending again";

/**
 * How long a request waits: ANSWER_MS, unless the page names another (`symphonyAnswerMs`, a
 * positive number of ms a browser test's script sets, so a read held past the limit is tested
 * without waiting 15 s).
 */
export function answerMs(): number {
  const named = (globalThis as { symphonyAnswerMs?: unknown }).symphonyAnswerMs;
  return typeof named === "number" && named > 0 ? named : ANSWER_MS;
}

/** How a request failed: refused for good, unknown (the server may have acted), or missing. */
export type FailureKind = "known" | "uncertain" | "missing";

export class Failure extends Error {
  constructor(
    readonly kind: FailureKind,
    readonly text: string,
    /**
     * What the server or the browser said of it, in their own words, before this client's around
     * them: a refusal's `error` (else that the server answered its status), the message of the
     * error a fetch or a body's read failed with. `text` where neither said anything.
     */
    readonly own: string = text,
  ) {
    super(text);
  }

  /** A refusal by the server's status: a 404 is missing, a server error leaves it unknown. */
  static refused(status: number, text: string, own = text): Failure {
    if (status === 404) return new Failure("missing", text, own);
    return new Failure(status >= 500 ? "uncertain" : "known", text, own);
  }
}

/** What a read naming the copy the page holds answered. */
export type Answer =
  /** `304`: the copy is current. */
  | { same: true }
  /** The body as the browser parsed it, and the tag the server answered it with. */
  | { same: false; value: unknown; etag: string };

/** `answer`, or `null` when `ms` passed first (the answer's deadline). */
export function inTime<T>(answer: Promise<T>, ms: number): Promise<{ value: T } | null> {
  return new Promise((resolve, reject) => {
    const ended = deadline(ms, () => resolve(null));
    answer.then(
      (value) => {
        ended();
        resolve({ value });
      },
      (error: unknown) => {
        ended();
        reject(error);
      },
    );
  });
}

let sent = 0;

/** Marks a post's way on the page's performance timeline as `send N POST PATH PHASE`. */
function marker(post: boolean, url: string): (phase: string) => void {
  if (!post) return () => {};
  const name = `send ${sent++} POST ${url.split("?")[0]}`;
  return (phase) => {
    performance.mark(`${name} ${phase}`);
  };
}

/**
 * One request: a POST of `payload`, else a GET naming `etag` when one is given (a kept read). Its
 * answer, and the headers it came with. The request is given up once `signal` ends, its fetch
 * failing with an `AbortError`, and when no answer comes in time, with a `TimeoutError`: the
 * shell's count of the page's reads (shell/shell.ts) takes the second as a read that got no
 * answer, and the first as none.
 */
async function exchange(
  url: string,
  payload?: unknown,
  etag?: string,
  signal?: AbortSignal,
): Promise<[Answer, Headers]> {
  const reading = payload === undefined;
  const mark = marker(!reading, url);
  const init: RequestInit = {};
  if (etag !== undefined) {
    // Past the browser's own cache, which the API's `no-store` keeps empty anyway.
    init.cache = "no-store";
    if (etag !== "") init.headers = { "If-None-Match": etag };
  }
  if (!reading) {
    init.method = "POST";
    init.keepalive = true;
    init.body = JSON.stringify(payload);
    init.headers = { "Content-Type": "application/json" };
  }
  const controller = new AbortController();
  init.signal = controller.signal;
  const giveUp = () => controller.abort();
  if (signal?.aborted) giveUp();
  signal?.addEventListener("abort", giveUp, { once: true });
  try {
    return await exchanged(url, init, reading, controller, mark);
  } finally {
    signal?.removeEventListener("abort", giveUp);
  }
}

/** `exchange`'s request sent with `init`, which `controller` gives up. */
async function exchanged(
  url: string,
  init: RequestInit,
  reading: boolean,
  controller: AbortController,
  mark: (phase: string) => void,
): Promise<[Answer, Headers]> {
  mark("start");
  const fetched = fetch(here(project(), url), init);
  fetched.then(
    () => mark("response"),
    () => mark("response"),
  );
  let response: { value: Response } | null;
  try {
    response = await inTime(fetched, answerMs());
  } catch (error) {
    mark("failed");
    throw new Failure("uncertain", reading ? UNREACHABLE : String(error), said(error));
  }
  if (response === null) {
    const late = reading ? NOT_IN_TIME : NO_ANSWER;
    controller.abort(new DOMException(late, "TimeoutError"));
    mark("no-answer");
    throw new Failure("uncertain", late);
  }
  mark("fetched");
  return [await answer(response.value, reading, mark), response.value.headers];
}

/** A response's answer: `same` for a `304`; else its JSON body with its tag, or its refusal. */
async function answer(
  response: Response,
  reading: boolean,
  mark: (phase: string) => void,
): Promise<Answer> {
  if (response.status === 304) {
    mark("ok");
    return { same: true };
  }
  const etag = response.headers.get("ETag") ?? "";
  const parsing = response.json();
  parsing.then(
    () => mark("body"),
    () => mark("body"),
  );
  let body: { value: unknown } | null;
  let unreadable: unknown;
  try {
    body = await inTime(parsing, answerMs());
  } catch (error) {
    body = null;
    unreadable = error ?? "unreadable";
  }
  mark("read");
  if (!response.ok) {
    const said = body?.value as { error?: unknown } | undefined;
    const error = typeof said?.error === "string" ? said.error : undefined;
    const text = reading
      ? refusedRead(response.status, error)
      : (error ?? `The server answered ${response.status}`);
    mark("refused");
    throw Failure.refused(response.status, text, error ?? `the server answered ${response.status}`);
  }
  if (unreadable !== undefined) {
    mark("failed");
    throw new Failure("uncertain", String(unreadable), said(unreadable));
  }
  if (body === null) {
    mark("no-answer");
    throw new Failure("uncertain", reading ? NOT_IN_TIME : NO_ANSWER);
  }
  mark("ok");
  return { same: false, value: body.value, etag };
}

/** A read: its body as the browser parsed it. Given up, its request with it, once `signal` ends. */
export async function getJson(url: string, signal: AbortSignal): Promise<unknown> {
  const [answered] = await exchange(url, undefined, undefined, signal);
  if (answered.same) throw new Failure("uncertain", "The server answered 304 to a plain read");
  return answered.value;
}

/**
 * A read of something this device kept: it names the kept copy's tag (`If-None-Match`, when there
 * is one) and goes past the browser's cache. Given up, its request with it, once `signal` ends.
 */
export async function getIf(url: string, etag: string, signal: AbortSignal): Promise<Answer> {
  return (await exchange(url, undefined, etag, signal))[0];
}

/** What a read answered, and the feed's cursor it was read at. */
export interface AnswerAt {
  answer: Answer;
  /** The cursor the server named (`VERSION_HEADER`), on its `200` and its `304` alike: every feed
   * event at or before it is in the answer. None from a server that names none. */
  cursor: number | null;
}

/** What a document read answered, and the feed's cursor it was read at. */
export type DocumentAnswer = AnswerAt;

/**
 * The feed's cursor a header names: a whole number, as Rust's `u64` parses it (an optional `+`,
 * then digits), within the safe integers; null for any other or none.
 */
export function cursorOf(header: string | null): number | null {
  if (header === null || !/^\+?[0-9]+$/.test(header)) return null;
  const cursor = Number(header);
  return Number.isSafeInteger(cursor) ? cursor : null;
}

/**
 * A read naming the copy this device kept (`getIf`), with the feed's cursor it was read at, so a
 * page drops what its feed says of the read at or before it, and one that draws its kept copy on
 * a `304`, which has no body, opens the feed from it. Given up, as `getIf`, once `signal` ends.
 */
export async function getAt(url: string, etag: string, signal: AbortSignal): Promise<AnswerAt> {
  const [answer, headers] = await exchange(url, undefined, etag, signal);
  return { answer, cursor: cursorOf(headers.get(VERSION_HEADER)) };
}

/** A document read (`getAt`); null when the server has no such document (a `404`). */
export async function getDocument(
  url: string,
  etag: string,
  signal: AbortSignal,
): Promise<DocumentAnswer | null> {
  try {
    return await getAt(url, etag, signal);
  } catch (failure) {
    if (failure instanceof Failure && failure.kind === "missing") return null;
    throw failure;
  }
}

/** A post of `fields` as JSON: the reply's body. */
export async function post(url: string, fields: Record<string, unknown>): Promise<unknown> {
  const [answered] = await exchange(url, fields);
  if (answered.same) throw new Failure("uncertain", "The server answered 304 to a plain read");
  return answered.value;
}

/** What a failed request says: its own text, or the error's. */
export function said(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
