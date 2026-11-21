import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FRESH, opened, shown as shownFromDevice, taskItem, waits } from "./cache";
import { KEPT_EVENTS, type Part, Parts, PartsContext, type Shown, usePart } from "./parts";
import { field } from "./shared";
import { VERSION_HEADER } from "./wire";

/** A response as a read takes one: its status, its JSON body, its tag and the cursor it names. */
function answer(status: number, body: unknown = null, etag = "", version?: number): Response {
  const headers: Record<string, string> = {};
  if (etag !== "") headers.ETag = etag;
  if (version !== undefined) headers[VERSION_HEADER] = `${version}`;
  return {
    status,
    ok: status >= 200 && status < 300,
    headers: { get: (name: string) => headers[name] ?? null },
    json: async () => body,
  } as unknown as Response;
}

/** A count the tests' server answers, and what the tests' feed says of one. */
interface Count {
  n: number;
}
interface Said {
  key: string;
  add?: number;
  again?: boolean;
  gone?: boolean;
}

/** The part `key`: a count, which an event of its key adds to, reads again or says gone. */
function count(key = "count", more: Partial<Part<Count, Said>> = {}): Part<Count, Said> {
  return {
    key,
    url: `/api/${key}`,
    read: (value) => (typeof field(value, "n") === "number" ? (value as Count) : null),
    hear: (value, event) => {
      if (event.key !== key) return null;
      if (event.gone) return { value: undefined, gone: true };
      const next = value === undefined || !event.add ? value : { n: value.n + event.add };
      return { value: next, again: event.again === true };
    },
    ...more,
  };
}

const ITEM = taskItem("", "w/a");
/** The part `count`, kept on this device as the task `w/a`'s. */
const keptCount = (more: Partial<Part<Count, Said>> = {}) =>
  count("count", { kept: { item: ITEM, part: "count" }, ...more });

type Route = (url: string, etag: string) => Response | Promise<Response>;

interface Kept {
  value: unknown;
  etag: string;
  saved: number;
}

/**
 * A store over a server that answers each read as `route` does, on a device that kept `kept` (by
 * the item's id and part): what it asked, what its host heard and what the device's store was told.
 */
function page(route: Route = () => answer(503), kept: Record<string, Kept> = {}) {
  const asked: [string, string][] = [];
  const given: AbortSignal[] = [];
  vi.stubGlobal("document", { getElementById: () => null });
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    const etag = (init?.headers as Record<string, string> | undefined)?.["If-None-Match"] ?? "";
    asked.push([url, etag]);
    if (init?.signal) given.push(init.signal);
    return route(url, etag);
  });
  const device = {
    read: vi.fn(
      async (_format: number, _project: string, id: string, part: string) => kept[`${id} ${part}`],
    ),
    keep: vi.fn(async () => undefined),
    visit: vi.fn(async () => undefined),
    forget: vi.fn(async () => undefined),
  };
  vi.stubGlobal("symphonyCache", device);
  const host = { freshness: vi.fn(), answered: vi.fn() };
  const mount = new AbortController();
  const parts = new Parts<Said>(mount.signal, host);
  const drawn = vi.fn();
  parts.subscribe(drawn);
  return { parts, asked, given, host, device, mount, drawn };
}

/** A promise the test settles: what a read waits on while the test does something meanwhile. */
function later<T>(): { promise: Promise<T>; settle: (value: T) => void } {
  let settle: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => {
    settle = done;
  });
  return { promise, settle };
}

const NOTHING: Shown<Count> = {
  value: undefined,
  stale: false,
  error: null,
  retrying: false,
  missing: false,
  answered: false,
};
const drawnAs = (n: number): Shown<Count> => ({ ...NOTHING, value: { n }, answered: true });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  performance.clearMarks();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** Lets `ms` pass, and what it settles run. */
const pass = (ms = 0) => vi.advanceTimersByTimeAsync(ms);
const marks = () => performance.getEntriesByType("mark").map((mark) => mark.name);

