import { afterEach, describe, expect, it, vi } from "vitest";
import { waits } from "./cache";
import { documentItem, FOLDER as FOLDER_PART, OUTLINE, SECTIONS } from "./cache-document";
import { documentState } from "./document";
import {
  api,
  busy,
  type Copy,
  DocumentLive,
  documentLive,
  FolderLive,
  followFolder,
  fromDevice,
  keysQuery,
  loadSections,
  named,
  outlined,
  readOutlineAt,
  sentBodies,
  took,
  vanished,
} from "./document-live";
import { fakeAnswer as answered, CASES, feeds as pageFeeds, reply, say, until } from "./testing";
import { UNREACHABLE } from "./trouble";
import type { Document, DocumentCard, DocumentOutline, Folder, Section } from "./wire";
import { CACHE_FORMAT, FEED_EVENT } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
});

const PATH = "docs/DECISIONS.md";

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

const PALETTE: DocumentCard = {
  slug: "palette",
  state: "pending",
  text: "Palette, whole",
  group: "decide",
  open: true,
};

/** The document at `PATH` with the bodies of `sections`, its card's thread saying `said`. */
function documentOf(sections: Section[], said = "Agreed", path = PATH): Document {
  return {
    version: 7,
    path,
    title: "Decisions",
    waiting: [],
    crumbs: [],
    headings: [],
    sections,
    cards: [PALETTE],
    threads: {
      palette: { blocks: [{ type: "paragraph", spans: [{ type: "text", text: said }] }] },
    },
    tasks: [],
  };
}

const SECTIONS_NOW = [
  section("intro", "i1", "Intro"),
  section("card:palette", "p1", "Palette"),
  section("lamps", "l1", "Lamps"),
];

/** The server's outline of `document`: no bodies, its card's thread by the hash `t1`. */
function outlineOf(document: Document): DocumentOutline {
  return {
    document: {
      ...document,
      sections: document.sections.map((one) => ({ ...one, blocks: [] })),
      threads: {},
    },
    anchors: { intro: ["intro"] },
    thread_hashes: { palette: "t1" },
  };
}

const OUTLINE_NOW = outlineOf(documentOf(SECTIONS_NOW));

/** The frame a page tells what it heard. */
const frameOf = () => ({ refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() });

/** The shell's store of what this device keeps: `kept` by `ID PART`, and each call it was given. */
function deviceOf(kept: Record<string, unknown> = {}) {
  const store = {
    read: vi.fn(
      async (_format: number, _project: string, id: string, part: string) =>
        kept[`${id} ${part}`] ?? null,
    ),
    keep: vi.fn(async () => {}),
    visit: vi.fn(async () => {}),
    forget: vi.fn(async () => {}),
  };
  vi.stubGlobal("symphonyCache", store);
  return store;
}

/** A page with a document whose elements no section holds, and no field focused. */
function pageWithoutWork() {
  vi.stubGlobal(
    "document",
    Object.assign(new EventTarget(), {
      getElementById: () => null,
      querySelector: () => null,
      querySelectorAll: () => [],
      activeElement: null,
    }),
  );
  vi.stubGlobal("window", new EventTarget());
}

/**
 * One request the stubbed server holds: its address, its headers, how to answer it, and the signal
 * that gives it up.
 */
interface Held {
  url: string;
  headers: Record<string, string>;
  answer: (response: Response) => void;
  signal: AbortSignal | null;
}

/**
 * The server, its answers held until a test gives them, on a page with nothing engaged. As the
 * browser's fetch, a request refuses with its signal's reason once that ends before its answer.
 */
function serverOf(): Held[] {
  const asked: Held[] = [];
  pageWithoutWork();
  vi.stubGlobal(
    "fetch",
    (url: string, init?: RequestInit) =>
      new Promise<Response>((answer, refuse) => {
        const headers = (init?.headers ?? {}) as Record<string, string>;
        const signal = init?.signal ?? null;
        signal?.addEventListener("abort", () => refuse(signal.reason), { once: true });
        asked.push({ url, headers, answer, signal });
      }),
  );
  return asked;
}

/** Until `asked` holds `count`, the page's waits running meanwhile. */
const askedFor = (asked: unknown[], count: number) =>
  until(() => expect(asked).toHaveLength(count));

describe("a document's address", () => {
  it("names a checkout's document under its scope", () => {
    expect(named("", PATH)).toBe(PATH);
    expect(named("design~policy", PATH)).toBe(`@design~policy/${PATH}`);
  });

  it("is an API route with the path as its query", () => {
    expect(api("", "/api/document", "docs/a b&c.md")).toBe(
      "/api/document?path=docs%2Fa%20b%26c.md",
    );
    expect(api("design~policy", "/api/section", PATH)).toBe(
      "/@design~policy/api/section?path=docs%2FDECISIONS.md",
    );
  });

  it("names section keys as a JSON list, in order", () => {
    expect(decodeURIComponent(keysQuery(new Set(["lamps", "card:palette", "intro"])))).toBe(
      '["card:palette","intro","lamps"]',
    );
    expect(keysQuery(new Set())).toBe("%5B%5D");
  });
});

describe("the bodies as the server sent them", () => {
  it("are the drawn sections, a card's with its card and thread", () => {
    const sent = sentBodies(
      [section("intro", "i1", "Intro"), section("card:palette", "p1", "Palette")],
      new Map([["palette", PALETTE]]),
      { palette: { blocks: [] } },
    );
    expect([...sent.keys()]).toEqual(["intro", "card:palette"]);
    expect(sent.get("intro")).toEqual({ section: section("intro", "i1", "Intro") });
    expect(sent.get("card:palette")).toEqual({
      section: section("card:palette", "p1", "Palette"),
      card: PALETTE,
      thread: { blocks: [] },
    });
  });

  it("leave out a section without its blocks, and a card or thread the read did not bring", () => {
    const sent = sentBodies(
      [section("lamps", "l1", ""), section("card:palette", "p1", "Palette")],
      new Map(),
      { palette: null },
    );
    expect([...sent.keys()]).toEqual(["card:palette"]);
    expect(sent.get("card:palette")).toEqual({ section: section("card:palette", "p1", "Palette") });
    const inherited = sentBodies([section("card:toString", "c1", "Card")], new Map(), {});
    expect(inherited.get("card:toString")).toEqual({
      section: section("card:toString", "c1", "Card"),
    });
  });
});

/** An element in the section `key` (none outside every section). */
const inSection = (key: string | null, matches = false) => ({
  closest: () => (key === null ? null : { getAttribute: () => key }),
  matches: () => matches,
});

