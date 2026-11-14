// The browser's JSON transport, under the page's project's base: reads, reads of what this device
// kept (naming its tag), and posts. A read's failure is what a page says in place of what it could
// not show (trouble.tsx): the server's own error with its status, or that the server could not be
// reached. A post's keeps the server's words alone, beside the draft it kept.

import { here, project } from "./project";
import { answered as refusedRead, UNREACHABLE } from "./trouble";

/** How long a request waits for the server's response, and then again for its body. */
export const ANSWER_MS = 15_000;
/** What a request fails with when no answer came in time. */
export const NO_ANSWER = "✖ no answer from the server; check before sending again";

/** How a request failed: refused for good, unknown (the server may have acted), or missing. */
export type FailureKind = "known" | "uncertain" | "missing";

export class Failure extends Error {
  constructor(
    readonly kind: FailureKind,
    readonly text: string,
  ) {
    super(text);
  }

  /** A refusal by the server's status: a 404 is missing, a server error leaves it unknown. */
  static refused(status: number, text: string): Failure {
    if (status === 404) return new Failure("missing", text);
    return new Failure(status >= 500 ? "uncertain" : "known", text);
  }
}

/** What a read naming the copy the page holds answered. */
export type Answer =
  /** `304`: the copy is current. */
  | { same: true }
  /** The body as the browser parsed it, and the tag the server answered it with. */
  | { same: false; value: unknown; etag: string };

/** `answer`, or `null` when `ms` passed first. */
export function inTime<T>(answer: Promise<T>, ms: number): Promise<{ value: T } | null> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => resolve(null), ms);
    answer.then(
      (value) => {
        clearTimeout(timer);
        resolve({ value });
      },
      (error: unknown) => {
        clearTimeout(timer);
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

/** One request: a POST of `payload`, else a GET naming `etag` when one is given (a kept read). */
async function exchange(url: string, payload?: unknown, etag?: string): Promise<Answer> {
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
  mark("start");
  const fetched = fetch(here(project(), url), init);
  fetched.then(
    () => mark("response"),
    () => mark("response"),
  );
  let response: { value: Response } | null;
  try {
    response = await inTime(fetched, ANSWER_MS);
  } catch (error) {
    mark("failed");
    throw new Failure("uncertain", reading ? UNREACHABLE : String(error));
  }
  if (response === null) {
    controller.abort();
    mark("no-answer");
    throw new Failure("uncertain", NO_ANSWER);
  }
  mark("fetched");
  return answer(response.value, reading, mark);
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
    body = await inTime(parsing, ANSWER_MS);
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
    throw Failure.refused(response.status, text);
  }
  if (unreadable !== undefined) {
    mark("failed");
    throw new Failure("uncertain", String(unreadable));
  }
  if (body === null) {
    mark("no-answer");
    throw new Failure("uncertain", NO_ANSWER);
  }
  mark("ok");
  return { same: false, value: body.value, etag };
}

/** A read: its body as the browser parsed it. */
export async function getJson(url: string): Promise<unknown> {
  const answered = await exchange(url);
  if (answered.same) throw new Failure("uncertain", "The server answered 304 to a plain read");
  return answered.value;
}

/**
 * A read of something this device kept: it names the kept copy's tag (`If-None-Match`, when there
 * is one) and goes past the browser's cache.
 */
export function getIf(url: string, etag: string): Promise<Answer> {
  return exchange(url, undefined, etag);
}

/** A post of `fields` as JSON: the reply's body. */
export async function post(url: string, fields: Record<string, unknown>): Promise<unknown> {
  const answered = await exchange(url, fields);
  if (answered.same) throw new Failure("uncertain", "The server answered 304 to a plain read");
  return answered.value;
}

/** What a failed request says: its own text, or the error's. */
export function said(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