describe("a part opened", () => {
  it("shows nothing before, then its read's answer, marked and told once", async () => {
    const { parts, asked, host, device, drawn } = page(() => answer(200, { n: 1 }, '"a"', 7));
    const part = count("count", { first: true });
    expect(parts.shown(part)).toEqual(NOTHING);
    parts.open(part);
    await pass();
    expect(parts.shown(part)).toEqual(drawnAs(1));
    expect(asked).toEqual([["/api/count", ""]]);
    expect(host.answered.mock.calls).toEqual([["count"]]);
    expect(marks()).toEqual(["part count", "first-part"]);
    // Kept nowhere: the part names no place on the device.
    expect(device.read).not.toHaveBeenCalled();
    expect(device.keep).not.toHaveBeenCalled();
    expect(drawn).toHaveBeenCalledTimes(1);
    // Answered again, it is marked no more.
    await parts.read(part);
    expect(marks()).toHaveLength(2);
  });

  it("is read once however many name it, and an ask while its read is out joins it", async () => {
    const held = later<Response>();
    const { parts, asked } = page(() => held.promise);
    const part = count();
    parts.open(part);
    parts.open(count());
    await pass();
    const first = parts.read(part);
    const second = parts.read(count());
    expect(asked).toHaveLength(1);
    held.settle(answer(200, { n: 2 }, "", 3));
    await Promise.all([first, second]);
    expect(parts.shown(part)).toEqual(drawnAs(2));
    expect(asked).toHaveLength(1);
  });

  it("draws this device's copy at once, stale, and a 304 naming its tag confirms it", async () => {
    const held = later<Response>();
    const { parts, asked, host, device } = page(() => held.promise, {
      "task:w/a count": { value: { n: 4 }, etag: '"k"', saved: 1_000 },
    });
    const part = keptCount();
    parts.open(part);
    await pass();
    expect(parts.shown(part)).toEqual({ ...NOTHING, value: { n: 4 }, stale: true });
    expect(waits(parts.fresh, "count")).toBe(true);
    expect(shownFromDevice(parts.fresh)).toEqual([1_000, false]);
    expect(asked).toEqual([["/api/count", '"k"']]);
    held.settle(answer(304, null, "", 9));
    await pass();
    expect(parts.shown(part)).toEqual(drawnAs(4));
    expect(waits(parts.fresh, "count")).toBe(false);
    expect(device.keep).not.toHaveBeenCalled();
    expect(host.freshness).toHaveBeenCalledTimes(2);
  });

  it("replaces the copy with a new answer, kept on the device with its tag", async () => {
    const { parts, device } = page(() => answer(200, { n: 5 }, '"new"', 9), {
      "task:w/a count": { value: { n: 4 }, etag: '"k"', saved: 1_000 },
    });
    const part = keptCount();
    parts.open(part);
    await pass();
    expect(parts.shown(part)).toEqual(drawnAs(5));
    const [, , id, kind, name, etag, value] = device.keep.mock.calls[0] as unknown as unknown[];
    expect([id, kind, name, etag, value]).toEqual(["task:w/a", "task", "count", '"new"', { n: 5 }]);
  });

  it("draws no copy the part says is not its own, nor one over an answer already drawn", async () => {
    const copy = { "task:w/a count": { value: { n: 4 }, etag: '"k"', saved: 1_000 } };
    const other = page(() => answer(200, { n: 6 }, "", 1), copy);
    const part = keptCount({ absent: (value) => value.n === 4 });
    other.parts.open(part);
    await pass();
    expect(other.asked).toEqual([["/api/count", ""]]);
    expect(other.parts.shown(part)).toEqual(drawnAs(6));
  });

  it("gives its answer the shape the part merges it to, told what was shown before", async () => {
    const seen: [Count | undefined, boolean][] = [];
    const merge = (next: Count, held: Count | undefined, fresh: boolean) => {
      seen.push([held, fresh]);
      return { n: next.n + (held?.n ?? 0) };
    };
    const { parts } = page(() => answer(200, { n: 1 }, "", 1), {
      "task:w/a count": { value: { n: 10 }, etag: '"k"', saved: 1 },
    });
    const part = keptCount({ merge });
    parts.open(part);
    await pass();
    await parts.read(part);
    // The copy alone, then the answer over the copy, then an answer over an answer.
    expect(seen).toEqual([
      [undefined, false],
      [{ n: 10 }, false],
      [{ n: 11 }, true],
    ]);
    expect(parts.shown(part).value).toEqual({ n: 12 });
  });
});

