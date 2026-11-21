import { afterEach, describe, expect, it, vi } from "vitest";
import { anchor, popupRect, position, type Rect, reveal, revealBy } from "./popup-position";

afterEach(() => vi.unstubAllGlobals());

describe("where a popup is placed", () => {
  it("fits below, flips above, and covers its trigger when neither side holds it", () => {
    expect(popupRect([20, 30, 80, 36], [0, 0, 400, 800], [100, 200])).toEqual([20, 70, 100, 200]);
    expect(popupRect([20, 740, 80, 36], [0, 0, 400, 800], [100, 200])).toEqual([20, 536, 100, 200]);
    // 292 px below and 288 above, for a list of 400 the window holds: drawn whole, over its
    // trigger, as low as the window lets it.
    expect(popupRect([20, 300, 80, 36], [0, 0, 400, 640], [100, 400])).toEqual([20, 232, 100, 400]);
    // More room above: from the window's top edge down over its trigger.
    expect(popupRect([20, 380, 80, 36], [0, 0, 400, 640], [100, 400])).toEqual([20, 8, 100, 400]);
  });

  it("scrolls a list only when the window cannot hold it", () => {
    // Twelve options with the whole of a tall window below: every one shows, nothing scrolls.
    expect(popupRect([20, 30, 80, 36], [0, 0, 1280, 1000], [100, 450])).toEqual([20, 70, 100, 450]);
    // Room for all of it above only: it flips, whole.
    expect(popupRect([20, 600, 80, 36], [0, 0, 400, 778], [100, 500])).toEqual([20, 96, 100, 500]);
    // Taller than the window: it fills the roomier side of its trigger, attached to it, and the
    // rest scrolls.
    for (const [top, placed] of [
      [30, [20, 70, 100, 562]],
      [300, [20, 340, 100, 292]],
      [600, [20, 8, 100, 588]],
    ] as [number, Rect][]) {
      expect(popupRect([20, top, 80, 36], [0, 0, 400, 640], [100, 900]), `at ${top}`).toEqual(
        placed,
      );
    }
    // As tall as the window holds, it is drawn whole over its trigger instead.
    expect(popupRect([20, 300, 80, 36], [0, 0, 400, 640], [100, 624])).toEqual([20, 8, 100, 624]);
  });

  it("clamps wide and offscreen triggers' popups into the visible viewport", () => {
    expect(popupRect([380, 30, 80, 36], [0, 0, 400, 800], [900, 200])).toEqual([8, 70, 384, 200]);
    expect(popupRect([-20, 900, 80, 36], [0, 0, 400, 800], [100, 200])).toEqual([8, 592, 100, 200]);
    expect(popupRect([0, 460, 80, 36], [10, 200, 380, 300], [100, 200])).toEqual([
      18, 256, 100, 200,
    ]);
  });

  it("is as wide as its trigger at least, and keeps a tiny viewport's edges halved", () => {
    expect(popupRect([20, 30, 160, 36], [0, 0, 400, 800], [100, 200])[2]).toBe(160);
    // A viewport 10 pixels across keeps 5 at each edge, and nothing is wider than what is left.
    expect(popupRect([0, 0, 80, 4], [0, 0, 10, 600], [100, 20])).toEqual([5, 8, 0, 20]);
  });
});

const rect = (top: number, bottom: number) => ({ top, bottom }) as DOMRect;

describe("an option kept in its list's view", () => {
  it("scrolls by what brings the option 4 pixels within the list's edges, else not at all", () => {
    expect(revealBy(rect(102, 130), rect(100, 300))).toBe(-2);
    expect(revealBy(rect(280, 299), rect(100, 300))).toBe(3);
    expect(revealBy(rect(104, 296), rect(100, 300))).toBe(0);
  });
});

/** An element of the page as the placement reads and writes it. */
class Box {
  attributes = new Map<string, string>();
  scrollTop = 0;
  offsetWidth = 120;
  clientWidth = 120;
  scrollHeight = 240;
  children: Box[] = [];
  constructor(
    readonly id: string,
    readonly box: { left: number; top: number; width: number; height: number; bottom?: number },
    readonly role: string | null = null,
  ) {}
  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
    // A list placed as scrolling draws its scrollbar inside its width.
    if (name === "style" && value.includes("overflow-y:auto")) this.clientWidth = this.bar;
    // Measured as tall as its options, it has nowhere to scroll, as a browser clamps it.
    if (name === "style" && value.includes("visibility:hidden")) this.scrollTop = 0;
  }
  bar = 120;
  getAttribute(name: string) {
    return name === "role" ? this.role : (this.attributes.get(name) ?? null);
  }
  getBoundingClientRect() {
    return { ...this.box, bottom: this.box.bottom ?? this.box.top + this.box.height } as DOMRect;
  }
  contains(other: unknown) {
    return other === this || this.children.includes(other as Box);
  }
}

