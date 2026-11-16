import { afterEach, describe, expect, it, vi } from "vitest";
import { escapeCloses, focusFirst, returnFocus, takeFocus } from "./panel-focus";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** An element of the page as the focus moves read it: where it sits, its box and its marks. */
class Spot {
  readonly marks = new Map<string, string>();
  focused = 0;
  constructor(
    readonly page: Page,
    readonly inside: string[] = [],
    public box = { width: 10, height: 10 },
    public isConnected = true,
  ) {}
  getBoundingClientRect() {
    return this.box;
  }
  focus() {
    this.focused += 1;
    if (this.box.width > 0 || this.box.height > 0) this.page.activeElement = this;
  }
  closest(selector: string) {
    return selector.split(", ").some((one) => this.inside.includes(one)) ? this : null;
  }
  setAttribute(name: string, value: string) {
    this.marks.set(name, value);
  }
  removeAttribute(name: string) {
    this.marks.delete(name);
  }
}

/** The page: the elements each selector finds, and the one with the focus. */
class Page {
  activeElement: Spot | null = null;
  readonly found = new Map<string, Spot[]>();
  querySelectorAll(selector: string) {
    if (selector === "[data-panel-opener]") {
      return [...this.found.values()].flat().filter((spot) => spot.marks.has("data-panel-opener"));
    }
    return this.found.get(selector) ?? [];
  }
  querySelector(selector: string) {
    return this.querySelectorAll(selector)[0] ?? null;
  }
}

/** A window holding `values`, and a page stubbed as the document. */
function stubbed(values: Record<string, unknown> = {}) {
  const page = new Page();
  const window = Object.assign(new EventTarget(), values);
  vi.stubGlobal("window", window);
  vi.stubGlobal("document", page);
  vi.stubGlobal("Element", Spot);
  return { page, window: window as EventTarget & Record<string, unknown> };
}

describe("the focus moved into a sheet", () => {
  it("goes to the first selector's shown element once the task that opened the sheet ends", async () => {
    vi.useFakeTimers();
    const { page } = stubbed();
    const hidden = new Spot(page, [], { width: 0, height: 0 });
    const field = new Spot(page);
    const tab = new Spot(page);
    page.found.set("#field", [hidden, field]);
    page.found.set("#tab", [tab]);
    const done = focusFirst(["#field", "#tab"]);
    expect(field.focused).toBe(0);
    await vi.advanceTimersByTimeAsync(0);
    await done;
    expect(page.activeElement).toBe(field);
    expect(tab.focused).toBe(0);
  });

  it("is tried again while the sheet draws, then given up", async () => {
    vi.useFakeTimers();
    const { page } = stubbed();
    const done = focusFirst(["#late"]);
    await vi.advanceTimersByTimeAsync(100);
    const late = new Spot(page);
    page.found.set("#late", [late]);
    await vi.advanceTimersByTimeAsync(20);
    await done;
    expect(page.activeElement).toBe(late);
    const never = focusFirst(["#none"]);
    await vi.advanceTimersByTimeAsync(2_000);
    await expect(never).resolves.toBeUndefined();
  });

  it("leaves the focus where the reader put it in the sheet, before a try or between two", async () => {
    vi.useFakeTimers();
    const { page } = stubbed();
    const field = new Spot(page);
    page.found.set("#field", [field]);
    const tab = new Spot(page, ["#quick-sheet"]);
    page.activeElement = tab;
    const before = focusFirst(["#field"], "#quick-sheet");
    await vi.advanceTimersByTimeAsync(0);
    await before;
    expect(page.activeElement).toBe(tab);
    expect(field.focused).toBe(0);

    page.activeElement = null;
    const between = focusFirst(["#late"], "#quick-sheet");
    await vi.advanceTimersByTimeAsync(100);
    page.activeElement = tab;
    const late = new Spot(page);
    page.found.set("#late", [late]);
    await vi.advanceTimersByTimeAsync(20);
    await between;
    expect(page.activeElement).toBe(tab);
    expect(late.focused).toBe(0);
  });

  it("takes the focus from the shell's stand-in for the sheet, and from outside it", async () => {
    vi.useFakeTimers();
    for (const from of [["#quick-sheet", "#panel-loading"], ["#quick-open"]]) {
      const { page } = stubbed();
      const field = new Spot(page);
      page.found.set("#field", [field]);
      page.activeElement = new Spot(page, from);
      const done = focusFirst(["#field"], "#quick-sheet");
      await vi.advanceTimersByTimeAsync(0);
      await done;
      expect(page.activeElement, from.join()).toBe(field);
    }
  });

  it("moves into the sheet it opened, not where the reader already is in it", async () => {
    vi.useFakeTimers();
    const { page } = stubbed({ reviewPanelFocus: "activity" });
    const close = new Spot(page);
    page.found.set("#close", [close]);
    const link = new Spot(page, ["#activity-panel"]);
    page.activeElement = link;
    takeFocus("activity", ["#close"]);
    await vi.advanceTimersByTimeAsync(0);
    expect(page.activeElement).toBe(link);
    expect(close.focused).toBe(0);
  });

  it("moves only when the sheet's opener asked for it, and once", async () => {
    vi.useFakeTimers();
    const { page, window } = stubbed({ reviewPanelFocus: "activity" });
    const close = new Spot(page);
    page.found.set("#close", [close]);
    takeFocus("quick", ["#close"]);
    expect(window.reviewPanelFocus).toBe("activity");
    takeFocus("activity", ["#close"]);
    expect(window.reviewPanelFocus).toBeNull();
    await vi.advanceTimersByTimeAsync(0);
    expect(page.activeElement).toBe(close);
  });
});