describe("the sections the owner is engaged in", () => {
  it("are none where there is no page", () => {
    expect(busy()).toEqual([new Set(), new Set()]);
  });

  it("hold an open form, details or focused field; a closed draft apart", () => {
    vi.stubGlobal("document", {
      querySelectorAll: (selector: string) =>
        selector === "[data-draft=true]"
          ? [inSection("lamps")]
          : [inSection("intro"), inSection(null), inSection("intro")],
      activeElement: inSection("card:palette", true),
    });
    expect(busy()).toEqual([new Set(["intro", "card:palette"]), new Set(["lamps"])]);
  });

  it("leave out a focus on no field, or outside every section", () => {
    for (const activeElement of [inSection("card:palette"), inSection(null, true), null]) {
      vi.stubGlobal("document", { querySelectorAll: () => [], activeElement });
      expect(busy()).toEqual([new Set(), new Set()]);
    }
  });
});

describe("the outline read naming its copy's tag", () => {
  const URL = "/api/document?path=docs%2FDECISIONS.md&view=outline";
  const item = documentItem(PATH, false);
  /** The signal of a page that stays. */
  const STAYS = new AbortController().signal;

  it("is the server's outline, kept with its tag for the next visit", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const copy = { current: null };
    const read = readOutlineAt(URL, copy, item, STAYS);
    await askedFor(asked, 1);
    expect(asked[0]?.headers).toEqual({});
    asked[0]?.answer(answered(200, OUTLINE_NOW, '"o1"', 7));
    await expect(read).resolves.toEqual(OUTLINE_NOW);
    expect(copy.current).toEqual({ tag: '"o1"', outline: OUTLINE_NOW });
    expect(store.keep).toHaveBeenCalledWith(
      CACHE_FORMAT,
      "",
      `document:${PATH}`,
      "document",
      OUTLINE,
      '"o1"',
      OUTLINE_NOW,
      undefined,
      false,
    );
  });

  it("keeps nothing answered with no tag, or for a page that keeps nothing", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const untagged = readOutlineAt(URL, { current: null }, item, STAYS);
    await askedFor(asked, 1);
    asked[0]?.answer(answered(200, OUTLINE_NOW));
    await untagged;
    const unkept = readOutlineAt(URL, { current: null }, null, STAYS);
    await askedFor(asked, 2);
    asked[1]?.answer(answered(200, OUTLINE_NOW, '"o1"'));
    await unkept;
    expect(store.keep).not.toHaveBeenCalled();
  });

  it("is the copy again on a 304, at the feed's cursor the answer names", async () => {
    const asked = serverOf();
    deviceOf();
    const copy = { current: { tag: '"o1"', outline: OUTLINE_NOW } };
    const read = readOutlineAt(URL, copy, item, STAYS);
    await askedFor(asked, 1);
    expect(asked[0]?.headers).toEqual({ "If-None-Match": '"o1"' });
    asked[0]?.answer(answered(304, null, "", 15));
    const outline = await read;
    expect(outline?.document.version).toBe(15);
    expect({ ...outline, document: { ...outline?.document, version: 7 } }).toEqual(OUTLINE_NOW);
    expect(copy.current?.outline.document.version).toBe(7);
  });

  it("drops the copy on a 304 that names no cursor, and reads the outline whole", async () => {
    const asked = serverOf();
    deviceOf();
    const copy = {
      current: { tag: '"o1"', outline: OUTLINE_NOW } as {
        tag: string;
        outline: DocumentOutline;
      } | null,
    };
    const read = readOutlineAt(URL, copy, item, STAYS);
    await askedFor(asked, 1);
    asked[0]?.answer(answered(304, null));
    await askedFor(asked, 2);
    expect(asked[1]?.headers).toEqual({});
    asked[1]?.answer(answered(200, OUTLINE_NOW, '"o2"', 8));
    await expect(read).resolves.toEqual(OUTLINE_NOW);
    expect(copy.current?.tag).toBe('"o2"');
  });

  it("is none when the server has no such document, and the copy goes", async () => {
    const asked = serverOf();
    deviceOf();
    const copy = {
      current: { tag: '"o1"', outline: OUTLINE_NOW } as {
        tag: string;
        outline: DocumentOutline;
      } | null,
    };
    const read = readOutlineAt(URL, copy, item, STAYS);
    await askedFor(asked, 1);
    asked[0]?.answer(reply(404, { error: "no such document" }));
    await expect(read).resolves.toBeNull();
    expect(copy.current).toBeNull();
  });

  it("fails on a 304 to a plain read, and on an outline it cannot read", async () => {
    const asked = serverOf();
    deviceOf();
    const plain = readOutlineAt(URL, { current: null }, item, STAYS);
    await askedFor(asked, 1);
    asked[0]?.answer(answered(304, null, "", 7));
    await expect(plain).rejects.toThrow("The server answered 304 to a plain read");
    const unreadable = readOutlineAt(URL, { current: null }, item, STAYS);
    await askedFor(asked, 2);
    asked[1]?.answer(answered(200, { document: "none" }, '"o1"'));
    await expect(unreadable).rejects.toThrow("The server's outline could not be read");
  });

  /** A copy tagged `"o1"`, as the device or an earlier read left it. */
  const held = () => ({
    current: { tag: '"o1"', outline: OUTLINE_NOW } as Copy | null,
  });

  it("fails, reading and changing nothing more, once its page left before its answer", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const copy = held();
    const left = new AbortController();
    const read = readOutlineAt(URL, copy, item, left.signal);
    await askedFor(asked, 1);
    expect(asked[0]?.signal?.aborted).toBe(false);
    left.abort();
    // Given up on the wire as well, so the fetch refuses, as the browser's does.
    expect(asked[0]?.signal?.aborted).toBe(true);
    await expect(read).rejects.toMatchObject({ kind: "uncertain", text: UNREACHABLE });
    // The read is over: no second request can follow it.
    expect(asked).toHaveLength(1);
    expect(copy.current?.tag).toBe('"o1"');
    expect(store.keep).not.toHaveBeenCalled();
  });

  it("is none, reading nothing again, on a 304 naming no cursor its page left as it came", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const copy = held();
    const left = new AbortController();
    const read = readOutlineAt(URL, copy, item, left.signal);
    await askedFor(asked, 1);
    // The answer is in, but the page leaves before the read takes it up.
    asked[0]?.answer(answered(304, null));
    left.abort();
    await expect(read).resolves.toBeNull();
    expect(asked).toHaveLength(1);
    expect(copy.current?.tag).toBe('"o1"');
    expect(store.keep).not.toHaveBeenCalled();
  });

  it("keeps and changes nothing once its page left as the server answered, whatever it answered", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const answers = [
      answered(200, OUTLINE_NOW, '"o2"', 8),
      reply(404, { error: "no such document" }),
      answered(304, null, "", 15),
      answered(200, { document: "none" }, '"o2"'),
    ];
    for (const [at, answer] of answers.entries()) {
      const copy = held();
      const left = new AbortController();
      const read = readOutlineAt(URL, copy, item, left.signal);
      await askedFor(asked, at + 1);
      asked[at]?.answer(answer);
      left.abort();
      await expect(read).resolves.toBeNull();
      expect(copy.current).toEqual({ tag: '"o1"', outline: OUTLINE_NOW });
    }
    expect(asked).toHaveLength(answers.length);
    expect(store.keep).not.toHaveBeenCalled();
  });
});