describe("a part's read that fails", () => {
  it("says why, keeps what is shown, and is tried again after each wait until it answers", async () => {
    let fails = 2;
    const { parts, asked, host } = page(() =>
      fails-- > 0 ? answer(503, { error: "Not ready" }) : answer(200, { n: 1 }, "", 1),
    );
    const part = count();
    parts.open(part);
    await pass();
    expect(parts.shown(part).error?.message).toBe("Not ready (the server answered 503)");
    expect(parts.fresh.failing).toEqual(["count"]);
    expect(asked).toHaveLength(1);
    await pass(999);
    expect(asked).toHaveLength(1);
    await pass(1);
    expect(asked).toHaveLength(2);
    await pass(2_000);
    expect(asked).toHaveLength(3);
    expect(parts.shown(part)).toEqual(drawnAs(1));
    expect(parts.fresh.failing).toEqual([]);
    // Answered, nothing tries again.
    await vi.runAllTimersAsync();
    expect(asked).toHaveLength(3);
    expect(host.answered).toHaveBeenCalledTimes(1);
  });

  it("with nothing drawn, looks on this device again, and its next try names the copy's tag", async () => {
    // The device's store went on without a database slow to open: the first lookup misses.
    const copy: Kept = { value: { n: 4 }, etag: '"k"', saved: 1_000 };
    let fails = 2;
    const { parts, asked, device } = page(() =>
      fails-- > 0 ? answer(503) : answer(304, null, "", 9),
    );
    device.read.mockResolvedValueOnce(undefined).mockResolvedValue(copy);
    const part = keptCount();
    parts.open(part);
    await pass();
    expect(asked).toEqual([["/api/count", ""]]);
    const failed = parts.shown(part);
    expect(failed.error?.message).toBe("The server answered 503");
    expect(failed).toEqual({ ...NOTHING, value: { n: 4 }, stale: true, error: failed.error });
    expect(shownFromDevice(parts.fresh)).toEqual([1_000, true]);
    expect(device.read).toHaveBeenCalledTimes(2);
    // A try that fails over the copy drawn looks for none.
    await pass(1_000);
    expect(asked).toEqual([
      ["/api/count", ""],
      ["/api/count", '"k"'],
    ]);
    expect(device.read).toHaveBeenCalledTimes(2);
    expect(parts.shown(part).value).toEqual({ n: 4 });
    await pass(2_000);
    expect(parts.shown(part)).toEqual(drawnAs(4));
    expect(waits(parts.fresh, "count")).toBe(false);
    expect(parts.fresh.failing).toEqual([]);
  });

  it("with nothing drawn and nothing kept, draws nothing, and looks at each try that fails", async () => {
    const { parts, device } = page(() => answer(503));
    const part = keptCount();
    parts.open(part);
    await pass();
    expect(device.read).toHaveBeenCalledTimes(2);
    expect(parts.shown(part).value).toBeUndefined();
    await pass(1_000);
    expect(device.read).toHaveBeenCalledTimes(3);
    expect(parts.shown(part).value).toBeUndefined();
    expect(shownFromDevice(parts.fresh)).toBeNull();
  });

  it("draws no copy the device gives once the server answered, nor one after the part went", async () => {
    const found = later<Kept | undefined>();
    let fails = 1;
    const { parts, device } = page(() => (fails-- > 0 ? answer(503) : answer(200, { n: 1 }, "", 1)));
    device.read.mockResolvedValueOnce(undefined).mockReturnValue(found.promise);
    const part = keptCount();
    parts.open(part);
    await pass();
    expect(parts.shown(part).error).not.toBeNull();
    await pass(1_000);
    expect(parts.shown(part)).toEqual(drawnAs(1));
    found.settle({ value: { n: 4 }, etag: '"k"', saved: 1_000 });
    await pass();
    expect(parts.shown(part)).toEqual(drawnAs(1));
    expect(shownFromDevice(parts.fresh)).toBeNull();
  });

  it("says a read is out after it, the store's own or a reader's, until that answers or fails", async () => {
    const replies: ReturnType<typeof later<Response>>[] = [];
    const { parts, asked, drawn } = page(() => {
      const reply = later<Response>();
      replies.push(reply);
      return reply.promise;
    });
    const part = count();
    parts.open(part);
    await pass();
    // Its first read is its mock-up's to say.
    expect(parts.shown(part).retrying).toBe(false);
    replies[0]?.settle(answer(503));
    await pass();
    expect(parts.shown(part)).toMatchObject({ retrying: false, answered: false });
    expect(parts.shown(part).error).not.toBeNull();
    // The store's own try again, after its wait.
    await pass(1_000);
    expect(asked).toHaveLength(2);
    expect(parts.shown(part).retrying).toBe(true);
    expect(parts.shown(part).error, "the failure stays said while it is tried").not.toBeNull();
    replies[1]?.settle(answer(503));
    await pass();
    expect(parts.shown(part).retrying).toBe(false);
    // The reader's ask, which the store's next try would join.
    const asking = parts.read(part);
    expect(asked).toHaveLength(3);
    expect(parts.shown(part).retrying).toBe(true);
    replies[2]?.settle(answer(200, { n: 1 }, "", 1));
    await asking;
    expect(parts.shown(part)).toEqual(drawnAs(1));
    // A read over an answer is the answer's to show: nothing more is said, nor drawn.
    const draws = drawn.mock.calls.length;
    const again = parts.read(part);
    expect(parts.shown(part).retrying).toBe(false);
    replies[3]?.settle(answer(200, { n: 1 }, "", 2));
    await again;
    expect(parts.shown(part).retrying).toBe(false);
    expect(drawn.mock.calls.length - draws).toBe(1);
  });

  it("leaves the other parts of the page to draw", async () => {
    const { parts } = page((url) =>
      url === "/api/broken" ? answer(500) : answer(200, { n: 3 }, "", 1),
    );
    const broken = count("broken");
    const sound = count("sound");
    parts.open(broken);
    parts.open(sound);
    await pass();
    expect(parts.shown(sound)).toEqual(drawnAs(3));
    expect(parts.shown(broken).value).toBeUndefined();
    expect(parts.shown(broken).error?.message).toBe("The server answered 500");
  });

  it("keeps the answer shown while a later read fails, and one reader's ask reads at once", async () => {
    let down = false;
    const { parts, asked } = page(() => (down ? answer(500) : answer(200, { n: 1 }, "", 1)));
    const part = count();
    parts.open(part);
    await pass();
    down = true;
    await parts.read(part);
    expect(parts.shown(part)).toMatchObject({ value: { n: 1 }, answered: true });
    expect(parts.shown(part).error).not.toBeNull();
    down = false;
    await parts.read(part);
    expect(asked).toHaveLength(3);
    expect(parts.shown(part)).toEqual(drawnAs(1));
  });

  it("fails an answer the part cannot read in the part's words, and a 304 to a plain read", async () => {
    const unread = page(() => answer(200, { n: "one" }));
    const part = count("count", { unreadable: "The count is not readable" });
    unread.parts.open(part);
    await pass();
    expect(unread.parts.shown(part).error?.message).toBe("The count is not readable");
    const plain = page(() => answer(304));
    plain.parts.open(count());
    await pass();
    expect(plain.parts.shown(count()).error?.message).toBe(
      "The server answered 304 to a plain read",
    );
    const worded = page(() => answer(200, {}));
    worded.parts.open(count());
    await pass();
    expect(worded.parts.shown(count()).error?.message).toBe("The server's answer is not readable");
  });

  it("is missing, for good, where the server has no such thing: nothing kept, nothing tried", async () => {
    const { parts, asked, device, host } = page(() => answer(404, { error: "no such task" }), {
      "task:w/a count": { value: { n: 4 }, etag: '"k"', saved: 1_000 },
    });
    const part = keptCount();
    parts.open(part);
    await pass();
    expect(parts.shown(part)).toEqual({ ...NOTHING, missing: true, answered: true });
    expect(device.forget).toHaveBeenCalledTimes(1);
    expect(shownFromDevice(parts.fresh)).toEqual([1_000, false]);
    expect(waits(parts.fresh, "count")).toBe(false);
    expect(host.answered.mock.calls).toEqual([["count"]]);
    await vi.runAllTimersAsync();
    expect(asked).toHaveLength(1);
  });

  it("is missing where the part reads the answer as another's", async () => {
    const { parts } = page(() => answer(200, { n: 0 }, "", 1));
    const part = count("count", { absent: (value) => value.n === 0 });
    parts.open(part);
    await pass();
    expect(parts.shown(part)).toEqual({ ...NOTHING, missing: true, answered: true });
  });
});