describe("the focus given back as a sheet closes", () => {
  it("goes to the control that opened it, its mark taken off after", async () => {
    vi.useFakeTimers();
    const { page, window } = stubbed();
    const opener = new Spot(page);
    page.found.set("#opener", [opener]);
    page.found.set("#quick-open", [new Spot(page)]);
    window.reviewPanelOpener = opener;
    returnFocus("#quick-open");
    expect(window.reviewPanelOpener).toBeNull();
    expect(opener.marks.has("data-panel-opener")).toBe(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(page.activeElement).toBe(opener);
    expect(opener.marks.has("data-panel-opener")).toBe(false);
  });

  it("goes to the fallback for an opener gone, inside the panel, or no element", async () => {
    vi.useFakeTimers();
    for (const opener of [
      (page: Page) => new Spot(page, [], undefined, false),
      (page: Page) => new Spot(page, ["#quick-sheet"]),
      () => "not an element",
    ]) {
      const { page, window } = stubbed();
      const fallback = new Spot(page);
      page.found.set("#bell", [fallback]);
      window.reviewPanelOpener = opener(page);
      returnFocus("#bell");
      await vi.advanceTimersByTimeAsync(0);
      expect(page.activeElement).toBe(fallback);
    }
  });
});

describe("an Escape in a sheet", () => {
  const target = (...inside: string[]) => ({
    closest: (selector: string) => (inside.some((one) => selector.includes(one)) ? {} : null),
  });
  const pressed = (fields: Partial<Parameters<typeof escapeCloses>[0]>) => ({
    key: "Escape",
    defaultPrevented: false,
    target: target("#quick-sheet") as unknown as EventTarget,
    ...fields,
  });

  it("closes the sheet the focus is in", () => {
    expect(escapeCloses(pressed({}), "#quick-sheet")).toBe(true);
  });

  it("leaves it open for another key, a key a control took, or one pressed elsewhere", () => {
    expect(escapeCloses(pressed({ key: "Enter" }), "#quick-sheet")).toBe(false);
    expect(escapeCloses(pressed({ defaultPrevented: true }), "#quick-sheet")).toBe(false);
    expect(escapeCloses(pressed({}), "#activity-panel")).toBe(false);
    expect(escapeCloses(pressed({ target: null }), "#quick-sheet")).toBe(false);
    const inList = target("#quick-sheet", "[role=listbox]") as unknown as EventTarget;
    expect(escapeCloses(pressed({ target: inList }), "#quick-sheet")).toBe(false);
  });
});
