import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { answer, drawn, OverlayHost, openingKey } from "./overlay";

/** The events the host draws again on, as it asked to hear them. */
const heard = vi.hoisted(() => ({ events: [] as (readonly string[])[] }));

/** The host's kept select while a test draws it (`on`): what its `useState` gives, and each value
 * its setter was given. */
const kept = vi.hoisted(() => ({ on: false, last: null as unknown, set: [] as unknown[] }));

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useState: (initial: unknown) =>
      // biome-ignore lint/correctness/useHookAtTopLevel: the mock answers for the host called outside a render, and calls React's own only while React renders.
      kept.on ? [kept.last, (value: unknown) => kept.set.push(value)] : actual.useState(initial),
  };
});

// The host's other hook: it is drawn again on the shell's `review:overlay`, so it can be called as
// React would draw it.
vi.mock("./shared", async (original) => {
  const actual = await original<typeof import("./shared")>();
  return {
    ...actual,
    useWindowEvents: (events: readonly string[]) => {
      heard.events.push(events);
    },
  };
});

afterEach(() => vi.unstubAllGlobals());

/** The host called as React would draw it, `request` on the window and `last` the select it kept. */
function hosted<Props>(request: unknown, last: unknown = null) {
  vi.stubGlobal("window", { reviewOverlayRequest: request });
  kept.on = true;
  kept.last = last;
  kept.set = [];
  try {
    return OverlayHost() as ReactElement<Props> | null;
  } finally {
    kept.on = false;
  }
}

type Popup = {
  onChange: (value: string) => void;
  onClose: () => void;
  closed: boolean;
  onLeft?: () => void;
};

describe("what the overlay is asked to draw", () => {
  it("is a select's popup, each of its fields of its type", () => {
    expect(
      drawn({
        kind: "select",
        owner: "priority",
        id: "priority",
        label: "Priority",
        value: "2",
        options: [["1", "High"], ["2", "Normal"], ["3"], [4, "four"], "five", ["6", "Low", "x"]],
        callback: () => {},
      }),
    ).toEqual({
      kind: "select",
      owner: "priority",
      id: "priority",
      label: "Priority",
      value: "2",
      options: [
        ["1", "High"],
        ["2", "Normal"],
        ["6", "Low"],
      ],
    });
    expect(drawn({ kind: "select", owner: 7, options: "none" })).toEqual({
      kind: "select",
      owner: "",
      id: "",
      label: "",
      value: "",
      options: [],
    });
  });

  it("is the picture viewer, at a picture's place among them", () => {
    expect(
      drawn({
        kind: "picture",
        owner: "home-pictures",
        photos: [{ src: "/a.png", caption: "A" }, { src: "/b.png" }, null],
        at: 1.7,
      }),
    ).toEqual({
      kind: "picture",
      owner: "home-pictures",
      photos: [
        { src: "/a.png", caption: "A" },
        { src: "/b.png", caption: "" },
        { src: "", caption: "" },
      ],
      at: 1,
    });
    for (const at of [-3, Number.NaN, "2", undefined]) {
      expect(drawn({ kind: "picture", owner: "x", photos: [], at })).toMatchObject({ at: 0 });
    }
  });

  it("is nothing for a request of no kind it draws", () => {
    expect(drawn(null)).toBeNull();
    expect(drawn(undefined)).toBeNull();
    expect(drawn({ kind: "close", owner: "priority" })).toBeNull();
    expect(drawn("select")).toBeNull();
  });
});

describe("the overlay's answer", () => {
  it("goes to the request's callback, when it has one", () => {
    const callback = vi.fn();
    answer({ callback }, "2");
    answer({ callback }, null);
    expect(callback.mock.calls).toEqual([["2"], [null]]);
    expect(() => answer({ callback: "no" }, 1)).not.toThrow();
    expect(() => answer(null, 1)).not.toThrow();
  });
});