describe("a feed event", () => {
  it("is applied to a part answered before it, and dropped at or before its answer", async () => {
    const { parts } = page(() => answer(200, { n: 1 }, "", 10));
    const part = count();
    parts.open(part);
    await pass();
    parts.heard(9, { key: "count", add: 100 });
    parts.heard(10, { key: "count", add: 100 });
    expect(parts.shown(part)).toEqual(drawnAs(1));
    parts.heard(11, { key: "count", add: 1 });
    expect(parts.shown(part)).toEqual(drawnAs(2));
    // One that says nothing of the part, or leaves it as it is, draws nothing.
    parts.heard(12, { key: "other", add: 1 });
    parts.heard(13, { key: "count" });
    expect(parts.shown(part)).toEqual(drawnAs(2));
  });

  it("is applied to a copy from this device, whose answer the server has not given", async () => {
    const held = later<Response>();
    const { parts } = page(() => held.promise, {
      "task:w/a count": { value: { n: 4 }, etag: '"k"', saved: 1_000 },
    });
    const part = keptCount();
    parts.open(part);
    await pass();
    parts.heard(3, { key: "count", add: 1 });
    expect(parts.shown(part).value).toEqual({ n: 5 });
    // The answer is the copy at a cursor after that event: the copy as it was kept is shown.
    held.settle(answer(304, null, "", 3));
    await pass();
    expect(parts.shown(part)).toEqual(drawnAs(4));
  });

  it("reads the part again when it names a change it does not carry", async () => {
    let n = 1;
    const { parts, asked } = page(() => answer(200, { n: n++ }, "", 10 + n));
    const part = count();
    parts.open(part);
    await pass();
    parts.heard(20, { key: "count", again: true });
    await pass();
    expect(asked).toHaveLength(2);
    expect(parts.shown(part)).toEqual(drawnAs(2));
  });

  it("says the server has the part no more", async () => {
    const { parts, device } = page(() => answer(200, { n: 1 }, '"a"', 1));
    const part = keptCount();
    parts.open(part);
    await pass();
    parts.heard(2, { key: "count", gone: true });
    expect(parts.shown(part)).toEqual({ ...NOTHING, missing: true, answered: true });
    expect(device.forget).toHaveBeenCalledTimes(1);
  });

  it("is told to a part with nothing drawn, which may ask to be read", async () => {
    let found = false;
    const { parts, asked } = page(() =>
      found ? answer(200, { n: 1 }, "", 5) : answer(404, { error: "none" }),
    );
    const part = count();
    parts.open(part);
    await pass();
    expect(parts.shown(part).missing).toBe(true);
    found = true;
    parts.heard(4, { key: "count", again: true });
    await pass();
    expect(asked).toHaveLength(2);
    expect(parts.shown(part)).toEqual(drawnAs(1));
  });
});