/** A page of `elements`, its window `size` with no visual viewport unless `visual` gives one. */
function page(elements: Box[], active: Box | null = null, visual?: Record<string, unknown>) {
  const heard: [string, unknown, unknown][] = [];
  const listen = (on: string) => (type: string, listener: unknown, options?: unknown) =>
    heard.push([`${on}:${type}`, listener, options]);
  const unlisten = (on: string) => (type: string, listener: unknown) => {
    const at = heard.findIndex(([name, kept]) => name === `${on}:${type}` && kept === listener);
    if (at >= 0) heard.splice(at, 1);
  };
  vi.stubGlobal("document", {
    activeElement: active,
    getElementById: (id: string) => elements.find((element) => element.id === id) ?? null,
    addEventListener: listen("document"),
    removeEventListener: unlisten("document"),
  });
  vi.stubGlobal("window", {
    visualViewport: visual
      ? { ...visual, addEventListener: listen("visual"), removeEventListener: unlisten("visual") }
      : undefined,
  });
  vi.stubGlobal("innerWidth", 400);
  vi.stubGlobal("innerHeight", 800);
  vi.stubGlobal("addEventListener", listen("window"));
  vi.stubGlobal("removeEventListener", unlisten("window"));
  vi.stubGlobal("Node", Box);
  vi.stubGlobal("getComputedStyle", (element: Box) => ({ width: `${element.box.width}px` }));
  return heard;
}

