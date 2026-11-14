import { describe, expect, it } from "vitest";
import { boardItem } from "./cache";
import {
  Bodies,
  documentItem,
  FOLDER,
  type KeptBody,
  OUTLINE,
  readAhead,
  SECTIONS,
  split,
  validator,
  validators,
} from "./cache-document";
import type { Document, DocumentChange, DocumentOutline, Section } from "./wire";

/** A section `key` of `hash`, its body one paragraph saying `text` (none drawn when empty). */
function section(key: string, hash: string, text: string): Section {
  return {
    start: 1,
    end: 2,
    key,
    hash,
    part: null,
    blocks: text === "" ? [] : [{ type: "paragraph", spans: [{ type: "text", text }] }],
  };
}

type Named = readonly [string, string, string];

/** A document of `sections`, its card `palette` with a thread saying `said`. */
function documentOf(sections: readonly Named[], said: string): Document {
  return {
    version: 7,
    path: "docs/DECISIONS.md",
    title: "Decisions",
    waiting: [],
    crumbs: [],
    headings: [],
    sections: sections.map(([key, hash, text]) => section(key, hash, text)),
    cards: [
      { slug: "palette", state: "pending", text: "Palette, whole", group: "decide", open: true },
    ],
    threads: {
      palette: { blocks: [{ type: "paragraph", spans: [{ type: "text", text: said }] }] },
    },
    tasks: [],
  };
}

/** The server's outline of `document`: no bodies, its card's thread by the hash `thread`. */
function outlineOf(document: Document, thread: string): DocumentOutline {
  return {
    document: {
      ...document,
      sections: document.sections.map((one) => ({ ...one, blocks: [] })),
      threads: {},
    },
    anchors: {},
    thread_hashes: { palette: thread },
  };
}

const keys = (...named: string[]) => new Set(named);

const DOCUMENT: readonly Named[] = [
  ["intro", "i1", "Intro"],
  ["card:palette", "p1", "Palette"],
  ["lamps", "l1", "Lamps"],
];
const [INTRO, PALETTE, LAMPS] = DOCUMENT as [Named, Named, Named];

/** Every section of `DOCUMENT`, as a first visit reads it. */
const ALL = keys("intro", "card:palette", "lamps");

/** The bodies a first visit to `read` leaves taken, its card's thread by the hash `thread`. */
function firstVisit(read: Document, thread: string): Bodies {
  const bodies = new Bodies();
  bodies.answered(outlineOf(read, thread), keys());
  bodies.read(ALL, read.sections);
  return bodies;
}

const validated = (kept: Record<string, KeptBody>) =>
  Object.entries(kept).map(([key, one]) => [key, one.validator]);

describe("a document's item", () => {
  it("is named by its path, pinned for an entrypoint's own page", () => {
    expect(documentItem("docs/X.md", false)).toEqual({
      kind: "document",
      id: "document:docs/X.md",
      pinned: false,
    });
    expect(documentItem("@work~x/docs/", true)).toEqual({
      kind: "document",
      id: "document:@work~x/docs/",
      pinned: true,
    });
    expect(documentItem("x", false).id).not.toBe(boardItem("").id);
    expect(documentItem("board:", false).id).not.toBe(boardItem("").id);
    expect(new Set([OUTLINE, SECTIONS, FOLDER]).size).toBe(3);
  });
});

describe("a section's validator", () => {
  it("is its hash, a card's its hash and thread's, and none for a difference", () => {
    const threads = { palette: "t1" };
    expect(validator(section("intro", "i1", ""), threads)).toBe("i1");
    expect(validator(section("card:palette", "p1", ""), threads)).toBe("p1/t1");
    expect(validator(section("card:bare", "b1", ""), threads)).toBe("b1/");
    expect(validator(section("card:constructor", "c1", ""), {})).toBe("c1/");
    const differs = { ...section("changed", "c1", ""), comparison: { kind: "changed" as const } };
    expect(validator(differs, threads)).toBeNull();
    const outline = outlineOf(documentOf(DOCUMENT, "Said"), "t1");
    outline.document.sections.push(differs);
    // A section showing a difference is never kept.
    expect(validators(outline)).toEqual(
      new Map([
        ["intro", "i1"],
        ["card:palette", "p1/t1"],
        ["lamps", "l1"],
      ]),
    );
    const { thread_hashes: _, ...hashless } = outline;
    expect(validators(hashless).get("card:palette")).toBe("p1/");
  });
});