describe("an answer read while the feed spoke", () => {
  it("takes again the events after its cursor, and none at or before it", async () => {
    const held = later<Response>();
    const { parts } = page(() => held.promise);
    const part = count();
    parts.open(part);
    await pass();
    parts.heard(5, { key: "count", add: 100 });
    parts.heard(6, { key: "count", add: 1 });
    parts.heard(7, { key: "count", add: 1 });
    held.settle(answer(200, { n: 10 }, "", 5));
    await pass();
    expect(parts.shown(part)).toEqual(drawnAs(12));
  });

  it("takes again every event heard since it was asked where the server names no cursor", async () => {
    const held = later<Response>();
    const { parts } = page(() => held.promise);
    const part = count();
    parts.heard(4, { key: "count", add: 100 });
    parts.open(part);
    await pass();
    parts.heard(5, { key: "count", add: 1 });
    held.settle(answer(200, { n: 10 }));
    await pass();
    expect(parts.shown(part)).toEqual(drawnAs(11));
  });

  it("is read once more for an event newer than it that asked meanwhile, not for an older", async () => {
    const answers = [later<Response>(), later<Response>()];
    let at = 0;
    const { parts, asked } = page(() => (answers[at++] as (typeof answers)[0]).promise);
    const part = count();
    parts.open(part);
    await pass();
    void parts.read(part, 8);
    void parts.read(part, 6);
    answers[0]?.settle(answer(200, { n: 1 }, "", 7));
    await pass();
    expect(asked).toHaveLength(2);
    void parts.read(part, 9);
    answers[1]?.settle(answer(200, { n: 2 }, "", 9));
    await pass();
    expect(asked).toHaveLength(2);
    expect(parts.shown(part)).toEqual(drawnAs(2));
  });

  it("is read once more when an event applied again names a change it does not carry", async () => {
    const answers = [later<Response>(), later<Response>()];
    let at = 0;
    const { parts, asked } = page(() => (answers[at++] as (typeof answers)[0]).promise);
    const part = count();
    parts.open(part);
    await pass();
    parts.heard(8, { key: "count", again: true });
    answers[0]?.settle(answer(200, { n: 1 }, "", 7));
    await pass();
    expect(asked).toHaveLength(2);
    answers[1]?.settle(answer(200, { n: 2 }, "", 8));
    await pass();
    expect(asked).toHaveLength(2);
    expect(parts.shown(part)).toEqual(drawnAs(2));
  });

  it("is gone when an event after it says so", async () => {
    const held = later<Response>();
    const { parts } = page(() => held.promise);
    const part = count();
    parts.open(part);
    await pass();
    parts.heard(8, { key: "count", gone: true });
    held.settle(answer(200, { n: 1 }, "", 7));
    await pass();
    expect(parts.shown(part).missing).toBe(true);
  });

  it("is read again when the events it needed were dropped from those kept", async () => {
    const answers = [later<Response>(), later<Response>()];
    let at = 0;
    const { parts, asked } = page(() => (answers[at++] as (typeof answers)[0]).promise);
    const part = count();
    parts.heard(1, { key: "other" });
    parts.open(part);
    await pass();
    for (let said = 0; said <= KEPT_EVENTS; said += 1) parts.heard(2 + said, { key: "other" });
    answers[0]?.settle(answer(200, { n: 1 }, "", 1));
    await pass();
    expect(asked).toHaveLength(2);
    answers[1]?.settle(answer(200, { n: 2 }, "", 500));
    await pass();
    expect(asked).toHaveLength(2);
    expect(parts.shown(part)).toEqual(drawnAs(2));
  });
});