/** A page of the document at `PATH`, kept as its own item unless `kept` says otherwise. */
const pageOf = (kept = documentItem(PATH, false), scope = "") =>
  new DocumentLive(PATH, scope, kept, frameOf());

describe("a document page's model", () => {
  it("names its document under its checkout's scope", () => {
    expect(pageOf().name).toBe(PATH);
    expect(pageOf(documentItem(PATH, false), "design~policy").name).toBe(`@design~policy/${PATH}`);
  });

  it("wants the sections near the screen, and is ready once each has its body", () => {
    const page = pageOf();
    const told = vi.fn();
    const stop = page.subscribe(told);
    expect(page.ready()).toBe(false);
    page.want("intro");
    page.want("intro");
    page.want("lamps");
    expect(told).toHaveBeenCalledTimes(2);
    expect(page.snapshot()).toBe(2);
    expect(page.loading()).toEqual(new Set(["intro", "lamps"]));
    page.loaded = new Set(["intro"]);
    expect(page.ready()).toBe(false);
    page.loaded = new Set(["intro", "lamps"]);
    expect(page.loading()).toEqual(new Set());
    expect(page.ready()).toBe(true);
    stop();
    page.changed();
    expect(told).toHaveBeenCalledTimes(2);
  });

  it("has its feed name the sections it wants and those whose bodies it holds, once each", () => {
    const page = pageOf();
    expect(page.followed()).toEqual(new Set());
    page.loaded = new Set(["intro", "card:palette"]);
    expect(page.followed()).toEqual(new Set(["intro", "card:palette"]));
    page.want("intro");
    page.want("lamps");
    expect(page.followed()).toEqual(new Set(["intro", "card:palette", "lamps"]));
    // A copy, which the page's later wants leave as it was.
    const named = page.followed();
    page.want("later");
    expect(named.has("later")).toBe(false);
  });

  it("gives this device the bodies it holds once one was taken, as the server sent them", () => {
    const store = deviceOf();
    const page = pageOf();
    page.bodies.answered(OUTLINE_NOW, new Set());
    const sent = documentOf(SECTIONS_NOW);
    took(page, new Set(["intro"]), { path: PATH, sections: [sent.sections[0] as Section] });
    expect(store.keep).toHaveBeenCalledTimes(0);
    page.state = documentState(OUTLINE_NOW.document);
    page.keepBodies();
    // Taken before the page had a state: given now, under the outline's validator.
    expect(store.keep).toHaveBeenCalledWith(
      CACHE_FORMAT,
      "",
      `document:${PATH}`,
      "document",
      SECTIONS,
      "",
      { intro: { validator: "i1", body: { section: sent.sections[0] } } },
      undefined,
      false,
    );
    page.keepBodies();
    expect(store.keep).toHaveBeenCalledTimes(1);
    const unkept = pageOf(null as never);
    unkept.bodies.answered(OUTLINE_NOW, new Set());
    unkept.bodies.read(new Set(["intro"]), [sent.sections[0] as Section]);
    unkept.keepBodies();
    expect(store.keep).toHaveBeenCalledTimes(1);
  });
});

describe("an event of the document's feed", () => {
  /** A page showing the server's outline, the sections `wanted` near the screen with bodies. */
  const following = (...wanted: string[]) => {
    const page = pageOf(null as never);
    page.wanted = new Set(wanted);
    outlined(page, OUTLINE_NOW);
    took(page, new Set(wanted), {
      path: PATH,
      sections: SECTIONS_NOW.filter((one) => wanted.includes(one.key)),
    });
    return page;
  };

  it("is read only with its cursor and its changes; the frame hears it all the same", () => {
    const page = following();
    for (const value of [
      null,
      { changes: [] },
      { version: -1, changes: [] },
      { version: 1.5, changes: [] },
      { version: 9 },
      { version: 9, changes: [{ region: "document", data: { sections: "none" } }] },
    ]) {
      expect(page.heard(value)).toBe(false);
    }
    expect(page.frame.heard).toHaveBeenCalledTimes(6);
    expect(page.heard({ version: 9, changes: [] })).toBe(true);
    expect(page.cursor).toBe(9);
  });

  it("counts the moves of the review queue's cards, for the postponed list", () => {
    const page = following();
    page.heard({
      version: 9,
      changes: [{ region: "queue-item" }, { region: "queue-item-order" }, { region: "board" }],
    });
    expect(page.orders).toBe(2);
    expect(page.held).toBe(0);
  });

  it("counts the comments held or passed on, for the page's held comments", () => {
    const page = following();
    const told = vi.fn();
    page.subscribe(told);
    page.heard({ version: 9, changes: [{ region: "held", data: {} }, { region: "held" }] });
    expect([page.held, page.orders]).toEqual([2, 0]);
    expect(told).toHaveBeenCalled();
  });

  it("applies its changes of this document alone, and the anchors they bring", () => {
    const page = following("intro");
    const told = vi.fn();
    page.subscribe(told);
    const other = { path: "docs/OTHER.md", title: "Other" };
    page.heard({ version: 9, changes: [{ region: "document", data: other }] });
    expect(page.state?.latest.title).toBe("Decisions");
    page.heard({
      version: 10,
      changes: [
        {
          region: "document",
          data: {
            path: PATH,
            title: "Decisions, again",
            sections: [section("intro", "i2", "Intro, again")],
            anchors: { intro: ["intro", "again"] },
          },
        },
      ],
    });
    expect(page.state?.latest.title).toBe("Decisions, again");
    expect(page.state?.latest.version).toBe(10);
    expect(page.state?.latest.sections[0]?.blocks).toEqual(
      section("intro", "i2", "Intro, again").blocks,
    );
    expect(page.anchors).toEqual({ intro: ["intro", "again"] });
    expect(told).toHaveBeenCalledTimes(2);
  });

  it("keeps a wanted body its change left alike, and reads again one it changed", () => {
    const page = following("intro", "lamps");
    page.heard({
      version: 9,
      changes: [
        {
          region: "document",
          data: { path: PATH, sections: [section("intro", "i1", ""), section("lamps", "l2", "")] },
        },
      ],
    });
    expect(page.state?.latest.sections.find((one) => one.key === "intro")?.blocks).toEqual(
      section("intro", "i1", "Intro").blocks,
    );
    expect(page.loaded).toEqual(new Set(["intro"]));
    expect(page.loading()).toEqual(new Set(["lamps"]));
  });

  it("of a checkout's document names it under its scope", () => {
    const page = new DocumentLive(PATH, "design~policy", null, frameOf());
    // The server names a checkout's document under its scope in the document as in its changes.
    page.state = documentState({ ...OUTLINE_NOW.document, path: `@design~policy/${PATH}` });
    page.heard({
      version: 9,
      changes: [{ region: "document", data: { path: PATH, title: "Main's" } }],
    });
    expect(page.state?.latest.title).toBe("Decisions");
    page.heard({
      version: 10,
      changes: [{ region: "document", data: { path: `@design~policy/${PATH}`, title: "Its" } }],
    });
    expect(page.state?.latest.title).toBe("Its");
  });
});

