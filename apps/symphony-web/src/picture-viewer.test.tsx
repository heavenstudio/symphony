// The picture viewer, as it is drawn and as it is used. The use is the component called as React
// would draw it, its hooks answered here (`runtime`): its refs kept from one draw to the next and
// its effects held for the test to run, on a page of stand-ins for the elements it reads.
/* biome-ignore-all lint/correctness/useHookAtTopLevel: the hooks mocked here answer for a component
 * called outside a render, and call React's own only while React renders. */

import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { focusable, type Photo, PictureViewer, trapFocus } from "./picture-viewer";

const runtime = vi.hoisted(() => ({
  on: false,
  refs: [] as { current: unknown }[],
  ref: 0,
  effects: [] as (() => undefined | (() => void))[],
}));

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useRef: (initial: unknown) => {
      if (!runtime.on) return actual.useRef(initial);
      const at = runtime.ref++;
      runtime.refs[at] ??= { current: initial };
      return runtime.refs[at];
    },
    useEffect: (effect: () => undefined | (() => void), deps?: unknown[]) => {
      if (!runtime.on) return actual.useEffect(effect, deps);
      runtime.effects.push(effect);
    },
  };
});

beforeEach(() => {
  runtime.refs = [];
});

afterEach(() => vi.unstubAllGlobals());

const PHOTOS: Photo[] = [
  { src: "/first.svg", caption: "First" },
  { src: "/second.svg", caption: "Second" },
  { src: "/third.svg", caption: "Third" },
];

describe("what the focus stops at", () => {
  const element = (tagName: string, fields: Record<string, unknown> = {}) => ({
    tabIndex: 0,
    tagName,
    getAttribute: (name: string) =>
      name === "disabled" ? ((fields.disabled as string) ?? null) : null,
    ...fields,
  });

  it("is a control that takes it by Tab", () => {
    expect(focusable(element("BUTTON"))).toBe(true);
    expect(focusable(element("SELECT"))).toBe(true);
    expect(focusable(element("TEXTAREA"))).toBe(true);
    expect(focusable(element("INPUT", { type: "text" }))).toBe(true);
    expect(focusable(element("A", { href: "/pictures" }))).toBe(true);
  });

  it("is never a hidden field, a link to nowhere or one to ignore, a disabled control, or other", () => {
    expect(focusable(element("INPUT", { type: "hidden" }))).toBe(false);
    expect(focusable(element("A", { href: "" }))).toBe(false);
    expect(focusable(element("A", { href: "/pictures", rel: "ignore" }))).toBe(false);
    expect(focusable(element("BUTTON", { disabled: "true" }))).toBe(false);
    expect(focusable(element("BUTTON", { tabIndex: -1 }))).toBe(false);
    expect(focusable(element("IMG"))).toBe(false);
    expect(focusable(element("DIV"))).toBe(false);
  });
});

/** The viewer called as React would draw it, its hooks answered here: what it draws. */
function call(photos: Photo[], at: number, command: (direction: number) => void) {
  runtime.on = true;
  runtime.ref = 0;
  runtime.effects = [];
  try {
    return PictureViewer({ photos, at, command }) as ReactElement | null;
  } finally {
    runtime.on = false;
  }
}

/** A control of the viewer, a button: whether it has the focus. */
class Control {
  focused = 0;
  tabIndex = 0;
  tagName = "BUTTON";
  getAttribute = () => null;
  constructor(readonly name: string) {}
  focus() {
    this.focused += 1;
    held.focus = this;
  }
}

/** What has the focus. */
const held: { focus: Control | null } = { focus: null };

/** The container's listeners, by event. */
type Listeners = Map<string, (event: unknown) => void>;

