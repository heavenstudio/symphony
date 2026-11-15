import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { answer, drawn, OverlayHost } from "./overlay";

/** The events the host draws again on, as it asked to hear them. */
const heard = vi.hoisted(() => ({ events: [] as (readonly string[])[] }));

// The host's one hook: it is drawn again on the shell's `review:overlay`, so it can be called as
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
    vi.stubGlobal("window", {
      reviewOverlayRequest: { kind: "select", owner: "status", options: [], callback },
    });
    const popup = OverlayHost() as ReactElement<{
      onChange: (value: string) => void;
      onClose: () => void;
    }>;
    expect(heard.events).toEqual([["review:overlay"]]);
    expect(popup.key).toBe("status");
    popup.props.onChange("done");
    popup.props.onClose();
    expect(callback.mock.calls).toEqual([["done"], [null]]);
    vi.stubGlobal("window", {
      reviewOverlayRequest: { kind: "picture", owner: "docs-pictures", photos: [], callback },
    });
    const viewer = OverlayHost() as ReactElement<{ command: (direction: number) => void }>;
    expect(viewer.key).toBe("docs-pictures");
    viewer.props.command(-1);
    viewer.props.command(0);
    expect(callback.mock.calls.slice(2)).toEqual([[-1], [0]]);
  });
});
