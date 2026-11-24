import { describe, expect, it } from "vitest";
import {
  heardEntry,
  readBrief,
  readEntry,
  readHistory,
  readMessage,
  readOffer,
  readSessionGo,
  readTaskPage,
} from "./task-read";
import { aTask } from "./testing";
import { NO_WORK } from "./work";

const MESSAGE = {
  id: 7,
  slug: "w/a",
  sender: "owner",
  recipient: null,
  kind: "message",
  text: "Said",
  ts: 1.5,
  person: false,
};
const PARAGRAPH = { type: "paragraph", spans: [{ type: "text", text: "Said" }] };
const ENTRY = { message: MESSAGE, stamp: "Nov 11 10:00", blocks: [PARAGRAPH] };
/** A page as an older server sends it: none of the fields serde defaults. */
const BARE_PAGE = { task: aTask("w/a"), blocks: [], children: [], children_count: 0 };

describe("a task's message", () => {
  it("is read with its recipient, none absent or null", () => {
    expect(readMessage(MESSAGE)).toEqual(MESSAGE);
    expect(readMessage({ ...MESSAGE, recipient: "amber" })?.recipient).toBe("amber");
    const { recipient: _, ...unaddressed } = MESSAGE;
    expect(readMessage(unaddressed)?.recipient).toBeNull();
  });

  it("is a person's only when it says so: an older server's names nobody", () => {
    expect(readMessage({ ...MESSAGE, sender: "ana", person: true })?.person).toBe(true);
    const { person: _, ...older } = MESSAGE;
    expect(readMessage(older)?.person).toBe(false);
    expect(readMessage({ ...MESSAGE, person: "yes" })?.person).toBe(false);
  });

  it("is none without a whole id, its words or its time", () => {
    for (const broken of [
      { ...MESSAGE, id: 1.5 },
      { ...MESSAGE, id: "7" },
      { ...MESSAGE, sender: 1 },
      { ...MESSAGE, kind: null },
      { ...MESSAGE, text: undefined },
      { ...MESSAGE, ts: "1" },
      { ...MESSAGE, recipient: 3 },
      null,
      "message",
    ]) {
      expect(readMessage(broken)).toBeNull();
    }
  });
});

describe("a thread's entry", () => {
  it("is its message, stamp and blocks", () => {
    expect(readEntry(ENTRY)).toEqual(ENTRY);
  });

  it("takes a block of a kind a newer server sends, which the thread says in its place", () => {
    const later = { ...ENTRY, blocks: [{ type: "marquee", text: "Scrolling" }] };
    expect(readEntry(later)).toEqual(later);
  });

  it("is none when one of them is not", () => {
    expect(readEntry({ ...ENTRY, stamp: 1 })).toBeNull();
    expect(readEntry({ ...ENTRY, blocks: [{ type: 0 }] })).toBeNull();
    expect(readEntry({ ...ENTRY, blocks: [{ type: "code" }] })).toBeNull();
    expect(readEntry({ ...ENTRY, message: {} })).toBeNull();
  });
});

describe("a thread's page", () => {
  it("is its entries and cursors, none absent or null", () => {
    expect(readHistory({ entries: [ENTRY], before: 3, after: null })).toEqual({
      entries: [ENTRY],
      before: 3,
      after: null,
    });
    expect(readHistory({ entries: [] })).toEqual({ entries: [], before: null, after: null });
  });

  it("is none with an entry it cannot read or a cursor that is no whole number", () => {
    expect(readHistory({ entries: [ENTRY, {}] })).toBeNull();
    expect(readHistory({ entries: "none" })).toBeNull();
    expect(readHistory({ entries: [], before: 1.5 })).toBeNull();
    expect(readHistory({ entries: [], after: "2" })).toBeNull();
  });
});

describe("a message the feed brings", () => {
  it("is its record with the thread's stamp and blocks", () => {
    expect(heardEntry({ ...MESSAGE, stamp: "now", blocks: [PARAGRAPH] })).toEqual({
      message: MESSAGE,
      stamp: "now",
      blocks: [PARAGRAPH],
    });
  });

  it("has no blocks and an empty stamp when the feed sends none", () => {
    expect(heardEntry(MESSAGE)).toEqual({ message: MESSAGE, stamp: "", blocks: [] });
    expect(heardEntry({ ...MESSAGE, stamp: 5 })?.stamp).toBe("");
  });

  it("is none when it is no message or its blocks are not blocks", () => {
    expect(heardEntry({})).toBeNull();
    expect(heardEntry({ ...MESSAGE, blocks: "text" })).toBeNull();
    expect(heardEntry({ ...MESSAGE, blocks: null })).toBeNull();
  });
});

