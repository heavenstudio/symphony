import { afterEach, describe, expect, it, vi } from "vitest";
import {
  keyed,
  nameOf,
  panelShowsMedia,
  photo,
  pictureSet,
  pressed,
  type Reach,
  scaleFrame,
  scan,
  startMedia,
  Viewer,
  zoomable,
} from "./media-layer";
import { WAITING } from "./media-picture";

// The tests run without a DOM: the page here is a tree of elements that knows the selectors the
// layer asks for (a tag, `#id`, `.class`, `[name]`, `[name=value]`, `:not(…)`, the descendant and
// the child combinators, and lists of them).

interface Compound {
  tag: string | null;
  ids: string[];
  classes: string[];
  attributes: [string, string | null][];
  nots: Compound[];
}

/** `text` split where `at` is a character outside brackets and parentheses. */
function split(text: string, at: (char: string) => boolean): string[] {
  const parts: string[] = [];
  let depth = 0;
  let part = "";
  for (const char of text) {
    if (char === "(" || char === "[") depth += 1;
    if (char === ")" || char === "]") depth -= 1;
    if (depth === 0 && at(char)) {
      parts.push(part);
      part = "";
    } else {
      part += char;
    }
  }
  return [...parts, part].map((piece) => piece.trim()).filter((piece) => piece !== "");
}