describe("a popup placed beside its trigger", () => {
  it("is measured hidden, then placed and marked positioned", () => {
    const trigger = new Box("field", { left: 20, top: 30, width: 80, height: 36 });
    const popup = new Box("field-popup", { left: 0, top: 0, width: 120, height: 240 });
    const written: string[] = [];
    const set = popup.setAttribute.bind(popup);
    popup.setAttribute = (name, value) => {
      if (name === "style") written.push(value);
      set(name, value);
    };
    page([trigger, popup]);
    position("field", "field-popup");
    expect(written).toEqual([
      "position:fixed;visibility:hidden;left:0;top:0;width:max-content;min-width:0;max-width:384px;overflow:hidden;margin:0;z-index:1100",
      "position:fixed;left:20px;top:70px;width:120px;min-width:0;max-height:240px;overflow-y:auto;overflow-x:hidden;margin:0;z-index:1100",
    ]);
    expect(popup.attributes.get("data-positioned")).toBe("true");
    expect(popup.attributes.get("data-side")).toBe("bottom");
  });

  it("is marked as opened above its trigger only when it lies wholly above it", () => {
    const popup = new Box("field-popup", { left: 0, top: 0, width: 120, height: 240 });
    page([new Box("field", { left: 20, top: 700, width: 80, height: 36 }), popup]);
    position("field", "field-popup");
    expect(popup.attributes.get("style")).toContain("top:456px;");
    expect(popup.attributes.get("data-side")).toBe("top");
    // Too tall for either side, it is drawn whole over its trigger: it grows from its top.
    popup.scrollHeight = 600;
    page([new Box("field", { left: 20, top: 300, width: 80, height: 36 }), popup]);
    position("field", "field-popup");
    expect(popup.attributes.get("style")).toContain("top:192px;");
    expect(popup.attributes.get("data-side")).toBe("bottom");
  });

  it("rounds the natural width up before fixing it, without measuring the opening transform", () => {
    const trigger = new Box("field", { left: 20, top: 30, width: 80, height: 36 });
    const popup = new Box("field-popup", { left: 0, top: 0, width: 172.375, height: 240 });
    popup.offsetWidth = 172;
    popup.clientWidth = popup.bar = 172;
    popup.getBoundingClientRect = () => ({ width: 163.75 }) as DOMRect;
    page([trigger, popup]);
    position("field", "field-popup");
    expect(popup.attributes.get("style")).toContain("width:173px;");
  });

  it("is placed again as wide as its scrollbar more, in the visible viewport", () => {
    const trigger = new Box("field", { left: 20, top: 30, width: 80, height: 36 });
    const popup = new Box("field-popup", { left: 0, top: 0, width: 120, height: 240 });
    popup.bar = 105;
    page([trigger, popup], null, { offsetLeft: 0, offsetTop: 0, width: 300, height: 600 });
    position("field", "field-popup");
    expect(popup.attributes.get("style")).toBe(
      "position:fixed;left:20px;top:70px;width:135px;min-width:0;max-height:240px;overflow-y:auto;overflow-x:hidden;margin:0;z-index:1100",
    );
  });

  it("does nothing without its trigger or its list", () => {
    const popup = new Box("field-popup", { left: 0, top: 0, width: 120, height: 240 });
    page([popup]);
    position("field", "field-popup");
    expect(popup.attributes.size).toBe(0);
  });

  it("scrolls its focused option into view, and no other element", () => {
    const option = new Box(
      "field-popup-3",
      { left: 0, top: 290, width: 100, height: 30 },
      "option",
    );
    const popup = new Box("field-popup", { left: 0, top: 100, width: 120, height: 200 });
    popup.children.push(option);
    page([popup], option);
    reveal(popup as unknown as HTMLElement);
    expect(popup.scrollTop).toBe(24);
    const outside = new Box("other", { left: 0, top: 290, width: 100, height: 30 }, "option");
    page([popup], outside);
    reveal(popup as unknown as HTMLElement);
    expect(popup.scrollTop).toBe(24);
    page([popup], popup);
    reveal(popup as unknown as HTMLElement);
    expect(popup.scrollTop).toBe(24);
  });

  it("follows the window, the page's scrolls and the visible viewport until its cleanup", () => {
    const trigger = new Box("field", { left: 20, top: 30, width: 80, height: 36 });
    const popup = new Box("field-popup", { left: 0, top: 100, width: 120, height: 200 });
    // The option focused, 134 pixels below the list's view, is the list's once the focus moves
    // into it, below.
    const option = new Box(
      "field-popup-0",
      { left: 0, top: 400, width: 100, height: 30 },
      "option",
    );
    const heard = page([trigger, popup], option, {
      offsetLeft: 0,
      offsetTop: 0,
      width: 400,
      height: 800,
    });
    const stop = anchor("field", "field-popup");
    expect(heard.map(([name]) => name)).toEqual([
      "window:resize",
      "document:scroll",
      "document:focusin",
      "visual:resize",
      "visual:scroll",
    ]);
    expect(heard[1]?.[2]).toBe(true);
    const listener = heard[0]?.[1] as (event: { type: string; target?: unknown }) => void;
    // The page moved: placed again, from where the trigger is now.
    trigger.box.top = 130;
    listener({ type: "resize" });
    expect(popup.attributes.get("style")).toContain("top:170px");
    // The list scrolled itself: left where the reader scrolled it.
    popup.children.push(option);
    trigger.box.top = 230;
    listener({ type: "scroll", target: option });
    expect(popup.attributes.get("style")).toContain("top:170px");
    expect(popup.scrollTop).toBe(0);
    // The page scrolled: placed again, its focused option brought into view.
    listener({ type: "scroll", target: trigger });
    expect(popup.attributes.get("style")).toContain("top:270px");
    expect(popup.scrollTop).toBe(134);
    // The focus moved within it: the focused option is brought into view, nothing placed.
    trigger.box.top = 330;
    listener({ type: "focusin" });
    expect(popup.scrollTop).toBe(268);
    expect(popup.attributes.get("style")).toContain("top:270px");
    stop();
    expect(heard).toEqual([]);
  });

  it("keeps where the reader scrolled its options when the page moves it", () => {
    const trigger = new Box("field", { left: 20, top: 30, width: 80, height: 36 });
    const popup = new Box("field-popup", { left: 0, top: 70, width: 120, height: 200 });
    popup.scrollHeight = 900;
    const heard = page([trigger, popup], popup);
    const stop = anchor("field", "field-popup");
    expect(popup.scrollTop).toBe(0);
    const listener = heard[1]?.[1] as (event: { type: string; target?: unknown }) => void;
    // The reader scrolls the list to an option further down, then the page scrolls under it.
    popup.scrollTop = 57;
    trigger.box.top = 10;
    listener({ type: "scroll", target: trigger });
    expect(popup.attributes.get("style")).toContain("top:50px;");
    expect(popup.scrollTop).toBe(57);
    // And so does a resize, which measures it again too.
    listener({ type: "resize" });
    expect(popup.scrollTop).toBe(57);
    stop();
  });

  it("is placed again by any event once its list is gone, which finds nothing to place", () => {
    const heard = page([]);
    const stop = anchor("field", "field-popup");
    const listener = heard[0]?.[1] as (event: { type: string }) => void;
    expect(() => listener({ type: "focusin" })).not.toThrow();
    stop();
    expect(heard).toEqual([]);
  });
});
