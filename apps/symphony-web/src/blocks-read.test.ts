import { describe, expect, it } from "vitest";
import { isMedia, isSpan, readBlocks } from "./blocks-read";
import { CASES } from "./testing";

const text = (words: string) => ({ type: "text", text: words });
const paragraph = (spans: unknown[]) => ({ type: "paragraph", spans });
const VIEWER = { element: "orbit-view", script: "/orbit.js", attributes: [["controls", ""]] };

describe("blocks read where they arrive", () => {
  it("are every block crates/symphony-api writes, as it writes them", () => {
    const sample = CASES.samples.blocks;
    expect(readBlocks(JSON.parse(JSON.stringify(sample)))).toEqual(sample);
    expect(readBlocks([])).toEqual([]);
  });

  it.each([
    ["a table without its presentation", { type: "table", head: [], rows: [] }],
    ["an empty presentation", { type: "container", tag: "div", presentation: {}, blocks: [] }],
    [
      "an icon without its tip",
      paragraph([{ type: "icon", shape: [], label: "Lamp", named: false }]),
    ],
    ["a figure without its picture, link or badge", { type: "figure", caption: [], size: "wide" }],
    [
      "a viewer without its views, its download or its labels",
      { type: "media", media: { kind: "viewer", src: "/a.item", label: "A", viewer: VIEWER } },
    ],
    [
      "a view without its title or call",
      {
        type: "media",
        media: {
          kind: "viewer",
          src: "/a.item",
          label: "A",
          viewer: { ...VIEWER, views: [{ label: "Above", set: [] }] },
        },
      },
    ],
    [
      "a gallery without its viewer",
      { type: "media", media: { kind: "gallery", items: [{ name: "A", src: "/a.item" }] } },
    ],
    [
      "a link without its target",
      paragraph([{ type: "wiki-link", href: "/a", spans: [text("a")] }]),
    ],
    [
      "an entry with no time or verdict",
      { type: "thread", entries: [{ who: "agent", blocks: [] }] },
    ],
    ["a comment", { type: "comment", text: "a note" }],
    ["the lowest level", { type: "heading", level: 0, anchor: "", spans: [] }],
    ["the highest level a byte holds", { type: "heading", level: 255, anchor: "", spans: [] }],
    ["a list from none", { type: "list", ordered: true, start: 0, items: [] }],
    [
      "a list from the largest safe integer",
      { type: "list", ordered: true, start: Number.MAX_SAFE_INTEGER, items: [] },
    ],
    ["fields a newer server adds", { type: "rule", width: 3, colour: "red" }],
  ])("take %s as serde does", (_, block) => {
    expect(readBlocks([block])).toEqual([block]);
  });

  // The kit's lists of kinds grow in a minor (docs/engineering/PLUGIN_PROTOCOL.md): a kind a newer server
  // sends is drawn as unsupported, and the blocks around it as they are.
  it.each([
    ["a block kind it does not know", { type: "marquee" }],
    ["a block kind it does not know, whatever its fields", { type: "marquee", spans: 3 }],
    ["an object's own name as a kind", { type: "constructor" }],
    ["a kind of no letters", { type: "" }],
    ["a span kind it does not know", paragraph([{ type: "blink", text: "x" }])],
    ["a media kind it does not know", { type: "media", media: { kind: "sound", src: "/a" } }],
    [
      "a span's media of a kind it does not know",
      paragraph([{ type: "media", media: { kind: "sound" } }]),
    ],
    [
      "a kind it does not know deep in a list",
      { type: "list", ordered: false, start: 1, items: [[paragraph([{ type: "x" }])]] },
    ],
  ])("take %s, which the components say", (_, block) => {
    const blocks = [paragraph([text("fine")]), block];
    expect(readBlocks(blocks)).toEqual(blocks);
  });

  it.each([
    ["nothing", undefined],
    ["a block", paragraph([])],
    ["text", "blocks"],
    ["a list holding nothing", [null]],
    ["a list holding a list", [[]]],
  ])("are none for %s", (_, value) => {
    expect(readBlocks(value)).toBeNull();
  });

  it.each([
    ["a kind that is not text", { type: 1 }],
    ["no kind", { spans: [] }],
    ["a null kind", { type: null }],
    ["a span whose kind is not text", paragraph([{ type: ["blink"], text: "x" }])],
    ["a span with no kind", paragraph([{ text: "x" }])],
    ["media whose kind is not text", { type: "media", media: { kind: 3, src: "/a" } }],
    ["media with no kind", { type: "media", media: { src: "/a" } }],
    [
      "a callout kind it does not know",
      { type: "callout", kind: "shout", title: [], resolved: false, blocks: [] },
    ],
    ["a figure size it does not know", { type: "figure", caption: [], size: "huge" }],
    [
      "a container it does not know",
      { type: "container", tag: "nav", presentation: {}, blocks: [] },
    ],
    [
      "an inline tag it does not know",
      paragraph([{ type: "styled", tag: "blink", presentation: {}, spans: [] }]),
    ],
    [
      "a shape it does not know",
      paragraph([
        { type: "icon", shape: [{ kind: "arc", attributes: {} }], label: "", named: false },
      ]),
    ],
    ["a thread author it does not know", { type: "thread", entries: [{ who: "bot", blocks: [] }] }],
    ["a level that is not a number", { type: "heading", level: "2", anchor: "", spans: [] }],
    // A level is a u8 and a list's start a u64: serde refuses a fraction, a sign or a size past
    // them, and the Dioxus client's reader a number past 2^53 - 1 it cannot tell apart.
    ["a level of a fraction", { type: "heading", level: 1.5, anchor: "", spans: [] }],
    ["a level past a byte", { type: "heading", level: 256, anchor: "", spans: [] }],
    ["a level below none", { type: "heading", level: -1, anchor: "", spans: [] }],
    [
      "a level that is no number at all",
      { type: "heading", level: Number.NaN, anchor: "", spans: [] },
    ],
    ["a start of a fraction", { type: "list", ordered: true, start: 1.5, items: [] }],
    ["a start below none", { type: "list", ordered: true, start: -1, items: [] }],
    ["a start past a safe integer", { type: "list", ordered: true, start: 2 ** 53, items: [] }],
    [
      "a start of infinity",
      { type: "list", ordered: true, start: Number.POSITIVE_INFINITY, items: [] },
    ],
    ["spans that are no list", paragraph(text("x") as unknown as unknown[])],
    ["a missing field", { type: "code", language: "rust" }],
    [
      "a class that is not text",
      { type: "container", tag: "div", presentation: { class: 1 }, blocks: [] },
    ],
    ["a null class", { type: "container", tag: "div", presentation: { class: null }, blocks: [] }],
    [
      "an attribute that is not text",
      { type: "container", tag: "div", presentation: { attributes: { width: 3 } }, blocks: [] },
    ],
    ["a null presentation", { type: "table", head: [], rows: [], presentation: null }],
    ["a row that is no list", { type: "table", head: [], rows: [{}] }],
    [
      "a viewer's pair of three",
      {
        type: "media",
        media: {
          kind: "viewer",
          src: "/a",
          label: "",
          viewer: { ...VIEWER, attributes: [["a", "b", "c"]] },
        },
      },
    ],
    [
      "a null list of views",
      { type: "media", media: { kind: "gallery", items: [], viewer: { ...VIEWER, views: null } } },
    ],
    [
      "a bad block deep in a list",
      { type: "list", ordered: false, start: 1, items: [[paragraph([{ type: 7 }])]] },
    ],
  ])("fail the whole read for %s", (_, block) => {
    expect(readBlocks([paragraph([text("fine")]), block])).toBeNull();
  });

  const card = {
    type: "card",
    slug: "lamp",
    mark: "pending",
    title: "Which?",
    body: [],
    verdict: { group: "decide", open: true },
    thread: [],
    comparison: [],
  };

  it("take a card without its source line, and refuse a state or a group it does not know", () => {
    expect(readBlocks([card])).toEqual([card]);
    expect(readBlocks([{ ...card, mark: "maybe" }])).toBeNull();
    expect(readBlocks([{ ...card, verdict: { group: "later", open: true } }])).toBeNull();
    expect(readBlocks([{ ...card, verdict: { group: "decide", open: "yes" } }])).toBeNull();
  });
});

describe("a span or media read alone", () => {
  it("is of a kind it knows, with its fields", () => {
    expect(isSpan({ type: "line-break" })).toBe(true);
    expect(isSpan({ type: "text" })).toBe(false);
    // A kind it does not know is a later kind, said where it stands.
    expect(isSpan({ type: "rule" })).toBe(true);
    expect(isSpan({ type: 4 })).toBe(false);
    expect(isMedia({ kind: "sound" })).toBe(true);
    expect(isMedia({ src: "/a.html" })).toBe(false);
    expect(isSpan(null)).toBe(false);
    expect(isMedia({ kind: "mockup", src: "/a.html", label: "" })).toBe(true);
    expect(isMedia({ kind: "mockup", src: "/a.html" })).toBe(false);
    expect(isMedia([])).toBe(false);
  });
});
