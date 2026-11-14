import { afterEach, describe, expect, it, vi } from "vitest";
import { field, list, publish, remember, snapshot, store, useWindowEvents } from "./shared";

// React's hooks and `flushSync` stood in for, as no DOM runs here: each effect a hook sets is
// kept to be run by its test, and each draw says whether it came within `flushSync`.
const react = vi.hoisted(() => ({
  effects: [] as (() => undefined | (() => void))[],
  drawn: [] as boolean[],
  flushing: false,
}));

vi.mock("react", async (original) => ({
  ...(await original<typeof import("react")>()),
  useEffect: (effect: () => undefined | (() => void)) => {
    react.effects.push(effect);
  },
  useReducer: () => [0, () => react.drawn.push(react.flushing)],
}));

vi.mock("react-dom", async (original) => ({
  ...(await original<typeof import("react-dom")>()),
  flushSync: (draw: () => void) => {
    react.flushing = true;
    try {
      draw();
    } finally {
      react.flushing = false;
    }
  },
}));

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

  it("draws a part again within the event, on each event it names and on no other", () => {
    const page = new EventTarget();
    vi.stubGlobal("addEventListener", page.addEventListener.bind(page));
    vi.stubGlobal("removeEventListener", page.removeEventListener.bind(page));
    react.effects.length = 0;
    react.drawn.length = 0;
    useWindowEvents(["review:quick", "review:activity"]);
    expect(react.effects).toHaveLength(1);
    const leave = react.effects[0]?.();
    page.dispatchEvent(new Event("review:quick"));
    page.dispatchEvent(new Event("review:activity"));
    page.dispatchEvent(new Event("review:frame"));
    // Within `flushSync`: drawn before the event ends, so before the shell's next frame.
    expect(react.drawn).toEqual([true, true]);
    leave?.();
    page.dispatchEvent(new Event("review:quick"));
    expect(react.drawn).toEqual([true, true]);
  });

  it("reads a field or a list of anything, none where there is none", () => {
    expect(field({ a: 1 }, "a")).toBe(1);
    expect(field(null, "a")).toBeUndefined();
    expect(field("text", "length")).toBeUndefined();
    expect(list([1])).toEqual([1]);
    expect(list({ 0: 1 })).toEqual([]);
  });
});