describe("the feed opened from a version", () => {
  it("reads again a part answered at another, and none answered at it", async () => {
    let cursor = 5;
    const { parts, asked } = page((url) =>
      answer(200, { n: cursor }, "", url.endsWith("b") ? 9 : cursor),
    );
    const a = count("a");
    const b = count("b");
    parts.open(a);
    parts.open(b);
    await pass();
    expect(asked).toHaveLength(2);
    cursor = 9;
    parts.since(9);
    await pass();
    expect(asked.map(([url]) => url)).toEqual(["/api/a", "/api/b", "/api/a"]);
    expect(parts.shown(a)).toEqual(drawnAs(9));
    // Opened again where it was, nothing is read.
    parts.since(9);
    await pass();
    expect(asked).toHaveLength(3);
    // Nor where the feed opens from before the parts' answers, which hold what it will say.
    parts.since(7);
    await pass();
    expect(asked).toHaveLength(3);
  });

  it("reads once more a part asked before it whose answer names an older cursor", async () => {
    const answers = [later<Response>(), later<Response>()];
    let at = 0;
    const { parts, asked } = page(() => (answers[at++] as (typeof answers)[0]).promise);
    const part = count();
    parts.open(part);
    await pass();
    parts.since(9);
    answers[0]?.settle(answer(200, { n: 1 }, "", 8));
    await pass();
    expect(asked).toHaveLength(2);
    answers[1]?.settle(answer(200, { n: 2 }, "", 9));
    await pass();
    expect(asked).toHaveLength(2);
    expect(parts.shown(part)).toEqual(drawnAs(2));
  });

  it("reads once more a part asked before it where the server names no cursor, never one asked after", async () => {
    const answers = [later<Response>(), later<Response>(), later<Response>()];
    let at = 0;
    const { parts, asked } = page(() => (answers[at++] as (typeof answers)[0]).promise);
    const part = count();
    parts.open(part);
    await pass();
    parts.since(9);
    answers[0]?.settle(answer(200, { n: 1 }));
    await pass();
    expect(asked).toHaveLength(2);
    answers[1]?.settle(answer(200, { n: 2 }));
    await pass();
    // Answered after the feed opened, it stands.
    expect(asked).toHaveLength(2);
    expect(parts.shown(part)).toEqual(drawnAs(2));
  });

  it("stops reading again a part the server keeps answering before it", async () => {
    const { parts, asked } = page(() => answer(200, { n: 1 }, "", 3));
    const part = count();
    parts.since(9);
    parts.open(part);
    await pass();
    expect(asked).toHaveLength(3);
    expect(parts.shown(part)).toEqual(drawnAs(1));
  });

  it("leaves a part not answered yet to its own read", async () => {
    const { parts, asked } = page(() => answer(500));
    parts.open(count());
    await pass();
    parts.since(9);
    await pass();
    expect(asked).toHaveLength(1);
  });
});

