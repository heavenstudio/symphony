import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ANSWER_MS,
  answerMs,
  cursorOf,
  Failure,
  getAt,
  getDocument,
  getIf,
  getJson,
  inTime,
  NO_ANSWER,
  post,
  said,
} from "./request";
import { NOT_IN_TIME, UNREACHABLE } from "./trouble";
import { VERSION_HEADER } from "./wire";

/** A response of `status` with the JSON `body` and the headers given. */
const answer = (status: number, body?: unknown, headers: Record<string, string> = {}) =>
  new Response(body === undefined ? null : JSON.stringify(body), { status, headers });

/** A `200` whose head came and whose body never does. */
const stalled = () =>
  ({
    status: 200,
    ok: true,
    headers: new Headers(),
    json: () => new Promise(() => {}),
  }) as Response;

let fetched: ReturnType<typeof vi.fn>;

/** The signal of a read nothing gives up. */
const STAYS = new AbortController().signal;

/**
 * Fetches that never answer and, as the browser's, refuse once their signal ends, at once for one
 * ended already: the signals they were given, in order.
 */
function unanswered(): AbortSignal[] {
  const signals: AbortSignal[] = [];
  fetched.mockImplementation(
    (_: string, init: RequestInit) =>
      new Promise((_answer, refuse) => {
        const signal = init.signal;
        if (!signal) return;
        signals.push(signal);
        const aborted = () => refuse(new DOMException("aborted", "AbortError"));
        if (signal.aborted) aborted();
        signal.addEventListener("abort", aborted);
      }),
  );
  return signals;
}

