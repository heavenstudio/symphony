import { describe, expect, it } from "vitest";
import {
  readAsks,
  readChange,
  readDiff,
  readFolder,
  readKeptBodies,
  readOutline,
  readSections,
  readVersions,
} from "./document-read";
import { CASES, type DocumentSample } from "./testing";

/** A sample as `JSON.parse` gives it, a value of its own each time. */
const sample = (name: DocumentSample) => JSON.parse(JSON.stringify(CASES.samples[name]));

/** `value` with `path` (keys and list indices) set to `to`, or removed for undefined. */
function edited(value: unknown, path: (string | number)[], to: unknown): unknown {
  const copy = JSON.parse(JSON.stringify(value));
  let at = copy;
  for (const key of path.slice(0, -1)) at = at[key];
  const last = path.at(-1) as string | number;
  if (to === undefined) delete at[last];
  else at[last] = to;
  return copy;
}

describe("a document's reads", () => {
  it("are every field crates/symphony-api writes, and then those it always sends", () => {
    const reads: [DocumentSample, (value: unknown) => unknown][] = [
      ["outline", readOutline],
      ["bareOutline", readOutline],
      ["sections", readSections],
      ["change", readChange],
      ["bareChange", readChange],
      ["folder", readFolder],
      ["bareFolder", readFolder],
      ["diff", readDiff],
    ];
    for (const [name, read] of reads) {
      expect(read(sample(name)), name).toEqual(CASES.samples[name]);
    }
    expect(readVersions(sample("versions"))).toEqual(sample("versions").versions);
  });

  it("take what a newer server adds, and what the page does not draw as it comes", () => {
    const outline = edited(sample("outline"), ["document", "lamps"], 3);
    expect(readOutline(outline)).toEqual(outline);
    const ask = edited(sample("asks"), ["asks", 0, "answers"], "none");
    expect(readAsks(ask)?.asks).toHaveLength(1);
  });

  it.each([
    ["no document", ["document"], undefined],
    ["a version past a safe integer", ["document", "version"], 2 ** 53],
    ["a version of a fraction", ["document", "version"], 1.5],
    ["a title that is no text", ["document", "title"], 1],
    ["a waiting list of numbers", ["document", "waiting"], [1]],
    ["a crumb's link that is no text", ["document", "crumbs", 0, "href"], 1],
    ["a heading's level past a byte", ["document", "headings", 0, "level"], 256],
    ["a section's start below none", ["document", "sections", 0, "start"], -1],
    ["a section's end past 32 bits", ["document", "sections", 0, "end"], 2 ** 32],
    ["a section's part of a fraction", ["document", "sections", 0, "part"], 0.5],
    ["a section with no blocks", ["document", "sections", 0, "blocks"], undefined],
    ["a block of no kind", ["document", "sections", 0, "blocks", 0], { text: "marquee" }],
    ["a difference it does not know", ["document", "sections", 0, "comparison", "kind"], "moved"],
    ["a difference's before of no blocks", ["document", "sections", 0, "comparison", "before"], 1],
    ["a section's ends of one block", ["document", "sections", 0, "ends"], ["paragraph"]],
    ["a section's ends of three", ["document", "sections", 0, "ends"], ["list", "code", "list"]],
    ["a section's ends that are no texts", ["document", "sections", 0, "ends"], [1, 2]],
    ["a section's ends that are no list", ["document", "sections", 0, "ends"], "paragraph"],
    ["a card that is not open or shut", ["document", "cards", 0, "open"], "yes"],
    ["a thread that is no fragment", ["document", "threads", "lamp-colour"], null],
    ["a task with no status", ["document", "tasks", 0, "status"], undefined],
    ["a comparison with no task", ["document", "comparison"], {}],
    ["anchors that are no lists", ["anchors", "colour"], "colour"],
    ["a thread's hash that is no text", ["thread_hashes", "lamp-colour"], 1],
  ])("refuse an outline with %s", (_, path, to) => {
    expect(readOutline(edited(sample("outline"), path, to))).toBeNull();
  });

  it("take an outline whose optional fields are null", () => {
    const outline = edited(sample("outline"), ["document", "comparison"], null);
    expect(readOutline(edited(outline, ["document", "sections", 0, "comparison"], null))).not.toBe(
      null,
    );
    expect(readOutline(edited(outline, ["document", "crumbs", 0, "href"], null))).not.toBeNull();
    // A section's ends, null or absent as in an outline kept from before the server named them.
    expect(readOutline(edited(outline, ["document", "sections", 0, "ends"], null))).not.toBeNull();
    const kept = edited(outline, ["document", "sections", 0, "ends"], undefined);
    expect(readOutline(kept)).toEqual(kept);
  });

  it.each([
    ["no path", ["path"], undefined],
    ["a removal that is no flag", ["removed"], "yes"],
    ["deleted sections that are no names", ["deleted", 0], 1],
    ["a position past 32 bits", ["positions", 0, "start"], 2 ** 32],
    ["a position's part of text", ["positions", 0, "part"], "1"],
    ["an order of numbers", ["order", 0], 0],
    ["a thread that is neither a fragment nor null", ["threads", "lamp-size"], 1],
    ["a title that is no text", ["title"], 1],
    ["a card change with no key", ["cards", 0, "key"], undefined],
    ["a card change's card with no slug", ["cards", 0, "value", "slug"], undefined],
    ["a card removed that is no flag", ["cards", 1, "removed"], 1],
    ["tasks that are no list", ["tasks"], {}],
    ["a comparison that is no list", ["comparison"], { task: "x" }],
    ["sections that are no list", ["sections"], {}],
  ])("refuse a change with %s", (_, path, to) => {
    expect(readChange(edited(sample("change"), path, to))).toBeNull();
    expect(readSections({ version: 1, change: edited(sample("change"), path, to) })).toBeNull();
  });

  it("refuse sections with no version, or of a change it cannot read", () => {
    expect(readSections(edited(sample("sections"), ["version"], -1))).toBeNull();
    expect(readSections(edited(sample("sections"), ["change"], null))).toBeNull();
    expect(readSections(null)).toBeNull();
  });

  it.each([
    ["no title", ["title"], undefined],
    ["a count of a fraction", ["docs"], 1.5],
    ["an intro it cannot read", ["intro", "version"], "12"],
    ["an entry whose waiting count is below none", ["entries", 0, "waiting"], -1],
    ["an entry with no summary", ["entries", 1, "summary"], undefined],
    ["entries that are no list", ["entries"], null],
  ])("refuse a folder with %s", (_, path, to) => {
    expect(readFolder(edited(sample("folder"), path, to))).toBeNull();
  });

  it("refuse versions or a diff missing a field drawn", () => {
    expect(readVersions(edited(sample("versions"), ["versions", 0, "date"], undefined))).toBeNull();
    expect(readVersions({})).toBeNull();
    expect(readVersions([])).toBeNull();
    expect(readDiff(edited(sample("diff"), ["diff"], null))).toBeNull();
  });
});