describe("the page's own change to a part", () => {
  it("is drawn, and is nothing for a part with no value or one left as it is", async () => {
    const { parts, drawn } = page(() => answer(200, { n: 1 }, "", 1));
    const part = count();
    parts.change(part, (value) => ({ n: value.n + 1 }));
    parts.open(part);
    parts.change(part, (value) => ({ n: value.n + 1 }));
    await pass();
    drawn.mockClear();
    parts.change(part, (value) => value);
    expect(drawn).not.toHaveBeenCalled();
    parts.change(part, (value) => ({ n: value.n + 1 }));
    expect(parts.shown(part)).toEqual(drawnAs(2));
    expect(drawn).toHaveBeenCalledTimes(1);
  });
});

describe("the order the reads are asked in", () => {
  it("is the order the parts were opened in, whatever the device's lookup takes", async () => {
    const { parts, asked, device } = page(() => answer(200, { n: 1 }, "", 1));
    const lookup = later<undefined>();
    device.read.mockImplementation(() => lookup.promise as Promise<never>);
    parts.open(keptCount());
    parts.open(count("other"));
    parts.open(count("third"));
    await pass();
    // The part opened first still looks its copy up: those opened after it ask nothing yet.
    expect(asked).toEqual([]);
    lookup.settle(undefined);
    await pass();
    expect(asked.map(([url]) => url)).toEqual(["/api/count", "/api/other", "/api/third"]);
  });

  it("lets the parts after one dropped before it asked go on", async () => {
    const { parts, asked, device } = page(() => answer(200, { n: 1 }, "", 1));
    const lookup = later<undefined>();
    device.read.mockImplementation(() => lookup.promise as Promise<never>);
    parts.open(keptCount());
    parts.open(count("other"));
    parts.drop((key) => key === "count");
    lookup.settle(undefined);
    await pass();
    expect(asked.map(([url]) => url)).toEqual(["/api/other"]);
    expect(parts.shown(count("other"))).toEqual(drawnAs(1));
  });
});