beforeEach(() => {
  fetched = vi.fn();
  vi.stubGlobal("fetch", fetched);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("a read", () => {
  it("is the body the server answered", async () => {
    fetched.mockResolvedValue(answer(200, { n: 1 }));
    expect(await getJson("/api/activity", STAYS)).toEqual({ n: 1 });
    expect(fetched.mock.calls[0]?.[0]).toBe("/api/activity");
  });

  it("says the server's own error with its status", async () => {
    fetched.mockResolvedValue(answer(400, { error: "No such card." }));
    await expect(getJson("/api/x", STAYS)).rejects.toEqual(
      new Failure("known", "No such card (the server answered 400)", "No such card.", {
        error: "No such card.",
      }),
    );
  });

  it("is missing for a 404, unknown for a server error", async () => {
    fetched.mockResolvedValue(answer(404, {}));
    await expect(getJson("/api/x", STAYS)).rejects.toMatchObject({ kind: "missing" });
    fetched.mockResolvedValue(answer(503, "not json"));
    await expect(getJson("/api/x", STAYS)).rejects.toMatchObject({
      kind: "uncertain",
      text: "The server answered 503",
    });
  });

  it("says the server could not be reached when the fetch fails", async () => {
    fetched.mockRejectedValue(new TypeError("Failed to fetch"));
    await expect(getJson("/api/x", STAYS)).rejects.toEqual(
      new Failure("uncertain", UNREACHABLE, "Failed to fetch"),
    );
  });

  it("fails as unknown when its body does not parse", async () => {
    fetched.mockResolvedValue(new Response("{", { status: 200 }));
    await expect(getJson("/api/x", STAYS)).rejects.toMatchObject({ kind: "uncertain" });
  });

  it("fails when no answer comes in time, aborting the fetch as timed out", async () => {
    vi.useFakeTimers();
    let signal: AbortSignal | undefined;
    fetched.mockImplementation((_: string, init: RequestInit) => {
      signal = init.signal ?? undefined;
      return new Promise(() => {});
    });
    const read = getJson("/api/x", STAYS);
    // A read's own words: nothing was sent to check before sending again.
    const failed = expect(read).rejects.toEqual(new Failure("uncertain", NOT_IN_TIME));
    await vi.advanceTimersByTimeAsync(ANSWER_MS);
    await failed;
    expect(signal?.aborted).toBe(true);
    // What the shell's count of the page's reads takes as a read that got no answer.
    expect(signal?.reason).toMatchObject({ name: "TimeoutError", message: NOT_IN_TIME });
  });

  it("fails in its own words when its body does not come in time", async () => {
    vi.useFakeTimers();
    fetched.mockResolvedValue(stalled());
    const read = getJson("/api/x", STAYS);
    const failed = expect(read).rejects.toEqual(new Failure("uncertain", NOT_IN_TIME));
    await vi.advanceTimersByTimeAsync(ANSWER_MS);
    await failed;
  });

  it("waits as long as the page names, else ANSWER_MS", async () => {
    vi.useFakeTimers();
    expect(answerMs()).toBe(ANSWER_MS);
    for (const ignored of [0, -5, "500", Number.NaN]) {
      vi.stubGlobal("symphonyAnswerMs", ignored);
      expect(answerMs(), String(ignored)).toBe(ANSWER_MS);
    }
    vi.stubGlobal("symphonyAnswerMs", 500);
    expect(answerMs()).toBe(500);
    const signals = unanswered();
    let settled = false;
    const read = getJson("/api/x", STAYS).finally(() => {
      settled = true;
    });
    const failed = expect(read).rejects.toEqual(new Failure("uncertain", NOT_IN_TIME));
    await vi.advanceTimersByTimeAsync(499);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    await failed;
    expect(signals[0]?.aborted).toBe(true);
  });

  it("refuses a 304 it did not ask for", async () => {
    fetched.mockResolvedValue(new Response(null, { status: 304 }));
    await expect(getJson("/api/x", STAYS)).rejects.toMatchObject({ kind: "uncertain" });
  });

  it("is given up, its fetch aborted, once the signal it was given ends", async () => {
    const signals = unanswered();
    const replaced = new AbortController();
    const read = getJson("/api/x", replaced.signal);
    expect(signals[0]?.aborted).toBe(false);
    replaced.abort();
    await expect(read).rejects.toEqual(new Failure("uncertain", UNREACHABLE, "aborted"));
    expect(signals[0]?.aborted).toBe(true);
    // Given up, not timed out: the shell's count of the page's reads leaves it out.
    expect(signals[0]?.reason).toMatchObject({ name: "AbortError" });
    const gone = new AbortController();
    gone.abort();
    await expect(getJson("/api/x", gone.signal)).rejects.toMatchObject({ kind: "uncertain" });
    expect(signals[1]?.aborted).toBe(true);
  });

  it("answered before its signal ends leaves the signal as it was", async () => {
    fetched.mockResolvedValue(answer(200, { n: 1 }));
    const kept = new AbortController();
    const listened = vi.spyOn(kept.signal, "removeEventListener");
    expect(await getJson("/api/x", kept.signal)).toEqual({ n: 1 });
    expect(listened).toHaveBeenCalledWith("abort", expect.any(Function));
    kept.abort();
  });
});

describe("a read of what this device kept", () => {
  it("names the kept copy's tag, past the browser's cache, and is the same on a 304", async () => {
    fetched.mockResolvedValue(new Response(null, { status: 304 }));
    expect(await getIf("/api/queue", '"e1"', STAYS)).toEqual({ same: true });
    expect(fetched.mock.calls[0]?.[1]).toMatchObject({
      cache: "no-store",
      headers: { "If-None-Match": '"e1"' },
    });
  });

  it("is the new value with its tag, and names none for a copy without one", async () => {
    fetched.mockResolvedValue(answer(200, { n: 2 }, { ETag: '"e2"' }));
    expect(await getIf("/api/queue", "", STAYS)).toEqual({
      same: false,
      value: { n: 2 },
      etag: '"e2"',
    });
    expect(fetched.mock.calls[0]?.[1]).not.toHaveProperty("headers");
  });

  it("is given up, its fetch aborted, once the signal it was given ends", async () => {
    const signals = unanswered();
    const page = new AbortController();
    const read = getIf("/api/queue", '"e1"', page.signal);
    expect(signals[0]?.aborted).toBe(false);
    page.abort();
    await expect(read).rejects.toEqual(new Failure("uncertain", UNREACHABLE, "aborted"));
    expect(signals[0]?.aborted).toBe(true);
    await expect(getIf("/api/queue", "", page.signal)).rejects.toMatchObject({
      kind: "uncertain",
    });
    expect(signals[1]?.aborted).toBe(true);
  });
});

describe("a document's read", () => {
  it("is its answer with the feed's cursor it names, on a 304 as on a 200", async () => {
    fetched.mockResolvedValue(
      new Response(null, { status: 304, headers: { [VERSION_HEADER]: "41" } }),
    );
    expect(await getDocument("/api/document?path=docs%2FA.md", '"e1"', STAYS)).toEqual({
      answer: { same: true },
      cursor: 41,
    });
    expect(fetched.mock.calls[0]?.[1]).toMatchObject({ headers: { "If-None-Match": '"e1"' } });
    fetched.mockResolvedValue(answer(200, { n: 1 }, { ETag: '"e2"', [VERSION_HEADER]: "42" }));
    expect(await getDocument("/api/document", "", STAYS)).toEqual({
      answer: { same: false, value: { n: 1 }, etag: '"e2"' },
      cursor: 42,
    });
    fetched.mockResolvedValue(answer(200, { n: 1 }));
    expect((await getDocument("/api/document", "", STAYS))?.cursor).toBeNull();
  });

  it("is none for a document the server does not have, and fails as any read else", async () => {
    fetched.mockResolvedValue(answer(404, { error: "No such document." }));
    expect(await getDocument("/api/document", "", STAYS)).toBeNull();
    fetched.mockResolvedValue(answer(500));
    await expect(getDocument("/api/document", "", STAYS)).rejects.toMatchObject({
      kind: "uncertain",
    });
    fetched.mockRejectedValue(new TypeError("Failed to fetch"));
    await expect(getDocument("/api/document", "", STAYS)).rejects.toEqual(
      new Failure("uncertain", UNREACHABLE, "Failed to fetch"),
    );
  });

  it("as any read naming its cursor, names it on every answer and fails a 404 as missing", async () => {
    fetched.mockResolvedValue(answer(200, { n: 1 }, { ETag: '"g1"', [VERSION_HEADER]: "7" }));
    expect(await getAt("/api/task-gates?slug=a", "", STAYS)).toEqual({
      answer: { same: false, value: { n: 1 }, etag: '"g1"' },
      cursor: 7,
    });
    fetched.mockResolvedValue(
      new Response(null, { status: 304, headers: { [VERSION_HEADER]: "8" } }),
    );
    expect(await getAt("/api/task-gates?slug=a", '"g1"', STAYS)).toEqual({
      answer: { same: true },
      cursor: 8,
    });
    // A server that names none, or names what is no cursor.
    fetched.mockResolvedValue(answer(200, { n: 1 }, { [VERSION_HEADER]: "soon" }));
    expect((await getAt("/api/task-gates?slug=a", "", STAYS)).cursor).toBeNull();
    fetched.mockResolvedValue(answer(404, { error: "No such task." }));
    await expect(getAt("/api/task-gates?slug=a", "", STAYS)).rejects.toMatchObject({
      kind: "missing",
    });
  });

  it("is given up, its fetch aborted, once the signal it was given ends: a failure, not none", async () => {
    const signals = unanswered();
    const page = new AbortController();
    const read = getDocument("/api/document", '"e1"', page.signal);
    expect(signals[0]?.aborted).toBe(false);
    page.abort();
    await expect(read).rejects.toEqual(new Failure("uncertain", UNREACHABLE, "aborted"));
    expect(signals[0]?.aborted).toBe(true);
  });

  it.each([
    ["0", 0],
    ["+7", 7],
    ["007", 7],
    [String(Number.MAX_SAFE_INTEGER), Number.MAX_SAFE_INTEGER],
    [null, null],
    ["", null],
    ["+", null],
    ["-1", null],
    [" 1", null],
    ["1.5", null],
    ["1e3", null],
    ["0x10", null],
    ["18446744073709551615", null],
  ])("names the cursor %j as %j", (header, cursor) => {
    expect(cursorOf(header)).toBe(cursor);
  });
});

describe("a post", () => {
  beforeEach(() => vi.stubGlobal("performance", { mark: vi.fn() }));

  it("sends its fields as JSON, and is the reply's body", async () => {
    fetched.mockResolvedValue(answer(200, { next: "/docs/A.md#b" }));
    expect(await post("/api/postpone", { slug: "s" })).toEqual({ next: "/docs/A.md#b" });
    expect(fetched.mock.calls[0]?.[1]).toMatchObject({
      method: "POST",
      keepalive: true,
      body: '{"slug":"s"}',
      headers: { "Content-Type": "application/json" },
    });
    const marks = vi.mocked(performance.mark).mock.calls.map(([name]) => name);
    expect(marks[0]).toMatch(/^send \d+ POST \/api\/postpone start$/);
    expect(marks.at(-1)).toMatch(/ ok$/);
  });

  it("says the server's own words alone, or its status", async () => {
    fetched.mockResolvedValue(answer(409, { error: "The card moved." }));
    await expect(post("/api/postpone", {})).rejects.toEqual(
      // The refusal's body goes with it, for a page that reads more of it than its error.
      new Failure("known", "The card moved.", "The card moved.", { error: "The card moved." }),
    );
    fetched.mockResolvedValue(answer(500));
    await expect(post("/api/postpone", {})).rejects.toEqual(
      new Failure("uncertain", "The server answered 500", "the server answered 500"),
    );
  });

  it("says why the fetch failed, as the server may have acted", async () => {
    fetched.mockRejectedValue(new TypeError("Load failed"));
    await expect(post("/api/postpone", {})).rejects.toEqual(
      new Failure("uncertain", "TypeError: Load failed", "Load failed"),
    );
  });

  it("keeps its own words when no answer, or no body, comes in time: it may have acted", async () => {
    vi.useFakeTimers();
    let signal: AbortSignal | undefined;
    fetched.mockImplementation((_: string, init: RequestInit) => {
      signal = init.signal ?? undefined;
      return new Promise(() => {});
    });
    const sent = post("/api/postpone", {});
    const failed = expect(sent).rejects.toEqual(new Failure("uncertain", NO_ANSWER));
    await vi.advanceTimersByTimeAsync(ANSWER_MS);
    await failed;
    expect(signal?.reason).toMatchObject({ name: "TimeoutError", message: NO_ANSWER });
    fetched.mockResolvedValue(stalled());
    const again = post("/api/postpone", {});
    const late = expect(again).rejects.toEqual(new Failure("uncertain", NO_ANSWER));
    await vi.advanceTimersByTimeAsync(ANSWER_MS);
    await late;
  });
});

describe("in time", () => {
  it("is the answer, its failure, or none when the time is up first", async () => {
    vi.useFakeTimers();
    expect(await inTime(Promise.resolve(1), 10)).toEqual({ value: 1 });
    await expect(inTime(Promise.reject(new Error("no")), 10)).rejects.toThrow("no");
    const late = inTime(new Promise(() => {}), 10);
    await vi.advanceTimersByTimeAsync(10);
    expect(await late).toBeNull();
  });
});

describe("what a failure says", () => {
  it("is its own text, or the value's", () => {
    expect(said(new Failure("known", "No."))).toBe("No.");
    expect(said("plain")).toBe("plain");
  });
});
