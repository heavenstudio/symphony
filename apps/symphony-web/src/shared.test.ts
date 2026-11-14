import { afterEach, describe, expect, it, vi } from "vitest";
import { field, list, publish, remember, snapshot, store } from "./shared";

describe("a shared value", () => {
  it("tells each listener when it changes, and none when it is set as it is", () => {
    const shared = store(1);
    const heard = vi.fn();
    const leave = shared.subscribe(heard);
    shared.set(1);
    expect(heard).not.toHaveBeenCalled();
    shared.set(2);
    expect(shared.get()).toBe(2);
    expect(heard).toHaveBeenCalledTimes(1);
    leave();
    shared.set(3);
    expect(heard).toHaveBeenCalledTimes(1);
  });
});

describe("what the page shares with the shell", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is the window's values and its events", () => {
    const page = new EventTarget();
    vi.stubGlobal("window", page);
    vi.stubGlobal("dispatchEvent", page.dispatchEvent.bind(page));
    remember("reviewPanelReady", true);
    expect(snapshot("reviewPanelReady")).toBe(true);
    const heard = vi.fn();
    page.addEventListener("review:frame", heard);
    publish("review:frame");
    expect(heard).toHaveBeenCalledTimes(1);
  });

  it("reads a field or a list of anything, none where there is none", () => {
    expect(field({ a: 1 }, "a")).toBe(1);
    expect(field(null, "a")).toBeUndefined();
    expect(field("text", "length")).toBeUndefined();
    expect(list([1])).toEqual([1]);
    expect(list({ 0: 1 })).toEqual([]);
  });
});