describe("the overlay's host", () => {
  const host = (request: unknown) => {
    vi.stubGlobal("window", { reviewOverlayRequest: request });
    return renderToStaticMarkup(<OverlayHost />);
  };

  it("draws the popup a select asked for, beside its trigger", () => {
    const html = host({
      kind: "select",
      owner: "priority",
      id: "priority",
      label: "Priority",
      value: "1",
      options: [
        ["1", "High"],
        ["2", "Normal"],
      ],
    });
    expect(html).toContain('id="priority-popup" role="listbox"');
    expect(html).toContain('aria-label="Priority"');
    expect(html).toContain(
      'aria-selected="true" aria-disabled="false" data-disabled="false" class="dx-select-option">High',
    );
  });

  it("draws the picture viewer at the picture asked for", () => {
    const html = host({
      kind: "picture",
      owner: "task-pictures",
      photos: [
        { src: "/a.png", caption: "A" },
        { src: "/b.png", caption: "B" },
      ],
      at: 1,
    });
    expect(html).toContain('<div id="media-lightbox" data-state="open"');
    expect(html).toContain('<img src="/b.png" alt="B"/>');
    expect(html).toContain("B · 2 / 2");
  });

  it("draws nothing while nothing is asked, or what it cannot draw", () => {
    expect(host(null)).toBe("");
    expect(host(undefined)).toBe("");
    expect(host({ kind: "menu", owner: "x" })).toBe("");
  });

  it("draws again as the shell says the request moved, each owner's afresh", () => {
    heard.events = [];
    const callback = vi.fn();
    const request = { kind: "select", owner: "status", options: [], callback };
    const popup = hosted<Popup>(request);
    expect(heard.events).toEqual([["review:overlay"]]);
    expect(popup?.key).toBe(openingKey(request, "status"));
    expect(popup?.props.closed).toBe(false);
    expect(popup?.props.onLeft).toBeUndefined();
    expect(kept.set).toEqual([{ request, select: drawn(request) }]);
    popup?.props.onChange("done");
    popup?.props.onClose();
    expect(callback.mock.calls).toEqual([["done"], [null]]);
    const viewer = hosted<{ command: (direction: number) => void }>({
      kind: "picture",
      owner: "docs-pictures",
      photos: [],
      callback,
    });
    expect(viewer?.key).toBe("docs-pictures");
    viewer?.props.command(-1);
    viewer?.props.command(0);
    expect(callback.mock.calls.slice(2)).toEqual([[-1], [0]]);
  });

  it("keys each opening apart, an owner's second its own", () => {
    const first = { kind: "select", owner: "status" };
    const second = { kind: "select", owner: "status" };
    expect(openingKey(first, "status")).toBe(openingKey(first, "status"));
    expect(openingKey(first, "status")).not.toBe(openingKey(second, "status"));
    expect(openingKey(first, "status")).toMatch(/^status:/);
    expect(openingKey(null, "status")).toBe("status");
  });

  it("keeps a select's popup drawn closed once its request goes, until it has left", () => {
    const callback = vi.fn();
    const request = { kind: "select", owner: "status", id: "status", options: [], callback };
    const last = { request, select: drawn(request) };
    // The request kept: nothing to keep again.
    hosted<Popup>(request, last);
    expect(kept.set).toEqual([]);
    for (const gone of [null, { kind: "close", owner: "status" }]) {
      const leaving = hosted<Popup>(gone, last);
      expect(leaving?.key).toBe(openingKey(request, "status"));
      expect(leaving?.props.closed).toBe(true);
      expect(kept.set).toEqual([]);
      leaving?.props.onLeft?.();
      expect(kept.set).toEqual([null]);
      // Its callbacks are still its own request's.
      leaving?.props.onClose();
      expect(callback).toHaveBeenLastCalledWith(null);
    }
    expect(hosted(null, null)).toBeNull();
  });

  it("lets a leaving popup go for another opening or the picture viewer", () => {
    const request = { kind: "select", owner: "status", options: [] };
    const last = { request, select: drawn(request) };
    const again = { kind: "select", owner: "status", options: [] };
    const reopened = hosted<Popup>(again, last);
    expect(reopened?.key).toBe(openingKey(again, "status"));
    expect(reopened?.key).not.toBe(openingKey(request, "status"));
    expect(reopened?.props.closed).toBe(false);
    expect(kept.set).toEqual([{ request: again, select: drawn(again) }]);
    const viewer = hosted({ kind: "picture", owner: "docs-pictures", photos: [] }, last);
    expect(viewer?.key).toBe("docs-pictures");
    expect(kept.set).toEqual([null]);
  });
});