describe("bodies kept on this device", () => {
  it("are drawn when current by the outline kept with them", () => {
    const read = documentOf(DOCUMENT, "Said");
    const kept = firstVisit(read, "t1").kept(read);
    expect(Object.keys(kept)).toHaveLength(3);
    expect(kept["card:palette"]?.validator).toBe("p1/t1");
    expect(kept["card:palette"]?.body.card?.text).toBe("Palette, whole");
    expect(kept["card:palette"]?.body.thread).toBe(read.threads.palette);
    // A body kept under an older validator, or under another key than its section's, is not drawn.
    const stale = {
      ...kept,
      lamps: { ...(kept.lamps as KeptBody), validator: "l0" },
      other: kept.intro as KeptBody,
    };
    const device = new Bodies();
    const drawn = device.drawn(outlineOf(read, "t1"), stale);
    expect(drawn.map((body) => body.section.key)).toEqual(["card:palette", "intro"]);
    expect(device.confirmed()).toBe(false);
    expect(new Bodies().drawn(outlineOf(read, "t1"), {})).toEqual([]);
  });

  it("are kept when the server's outline names them alike, the rest read again", () => {
    const read = documentOf(DOCUMENT, "Said");
    const kept = firstVisit(read, "t1").kept(read);
    // Since: intro was edited, and the card's thread answered; lamps is as it was.
    const now = documentOf([["intro", "i2", "Intro, edited"], PALETTE, LAMPS], "Said, answered");
    const wanted = keys("intro", "card:palette");
    const device = (asked: Set<string>): [Bodies, Set<string>] => {
      const bodies = new Bodies();
      bodies.drawn(outlineOf(read, "t1"), kept);
      return [bodies, bodies.answered(outlineOf(now, "t2"), asked)];
    };
    const [waiting, current] = device(wanted);
    expect(current).toEqual(keys("lamps"));
    // Intro and the card are wanted, and wait for their reads.
    expect(waiting.confirmed()).toBe(false);
    // A body drawn from the device that the page no longer wants is not waited for.
    expect(device(keys())[0].confirmed()).toBe(true);

    // The two read again: taken under the server's validators, and every drawn one confirmed.
    const [again] = device(wanted);
    expect(again.read(wanted, now.sections.slice(0, 2))).toBe(true);
    expect(again.confirmed()).toBe(true);
    expect(validated(again.kept(now))).toEqual([
      ["intro", "i2"],
      ["card:palette", "p1/t2"],
      ["lamps", "l1"],
    ]);
    // The same bodies read again change nothing.
    expect(again.read(wanted, now.sections.slice(0, 2))).toBe(false);
    // A section asked for and not answered is gone: nothing of it is kept.
    expect(again.read(keys("lamps"), [])).toBe(true);
    expect(Object.keys(again.kept(now))).toHaveLength(2);
  });

  it("are not kept once the feed changed them, until the next outline names them", () => {
    const read = documentOf(DOCUMENT, "Said");
    const bodies = firstVisit(read, "t1");
    // A reply on the card's thread, and lamps edited.
    const delta: DocumentChange = {
      path: read.path,
      threads: { palette: null },
      sections: [section("lamps", "l2", "")],
    };
    expect(bodies.changed(delta)).toBe(true);
    // Only the body the feed left alone.
    expect(Object.keys(bodies.kept(read))).toEqual(["intro"]);
    // Lamps read again before the next outline: no validator names it, so it is not kept.
    const edited = documentOf([INTRO, PALETTE, ["lamps", "l2", "Lamps, edited"]], "Said");
    expect(bodies.read(keys("lamps"), edited.sections.slice(2))).toBe(false);
    expect(Object.keys(bodies.kept(edited))).toEqual(["intro"]);
    // A card changed by its card alone, or a section deleted, is as changed.
    const removed: DocumentChange = {
      path: "",
      cards: [{ key: "palette", removed: true }],
      deleted: ["intro"],
    };
    expect(bodies.changed(removed)).toBe(true);
    expect(bodies.kept(edited)).toEqual({});
    expect(bodies.changed(removed)).toBe(false);
    expect(bodies.changed({ path: "" })).toBe(false);
    // The next outline names them: read again, they are kept.
    bodies.answered(outlineOf(edited, "t1"), keys());
    expect(bodies.read(ALL, edited.sections)).toBe(true);
    expect(Object.keys(bodies.kept(edited))).toHaveLength(3);
  });

  it("are given to the device again only once one is taken or let go", () => {
    const read = documentOf(DOCUMENT, "Said");
    const first = firstVisit(read, "t1");
    expect(first.unkept()).toEqual(
      new Map([
        ["card:palette", "p1/t1"],
        ["intro", "i1"],
        ["lamps", "l1"],
      ]),
    );
    expect([...(firstVisit(read, "t1").unkept()?.keys() ?? [])]).toEqual([
      "card:palette",
      "intro",
      "lamps",
    ]);
    expect(first.unkept()).toBeNull();

    // The next visit draws them and the server's outline names them alike: nothing to give.
    const device = new Bodies();
    device.drawn(outlineOf(read, "t1"), first.kept(read));
    expect(device.unkept()).toBeNull();
    device.answered(outlineOf(read, "t1"), ALL);
    expect(device.unkept()).toBeNull();

    // A body the feed changed is not given: the next outline tells its copy out of date.
    device.changed({ path: read.path, sections: [section("lamps", "l2", "")] });
    expect(device.unkept()).toBeNull();

    // An outline naming another validator lets a body go; a read takes one.
    const edited = documentOf([["intro", "i2", "Intro, edited"], PALETTE, LAMPS], "Said");
    device.answered(outlineOf(edited, "t1"), ALL);
    expect([...(device.unkept()?.keys() ?? [])]).toEqual(["card:palette"]);
    device.read(keys("intro"), edited.sections.slice(0, 1));
    const given = device.unkept();
    expect([...(given?.keys() ?? [])]).toEqual(["card:palette", "intro"]);
    expect(given?.get("intro")).toBe("i2");
  });
});