describe("parts the page shows no more", () => {
  it("end their reads, are drawn no more, and are no longer what the device's copy waits on", async () => {
    const held = later<Response>();
    const { parts, given, drawn, asked } = page(() => held.promise, {
      "task:w/a count": { value: { n: 4 }, etag: '"k"', saved: 1_000 },
    });
    const part = keptCount();
    const other = count("other");
    parts.open(part);
    parts.open(other);
    await pass();
    expect(shownFromDevice(parts.fresh)).not.toBeNull();
    drawn.mockClear();
    parts.drop((key) => key === "count");
    // Opened first, the part kept on the device is asked first, its copy read before.
    expect(asked.map(([url]) => url)).toEqual(["/api/count", "/api/other"]);
    expect(given[0]?.aborted).toBe(true);
    expect(given[1]?.aborted).toBe(false);
    expect(parts.shown(part)).toEqual(NOTHING);
    expect(shownFromDevice(parts.fresh)).toBeNull();
    expect(drawn).toHaveBeenCalledTimes(1);
    held.settle(answer(200, { n: 9 }, "", 1));
    await pass();
    expect(parts.shown(part)).toEqual(NOTHING);
    expect(parts.shown(other)).toEqual(drawnAs(9));
    // Dropping none says nothing; a read or an event of one dropped does nothing.
    drawn.mockClear();
    parts.drop(() => false);
    await parts.read(part);
    parts.heard(5, { key: "count", add: 1 });
    expect(drawn).not.toHaveBeenCalled();
    expect(asked).toHaveLength(2);
  });

  it("are none of a page that ended: nothing opens, and a late answer or failure draws nothing", async () => {
    const held = later<Response>();
    const { parts, mount, asked, drawn } = page(() => held.promise);
    const part = count();
    parts.open(part);
    await pass();
    mount.abort();
    held.settle(answer(200, { n: 1 }, "", 1));
    await pass();
    expect(parts.shown(part)).toEqual(NOTHING);
    parts.open(count("late"));
    await pass();
    expect(asked).toHaveLength(1);
    expect(drawn).not.toHaveBeenCalled();
    const failing = page(async () => {
      throw new Error("offline");
    });
    failing.parts.open(part);
    failing.mount.abort();
    await pass();
    expect(failing.parts.shown(part)).toEqual(NOTHING);
  });

  it("stop a copy the device answers after they went", async () => {
    const kept = later<Kept | undefined>();
    const { parts, device, asked } = page(() => answer(200, { n: 1 }));
    device.read.mockImplementation(() => kept.promise);
    const part = keptCount();
    parts.open(part);
    parts.drop(() => true);
    kept.settle({ value: { n: 4 }, etag: '"k"', saved: 1 });
    await pass();
    expect(asked).toHaveLength(0);
    expect(parts.shown(part)).toEqual(NOTHING);
  });
});

describe("what the page shows from this device", () => {
  it("is told to its host at each change alone, the feed's own state among them", () => {
    const { parts, host } = page();
    parts.freshen((f) => f);
    expect(host.freshness).not.toHaveBeenCalled();
    parts.freshen(opened);
    expect(host.freshness).toHaveBeenCalledWith({ ...FRESH, open: true });
    expect(parts.fresh.open).toBe(true);
  });

  it("stops listeners that left", () => {
    const { parts } = page(() => answer(200, { n: 1 }));
    const heard = vi.fn();
    const stop = parts.subscribe(heard);
    stop();
    parts.drop(() => true);
    expect(heard).not.toHaveBeenCalled();
  });
});

/** A component that draws the count its `part` shows. */
function Drawn({ part }: { part: Part<Count, Said> }) {
  const shown = usePart(part);
  return createElement("b", null, `${shown.value?.n ?? "none"} ${shown.answered}`);
}

describe("a component's part", () => {
  it("is nothing outside a page's store, and what the store shows inside one", async () => {
    expect(renderToStaticMarkup(createElement(Drawn, { part: count() }))).toBe("<b>none false</b>");
    const { parts } = page(() => answer(200, { n: 3 }, "", 1));
    parts.open(count());
    await pass();
    const inside = createElement(
      PartsContext.Provider,
      { value: parts },
      createElement(Drawn, { part: count() }),
    );
    expect(renderToStaticMarkup(inside)).toBe("<b>3 true</b>");
  });
});