/** A container of `controls` in order, its walker stepping through them as the browser's would. */
function container(controls: Control[], inside: unknown[] = []) {
  const listeners: Listeners = new Map();
  const box = {
    addEventListener: (type: string, listener: (event: unknown) => void) =>
      listeners.set(type, listener),
    removeEventListener: (type: string) => listeners.delete(type),
    contains: (node: unknown) =>
      node === box || controls.includes(node as Control) || inside.includes(node),
  };
  vi.stubGlobal("NodeFilter", { SHOW_ELEMENT: 1, FILTER_ACCEPT: 1, FILTER_SKIP: 3 });
  vi.stubGlobal("HTMLElement", Control);
  const walkers: unknown[] = [];
  const createTreeWalker = (
    root: unknown,
    _show: number,
    filter: { acceptNode: (node: unknown) => number },
  ) => {
    // Each control and the elements between them are offered to the filter, which takes controls.
    walkers.push(filter);
    expect(root).toBe(box);
    const nodes = controls;
    let at = -1;
    const walker = {
      get currentNode() {
        return at < 0 ? root : nodes[at];
      },
      set currentNode(node: unknown) {
        at = node === root ? -1 : nodes.indexOf(node as Control);
      },
      nextNode: () => (at + 1 < nodes.length ? nodes[++at] : null),
      previousNode: () => (at > 0 ? nodes[--at] : null),
      lastChild: () => {
        at = nodes.length - 1;
        return nodes[at] ?? null;
      },
    };
    return walker;
  };
  return { box, listeners, createTreeWalker, walkers };
}

const tab = (shiftKey = false) => ({ key: "Tab", shiftKey, preventDefault: vi.fn() });

describe("the focus kept within the viewer", () => {
  it("goes to its first control, then round its controls by Tab and Shift+Tab", () => {
    const controls = ["previous", "close", "next"].map((name) => new Control(name));
    const { box, listeners, createTreeWalker, walkers } = container(controls);
    vi.stubGlobal("document", { createTreeWalker });
    const release = trapFocus(box as unknown as HTMLElement);
    expect(held.focus?.name).toBe("previous");
    const keys = listeners.get("keydown");
    const order: string[] = [];
    for (const shift of [false, false, false, true, true]) {
      const event = tab(shift);
      keys?.(event);
      expect(event.preventDefault).toHaveBeenCalled();
      order.push(held.focus?.name ?? "");
    }
    expect(order).toEqual(["close", "next", "previous", "next", "close"]);
    // The filter takes controls alone.
    const filter = walkers[0] as { acceptNode: (node: unknown) => number };
    expect(filter.acceptNode(controls[0])).toBe(1);
    expect(filter.acceptNode({})).toBe(3);
    // Any other key goes on as it would.
    const other = { key: "Enter", shiftKey: false, preventDefault: vi.fn() };
    keys?.(other);
    expect(other.preventDefault).not.toHaveBeenCalled();
    release();
    expect(listeners.has("keydown")).toBe(false);
  });

  it("takes nothing in a container without a control", () => {
    const { box, listeners, createTreeWalker } = container([]);
    vi.stubGlobal("document", { createTreeWalker });
    held.focus = null;
    trapFocus(box as unknown as HTMLElement);
    listeners.get("keydown")?.(tab(true));
    listeners.get("keydown")?.(tab());
    expect(held.focus).toBeNull();
  });
});

describe("the picture viewer drawn", () => {
  // react-dom/server puts a preload of the picture before it, a hint of its own the client's
  // render never draws.
  const markup = (photos: Photo[], at: number) =>
    renderToStaticMarkup(<PictureViewer photos={photos} at={at} command={() => {}} />).replace(
      /^<link rel="preload" as="image" href="[^"]*"\/>/,
      "",
    );

  it("is a modal dialog over the page, the picture with its caption and a Close button", () => {
    expect(markup(PHOTOS.slice(0, 1), 0)).toBe(
      '<div id="media-lightbox" data-state="open" class="dx-dialog-backdrop"><div id="media-lightbox-content" role="dialog" aria-modal="true" aria-label="Picture viewer" class="dx-dialog"><div class="lightbox-controls"><button class="dx-button" data-style="outline" data-size="default" type="button">Close</button></div><img src="/first.svg" alt="First"/><div class="lightbox-caption">First</div></div></div>',
    );
  });

  it("moves through several pictures, its place among them in its caption", () => {
    const html = markup(PHOTOS, 1);
    expect(html).toContain(
      '<div class="lightbox-controls"><button class="dx-button" data-style="outline" data-size="default" type="button" aria-label="Previous picture">←</button><button class="dx-button" data-style="outline" data-size="default" type="button">Close</button><button class="dx-button" data-style="outline" data-size="default" type="button" aria-label="Next picture">→</button></div>',
    );
    expect(html).toContain('<img src="/second.svg" alt="Second"/>');
    expect(html).toContain('<div class="lightbox-caption">Second · 2 / 3</div>');
  });

  it("is nothing without pictures, and an empty dialog for a place past them", () => {
    expect(markup([], 0)).toBe("");
    expect(markup(PHOTOS, 5)).toBe(
      '<div id="media-lightbox" data-state="open" class="dx-dialog-backdrop"><div id="media-lightbox-content" role="dialog" aria-modal="true" aria-label="Picture viewer" class="dx-dialog"></div></div>',
    );
  });

  it("answers each button with its direction", () => {
    const command = vi.fn();
    const element = call(PHOTOS, 0, command);
    const buttons: { props: { onClick: () => void; children: string } }[] = [];
    const find = (node: unknown) => {
      if (Array.isArray(node)) return node.forEach(find);
      if (!node || typeof node !== "object" || !("props" in node)) return;
      const props = (node as { props: Record<string, unknown> }).props;
      if (typeof props.onClick === "function") buttons.push(node as (typeof buttons)[number]);
      find(props.children);
    };
    find(element);
    for (const button of buttons) button.props.onClick();
    expect(buttons.map((button) => button.props.children)).toEqual(["←", "Close", "→"]);
    expect(command.mock.calls).toEqual([[-1], [0], [1]]);
  });
});