/** The first part of `DOCUMENT` a page read ahead: the leading sections `named` whole. */
function firstPart(read: Document, named: string[], thread: string): DocumentOutline {
  const first = outlineOf(read, thread);
  first.document.sections = read.sections.filter((one) => named.includes(one.key));
  if (named.includes("card:palette")) {
    first.document.threads = read.threads;
  } else {
    first.thread_hashes = {};
    first.document.cards = [];
  }
  return first;
}

describe("a document read ahead", () => {
  it("draws as the bodies a visit would have kept", () => {
    const read = documentOf(DOCUMENT, "Said");
    const first = firstPart(read, ["intro", "card:palette"], "t1");
    expect(readAhead(first)).toBe(true);
    // An outline carries no body.
    expect(readAhead(outlineOf(read, "t1"))).toBe(false);
    const [outline, kept] = split(first);
    expect(readAhead(outline)).toBe(false);
    expect(first.document.sections[0]?.blocks).toHaveLength(1);
    expect(outline.document.sections.map((one) => one.key)).toEqual(["intro", "card:palette"]);
    const visit = firstVisit(read, "t1").kept(read);
    expect(kept).toEqual({ intro: visit.intro, "card:palette": visit["card:palette"] });

    // Drawn from the device as a visit's: current by the outline the part was split into.
    const device = new Bodies();
    expect(device.drawn(outline, kept)).toHaveLength(2);
    // Drawn as the device keeps a visit's, the bodies are kept already.
    device.answered(outlineOf(read, "t1"), keys());
    expect(device.unkept()).toBeNull();

    // Drawn from a part read ahead, they are the device's to keep once the visit's outline
    // answers: those still current, under their validators, and not one it no longer names.
    const ahead = new Bodies();
    ahead.drawn(outline, kept);
    ahead.drawnAhead();
    const edited = outlineOf(read, "t1");
    edited.document.sections = edited.document.sections.map((one) =>
      one.key === "intro" ? { ...one, hash: "i2" } : one,
    );
    ahead.answered(edited, keys());
    expect(ahead.unkept()).toEqual(new Map([["card:palette", "p1/t1"]]));
    expect(ahead.unkept()).toBeNull();

    // A part with no card and no body of any section is no part read ahead.
    const empty = firstPart(documentOf([["intro", "i1", ""]], "Said"), ["intro"], "t1");
    expect(readAhead(empty)).toBe(false);
    // A part of a document showing a difference keeps none of its body.
    const differs = firstPart(read, ["intro"], "t1");
    differs.document.sections = differs.document.sections.map((one) => ({
      ...one,
      comparison: { kind: "added" },
    }));
    expect(split(differs)[1]).toEqual({});
  });
});
