import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
// biome-ignore lint/style/noRestrictedImports: the helpers' own tests, the baseline's sleep among them
import { deadline, debounce, retryWait, sleep, tick } from "./timing";

describe("the waits by a length of time", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it.each([
    ["a retry's wait", retryWait],
    ["a debounce's rest", debounce],
    ["a clock's tick", tick],
    ["the baseline's sleep", sleep],
  ])("%s ends when its time is up", async (_, wait) => {
    let done = false;
    void wait(500).then(() => {
      done = true;
    });
    await vi.advanceTimersByTimeAsync(499);
    expect(done).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(done).toBe(true);
  });

  it.each([
    ["a retry's wait", retryWait],
    ["a debounce's rest", debounce],
    ["a clock's tick", tick],
    ["the baseline's sleep", sleep],
  ])("%s ends at once when its signal ends, or had, and leaves no timer", async (_, wait) => {
    const controller = new AbortController();
    const waited = wait(60_000, controller.signal);
    controller.abort();
    await waited;
    await wait(60_000, controller.signal);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("a deadline", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

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