const sent = documentOf(SECTIONS_NOW);
/** The bodies this device kept: the intro's and the card's current by `OUTLINE_NOW`, the lamps' not. */
const keptBodies = {
  intro: { validator: "i1", body: { section: sent.sections[0] } },
  "card:palette": {
    validator: "p1/t1",
    body: { section: sent.sections[1], card: PALETTE, thread: sent.threads.palette },
  },
  lamps: { validator: "l0", body: { section: section("lamps", "l0", "Old lamps") } },
};
/** What this device kept of the document: `outline`, tagged `"o1"`, and `bodies`. */
const kept = (outline: unknown = OUTLINE_NOW, bodies: unknown = keptBodies) => ({
  [`document:${PATH} ${OUTLINE}`]: { value: outline, etag: '"o1"', saved: 1_000 },
  [`document:${PATH} ${SECTIONS}`]: { value: bodies, etag: "", saved: 2_000 },
});

describe("a document drawn from this device", () => {
  it("is its outline with the bodies current by it, one more visit counted", async () => {
    const store = deviceOf(kept());
    const page = pageOf();
    const told = vi.fn();
    page.subscribe(told);
    const copy = await fromDevice(page, new AbortController().signal);
    expect(copy).toEqual({ tag: '"o1"', outline: OUTLINE_NOW });
    expect(store.visit).toHaveBeenCalledWith(
      CACHE_FORMAT,
      "",
      `document:${PATH}`,
      "document",
      false,
    );
    expect(page.loaded).toEqual(new Set(["intro", "card:palette"]));
    // The body the outline no longer names as kept is not the page's to give back.
    expect([...page.sent.keys()]).toEqual(["intro", "card:palette"]);
    expect(page.anchors).toEqual({ intro: ["intro"] });
    expect(page.state?.latest.sections.map((one) => one.blocks.length)).toEqual([1, 1, 0]);
    expect(page.freshness.saved).toBe(1_000);
    expect(waits(page.freshness, OUTLINE)).toBe(true);
    expect(waits(page.freshness, SECTIONS)).toBe(true);
    expect(told).toHaveBeenCalledTimes(1);
  });

  it("counts no visit for a folder's intro, kept under the folder's own item", async () => {
    const folder = documentItem("docs/", true);
    const store = deviceOf({
      [`${folder.id} ${OUTLINE}`]: { value: OUTLINE_NOW, etag: '"o1"', saved: 1_000 },
    });
    const page = pageOf(folder);
    await fromDevice(page, new AbortController().signal);
    expect(store.visit).not.toHaveBeenCalled();
    expect(page.state).not.toBeNull();
    expect(page.loaded).toEqual(new Set());
    expect(waits(page.freshness, SECTIONS)).toBe(false);
  });

  it("is nothing for a page that keeps nothing, kept nothing, or moved on", async () => {
    deviceOf(kept(outlineOf(documentOf(SECTIONS_NOW, "Agreed", "docs/OTHER.md"))));
    const intro = pageOf();
    expect(await fromDevice(intro, new AbortController().signal)).toBeNull();
    expect(intro.state).toBeNull();
    expect(await fromDevice(pageOf(null as never), new AbortController().signal)).toBeNull();
    deviceOf();
    expect(await fromDevice(pageOf(), new AbortController().signal)).toBeNull();
    deviceOf(kept());
    const left = new AbortController();
    left.abort();
    const gone = pageOf();
    expect(await fromDevice(gone, left.signal)).toBeNull();
    expect(gone.state).toBeNull();
    const drawn = pageOf();
    drawn.state = documentState(sent);
    expect(await fromDevice(drawn, new AbortController().signal)).toBeNull();
    expect(drawn.state.latest).toBe(sent);
  });

  it("read ahead of its first visit draws the first part's bodies, for the visit to keep", async () => {
    const first: DocumentOutline = {
      document: { ...sent, sections: [sent.sections[0] as Section, section("lamps", "l1", "")] },
      anchors: {},
      thread_hashes: { palette: "t1" },
    };
    const store = deviceOf({
      [`document:${PATH} ${OUTLINE}`]: { value: first, etag: "", saved: 3_000 },
    });
    const page = pageOf();
    const copy = await fromDevice(page, new AbortController().signal);
    expect(copy?.outline.document.sections.every((one) => one.blocks.length === 0)).toBe(true);
    expect(page.loaded).toEqual(new Set(["intro"]));
    expect(page.freshness.saved).toBe(3_000);
    // Never asked of the device: the first part carried them.
    expect(store.read).toHaveBeenCalledTimes(1);
    page.keepBodies();
    expect(store.keep).toHaveBeenCalledWith(
      CACHE_FORMAT,
      "",
      `document:${PATH}`,
      "document",
      SECTIONS,
      "",
      { intro: { validator: "i1", body: { section: sent.sections[0] } } },
      undefined,
      false,
    );
  });

  it("answered by the server's outline keeps the bodies still current, confirmed", async () => {
    deviceOf(kept());
    const page = pageOf();
    page.wanted = new Set(["intro", "lamps"]);
    await fromDevice(page, new AbortController().signal);
    outlined(page, { ...OUTLINE_NOW, document: { ...OUTLINE_NOW.document, version: 12 } });
    expect(page.epoch).toBe(1);
    expect(page.cursor).toBe(12);
    expect(page.loaded).toEqual(new Set(["intro", "card:palette"]));
    expect(page.loading()).toEqual(new Set(["lamps"]));
    expect(waits(page.freshness, OUTLINE)).toBe(false);
    expect(page.state?.latest.version).toBe(12);
    expect(page.state?.removed).toBe(false);
  });

  it("keeps a body it holds off the screen that a change left alike, and reads again one it changed", async () => {
    deviceOf(kept());
    const page = pageOf();
    await fromDevice(page, new AbortController().signal);
    outlined(page, OUTLINE_NOW);
    // Held from this device, neither near the screen yet.
    expect(page.wanted).toEqual(new Set());
    expect(page.loaded).toEqual(new Set(["intro", "card:palette"]));
    page.heard({
      version: 9,
      changes: [
        {
          region: "document",
          data: {
            path: PATH,
            sections: [section("intro", "i1", ""), section("card:palette", "p2", "")],
          },
        },
      ],
    });
    const blocks = (key: string) =>
      page.state?.latest.sections.find((one) => one.key === key)?.blocks;
    expect(blocks("intro")).toEqual(section("intro", "i1", "Intro").blocks);
    expect(blocks("card:palette")).toEqual([]);
    expect(page.loaded).toEqual(new Set(["intro"]));
    // Scrolled to, the changed one is read, not left a ghost.
    page.want("intro");
    page.want("card:palette");
    expect(page.loading()).toEqual(new Set(["card:palette"]));
  });

  it("answered 404 goes, and so does its item unless it is pinned", async () => {
    const store = deviceOf(kept());
    const page = pageOf();
    await fromDevice(page, new AbortController().signal);
    vanished(page);
    expect(page.missing).toBe(true);
    expect(page.state).toBeNull();
    expect(waits(page.freshness, OUTLINE)).toBe(false);
    expect(waits(page.freshness, SECTIONS)).toBe(false);
    expect(store.forget).toHaveBeenCalledWith(CACHE_FORMAT, "", `document:${PATH}`);
    const pinned = documentItem(PATH, true);
    const intro = deviceOf({
      [`${pinned.id} ${OUTLINE}`]: { value: OUTLINE_NOW, etag: '"o1"', saved: 1_000 },
    });
    const entrypoint = pageOf(pinned);
    await fromDevice(entrypoint, new AbortController().signal);
    vanished(entrypoint);
    expect(entrypoint.state).toBeNull();
    expect(intro.forget).not.toHaveBeenCalled();
  });

  it("not drawn, answered 404, says the document is missing and forgets nothing", () => {
    const store = deviceOf();
    const page = pageOf();
    page.trouble = "earlier";
    vanished(page);
    expect(page.missing).toBe(true);
    expect(page.trouble).toBeNull();
    expect(store.forget).not.toHaveBeenCalled();
  });
});