describe("the asks' read", () => {
  it("is the asks, the chat's recipient, which is a name or none, and whether one may ask", () => {
    const asks = sample("asks");
    expect(readAsks(asks)).toEqual({ asks: asks.asks, recipient: "amber-heron", mayAsk: true });
    expect(readAsks(sample("noAsks"))).toEqual({ asks: [], recipient: null, mayAsk: false });
    // A server that does not say who may ask drew Ask for everyone, as it still is.
    expect(readAsks({ asks: [] })).toEqual({ asks: [], recipient: null, mayAsk: true });
    expect(readAsks({ asks: [], recipient: 7 })).toEqual({
      asks: [],
      recipient: null,
      mayAsk: true,
    });
    expect(readAsks(null)).toBeNull();
    expect(readAsks([[]])).toBeNull();
  });

  it.each([
    ["no asks", ["asks"], undefined],
    ["an id of a fraction", ["asks", 0, "id"], 1.5],
    ["an id past a safe integer", ["asks", 0, "id"], 2 ** 53],
    ["a quote that is no text", ["asks", 0, "quote"], null],
    ["a message whose seq is text", ["asks", 0, "messages", 0, "seq"], "1"],
    ["a message's blocks it cannot read", ["asks", 0, "messages", 0, "blocks"], [{ type: 7 }]],
    ["a message with no author", ["asks", 0, "messages", 1, "by"], undefined],
    ["a may_ask that is no flag", ["may_ask"], "yes"],
    ["a may_ask of null", ["may_ask"], null],
  ])("refuses %s", (_, path, to) => {
    expect(readAsks(edited(sample("asks"), path, to))).toBeNull();
  });

  it("takes a negative id, as an i64 is", () => {
    expect(readAsks(edited(sample("asks"), ["asks", 0, "id"], -3))?.asks[0]?.id).toBe(-3);
  });
});

describe("the bodies this device kept", () => {
  const change = CASES.samples.change as { sections: unknown[]; cards: { value: unknown }[] };
  const kept = {
    colour: { validator: "h1", body: { section: change.sections[0] } },
    "card:lamp-colour": {
      validator: "h1/t1",
      body: {
        section: change.sections[0],
        card: change.cards[0]?.value,
        thread: { blocks: [] },
      },
    },
  };

  it("are each a body under its validator, by key", () => {
    expect(readKeptBodies(kept)).toEqual(kept);
    expect(readKeptBodies({})).toEqual({});
    const bare = edited(kept, ["card:lamp-colour", "body", "card"], null);
    expect(readKeptBodies(bare)).toEqual(bare);
  });

  it.each([
    ["no validator", ["colour", "validator"], undefined],
    ["no body", ["colour", "body"], undefined],
    ["a section it cannot read", ["colour", "body", "section", "key"], 1],
    ["a card it cannot read", ["card:lamp-colour", "body", "card", "open"], 1],
    ["a thread it cannot read", ["card:lamp-colour", "body", "thread", "blocks"], 1],
  ])("are none with %s", (_, path, to) => {
    expect(readKeptBodies(edited(kept, path, to))).toBeNull();
  });

  it("are none for what is no map", () => {
    expect(readKeptBodies([])).toBeNull();
    expect(readKeptBodies("bodies")).toBeNull();
  });
});