describe("a session's way in", () => {
  it("names whose session, and its address and pane when known", () => {
    expect(readSessionGo({ handle: "amber", url: "https://h/x", pane: "t7" })).toEqual({
      handle: "amber",
      url: "https://h/x",
      pane: "t7",
    });
    expect(readSessionGo({ handle: "amber" })).toEqual({ handle: "amber", url: null, pane: null });
  });

  it("is none without a handle, or with an address or pane that is no text", () => {
    expect(readSessionGo({ url: "https://h/x" })).toBeNull();
    expect(readSessionGo({ handle: "amber", url: 1 })).toBeNull();
    expect(readSessionGo({ handle: "amber", pane: false })).toBeNull();
  });
});

describe("a task's page", () => {
  it("defaults what an older server leaves out, as serde does", () => {
    expect(readTaskPage(BARE_PAGE)).toEqual({
      machine_work: NO_WORK,
      task: { ...aTask("w/a") },
      blocks: [],
      children: [],
      children_count: 0,
      brief_omitted: 0,
      brief_next: 0,
      brief_revision: "",
      sessions: {},
      local: false,
      gates: null,
    });
  });

  it("keeps what the server sends", () => {
    const page = readTaskPage({
      ...BARE_PAGE,
      blocks: [PARAGRAPH],
      children: [aTask("w/a/b", { parent: "w/a" })],
      children_count: 3,
      brief_omitted: 9,
      brief_next: 24,
      brief_revision: "r1",
      sessions: { amber: { handle: "amber", url: null, pane: "t1" } },
      local: true,
      gates: { count: 2, waiting: 1, unsigned: 1 },
    });
    expect(page?.children.map((child) => child.slug)).toEqual(["w/a/b"]);
    expect(page?.blocks).toEqual([PARAGRAPH]);
    expect([page?.children_count, page?.brief_omitted, page?.brief_next]).toEqual([3, 9, 24]);
    expect(page?.brief_revision).toBe("r1");
    expect(page?.sessions).toEqual({ amber: { handle: "amber", url: null, pane: "t1" } });
    expect(page?.local).toBe(true);
    expect(page?.gates).toEqual({ count: 2, waiting: 1, unsigned: 1 });
  });

  it("keeps whose the task is where the server says it, and none where it is null", () => {
    const owner = { name: "design", kind: "group", by: "named", said: "named on it" };
    expect(readTaskPage({ ...BARE_PAGE, owner })?.owner).toEqual({ ...owner, hand_to: [] });
    expect(readTaskPage({ ...BARE_PAGE, owner: null })).not.toHaveProperty("owner");
    expect(readTaskPage({ ...BARE_PAGE, owner: { name: "design" } })).toBeNull();
  });

  it("reads gates sent as null as none", () => {
    expect(readTaskPage({ ...BARE_PAGE, gates: null })?.gates).toBeNull();
  });

  it("is none when a part it draws cannot be read", () => {
    for (const broken of [
      { ...BARE_PAGE, task: {} },
      { ...BARE_PAGE, blocks: [{}] },
      { ...BARE_PAGE, children: [{}] },
      { ...BARE_PAGE, children: null },
      { ...BARE_PAGE, children_count: -1 },
      { ...BARE_PAGE, machine_work: { machines: "none" } },
      { ...BARE_PAGE, sessions: [] },
      { ...BARE_PAGE, sessions: null },
      { ...BARE_PAGE, sessions: { amber: {} } },
      { ...BARE_PAGE, gates: { waiting: 1 } },
      { ...BARE_PAGE, brief_omitted: "9" },
      { ...BARE_PAGE, brief_next: -2 },
      { ...BARE_PAGE, brief_revision: 1 },
      { ...BARE_PAGE, local: "yes" },
      null,
    ]) {
      expect(readTaskPage(broken)).toBeNull();
    }
  });
});

describe("a chunk of a long brief", () => {
  it("is its blocks, where the next starts, what is left and its revision", () => {
    expect(readBrief({ blocks: [PARAGRAPH], next: 5, omitted: 2, revision: "r" })).toEqual({
      blocks: [PARAGRAPH],
      next: 5,
      omitted: 2,
      revision: "r",
    });
    expect(readBrief({ blocks: [], next: 0, omitted: 0 })?.revision).toBe("");
  });

  it("is none without its counts or with blocks it cannot read", () => {
    expect(readBrief({ blocks: [], next: 0 })).toBeNull();
    expect(readBrief({ blocks: [{}], next: 0, omitted: 0 })).toBeNull();
    expect(readBrief({ blocks: [], next: 0, omitted: 0, revision: 3 })).toBeNull();
  });
});

describe("the landing a task offers", () => {
  it("is its action, label, branch and payload, none absent", () => {
    const offer = { action: "land", label: "Land it", branch: "w/a", payload: "{}" };
    expect(readOffer(offer)).toEqual(offer);
    expect(readOffer({ ...offer, payload: undefined })?.payload).toBeNull();
  });

  it("is none without its words or with a payload that is no text", () => {
    expect(readOffer({ action: "land", label: "Land it" })).toBeNull();
    expect(readOffer({ action: "land", label: "x", branch: "b", payload: {} })).toBeNull();
  });
});