describe("the bodies a page read", () => {
  it("show in its sections with their cards and threads, and are kept current", () => {
    const store = deviceOf();
    const page = pageOf();
    page.wanted = new Set(["card:palette"]);
    outlined(page, OUTLINE_NOW);
    page.trouble = "earlier";
    const sent = documentOf(SECTIONS_NOW);
    took(page, new Set(["card:palette"]), {
      path: PATH,
      sections: [sent.sections[1] as Section],
      cards: [
        { key: "palette", value: PALETTE },
        { key: "gone", removed: true },
      ],
      threads: { palette: sent.threads.palette ?? null },
    });
    expect(page.loaded).toEqual(new Set(["card:palette"]));
    expect(page.sent.get("card:palette")).toEqual({
      section: sent.sections[1],
      card: PALETTE,
      thread: sent.threads.palette,
    });
    expect(page.state?.latest.sections[1]?.blocks).toEqual(sent.sections[1]?.blocks);
    expect(page.state?.latest.threads.palette).toEqual(sent.threads.palette);
    expect(page.trouble).toBeNull();
    expect(store.keep).toHaveBeenLastCalledWith(
      CACHE_FORMAT,
      "",
      `document:${PATH}`,
      "document",
      SECTIONS,
      "",
      {
        "card:palette": {
          validator: "p1/t1",
          body: { section: sent.sections[1], card: PALETTE, thread: sent.threads.palette },
        },
      },
      undefined,
      false,
    );
  });

  it("drawn from this device and read again answer its sections' copy", async () => {
    deviceOf({
      [`document:${PATH} ${OUTLINE}`]: { value: OUTLINE_NOW, etag: '"o1"', saved: 1_000 },
      [`document:${PATH} ${SECTIONS}`]: {
        value: { lamps: { validator: "l0", body: { section: section("lamps", "l0", "Old") } } },
        etag: "",
        saved: 2_000,
      },
    });
    const page = pageOf();
    page.wanted = new Set(["lamps"]);
    await fromDevice(page, new AbortController().signal);
    expect(page.loaded).toEqual(new Set());
    outlined(page, OUTLINE_NOW);
    took(page, new Set(["lamps"]), { path: PATH, sections: [section("lamps", "l1", "Lamps")] });
    expect(waits(page.freshness, SECTIONS)).toBe(false);
  });
});

/** The page's WebSocket, each stream it opens kept in order: where, and how often closed. */
const feeds = () => pageFeeds("http://127.0.0.1/docs/DECISIONS.md");