describe("the picture viewer open", () => {
  /** The viewer drawn and its effect run on a page whose focus `before` held, with its
   * listeners. */
  function open(photos: Photo[], command: (direction: number) => void) {
    const close = new Control("close");
    const before = new Control("picture");
    const picture = {};
    const { box, createTreeWalker } = container([close], [picture]);
    const page = new Map<string, [(event: unknown) => void, unknown]>();
    vi.stubGlobal("document", {
      activeElement: before,
      createTreeWalker,
      addEventListener: (type: string, listener: (event: unknown) => void, capture: unknown) =>
        page.set(type, [listener, capture]),
      removeEventListener: (type: string) => page.delete(type),
    });
    // Every element of the page is a node, and only they are an event's targets.
    vi.stubGlobal("Node", Object);
    call(photos, 0, command);
    (runtime.refs[0] as { current: unknown }).current = box;
    const cleanup = runtime.effects[0]?.();
    return { cleanup, page, close, before, picture, box };
  }

  it("takes the focus in, and closes on Escape anywhere", () => {
    const command = vi.fn();
    const { page, close } = open(PHOTOS, command);
    expect(close.focused).toBe(1);
    const [keys] = page.get("keydown") ?? [];
    const other = { key: "ArrowRight", preventDefault: vi.fn() };
    keys?.(other);
    expect(other.preventDefault).not.toHaveBeenCalled();
    expect(command).not.toHaveBeenCalled();
    const leave = { key: "Escape", preventDefault: vi.fn() };
    keys?.(leave);
    expect(leave.preventDefault).toHaveBeenCalled();
    expect(command).toHaveBeenCalledWith(0);
  });

  it("closes on a press or a focus outside the picture's box, not within it", () => {
    const command = vi.fn();
    const { page, picture } = open(PHOTOS, command);
    for (const type of ["pointerdown", "focusin"]) {
      const [listener, capture] = page.get(type) ?? [];
      expect(capture, type).toBe(true);
      listener?.({ target: picture });
      expect(command).not.toHaveBeenCalled();
      listener?.({ target: {} });
      expect(command).toHaveBeenLastCalledWith(0);
      command.mockClear();
    }
  });

  it("calls the latest command a draw gave it", () => {
    const first = vi.fn();
    const { page } = open(PHOTOS, first);
    const latest = vi.fn();
    call(PHOTOS, 1, latest);
    page.get("keydown")?.[0]({ key: "Escape", preventDefault: () => {} });
    expect(first).not.toHaveBeenCalled();
    expect(latest).toHaveBeenCalledWith(0);
  });

  it("gives the focus back to what held it once closed, its listeners gone", () => {
    const { cleanup, page, before, box } = open(PHOTOS, vi.fn());
    cleanup?.();
    expect(before.focused).toBe(1);
    expect(page.size).toBe(0);
    expect(box).toBeDefined();
  });

  it("does nothing without pictures, or before its box is drawn", () => {
    const command = vi.fn();
    const { cleanup, page } = open([], command);
    expect(cleanup).toBeUndefined();
    expect(page.size).toBe(0);
    runtime.refs = [];
    call(PHOTOS, 0, command);
    expect(runtime.effects[0]?.()).toBeUndefined();
  });
});
