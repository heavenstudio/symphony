import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Backoff, NEVER_ANSWERED } from "./state";
// biome-ignore lint/style/noRestrictedImports: the helpers' own tests, the baseline's sleep among them
import { Debounce, deadline, retryWait, sleep, tick } from "./timing";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

/** Whether `waiting` has ended, its timers run `ms` on. */
async function endedAfter(waiting: Promise<unknown>, ms: number): Promise<boolean> {
  let ended = false;
  void waiting.then(() => {
    ended = true;
  });
  await vi.advanceTimersByTimeAsync(ms);
  return ended;
}

describe("a retry's wait", () => {
  it("is the next of its backoff's waits: 1 s, then 2 s", async () => {
    const backoff = new Backoff();
    const first = retryWait(backoff, NEVER_ANSWERED);
    expect(await endedAfter(first, 999)).toBe(false);
    expect(await endedAfter(first, 1)).toBe(true);
    const second = retryWait(backoff, NEVER_ANSWERED);
    expect(await endedAfter(second, 1_999)).toBe(false);
    expect(await endedAfter(second, 1)).toBe(true);
  });

  it("after a stream that held its longest wait starts the waits again", async () => {
    const backoff = new Backoff();
    for (const ms of [1_000, 2_000, 4_000]) {
      expect(await endedAfter(retryWait(backoff, NEVER_ANSWERED), ms)).toBe(true);
    }
    expect(await endedAfter(retryWait(backoff, { lost: 30_000 }), 1_000)).toBe(true);
  });

  it("after a page that went away is none, and the waits start again", async () => {
    const backoff = new Backoff();
    expect(await endedAfter(retryWait(backoff, NEVER_ANSWERED), 1_000)).toBe(true);
    expect(await endedAfter(retryWait(backoff, "away"), 0)).toBe(true);
    const next = retryWait(backoff, NEVER_ANSWERED);
    expect(await endedAfter(next, 999)).toBe(false);
    expect(await endedAfter(next, 1)).toBe(true);
  });

  it("ends at once when its signal ends, or had, and leaves no timer", async () => {
    const controller = new AbortController();
    const waited = retryWait(new Backoff(), NEVER_ANSWERED, controller.signal);
    controller.abort();
    await waited;
    await retryWait(new Backoff(), NEVER_ANSWERED, controller.signal);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("a debounce", () => {
  it("acts on a rest no other followed within its length", async () => {
    const resting = new Debounce(250);
    const rest = resting.rest();
    expect(await endedAfter(rest, 249)).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(await rest).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("acts on the last of a burst alone, its length from that rest, each earlier one over at once", async () => {
    const resting = new Debounce(250);
    const first = resting.rest();
    await vi.advanceTimersByTimeAsync(200);
    const second = resting.rest();
    expect(await first).toBe(false);
    await vi.advanceTimersByTimeAsync(200);
    const third = resting.rest();
    expect(await second).toBe(false);
    expect(vi.getTimerCount()).toBe(1);
    expect(await endedAfter(third, 249)).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(await third).toBe(true);
  });

  it("rests again once one has acted", async () => {
    const resting = new Debounce(250);
    const first = resting.rest();
    await vi.advanceTimersByTimeAsync(250);
    expect(await first).toBe(true);
    const again = resting.rest();
    await vi.advanceTimersByTimeAsync(250);
    expect(await again).toBe(true);
  });

  it("cancelled, acts not, at once, and leaves no timer; cancelled with none under way, nothing", async () => {
    const resting = new Debounce(250);
    resting.cancel();
    const rest = resting.rest();
    resting.cancel();
    expect(await rest).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
    resting.cancel();
  });

  it("acts not once its signal ends, or had", async () => {
    const controller = new AbortController();
    const resting = new Debounce(250, controller.signal);
    const acted = resting.rest();
    await vi.advanceTimersByTimeAsync(250);
    expect(await acted).toBe(true);
    const rest = resting.rest();
    controller.abort();
    expect(await rest).toBe(false);
    expect(await resting.rest()).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("a clock's tick", () => {
  it("comes at the next minute, wherever in the minute it was asked", async () => {
    vi.setSystemTime(new Date(2026, 0, 1, 12, 0, 40, 500));
    const next = tick();
    expect(await endedAfter(next, 19_499)).toBe(false);
    expect(await endedAfter(next, 1)).toBe(true);
    expect(new Date().getSeconds()).toBe(0);
  });

  it("asked on the minute, comes a minute on", async () => {
    vi.setSystemTime(new Date(2026, 0, 1, 12, 1, 0, 0));
    const next = tick();
    expect(await endedAfter(next, 59_999)).toBe(false);
    expect(await endedAfter(next, 1)).toBe(true);
  });

  it("ends at once when its signal ends, or had, and leaves no timer", async () => {
    const controller = new AbortController();
    const waited = tick(controller.signal);
    controller.abort();
    await waited;
    await tick(controller.signal);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("the baseline's sleep", () => {
  it("ends when its time is up", async () => {
    const waiting = sleep(500);
    expect(await endedAfter(waiting, 499)).toBe(false);
    expect(await endedAfter(waiting, 1)).toBe(true);
  });

  it("ends at once when its signal ends, or had, and leaves no timer", async () => {
    const controller = new AbortController();
    const waited = sleep(60_000, controller.signal);
    controller.abort();
    await waited;
    await sleep(60_000, controller.signal);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("a deadline", () => {
  it("says it passed once its time is up, and only once", () => {
    const passed = vi.fn();
    deadline(1_000, passed);
    vi.advanceTimersByTime(999);
    expect(passed).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(passed).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(60_000);
    expect(passed).toHaveBeenCalledOnce();
  });

  it("says nothing once the wait it bounds has ended, and leaves no timer", () => {
    const passed = vi.fn();
    const ended = deadline(1_000, passed);
    ended();
    vi.advanceTimersByTime(60_000);
    expect(passed).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    ended();
  });
});