describe("a document page's loops", () => {
  it("read the outline, follow the feed from its cursor, and read the bodies near the screen", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const opened = feeds();
    const page = pageOf();
    const left = new AbortController();
    const done = documentLive(page, left.signal);
    await askedFor(asked, 1);
    expect(asked[0]?.url).toBe("/api/document?path=docs%2FDECISIONS.md&view=outline");
    asked[0]?.answer(answered(200, OUTLINE_NOW, '"o1"', 7));
    await askedFor(opened, 1);
    expect(opened[0]?.url).toBe(
      "ws://127.0.0.1/api/feed?since=7&document=docs%2FDECISIONS.md&sections=%5B%5D",
    );
    opened[0]?.source.dispatchEvent(new Event("open"));
    await until(() => expect(page.stale).toBe(false));
    expect(page.frame.connected).toHaveBeenCalledTimes(1);
    // A section near the screen: its body is read, and the feed opened again for it.
    page.want("intro");
    await askedFor(opened, 2);
    expect(opened[0]?.closed).toBe(1);
    expect(opened[1]?.url).toContain(`&sections=${keysQuery(new Set(["intro"]))}`);
    await askedFor(asked, 2);
    expect(asked[1]?.url).toBe(
      `/api/section?path=docs%2FDECISIONS.md&format=blocks&keys=${keysQuery(new Set(["intro"]))}`,
    );
    asked[1]?.answer(
      reply(200, { version: 7, change: { path: PATH, sections: [SECTIONS_NOW[0]] } }),
    );
    await until(() => expect(page.ready()).toBe(true));
    expect(page.state?.latest.sections[0]?.blocks).toEqual(SECTIONS_NOW[0]?.blocks);
    expect(store.keep).toHaveBeenCalledWith(
      CACHE_FORMAT,
      "",
      `document:${PATH}`,
      "document",
      SECTIONS,
      "",
      { intro: { validator: "i1", body: { section: SECTIONS_NOW[0] } } },
      undefined,
      false,
    );
    left.abort();
    await done;
    expect(opened[1]?.closed).toBe(1);
  });

  it("tell the frame of a follow's first open alone: a stream opened again for sections missed nothing", async () => {
    vi.useFakeTimers();
    try {
      const asked = serverOf();
      deviceOf();
      const opened = feeds();
      const page = pageOf();
      const left = new AbortController();
      const done = documentLive(page, left.signal);
      const outlines = () => asked.filter((read) => read.url.endsWith("&view=outline"));
      /** The stream `at` opens, then says the event `version`, which moves no region. */
      const says = async (at: number, version: number) => {
        opened[at]?.source.dispatchEvent(new Event("open"));
        say(opened[at]?.source, FEED_EVENT, JSON.stringify({ version, changes: [] }));
        await vi.advanceTimersByTimeAsync(0);
        expect(page.cursor).toBe(version);
      };
      await vi.advanceTimersByTimeAsync(0);
      outlines()[0]?.answer(answered(200, OUTLINE_NOW, '"o1"', 7));
      await vi.advanceTimersByTimeAsync(0);
      await says(0, 8);
      expect(page.frame.connected).toHaveBeenCalledTimes(1);
      // Each section near the screen opens the stream again, from the last event heard.
      for (const [at, key] of ["intro", "lamps"].entries()) {
        page.want(key);
        await vi.advanceTimersByTimeAsync(200);
        expect(opened).toHaveLength(at + 2);
        expect(opened[at]?.closed).toBe(1);
        expect(opened[at + 1]?.url).toContain(`?since=${8 + at}&`);
        await says(at + 1, 9 + at);
      }
      expect(page.frame.connected).toHaveBeenCalledTimes(1);
      expect(page.stale).toBe(false);
      // A stream lost: the outline is read again after a pause, and the next follow's first open
      // tells the frame, so what the loss missed is read again.
      opened[2]?.source.dispatchEvent(new Event("error"));
      await vi.advanceTimersByTimeAsync(0);
      expect(page.stale).toBe(true);
      expect(opened[2]?.closed).toBe(1);
      await vi.advanceTimersByTimeAsync(1_000);
      expect(outlines()).toHaveLength(2);
      outlines()[1]?.answer(answered(200, OUTLINE_NOW, '"o1"'));
      await vi.advanceTimersByTimeAsync(0);
      expect(opened).toHaveLength(4);
      await says(3, 11);
      expect(page.frame.connected).toHaveBeenCalledTimes(2);
      expect(page.stale).toBe(false);
      left.abort();
      await vi.advanceTimersByTimeAsync(0);
      await done;
    } finally {
      vi.useRealTimers();
    }
  });

  it("open one feed on a return visit, naming the sections drawn from this device before they near the screen", async () => {
    vi.useFakeTimers();
    try {
      const asked = serverOf();
      deviceOf(kept());
      const opened = feeds();
      const page = pageOf();
      const left = new AbortController();
      const done = documentLive(page, left.signal);
      await vi.advanceTimersByTimeAsync(0);
      expect(asked).toHaveLength(1);
      expect(asked[0]?.headers).toEqual({ "If-None-Match": '"o1"' });
      // The check answers before any section has come near the screen.
      expect(page.wanted).toEqual(new Set());
      asked[0]?.answer(answered(304, null, "", 7));
      await vi.advanceTimersByTimeAsync(0);
      expect(opened).toHaveLength(1);
      const held = new Set(["intro", "card:palette"]);
      expect(opened[0]?.url).toBe(
        `ws://127.0.0.1/api/feed?since=7&document=docs%2FDECISIONS.md&sections=${keysQuery(held)}`,
      );
      opened[0]?.source.dispatchEvent(new Event("open"));
      await vi.advanceTimersByTimeAsync(0);
      expect(page.stale).toBe(false);
      // Their slots come near the screen a frame later and name them: the stream names them
      // already, so it stays, and nothing is read.
      page.want("intro");
      page.want("card:palette");
      await vi.advanceTimersByTimeAsync(1_000);
      expect(opened).toHaveLength(1);
      expect(opened[0]?.closed).toBe(0);
      expect(asked).toHaveLength(1);
      expect(page.ready()).toBe(true);
      // A section this device held no current body of opens it again, naming it too, and is read.
      page.want("lamps");
      await vi.advanceTimersByTimeAsync(200);
      expect(opened).toHaveLength(2);
      expect(opened[0]?.closed).toBe(1);
      expect(opened[1]?.url).toContain(
        `?since=7&document=docs%2FDECISIONS.md&sections=${keysQuery(new Set([...held, "lamps"]))}`,
      );
      expect(asked.map((read) => read.url)).toContain(
        `/api/section?path=docs%2FDECISIONS.md&format=blocks&keys=${keysQuery(new Set(["lamps"]))}`,
      );
      expect(page.frame.connected).toHaveBeenCalledTimes(1);
      left.abort();
      await vi.advanceTimersByTimeAsync(0);
      await done;
      expect(opened[1]?.closed).toBe(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("lose a feed whose event they cannot read, and read again after a pause", async () => {
    const asked = serverOf();
    deviceOf();
    const opened = feeds();
    const page = pageOf();
    const left = new AbortController();
    const done = documentLive(page, left.signal);
    await askedFor(asked, 1);
    asked[0]?.answer(answered(200, OUTLINE_NOW, '"o1"', 7));
    await askedFor(opened, 1);
    opened[0]?.source.dispatchEvent(new Event("open"));
    await until(() => expect(page.stale).toBe(false));
    say(opened[0]?.source, FEED_EVENT, '{"version":"x"}');
    await until(() => expect(page.stale).toBe(true));
    expect(opened[0]?.closed).toBe(1);
    left.abort();
    await done;
  });

  it("forget a copy drawn from this device that the server answers 404", async () => {
    const asked = serverOf();
    const store = deviceOf({
      [`document:${PATH} ${OUTLINE}`]: { value: OUTLINE_NOW, etag: '"o1"', saved: 1_000 },
    });
    const page = pageOf();
    const left = new AbortController();
    const done = documentLive(page, left.signal);
    await askedFor(asked, 1);
    expect(page.state).not.toBeNull();
    expect(asked[0]?.headers).toEqual({ "If-None-Match": '"o1"' });
    asked[0]?.answer(reply(404, { error: "no such document" }));
    await until(() => expect(page.missing).toBe(true));
    expect(page.state).toBeNull();
    expect(store.forget).toHaveBeenCalledTimes(1);
    left.abort();
    await done;
  });

  it("say why a read failed, and keep what they drew", async () => {
    const asked = serverOf();
    deviceOf({
      [`document:${PATH} ${OUTLINE}`]: { value: OUTLINE_NOW, etag: '"o1"', saved: 1_000 },
    });
    const page = pageOf();
    const left = new AbortController();
    const done = documentLive(page, left.signal);
    await askedFor(asked, 1);
    asked[0]?.answer(reply(500, { error: "the disk is full" }));
    await until(() => expect(page.trouble).not.toBeNull());
    expect(page.trouble).toContain("the disk is full");
    expect(page.freshness.failing).toEqual([OUTLINE]);
    expect(page.state).not.toBeNull();
    expect(page.stale).toBe(true);
    left.abort();
    await done;
  });

  it("take a feed that says nothing for 12.5 s as lost", async () => {
    vi.useFakeTimers();
    try {
      const asked = serverOf();
      deviceOf();
      const opened = feeds();
      const page = pageOf();
      const left = new AbortController();
      const done = documentLive(page, left.signal);
      await vi.advanceTimersByTimeAsync(0);
      asked[0]?.answer(answered(200, OUTLINE_NOW, '"o1"', 7));
      await vi.advanceTimersByTimeAsync(0);
      opened[0]?.source.dispatchEvent(new Event("open"));
      await vi.advanceTimersByTimeAsync(0);
      expect(page.stale).toBe(false);
      // Each timer the advance fires costs a real turn of the event loop, which a loaded machine
      // makes slow: the clock jumps across the silence (`setSystemTime` moves the date and every
      // waiting timer alike, firing none), so only the follow's waits on either side of 12.5 s
      // run, not 12 s of every loop's polls.
      vi.setSystemTime(Date.now() + 12_200);
      await vi.advanceTimersByTimeAsync(200);
      expect(page.stale).toBe(false);
      await vi.advanceTimersByTimeAsync(200);
      expect(page.stale).toBe(true);
      expect(opened[0]?.closed).toBe(1);
      left.abort();
      await vi.advanceTimersByTimeAsync(0);
      await done;
    } finally {
      vi.useRealTimers();
    }
  });

  it("say and change nothing once the page left before the outline answered", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const page = pageOf();
    const left = new AbortController();
    const done = documentLive(page, left.signal);
    await askedFor(asked, 1);
    const freshness = page.freshness;
    const told = vi.fn();
    page.subscribe(told);
    left.abort();
    await done;
    expect(asked[0]?.signal?.aborted).toBe(true);
    expect(page.trouble).toBeNull();
    expect(page.freshness).toEqual(freshness);
    expect(page.state).toBeNull();
    expect(store.keep).not.toHaveBeenCalled();
    expect(told).not.toHaveBeenCalled();
  });

  it("do nothing more once the page left as the outline answered", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const page = pageOf();
    const left = new AbortController();
    const done = documentLive(page, left.signal);
    await askedFor(asked, 1);
    asked[0]?.answer(answered(200, OUTLINE_NOW, '"o1"', 7));
    left.abort();
    await done;
    expect(page.state).toBeNull();
    // Nothing kept after the page left: the device holds only what it held.
    expect(store.keep).not.toHaveBeenCalled();
    expect(page.epoch).toBe(0);
  });

  it("read nothing again once the page left as a 304 naming no cursor came", async () => {
    const asked = serverOf();
    const store = deviceOf({
      [`document:${PATH} ${OUTLINE}`]: { value: OUTLINE_NOW, etag: '"o1"', saved: 1_000 },
    });
    const page = pageOf();
    const left = new AbortController();
    const done = documentLive(page, left.signal);
    await askedFor(asked, 1);
    expect(asked[0]?.headers).toEqual({ "If-None-Match": '"o1"' });
    asked[0]?.answer(answered(304, null));
    left.abort();
    await done;
    expect(asked).toHaveLength(1);
    expect(store.keep).not.toHaveBeenCalled();
    expect(page.epoch).toBe(0);
  });
});

describe("a read of the bodies near the screen", () => {
  /** A page whose outline answered at cursor 7, wanting the intro, its reads running. */
  async function reading() {
    vi.useFakeTimers();
    const asked = serverOf();
    deviceOf();
    const page = pageOf();
    outlined(page, OUTLINE_NOW);
    page.want("intro");
    const left = new AbortController();
    const done = loadSections(page, left.signal);
    await vi.advanceTimersByTimeAsync(50);
    expect(asked).toHaveLength(1);
    const end = async () => {
      left.abort();
      await vi.advanceTimersByTimeAsync(0);
      await done;
    };
    return { asked, page, end };
  }

  /** The bodies' answer, read at the feed's cursor `version`. */
  const bodies = (version: number) =>
    reply(200, { version, change: { path: PATH, sections: [SECTIONS_NOW[0]] } });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("waits for the feed to reach the cursor it was read at before it shows", async () => {
    const { asked, page, end } = await reading();
    asked[0]?.answer(bodies(9));
    await vi.advanceTimersByTimeAsync(1_000);
    expect(page.loaded).toEqual(new Set());
    // The feed brings the events up to 9.
    page.cursor = 9;
    await vi.advanceTimersByTimeAsync(100);
    expect(page.loaded).toEqual(new Set(["intro"]));
    expect(page.state?.latest.sections[0]?.blocks).toEqual(SECTIONS_NOW[0]?.blocks);
    await end();
  });

  it("is read again when the feed does not reach its cursor within two seconds", async () => {
    const { asked, page, end } = await reading();
    asked[0]?.answer(bodies(9));
    await vi.advanceTimersByTimeAsync(2_100);
    expect(page.loaded).toEqual(new Set());
    expect(asked).toHaveLength(2);
    asked[1]?.answer(bodies(7));
    await vi.advanceTimersByTimeAsync(0);
    expect(page.loaded).toEqual(new Set(["intro"]));
    await end();
  });

  it("is dropped when the page's outline was read again meanwhile", async () => {
    const { asked, page, end } = await reading();
    asked[0]?.answer(bodies(9));
    await vi.advanceTimersByTimeAsync(100);
    outlined(page, outlineOf({ ...documentOf(SECTIONS_NOW), version: 9 }));
    await vi.advanceTimersByTimeAsync(100);
    expect(page.loaded).toEqual(new Set());
    // Read again under the new outline, and shown.
    expect(asked).toHaveLength(2);
    asked[1]?.answer(bodies(9));
    await vi.advanceTimersByTimeAsync(0);
    expect(page.loaded).toEqual(new Set(["intro"]));
    await end();
  });

  it("says why it failed, and is tried again a second later", async () => {
    const { asked, page, end } = await reading();
    asked[0]?.answer(reply(500, { error: "the disk is full" }));
    await vi.advanceTimersByTimeAsync(0);
    expect(page.trouble).toContain("the disk is full");
    await vi.advanceTimersByTimeAsync(900);
    expect(asked).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(200);
    expect(asked).toHaveLength(2);
    asked[1]?.answer(bodies(7));
    await vi.advanceTimersByTimeAsync(0);
    expect(page.trouble).toBeNull();
    expect(page.loaded).toEqual(new Set(["intro"]));
    await end();
  });

  it("says and changes nothing once the page left before its bodies answered", async () => {
    const { asked, page, end } = await reading();
    const told = vi.fn();
    page.subscribe(told);
    await end();
    expect(asked[0]?.signal?.aborted).toBe(true);
    expect(page.trouble).toBeNull();
    expect(page.loaded).toEqual(new Set());
    expect(told).not.toHaveBeenCalled();
  });

  it("fails on an answer it cannot read", async () => {
    const { asked, page, end } = await reading();
    asked[0]?.answer(reply(200, { version: "seven" }));
    await vi.advanceTimersByTimeAsync(0);
    expect(page.trouble).toContain("The server's sections could not be read");
    await end();
  });
});

const FOLDER = CASES.samples.folder as Folder;

describe("a folder page's loop", () => {
  const URL = "/api/folder?path=docs&view=outline";

  it("reads the folder, keeps an entrypoint's, and reads it again naming its tag", async () => {
    const asked = serverOf();
    const store = deviceOf();
    const item = documentItem("/docs/", true);
    const page = new FolderLive("docs", "", item);
    const left = new AbortController();
    const done = followFolder(page, left.signal);
    await askedFor(asked, 1);
    expect(store.visit).toHaveBeenCalledWith(CACHE_FORMAT, "", item.id, "document", true);
    expect(asked[0]?.url).toBe(URL);
    asked[0]?.answer(reply(200, FOLDER, '"f1"'));
    await until(() => expect(page.folder).toEqual(FOLDER));
    expect(page.freshness.open).toBe(true);
    expect(store.keep).toHaveBeenCalledWith(
      CACHE_FORMAT,
      "",
      item.id,
      "document",
      FOLDER_PART,
      '"f1"',
      FOLDER,
      undefined,
      false,
    );
    left.abort();
    await done;
  });

  it("draws the folder this device kept, then the server's answer to it", async () => {
    const asked = serverOf();
    const item = documentItem("/docs/", true);
    deviceOf({ [`${item.id} ${FOLDER_PART}`]: { value: FOLDER, etag: '"f1"', saved: 5 } });
    const page = new FolderLive("docs", "", item);
    const left = new AbortController();
    const done = followFolder(page, left.signal);
    await askedFor(asked, 1);
    expect(page.folder).toEqual(FOLDER);
    expect(waits(page.freshness, FOLDER_PART)).toBe(true);
    expect(asked[0]?.headers).toEqual({ "If-None-Match": '"f1"' });
    asked[0]?.answer(answered(304, null));
    await until(() => expect(waits(page.freshness, FOLDER_PART)).toBe(false));
    expect(page.folder).toEqual(FOLDER);
    left.abort();
    await done;
  });

  it("gives its read up on the wire once its page left, and changes nothing", async () => {
    const asked = serverOf();
    deviceOf();
    const page = new FolderLive("docs", "", null);
    const left = new AbortController();
    const done = followFolder(page, left.signal);
    await askedFor(asked, 1);
    expect(asked[0]?.signal?.aborted).toBe(false);
    const freshness = page.freshness;
    const told = vi.fn();
    page.subscribe(told);
    left.abort();
    expect(asked[0]?.signal?.aborted).toBe(true);
    await done;
    expect(page.folder).toBeNull();
    expect(page.error).toBeNull();
    expect(page.missing).toBe(false);
    expect(page.freshness).toEqual(freshness);
    expect(told).not.toHaveBeenCalled();
  });

  it("shows nothing it drew of a folder gone while away", async () => {
    const asked = serverOf();
    const item = documentItem("/docs/", true);
    deviceOf({ [`${item.id} ${FOLDER_PART}`]: { value: FOLDER, etag: '"f1"', saved: 5 } });
    const page = new FolderLive("docs", "", item);
    const left = new AbortController();
    const done = followFolder(page, left.signal);
    await askedFor(asked, 1);
    asked[0]?.answer(reply(404, { error: "no such folder" }));
    await until(() => expect(page.missing).toBe(true));
    expect(page.folder).toBeNull();
    expect(page.error).toBeNull();
    left.abort();
    await done;
  });

  it("says why a read failed, and a 304 or a folder it cannot read is a failure", async () => {
    for (const [answer, said] of [
      [reply(500, { error: "the disk is full" }), "the disk is full"],
      [answered(304, null), "The server answered 304 to a plain read"],
      [reply(200, { path: 3 }), "The server's folder could not be read"],
    ] as const) {
      const asked = serverOf();
      deviceOf();
      const page = new FolderLive("docs", "", null);
      const left = new AbortController();
      const done = followFolder(page, left.signal);
      await askedFor(asked, 1);
      asked[0]?.answer(answer);
      await until(() => expect(page.error).not.toBeNull());
      expect(page.error).toContain(said);
      expect(page.freshness.failing).toEqual([FOLDER_PART]);
      left.abort();
      await done;
    }
  });
});