function compound(text: string): Compound {
  const read: Compound = { tag: null, ids: [], classes: [], attributes: [], nots: [] };
  let rest = text;
  const tag = /^(\*|[a-z][a-z0-9-]*)/i.exec(rest);
  if (tag) {
    read.tag = tag[1] === "*" ? null : (tag[1] ?? "").toUpperCase();
    rest = rest.slice(tag[0].length);
  }
  while (rest !== "") {
    const id = /^#([\w-]+)/.exec(rest);
    const name = /^\.([\w-]+)/.exec(rest);
    const attribute = /^\[([\w-]+)(?:=(?:"([^"]*)"|([^\]]*)))?\]/.exec(rest);
    const not = /^:not\(/.exec(rest);
    if (id) read.ids.push(id[1] ?? "");
    else if (name) read.classes.push(name[1] ?? "");
    else if (attribute) {
      read.attributes.push([attribute[1] ?? "", attribute[2] ?? attribute[3] ?? null]);
    } else if (not) {
      const inner = split(rest.slice(4), (char) => char === ")")[0] ?? "";
      read.nots.push(compound(inner.slice(1)));
      rest = rest.slice(4 + inner.length + 1);
      continue;
    } else throw new Error(`no selector reads ${rest}`);
    rest = rest.slice((id ?? name ?? attribute)?.[0].length ?? 0);
  }
  return read;
}

/** A selector of compounds, each after its combinator to the one before (` ` or `>`). */
type Chain = { compound: Compound; child: boolean }[];

function chains(selector: string): Chain[] {
  return split(selector, (char) => char === ",").map((one) => {
    const chain: Chain = [];
    let child = false;
    for (const piece of split(one.replaceAll(">", " > "), (char) => char === " ")) {
      if (piece === ">") child = true;
      else {
        chain.push({ compound: compound(piece), child });
        child = false;
      }
    }
    return chain;
  });
}

class FakeElement {
  readonly tagName: string;
  parent: FakeElement | null = null;
  children: FakeElement[] = [];
  /** The root of the page, to which every element on it is connected. */
  root = false;
  box = { width: 0, height: 0 };
  /** Its own text, which its children's follows. */
  text = "";
  /** How it was last given the focus, by its options; none when it never was. */
  focused: unknown[] = [];
  private readonly attributes = new Map<string, string>();

  constructor(tag: string, attributes: Record<string, string> = {}, children: FakeElement[] = []) {
    this.tagName = tag.toUpperCase();
    for (const [name, value] of Object.entries(attributes)) this.setAttribute(name, value);
    this.append(...children);
  }

  get id(): string {
    return this.getAttribute("id") ?? "";
  }

  get isConnected(): boolean {
    let top: FakeElement = this;
    while (top.parent) top = top.parent;
    return top.root;
  }

  getAttribute(name: string): string | null {
    return this.attributes.get(name) ?? null;
  }

  setAttribute(name: string, value: string): void {
    this.attributes.set(name, String(value));
  }

  hasAttribute(name: string): boolean {
    return this.attributes.has(name);
  }

  removeAttribute(name: string): void {
    this.attributes.delete(name);
  }

  append(...children: FakeElement[]): void {
    for (const child of children) {
      child.parent = this;
      this.children.push(child);
    }
  }

  remove(): void {
    if (this.parent) this.parent.children = this.parent.children.filter((one) => one !== this);
    this.parent = null;
  }

  getBoundingClientRect(): { width: number; height: number } {
    return this.box;
  }

  get textContent(): string {
    return this.text + this.children.map((child) => child.textContent).join("");
  }

  focus(options?: unknown): void {
    this.focused.push(options);
  }

  private is(read: Compound): boolean {
    const classes = (this.getAttribute("class") ?? "").split(" ");
    return (
      (read.tag === null || read.tag === this.tagName) &&
      read.ids.every((id) => this.id === id) &&
      read.classes.every((name) => classes.includes(name)) &&
      read.attributes.every(([name, value]) =>
        value === null ? this.hasAttribute(name) : this.getAttribute(name) === value,
      ) &&
      read.nots.every((not) => !this.is(not))
    );
  }

  private from(chain: Chain, at: number): boolean {
    const step = chain[at];
    if (!step || !this.is(step.compound)) return false;
    if (at === 0) return true;
    if (step.child) return this.parent?.from(chain, at - 1) ?? false;
    for (let up = this.parent; up; up = up.parent) if (up.from(chain, at - 1)) return true;
    return false;
  }

  matches(selector: string): boolean {
    return chains(selector).some((chain) => this.from(chain, chain.length - 1));
  }

  closest(selector: string): FakeElement | null {
    for (let at: FakeElement | null = this; at; at = at.parent) if (at.matches(selector)) return at;
    return null;
  }

  querySelectorAll(selector: string): FakeElement[] {
    const below = (element: FakeElement): FakeElement[] =>
      element.children.flatMap((child) => [child, ...below(child)]);
    return below(this).filter((element) => element.matches(selector));
  }

  querySelector(selector: string): FakeElement | null {
    return this.querySelectorAll(selector)[0] ?? null;
  }
}

const h = (tag: string, attributes: Record<string, string> = {}, ...children: FakeElement[]) =>
  new FakeElement(tag, attributes, children);

/** A page whose body holds `body`, its document stubbed with what the layer reads of it. */
function page(...body: FakeElement[]) {
  const head = h("head");
  const html = h("html", {}, head, h("body", {}, ...body));
  html.root = true;
  const doc = Object.assign(new EventTarget(), {
    documentElement: html,
    head,
    getElementById: (id: string) => html.querySelector(`#${id}`),
    querySelectorAll: (selector: string) => html.querySelectorAll(selector),
    createElement: (tag: string) => h(tag),
  });
  vi.stubGlobal("document", doc);
  return doc;
}

/** The requests the overlay module's host was given, through the shell's `reviewOverlay`. */
function overlay() {
  const asked: Record<string, unknown>[] = [];
  vi.stubGlobal("window", {
    reviewOverlay: (request: Record<string, unknown>) => asked.push(request),
  });
  return asked;
}

/** What each request asked, without its callback. */
const shown = (asked: Record<string, unknown>[]) =>
  asked.map(({ callback: _, ...request }) => request);

/** An event on `target`, as a press or a key gives it. */
function event(target: unknown, fields: Record<string, unknown> = {}) {
  return { target, preventDefault: vi.fn(), ...fields } as unknown as Event & {
    preventDefault: ReturnType<typeof vi.fn>;
  };
}

const click = (target: unknown, fields: Record<string, unknown> = {}) =>
  event(target, { button: 0, ctrlKey: false, metaKey: false, shiftKey: false, ...fields });

const key = (target: unknown, name: string, fields: Record<string, unknown> = {}) =>
  event(target, { key: name, repeat: false, ...fields }) as unknown as KeyboardEvent & {
    preventDefault: ReturnType<typeof vi.fn>;
  };

const asElement = (element: FakeElement) => element as unknown as Element;

/** Stubbed observers: what each observes, and the callback to call as the browser would. */
function observers() {
  const resized: {
    observed: Set<unknown>;
    disconnected: number;
    call: (entries: unknown) => void;
  }[] = [];
  const mutated: { options: unknown; disconnected: number; call: () => void }[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      readonly kept: (typeof resized)[number];
      constructor(call: (entries: unknown) => void) {
        this.kept = { observed: new Set(), disconnected: 0, call };
        resized.push(this.kept);
      }
      observe(target: unknown) {
        this.kept.observed.add(target);
      }
      unobserve(target: unknown) {
        this.kept.observed.delete(target);
      }
      disconnect() {
        this.kept.disconnected += 1;
      }
    },
  );
  vi.stubGlobal(
    "MutationObserver",
    class {
      readonly kept: (typeof mutated)[number];
      constructor(call: () => void) {
        this.kept = { options: null, disconnected: 0, call };
        mutated.push(this.kept);
      }
      observe(_: unknown, options: unknown) {
        this.kept.options = options;
      }
      disconnect() {
        this.kept.disconnected += 1;
      }
    },
  );
  return { resized, mutated };
}

afterEach(() => vi.unstubAllGlobals());

describe("a picture that opens in the viewer", () => {
  it("is one the page left as it is, outside a link or in a link to a picture", () => {
    expect(zoomable(asElement(h("img")))).toBe(true);
    for (const href of ["/a.PNG", "/a.jpg?v=2", "/a.jpeg#x", "/a.gif", "/a.webp", "/a.svg"]) {
      const image = h("img");
      h("a", { href }, image);
      expect(zoomable(asElement(image)), href).toBe(true);
    }
  });

  it("is none the page set apart, hid or made decoration, or that links elsewhere", () => {
    for (const attributes of [
      { "data-no-lightbox": "" },
      { "aria-hidden": "true" },
      { role: "presentation" },
      { role: "none" },
    ]) {
      expect(zoomable(asElement(h("img", attributes))), JSON.stringify(attributes)).toBe(false);
    }
    expect(zoomable(asElement(h("img", { "aria-hidden": "false", role: "img" })))).toBe(true);
    const linked = h("img");
    h("a", { href: "/docs/a.md" }, linked);
    expect(zoomable(asElement(linked))).toBe(false);
    const bare = h("img");
    h("a", {}, bare);
    expect(zoomable(asElement(bare))).toBe(true);
  });

  it("shows its original, else its src, captioned by its words", () => {
    expect(photo(asElement(h("img", { src: "/a.png", alt: "A" })))).toEqual({
      src: "/a.png",
      caption: "A",
    });
    expect(photo(asElement(h("img", { src: WAITING, "data-full": "/full.png" })))).toEqual({
      src: "/full.png",
      caption: "",
    });
    expect(photo(asElement(h("img", { src: WAITING })))).toBeNull();
    expect(photo(asElement(h("img")))).toBeNull();
  });
});

describe("a mockup's frame", () => {
  it("is scaled from 1280 pixels to its box", () => {
    const frame = h("iframe");
    const stage = h("div", {}, frame);
    stage.box = { width: 640, height: 400 };
    scaleFrame(asElement(stage));
    expect(frame.getAttribute("style")).toBe("transform:scale(0.5)");
  });

  it("is left alone while its box has no width, and a stage without one is nothing", () => {
    const frame = h("iframe", { style: "transform:scale(0.25)" });
    scaleFrame(asElement(h("div", {}, frame)));
    expect(frame.getAttribute("style")).toBe("transform:scale(0.25)");
    const empty = h("div");
    empty.box = { width: 640, height: 400 };
    expect(() => scaleFrame(asElement(empty))).not.toThrow();
  });
});

describe("what the page shows, readied", () => {
  const scanned = (...body: FakeElement[]) => {
    const doc = page(...body);
    const { resized } = observers();
    const resize = new ResizeObserver(() => {});
    const targets = new Set<Element>();
    scan(resize, targets);
    return { doc, resize, targets, observed: resized[0]?.observed ?? new Set() };
  };

  it("observes and scales each new mockup's frame once", () => {
    const frame = h("iframe");
    const stage = h("div", { class: "mockup-frame" }, frame);
    stage.box = { width: 320, height: 200 };
    const { resize, targets, observed } = scanned(h("main", {}, stage));
    expect(stage.getAttribute("data-media-observed")).toBe("true");
    expect(frame.getAttribute("style")).toBe("transform:scale(0.25)");
    expect([...observed]).toEqual([stage]);
    frame.setAttribute("style", "");
    scan(resize, targets);
    expect(frame.getAttribute("style")).toBe("");
    expect([...targets]).toEqual([stage]);
  });

  it("stops observing a frame the page dropped", () => {
    const stage = h("div", { class: "mockup-frame" }, h("iframe"));
    const { resize, targets, observed } = scanned(h("main", {}, stage));
    stage.remove();
    scan(resize, targets);
    expect(targets.size).toBe(0);
    expect(observed.size).toBe(0);
  });

  it("loads each viewer's script once, and only the project's own", () => {
    const stage = (script: string) =>
      h("div", { class: "viewer-stage", "data-viewer-script": script });
    const { doc, resize, targets } = scanned(
      h(
        "main",
        {},
        stage("/static/orbit.js"),
        stage("/static/orbit.js"),
        stage("https://elsewhere.test/x.js"),
        stage("/static/other.js"),
      ),
    );
    scan(resize, targets);
    const scripts = doc.head.children.map((script) => [
      script.tagName,
      script.getAttribute("type"),
      script.getAttribute("src"),
      script.getAttribute("data-viewer-module"),
    ]);
    expect(scripts).toEqual([
      ["SCRIPT", "module", "/static/orbit.js", "/static/orbit.js"],
      ["SCRIPT", "module", "/static/other.js", "/static/other.js"],
    ]);
  });

  it("makes each of the page's pictures that opens in the viewer a button that names it", () => {
    const plain = h("img", { alt: "Pendant A" });
    const own = h("img", { alt: "B", tabindex: "-1", role: "img", "aria-label": "Their own" });
    const hidden = h("img", { "aria-hidden": "true" });
    const linked = h("img");
    const outside = h("img");
    scanned(h("main", {}, plain, own, hidden, h("a", { href: "/a.png" }, linked)), outside);
    expect([plain.getAttribute("tabindex"), plain.getAttribute("role")]).toEqual(["0", "button"]);
    expect(plain.getAttribute("aria-label")).toBe("Open picture Pendant A");
    expect([own.getAttribute("tabindex"), own.getAttribute("role")]).toEqual(["-1", "img"]);
    expect(own.getAttribute("aria-label")).toBe("Their own");
    for (const image of [hidden, linked]) {
      expect(image.getAttribute("data-zoom-ready")).toBe("true");
      expect(image.hasAttribute("role")).toBe(false);
    }
    expect(outside.hasAttribute("data-zoom-ready")).toBe(false);
    const nameless = h("img");
    scanned(h("main", {}, nameless));
    expect(nameless.getAttribute("aria-label")).toBe("Open picture");
  });

  it("names a picture with no alt by its figure's caption, and tells ten of a row apart", () => {
    const caption = h("figcaption");
    caption.text = " Picture 2 ";
    const captioned = h("img", { alt: "  " });
    const names = ["Desktop dark before", "Desktop dark after", "Phone light before"];
    const row = names.map((alt) => h("figure", {}, h("img", { alt }), h("figcaption")));
    scanned(
      h("main", {}, h("div", { class: "compare" }, ...row, h("figure", {}, captioned, caption))),
    );
    expect(row.map((figure) => figure.children[0]?.getAttribute("aria-label"))).toEqual(
      names.map((name) => `Open picture ${name}`),
    );
    expect(captioned.getAttribute("aria-label")).toBe("Open picture Picture 2");
    expect(nameOf(asElement(captioned))).toBe("Picture 2");
    captioned.setAttribute("src", "/c.png");
    expect(photo(asElement(captioned))).toEqual({ src: "/c.png", caption: "Picture 2" });
    const bare = h("img");
    h("figure", {}, bare);
    expect(nameOf(asElement(bare))).toBe("");
    expect(nameOf(asElement(h("img")))).toBe("");
  });
});

describe("the pictures a picture opens the viewer on", () => {
  const img = (src: string, attributes: Record<string, string> = {}) =>
    h("img", { src, alt: src.slice(1, 2).toUpperCase(), ...attributes });

  it("are its card's comparison's, else its card's own from its history", () => {
    const [a, b] = [img("/a.png"), img("/b.png")];
    const old = img("/old.png");
    const card = h(
      "section",
      { class: "decision" },
      h("div", { class: "compare" }, a, b),
      h("div", { class: "history" }, old),
    );
    page(h("main", {}, card));
    expect(pictureSet(asElement(b))).toEqual([
      [
        { src: "/a.png", caption: "A" },
        { src: "/b.png", caption: "B" },
      ],
      1,
    ]);
    expect(pictureSet(asElement(old))?.[1]).toBe(2);
    expect(pictureSet(asElement(old))?.[0]).toHaveLength(3);
  });

  it("are the card's for a card without a comparison, else the page's", () => {
    const lone = img("/l.png");
    const card = h("section", { class: "decision" }, lone, img("/m.png"));
    const [p, q] = [img("/p.png"), img("/q.png")];
    page(h("main", {}, card, h("figure", {}, p), q));
    expect(pictureSet(asElement(lone))).toEqual([
      [
        { src: "/l.png", caption: "L" },
        { src: "/m.png", caption: "M" },
      ],
      0,
    ]);
    expect(pictureSet(asElement(q))?.[1]).toBe(3);
    expect(pictureSet(asElement(p))?.[0].map(({ src }) => src)).toEqual([
      "/l.png",
      "/m.png",
      "/p.png",
      "/q.png",
    ]);
  });

  it("leave out what is no picture yet or opens nowhere, and are none outside the page", () => {
    const shown = img("/s.png");
    const waiting = img(WAITING);
    const hidden = img("/h.png", { "aria-hidden": "true" });
    page(h("main", {}, shown, waiting, hidden));
    expect(pictureSet(asElement(shown))).toEqual([[{ src: "/s.png", caption: "S" }], 0]);
    expect(pictureSet(asElement(waiting))).toBeNull();
    expect(pictureSet(asElement(hidden))).toEqual([[{ src: "/h.png", caption: "H" }], 0]);
    expect(pictureSet(asElement(img("/x.png")))).toBeNull();
  });
});

describe("the picture viewer", () => {
  const photos = [
    { src: "/a.png", caption: "A" },
    { src: "/b.png", caption: "B" },
    { src: "/c.png", caption: "C" },
  ];

  it("opens as its owner on the pictures and the place given, each its own copy", () => {
    const asked = overlay();
    new Viewer("review-pictures", new AbortController().signal).open(photos, 1);
    expect(shown(asked)).toEqual([{ kind: "picture", owner: "review-pictures", at: 1, photos }]);
    const given = asked[0]?.photos as unknown[] | undefined;
    expect(given).not.toBe(photos);
    expect(given?.[0]).not.toBe(photos[0]);
  });

  it("moves round the set by its buttons' directions, and closes on 0 or anything else", () => {
    const asked = overlay();
    const viewer = new Viewer("review-pictures", new AbortController().signal);
    viewer.open(photos, 0);
    const answer = asked[0]?.callback as (direction: unknown) => void;
    expect(answer).toBe(viewer.answer);
    for (const direction of [-1, 1, 1, 1, 4, -5, 2.9]) answer(direction);
    expect(asked.slice(1).map(({ at }) => at)).toEqual([2, 0, 1, 2, 0, 1, 0]);
    for (const direction of [0, Number.NaN, Number.POSITIVE_INFINITY, "1", null, 0.5]) {
      viewer.open(photos, 0);
      answer(direction);
      expect(shown(asked).at(-1), String(direction)).toEqual({
        kind: "close",
        owner: "review-pictures",
      });
    }
  });

  it("asks nothing to move with no pictures, and closes on an empty set", () => {
    const asked = overlay();
    const viewer = new Viewer("review-pictures", new AbortController().signal);
    viewer.move(1);
    expect(asked).toEqual([]);
    viewer.open([], 0);
    expect(shown(asked)).toEqual([{ kind: "close", owner: "review-pictures" }]);
  });

  it("asks nothing once its page is gone, but its close for good", () => {
    const asked = overlay();
    const page = new AbortController();
    const viewer = new Viewer("review-pictures", page.signal);
    page.abort();
    viewer.open(photos, 0);
    viewer.close();
    expect(asked).toEqual([]);
    viewer.end();
    expect(shown(asked)).toEqual([{ kind: "close", owner: "review-pictures" }]);
  });

  it("asks no one without an overlay host", () => {
    vi.stubGlobal("window", { reviewOverlay: "not yet" });
    expect(() => new Viewer("o", new AbortController().signal).open(photos, 0)).not.toThrow();
  });
});

describe("a press on the page", () => {
  const opened = () => {
    const asked = overlay();
    return { asked, viewer: new Viewer("review-pictures", new AbortController().signal) };
  };

  it("on a picture opens the viewer on its set, the page's own action held back", () => {
    const [a, b] = [h("img", { src: "/a.png", alt: "A" }), h("img", { src: "/b.png" })];
    page(h("main", {}, a, b));
    const { asked, viewer } = opened();
    const press = click(b);
    pressed(press, viewer);
    expect(press.preventDefault).toHaveBeenCalled();
    expect(asked[0]).toMatchObject({ kind: "picture", at: 1 });
    // A picture the layer made no button takes no focus.
    expect(b.focused).toEqual([]);
  });

  it("gives the picture the focus before it opens, so the viewer gives it back there", () => {
    const [a, b] = ["A", "B"].map((alt) =>
      h("img", { src: `/${alt.toLowerCase()}.png`, alt, tabindex: "0", role: "button" }),
    ) as [FakeElement, FakeElement];
    page(h("main", {}, a, b));
    const { asked, viewer } = opened();
    pressed(click(b), viewer);
    expect(b.focused).toEqual([{ preventScroll: true }]);
    expect(a.focused).toEqual([]);
    expect(asked[0]).toMatchObject({
      kind: "picture",
      at: 1,
      photos: [
        { src: "/a.png", caption: "A" },
        { src: "/b.png", caption: "B" },
      ],
    });
  });

  it("with a modifier or another button, or on what opens nowhere, does nothing", () => {
    const picture = h("img", { src: "/a.png" });
    const hidden = h("img", { src: "/h.png", "aria-hidden": "true" });
    const waiting = h("img", { src: WAITING });
    const text = h("p");
    // A picture outside the page's own regions has no set to open on.
    const outside = h("img", { src: "/out.png", tabindex: "0" });
    page(h("main", {}, picture, hidden, waiting, text), outside);
    const { asked, viewer } = opened();
    for (const fields of [
      { ctrlKey: true },
      { metaKey: true },
      { shiftKey: true },
      { button: 1 },
    ]) {
      pressed(click(picture, fields), viewer);
    }
    for (const target of [hidden, waiting, text, outside, null, { closest: "no element" }]) {
      const press = click(target);
      pressed(press, viewer);
      expect(press.preventDefault).not.toHaveBeenCalled();
    }
    expect(asked).toEqual([]);
    expect(outside.focused).toEqual([]);
  });

  it("on the open viewer's backdrop closes it", () => {
    const backdrop = h("div", { id: "media-lightbox" });
    page(backdrop);
    const { asked, viewer } = opened();
    pressed(click(backdrop), viewer);
    expect(shown(asked)).toEqual([{ kind: "close", owner: "review-pictures" }]);
  });

  it("on a view's button sets its attributes on its viewer's element and calls its method", () => {
    const settle = vi.fn();
    const element = Object.assign(h("orbit-view", { zoom: "1" }), { settle });
    const label = h("span");
    const button = h(
      "button",
      { "data-viewer-set": "angle=0deg 25deg\nzoom=2\nBad=x", "data-viewer-call": "settle" },
      label,
    );
    page(
      h(
        "figure",
        { class: "media-viewer" },
        h("div", { class: "viewer-stage" }, element),
        h("div", { class: "views" }, button),
      ),
    );
    const { asked, viewer } = opened();
    pressed(click(label), viewer);
    expect([element.getAttribute("angle"), element.getAttribute("zoom")]).toEqual([
      "0deg 25deg",
      "2",
    ]);
    expect(element.hasAttribute("Bad")).toBe(false);
    expect(settle).toHaveBeenCalledTimes(1);
    expect(settle.mock.contexts[0]).toBe(element);
    expect(asked).toEqual([]);
  });

  it("on a view's button calls nothing it does not name or that is no method", () => {
    const element = Object.assign(h("orbit-view"), { settle: "not a method" });
    const plain = h("button", { "data-viewer-set": "zoom=3" });
    const named = h("button", { "data-viewer-set": "", "data-viewer-call": "settle" });
    const loose = h("button", { "data-viewer-set": "zoom=4" });
    page(
      h(
        "figure",
        { class: "media-viewer" },
        h("div", { class: "viewer-stage" }, element),
        plain,
        named,
      ),
      loose,
    );
    const { viewer } = opened();
    pressed(click(plain), viewer);
    expect(element.getAttribute("zoom")).toBe("3");
    expect(() => pressed(click(named), viewer)).not.toThrow();
    pressed(click(loose), viewer);
    expect(element.getAttribute("zoom")).toBe("3");
  });
});

describe("a key on the page", () => {
  const open = (state: string) => {
    const picture = h("img", { src: "/a.png" });
    page(h("div", { id: "media-lightbox", "data-state": state }), h("main", {}, picture));
    const asked = overlay();
    const viewer = new Viewer("review-pictures", new AbortController().signal);
    viewer.open(
      [
        { src: "/a.png", caption: "" },
        { src: "/b.png", caption: "" },
      ],
      0,
    );
    return { asked, viewer, picture };
  };

  it("in the open viewer moves by its arrows and closes on Escape, the page's own held back", () => {
    const { asked, viewer, picture } = open("open");
    const keys = ["ArrowRight", "ArrowLeft", "ArrowLeft", "Escape"].map((name) =>
      key(picture, name),
    );
    for (const press of keys) keyed(press, viewer);
    expect(keys.every((press) => press.preventDefault.mock.calls.length === 1)).toBe(true);
    expect(shown(asked).slice(1)).toEqual([
      expect.objectContaining({ at: 1 }),
      expect.objectContaining({ at: 0 }),
      expect.objectContaining({ at: 1 }),
      { kind: "close", owner: "review-pictures" },
    ]);
  });

  it("in the open viewer is nothing but its own keys", () => {
    const { asked, viewer, picture } = open("open");
    const enter = key(picture, "Enter");
    keyed(enter, viewer);
    expect(enter.preventDefault).not.toHaveBeenCalled();
    expect(asked).toHaveLength(1);
  });

  it("elsewhere presses on Enter or Space, once however long it is held", () => {
    for (const name of ["Enter", " "]) {
      const { asked, viewer, picture } = open("closed");
      keyed(key(picture, name), viewer);
      keyed(key(picture, name, { repeat: true }), viewer);
      keyed(key(picture, "ArrowRight"), viewer);
      expect(
        asked.map(({ kind }) => kind),
        JSON.stringify(name),
      ).toEqual(["picture", "picture"]);
    }
  });

  it("presses with no viewer drawn yet", () => {
    const picture = h("img", { src: "/a.png" });
    page(h("main", {}, picture));
    const asked = overlay();
    keyed(key(picture, "Enter"), new Viewer("review-pictures", new AbortController().signal));
    expect(asked.map(({ kind }) => kind)).toEqual(["picture"]);
  });
});

/** `type` said on `doc` as though from `target`. */
function fire(
  doc: EventTarget,
  type: string,
  target: unknown,
  fields: Record<string, unknown> = {},
) {
  const said = new Event(type, { cancelable: true });
  Object.defineProperty(said, "target", { value: target });
  Object.assign(said, fields);
  doc.dispatchEvent(said);
  return said;
}

describe("the media layer", () => {
  const started = (...body: FakeElement[]) => {
    const doc = page(...body);
    const kept = observers();
    const asked = overlay();
    const layer = new AbortController();
    startMedia("review-pictures", layer.signal);
    return { doc, asked, layer, ...kept };
  };

  it("readies the page as it starts and again as the page changes", () => {
    const stage = h("div", { class: "mockup-frame" }, h("iframe"));
    const main = h("main", {}, stage);
    const { mutated, resized } = started(main);
    expect(mutated[0]?.options).toEqual({ childList: true, subtree: true });
    expect([...(resized[0]?.observed ?? [])]).toEqual([stage]);
    const later = h("img", { src: "/later.png" });
    main.append(later);
    mutated[0]?.call();
    expect(later.getAttribute("role")).toBe("button");
  });

  it("scales a frame its observer says was resized", () => {
    const frame = h("iframe");
    const stage = h("div", { class: "mockup-frame" }, frame);
    const { resized } = started(h("main", {}, stage));
    stage.box = { width: 1280, height: 720 };
    resized[0]?.call([{ target: stage }]);
    expect(frame.getAttribute("style")).toBe("transform:scale(1)");
  });

  it("opens the viewer on a click and a key, as its owner", () => {
    const picture = h("img", { src: "/a.png", alt: "A" });
    const { doc, asked } = started(h("main", {}, picture));
    const press = fire(doc, "click", picture, { button: 0 });
    expect(press.defaultPrevented).toBe(true);
    fire(doc, "keydown", picture, { key: "Enter", repeat: false });
    expect(shown(asked)).toEqual([
      {
        kind: "picture",
        owner: "review-pictures",
        at: 0,
        photos: [{ src: "/a.png", caption: "A" }],
      },
      {
        kind: "picture",
        owner: "review-pictures",
        at: 0,
        photos: [{ src: "/a.png", caption: "A" }],
      },
    ]);
  });

  it("marks a fold busy while it is open and readies what it shows", () => {
    const inside = h("img", { src: "/in.png" });
    const fold = h("details", { open: "" }, inside);
    const { doc } = started(h("main", {}, fold));
    inside.removeAttribute("data-zoom-ready");
    inside.removeAttribute("role");
    fire(doc, "toggle", fold);
    expect(fold.getAttribute("data-busy")).toBe("true");
    expect(inside.getAttribute("role")).toBe("button");
    fold.removeAttribute("open");
    fire(doc, "toggle", fold);
    expect(fold.hasAttribute("data-busy")).toBe(false);
    const dialog = h("dialog", { open: "" });
    fire(doc, "toggle", dialog);
    fire(doc, "toggle", null);
    expect(dialog.hasAttribute("data-busy")).toBe(false);
  });

  it("ends with its signal: observers gone, nothing heard, frames unmarked, the viewer closed", () => {
    const stage = h("div", { class: "mockup-frame" }, h("iframe"));
    const picture = h("img", { src: "/a.png" });
    const fold = h("details", { open: "" });
    const { doc, asked, layer, mutated, resized } = started(h("main", {}, stage, picture, fold));
    layer.abort();
    expect([resized[0]?.disconnected, mutated[0]?.disconnected]).toEqual([1, 1]);
    expect(stage.hasAttribute("data-media-observed")).toBe(false);
    expect(shown(asked)).toEqual([{ kind: "close", owner: "review-pictures" }]);
    fire(doc, "click", picture, { button: 0 });
    fire(doc, "keydown", picture, { key: "Enter", repeat: false });
    fire(doc, "toggle", fold);
    expect(asked).toHaveLength(1);
    expect(fold.hasAttribute("data-busy")).toBe(false);
  });

  it("does not start for a page already gone", () => {
    page();
    const { resized, mutated } = observers();
    const layer = new AbortController();
    layer.abort();
    startMedia("review-pictures", layer.signal);
    expect([resized, mutated]).toEqual([[], []]);
  });
});

describe("a layer of the chat's pictures alone", () => {
  const CHAT: Reach = { pictures: "#quick-sheet .quick-message img", views: false };
  const message = (...pictures: FakeElement[]) =>
    h("article", { class: "quick-message" }, ...pictures);
  const chat = (...messages: FakeElement[]) =>
    h("aside", { id: "quick-sheet" }, h("div", { class: "quick-thread" }, ...messages));

  it("readies and opens the chat's pictures, the conversation's as one set", () => {
    const [a, b] = [h("img", { src: "/a.png", alt: "A" }), h("img", { src: "/b.png", alt: "B" })];
    const onPage = h("img", { src: "/page.png" });
    const doc = page(h("main", {}, onPage), chat(message(a), message(b)));
    observers();
    const asked = overlay();
    startMedia("panel-pictures", new AbortController().signal, CHAT);
    expect([a.getAttribute("role"), b.getAttribute("role")]).toEqual(["button", "button"]);
    expect(onPage.hasAttribute("data-zoom-ready")).toBe(false);
    fire(doc, "click", b, { button: 0 });
    fire(doc, "click", onPage, { button: 0 });
    expect(shown(asked)).toEqual([
      {
        kind: "picture",
        owner: "panel-pictures",
        at: 1,
        photos: [
          { src: "/a.png", caption: "A" },
          { src: "/b.png", caption: "B" },
        ],
      },
    ]);
  });

  it("leaves the page's frames, viewers, view buttons and folds to the page's own layer", () => {
    const stage = h("div", { class: "mockup-frame" }, h("iframe"));
    const viewer = h("div", { class: "viewer-stage", "data-viewer-script": "/static/v.js" });
    const settle = vi.fn();
    const element = Object.assign(h("orbit-view"), { settle });
    const button = h("button", { "data-viewer-set": "zoom=2", "data-viewer-call": "settle" });
    const fold = h("details", { open: "" });
    const doc = page(
      h(
        "main",
        {},
        stage,
        viewer,
        h(
          "figure",
          { class: "media-viewer" },
          h("div", { class: "viewer-stage" }, element),
          button,
        ),
        fold,
      ),
    );
    const { resized } = observers();
    overlay();
    const layer = new AbortController();
    startMedia("panel-pictures", layer.signal, CHAT);
    fire(doc, "click", button, { button: 0 });
    fire(doc, "toggle", fold);
    expect(stage.hasAttribute("data-media-observed")).toBe(false);
    expect(resized[0]?.observed.size).toBe(0);
    expect(doc.head.children).toEqual([]);
    expect(settle).not.toHaveBeenCalled();
    expect(element.hasAttribute("zoom")).toBe(false);
    expect(fold.hasAttribute("data-busy")).toBe(false);
    stage.setAttribute("data-media-observed", "true");
    layer.abort();
    expect(stage.getAttribute("data-media-observed")).toBe("true");
  });

  it("closes its viewer on the backdrop and moves it by the viewer's keys", () => {
    const [a, b] = [h("img", { src: "/a.png" }), h("img", { src: "/b.png" })];
    const lightbox = h("div", { id: "media-lightbox", "data-state": "closed" });
    const doc = page(lightbox, chat(message(a, b)));
    observers();
    const asked = overlay();
    startMedia("panel-pictures", new AbortController().signal, CHAT);
    fire(doc, "keydown", a, { key: "Enter", repeat: false });
    lightbox.setAttribute("data-state", "open");
    fire(doc, "keydown", a, { key: "ArrowRight", repeat: false });
    fire(doc, "click", lightbox, { button: 0 });
    expect(shown(asked).map((request) => [request.kind, request.at])).toEqual([
      ["picture", 0],
      ["picture", 1],
      ["close", undefined],
    ]);
  });
});

describe("the chat's panel", () => {
  it("shows media when a picture, a viewer or a frame in it has a box", () => {
    const sized = (tag: string, attributes: Record<string, string> = {}) => {
      const element = h(tag, attributes);
      element.box = { width: 10, height: 10 };
      return element;
    };
    for (const shown of [sized("img"), sized("div", { class: "viewer-stage" }), sized("iframe")]) {
      page(h("div", { id: "quick-root" }, shown));
      expect(panelShowsMedia(), shown.tagName).toBe(true);
    }
  });

  it("shows none when its media have no box, or only the page has any", () => {
    const flat = h("img");
    flat.box = { width: 10, height: 0 };
    const outside = h("img");
    outside.box = { width: 10, height: 10 };
    page(h("div", { id: "quick-root" }, flat, h("p")), h("main", {}, outside));
    expect(panelShowsMedia()).toBe(false);
  });
});
