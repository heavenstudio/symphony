import { describe, expect, it } from "vitest";
import {
  answersVerdicts,
  bodies,
  change,
  collapse,
  type DocumentState,
  documentState,
  drawBodies,
  ending,
  folderPill,
  fragment,
  held,
  isSettled,
  ownerLines,
  partEdges,
  partForm,
  put,
  refresh,
  related,
  release,
  releasedFrom,
  savedText,
  settle,
  tap,
  Unanswered,
  words,
} from "./document";
import { CASES } from "./testing";
import type {
  Document,
  DocumentCard,
  DocumentChange,
  DocumentTask,
  Folder,
  Section,
  Span,
  ThreadAuthor,
} from "./wire";

/** White space Rust's `split_whitespace` splits at, and a character it does not. */
const [NEL, NBSP, LS, IDEOGRAPHIC, BOM] = [0x85, 0xa0, 0x2028, 0x3000, 0xfeff].map((point) =>
  String.fromCodePoint(point),
);

const section = (key: string, hash: string): Section => ({
  start: 0,
  end: 0,
  key,
  hash,
  part: null,
  blocks: [{ type: "paragraph", spans: [] }],
});

const DOCUMENT: Document = {
  version: 0,
  path: "docs/DECISIONS.md",
  title: "Decisions",
  waiting: [],
  crumbs: [],
  headings: [],
  sections: [],
  cards: [],
  threads: {},
  tasks: [],
};

function document(sections: readonly (readonly [string, string])[]): Document {
  return { ...DOCUMENT, sections: sections.map(([key, hash]) => section(key, hash)) };
}

const shown = (state: DocumentState) =>
  state.shown.sections.map((one) => [one.key, one.hash] as const);
const keys = (...named: string[]) => new Set(named);
const NONE = new Set<string>();

/** `doc` with the card `slug`'s thread: the owner's lines and the agents', in turn. */
function thread(doc: Document, slug: string, owner: number, agent: number): Document {
  const entry = (who: ThreadAuthor) => ({ who, timestamp: null, verdict: null, blocks: [] });
  const entries = [
    ...Array.from({ length: owner }, () => entry("owner")),
    ...Array.from({ length: agent }, () => entry("agent")),
  ];
  return { ...doc, threads: { ...doc.threads, [slug]: { blocks: [{ type: "thread", entries }] } } };
}

const card = (slug: string, state: string, text: string): DocumentCard => ({
  slug,
  state,
  text,
  group: "decide",
  open: true,
});

/** The document as an outline holds it: its sections with no blocks, no threads, cards cut short. */
function outlined(doc: Document): Document {
  return {
    ...doc,
    sections: doc.sections.map((one) => ({ ...one, blocks: [] })),
    threads: {},
    cards: doc.cards.map((one) => ({ ...one, text: one.text.slice(0, 3) })),
  };
}

/** Alpha is answered and moves from the first group to the second, its excerpt open. */
const BEFORE = [
  ["needs", "n1"],
  ["card:alpha", "a1"],
  ["card:bravo", "b1"],
] as const;
const AFTER = [
  ["needs", "n2"],
  ["card:bravo", "b1"],
  ["waiting", "w1"],
  ["card:alpha", "a2"],
] as const;

/** A state whose latest copy is `latest`, shown as `shown` was. */
const moved = (state: DocumentState, latest: Document): DocumentState => ({ ...state, latest });

describe("a document's feed changes", () => {
  const delta = (fields: Partial<DocumentChange>): DocumentChange => ({
    path: DOCUMENT.path,
    ...fields,
  });

  it("replace, add, delete, place and order its sections", () => {
    let state = documentState(document(BEFORE));
    const untouched = state.latest.sections[2];
    state = change(
      state,
      delta({
        sections: [section("card:alpha", "a2"), section("waiting", "w1")],
        deleted: ["needs"],
        positions: [
          { key: "card:bravo", start: 4, end: 9, part: 1 },
          { key: "gone", start: 1, end: 2, part: null },
        ],
        order: ["waiting", "card:bravo"],
      }),
    );
    expect(state.latest.sections.map((one) => [one.key, one.hash, one.start, one.part])).toEqual([
      ["waiting", "w1", 0, null],
      ["card:bravo", "b1", 4, 1],
      ["card:alpha", "a2", 0, null],
    ]);
    expect(untouched?.start).toBe(0);
    expect(state.shown.sections.map((one) => one.key)).toEqual(BEFORE.map(([key]) => key));
    // The sections an order does not name follow those it names, in the order they had.
    const named = change(state, delta({ order: ["card:alpha"] }));
    expect(named.latest.sections.map((one) => one.key)).toEqual([
      "card:alpha",
      "waiting",
      "card:bravo",
    ]);
  });

  it("set and clear threads and cards, and the fields they name", () => {
    let state = documentState({
      ...thread(document(BEFORE), "alpha", 1, 0),
      cards: [card("alpha", "pending", "Alpha"), card("bravo", "pending", "Bravo")],
    });
    const reply = thread(DOCUMENT, "bravo", 0, 1).threads.bravo;
    state = change(
      state,
      delta({
        threads: { alpha: null, bravo: reply ?? null },
        cards: [
          { key: "alpha", removed: true },
          { key: "bravo", value: card("bravo", "approved", "Bravo") },
          { key: "charlie", value: card("charlie", "pending", "Charlie") },
          { key: "delta", removed: true, value: card("delta", "pending", "Delta") },
        ],
        title: "Choices",
        waiting: ["charlie"],
        crumbs: [{ name: "docs", href: "/docs/" }],
        headings: [{ level: 2, anchor: "a", text: "A" }],
        tasks: [{ slug: "app/a", title: "A", status: "ready" }],
      }),
    );
    const { latest } = state;
    expect(Object.keys(latest.threads)).toEqual(["bravo"]);
    expect(latest.cards.map((one) => [one.slug, one.state])).toEqual([
      ["bravo", "approved"],
      ["charlie", "pending"],
      ["delta", "pending"],
    ]);
    expect([latest.title, latest.waiting, latest.crumbs.length, latest.headings.length]).toEqual([
      "Choices",
      ["charlie"],
      1,
      1,
    ]);
    expect(latest.tasks).toEqual([{ slug: "app/a", title: "A", status: "ready" }]);
    const bare = change(state, delta({}));
    expect(bare.latest).toEqual(latest);
  });

  it("leave another document alone, and mark this one removed", () => {
    const state = documentState(document(BEFORE));
    expect(change(state, { path: "docs/OTHER.md", removed: true })).toBe(state);
    const removed = change(state, delta({ removed: true, title: "Gone" }));
    expect(removed.removed).toBe(true);
    expect(removed.latest).toBe(state.latest);
  });

  it("update and clear its comparison with main", () => {
    let state = documentState({ ...DOCUMENT, path: "@design~policy/docs/POLICY.md" });
    const comparison = { task: "design/policy" };
    state = refresh(change(state, { path: state.latest.path, comparison: [comparison] }), NONE);
    expect(state.shown.comparison).toEqual(comparison);
    state = refresh(change(state, { path: state.latest.path, comparison: [] }), NONE);
    expect(state.shown.comparison ?? null).toBeNull();
  });

  it("take a key such as an object's own as any other", () => {
    const threads = JSON.parse('{"__proto__": {"blocks": []}}');
    const state = change(documentState(DOCUMENT), delta({ threads }));
    expect(Object.hasOwn(state.latest.threads, "__proto__")).toBe(true);
    expect(Object.getPrototypeOf(state.latest.threads)).toBe(Object.prototype);
  });
});

describe("the shown copy", () => {
  it("stays the same value when nothing it draws changed", () => {
    const state = documentState(thread(document(BEFORE), "alpha", 1, 0));
    expect(refresh(state, NONE)).toBe(state);
    expect(refresh(state, keys("card:alpha"))).toBe(state);
    // Any field drawn that is another value is a change: the title, a thread more, another one.
    const retitled = moved(state, { ...state.latest, title: "Other" });
    expect(refresh(retitled, NONE).shown.title).toBe("Other");
    const rethreaded = moved(state, thread(state.latest, "bravo", 1, 0));
    expect(refresh(rethreaded, NONE).shown).toEqual(rethreaded.latest);
    const unthreaded = moved(state, { ...state.latest, threads: { bravo: { blocks: [] } } });
    expect(refresh(unthreaded, NONE).shown).toEqual(unthreaded.latest);
    // A list of the same sections is no change; a field that is another value is, its sections
    // kept as they were drawn.
    const copied = moved(state, { ...state.latest, sections: [...state.latest.sections] });
    expect(refresh(copied, NONE)).toBe(copied);
    const rewaited = moved(state, { ...state.latest, waiting: [] });
    const next = refresh(rewaited, NONE);
    expect(next.shown).not.toBe(state.shown);
    expect(next.shown.sections.every((one, at) => one === state.shown.sections[at])).toBe(true);
  });

  it("keeps a held section's markup at its latest place in the source", () => {
    const state = documentState(document(BEFORE));
    const latest = document([
      ["needs", "n2"],
      ["card:alpha", "a2"],
    ]);
    const placed = {
      ...latest,
      sections: latest.sections.map((one) => ({ ...one, start: 7, end: 8, part: 2 })),
    };
    const next = refresh(moved(state, placed), keys("needs", "card:bravo"));
    expect(next.shown.sections.map((one) => [one.key, one.hash, one.start, one.part])).toEqual([
      ["needs", "n1", 7, 2],
      ["card:alpha", "a2", 7, 2],
      ["card:bravo", "b1", 0, null],
    ]);
  });

  it("keeps a held card's own copy, or none, and its thread", () => {
    const shownDoc = {
      ...thread(document(BEFORE), "alpha", 1, 0),
      cards: [card("alpha", "pending", "Alpha")],
    };
    const latest = {
      ...thread(document(BEFORE), "bravo", 1, 0),
      cards: [card("bravo", "approved", "Bravo")],
    };
    const next = refresh(moved(documentState(shownDoc), latest), keys("card:alpha", "card:bravo"));
    expect(next.shown.cards.map((one) => [one.slug, one.state])).toEqual([
      ["bravo", "approved"],
      ["alpha", "pending"],
    ]);
    expect(Object.keys(next.shown.threads)).toEqual(["alpha"]);
    const withCard = refresh(
      moved(documentState(shownDoc), { ...latest, cards: [card("alpha", "approved", "Alpha")] }),
      keys("card:alpha"),
    );
    expect(withCard.shown.cards).toEqual([card("alpha", "pending", "Alpha")]);
  });

  it("is the latest, regrouped, once a release lets an engaged card go", () => {
    let state = moved(
      documentState(thread(document(BEFORE), "alpha", 0, 1)),
      thread(document(AFTER), "alpha", 1, 1),
    );
    const before = ownerLines(state.shown, "card:alpha");
    const engaged = keys("card:alpha");
    const released = new Map<string, number>();
    let holding = held(engaged, NONE, released, [], state.latest);
    expect(holding).toEqual(engaged);
    state = refresh(state, holding);
    // Another reader's open excerpt holds the card and the order.
    expect(shown(state)).toEqual([
      ["needs", "n2"],
      ["card:alpha", "a1"],
      ["card:bravo", "b1"],
      ["waiting", "w1"],
    ]);
    release(released, "card:alpha", before);
    holding = held(engaged, NONE, released, [], state.latest);
    expect(holding).toEqual(NONE);
    state = refresh(state, holding);
    settle(released, state.shown);
    expect(shown(state)).toEqual(AFTER);
    expect(released.size).toBe(0);
  });

  it("keeps a release while a reply shows before the owner's own change arrives", () => {
    // An agent's reply lands while the owner reads an excerpt beside the card, held there; the
    // owner saves an answer. The release shows the reply at once, but the answer is still to come.
    let state = documentState(thread(document(BEFORE), "alpha", 1, 0));
    const engaged = keys("card:alpha");
    const released = new Map<string, number>();
    const tick = () => {
      state = refresh(state, held(engaged, NONE, released, [], state.latest));
      settle(released, state.shown);
    };
    state = moved(state, thread(document([["card:alpha", "a2"]]), "alpha", 1, 1));
    tick();
    expect(shown(state)).toEqual([["card:alpha", "a1"]]);

    release(released, "card:alpha", ownerLines(state.latest, "card:alpha"));
    tick();
    expect(shown(state)).toEqual([["card:alpha", "a2"]]);
    expect(released.get("card:alpha")).toBe(2);

    state = moved(state, thread(document([["card:alpha", "a3"]]), "alpha", 2, 1));
    tick();
    expect(shown(state)).toEqual([["card:alpha", "a3"]]);
    expect(released.size).toBe(0);

    state = moved(state, thread(document([["card:alpha", "a4"]]), "alpha", 2, 2));
    tick();
    expect(shown(state)).toEqual([["card:alpha", "a3"]]);
  });

  it("holds a card still loading its body, though released", () => {
    const released = new Map([["card:alpha", 1]]);
    expect(held(keys("card:alpha"), NONE, released, ["card:alpha"], document(AFTER))).toEqual(
      keys("card:alpha"),
    );
  });

  it("holds a closed draft's card only once the card is gone", () => {
    const drafts = keys("card:bravo");
    let state = moved(documentState(document(BEFORE)), document(AFTER));
    let holding = held(NONE, drafts, new Map(), [], state.latest);
    expect(holding).toEqual(NONE);
    state = refresh(state, holding);
    expect(shown(state)).toEqual(AFTER);

    state = moved(
      state,
      document([
        ["needs", "n3"],
        ["waiting", "w1"],
        ["card:alpha", "a2"],
      ]),
    );
    holding = held(NONE, drafts, new Map(), [], state.latest);
    expect(holding).toEqual(drafts);
    state = refresh(state, holding);
    // A removed card with a draft stays where it was, for the draft to be copied.
    expect(shown(state)).toEqual([
      ["needs", "n3"],
      ["card:bravo", "b1"],
      ["waiting", "w1"],
      ["card:alpha", "a2"],
    ]);
  });
});

describe("a release", () => {
  it("lasts until its card shows the owner's line, or is gone", () => {
    const released = new Map<string, number>();
    release(released, "card:alpha", 0);
    settle(released, document(BEFORE));
    expect(released.size).toBe(1);
    settle(released, thread(document(BEFORE), "bravo", 1, 0));
    expect(released.size).toBe(1);
    settle(released, document([["needs", "n1"]]));
    expect(released.size).toBe(0);
  });

  it("waits for both of two saves before the first shows", () => {
    const released = new Map<string, number>();
    release(released, "card:alpha", 1);
    release(released, "card:alpha", 1);
    expect(released.get("card:alpha")).toBe(3);
    settle(released, thread(document(BEFORE), "alpha", 2, 0));
    expect(released.size).toBe(1);
    release(released, "card:alpha", 3);
    expect(released.get("card:alpha")).toBe(4);
    settle(released, thread(document(BEFORE), "alpha", 4, 0));
    expect(released.size).toBe(0);
  });

  it("is none for a tap the server answers as a repeat", () => {
    const released = new Map<string, number>();
    expect(releasedFrom(true, 2, null)).toBeNull();
    // An open form still holds the card.
    expect(
      held(keys("card:alpha"), NONE, released, [], thread(document(BEFORE), "alpha", 2, 0)),
    ).toEqual(keys("card:alpha"));
    release(released, "card:bravo", 0);
    const before = releasedFrom(true, 1, null);
    if (before !== null) release(released, "card:bravo", before);
    expect(released).toEqual(new Map([["card:bravo", 1]]));
    expect(releasedFrom(false, 1, null)).toBe(1);
  });

  it("of a repeat of an unanswered tap waits past its first sending", () => {
    const first = new Unanswered([words("approved", "Keep it")], "ui-1", 1);
    expect(releasedFrom(true, 2, first)).toBe(1);
    expect(releasedFrom(false, 2, first)).toBe(2);
    const released = new Map<string, number>();
    release(released, "card:alpha", 1);
    settle(released, thread(document(BEFORE), "alpha", 2, 0));
    expect(released.size).toBe(0);
  });
});

const unanswered = (action: string, note: string, tapped: string | null, before: number) =>
  new Unanswered([words(action, note)], tapped, before);

/** `doc` with the card `slug`'s thread holding `lines`, each the owner's verdict and its words. */
function answered(doc: Document, slug: string, lines: [string, Span[]][]): Document {
  const entries = lines.map(([verdict, spans]) => ({
    who: "owner" as const,
    timestamp: null,
    verdict,
    blocks: [{ type: "paragraph" as const, spans }],
  }));
  return { ...doc, threads: { ...doc.threads, [slug]: { blocks: [{ type: "thread", entries }] } } };
}

const said = (text: string): Span[] => [{ type: "text", text }];

describe("a save no reply answered", () => {
  it("is answered by the feed's line of the owner's, which releases its card", () => {
    let state = documentState(thread(document(BEFORE), "alpha", 0, 1));
    const sent = unanswered("approved", "Keep it", "ui-1", ownerLines(state.latest, "card:alpha"));
    const engaged = keys("card:alpha");
    const released = new Map<string, number>();
    state = moved(state, thread(document(BEFORE), "alpha", 0, 2));
    expect(sent.answered(state.latest, "card:alpha")).toBe(false);
    state = moved(state, thread(document(AFTER), "alpha", 1, 2));
    expect(sent.answered(state.latest, "card:bravo")).toBe(false);
    expect(sent.answered(state.latest, "card:alpha")).toBe(true);
    state = refresh(state, held(engaged, NONE, released, [], state.latest));
    expect(shown(state)).toEqual([
      ["needs", "n2"],
      ["card:alpha", "a1"],
      ["card:bravo", "b1"],
      ["waiting", "w1"],
    ]);
    release(released, "card:alpha", sent.before);
    const holding = held(engaged, NONE, released, [], state.latest);
    expect(holding).toEqual(NONE);
    state = refresh(state, holding);
    settle(released, state.shown);
    expect(shown(state)).toEqual(AFTER);
    expect(released.size).toBe(0);
  });

  it("is retried under its tap whatever the words now say", () => {
    const sent = unanswered("approved", "Keep it", "ui-1", 0);
    const stored = "ui-1\napproved\nKeep it";
    expect(tap(sent, stored, "rejected\nOn second thought")).toBe("ui-1");
    expect(tap(sent, null, "approved\nKeep it")).toBe("ui-1");
    expect(tap(null, stored, "approved\nKeep it")).toBe("ui-1");
    expect(tap(null, stored, "approved\nKeep it all")).toBeNull();
    expect(tap(null, null, "approved\nKeep it")).toBeNull();
    expect(tap(null, "", "")).toBeNull();
    expect(tap(null, "ui-1", "ui-1")).toBeNull();
    expect(tap(null, "\nsigned", "signed")).toBeNull();
    expect(tap(unanswered("message", "Hi", null, 0), stored, "rejected\nNo")).toBeNull();
  });

  it("keeps every sending's words under its tap and the first sending's lines", () => {
    const first = unanswered("approved", "Keep it", "ui-1", 1);
    const retry = Unanswered.after(first, words("rejected", "No"), "ui-1", 2);
    expect(retry).toEqual(
      new Unanswered([words("approved", "Keep it"), words("rejected", "No")], "ui-1", 1),
    );
    expect(retry.last()).toEqual(words("rejected", "No"));
    expect(retry.retriedBy("ui-1")).toBe(true);
    expect(retry.retriedBy("ui-2")).toBe(false);
    expect(retry.retriedBy(null)).toBe(false);
    const again = Unanswered.after(retry, words("approved", "Keep it"), "ui-1", 3);
    expect(again.sent).toEqual([words("rejected", "No"), words("approved", "Keep it")]);

    const other = Unanswered.after(first, words("message", "Hi"), null, 2);
    expect(other).toEqual(unanswered("message", "Hi", null, 2));
    expect(other.retriedBy(null)).toBe(false);
    expect(Unanswered.after(null, words("approved", "Keep it"), "ui-3", 4)).toEqual(
      unanswered("approved", "Keep it", "ui-3", 4),
    );
    expect(new Unanswered([], null, 0).last()).toBeNull();
  });

  it("is held as the words the feed's line shows, when the page can tell which", () => {
    const keep = words("approved", "Keep it");
    const why = words("approved", "Keep it, and say why");
    const one = unanswered("approved", "Keep it", "ui-1", 0);
    const other = answered(document(AFTER), "alpha", [["approved", said("Other")]]);
    expect(one.held(other, "card:alpha")).toEqual(keep);

    const two = Unanswered.after(one, why, "ui-1", 0);
    const lines: [Span[], typeof keep | null][] = [
      [said("Keep it"), keep],
      [said("  Keep\n it "), keep],
      [said(`Keep${NBSP}it${IDEOGRAPHIC}`), keep],
      [said("Keep it, and say why"), why],
      [
        [
          { type: "strong", spans: said("Keep") },
          { type: "line-break" },
          { type: "code", text: "it" },
        ],
        keep,
      ],
      [[{ type: "emphasis", spans: [{ type: "component", kind: "x", blocks: [] }] }], null],
      [said("Neither"), null],
      [[{ type: "component", kind: "status", blocks: [] }], null],
    ];
    for (const [line, expected] of lines) {
      const latest = answered(document(AFTER), "alpha", [
        ["rejected", said("An old answer")],
        ["approved", line],
      ]);
      expect(two.held(latest, "card:alpha")).toEqual(expected);
    }
    const rejected = answered(document(AFTER), "alpha", [["rejected", said("Keep it")]]);
    expect(two.held(rejected, "card:alpha")).toBeNull();
    expect(two.held(document(AFTER), "card:alpha")).toBeNull();
    expect(two.held(document(AFTER), "intro")).toBeNull();
    const alike = Unanswered.after(two, words("approved", "Keep  it"), "ui-1", 0);
    const latest = answered(document(AFTER), "alpha", [["approved", said("Keep it")]]);
    expect(alike.held(latest, "card:alpha")).toBeNull();
    const unworded = answered(document(AFTER), "alpha", [["approved", said("Keep it")]]);
    const thread = unworded.threads.alpha?.blocks[0];
    if (thread?.type === "thread" && thread.entries[0] !== undefined) {
      thread.entries[0].blocks.push({ type: "rule" });
    }
    expect(two.held(unworded, "card:alpha")).toBeNull();
    // A line whose verdict the server left out, which the guard reads as none as serde does, is
    // no sending's: each sent one had a verdict.
    const unruled = answered(document(AFTER), "alpha", [["approved", said("Keep it")]]);
    const ruled = unruled.threads.alpha?.blocks[0];
    if (ruled?.type === "thread" && ruled.entries[0] !== undefined) {
      Reflect.deleteProperty(ruled.entries[0], "verdict");
    }
    expect(two.held(unruled, "card:alpha")).toBeNull();
  });
});

describe("an answered save", () => {
  it("clears only the words the server holds", () => {
    const keep = words("approved", "Keep it");
    const why = words("approved", "Keep it, and say why");
    const notSent = "Verdict saved: approved; your change was not sent: send it again to add it";
    const unknown =
      "Verdict saved; the card shows its words: send yours again if they are not there";
    const cases: [typeof keep | null, typeof keep, string, string, boolean, boolean][] = [
      [keep, keep, "Keep it", "Verdict saved: approved", true, true],
      [keep, keep, "Keep it, and more", "Verdict saved: approved", false, false],
      [keep, why, "Keep it, and say why", notSent, false, true],
      [keep, why, "Something else", "Verdict saved: approved", false, false],
      [
        words("rejected", "Keep it, and say why"),
        why,
        "Keep it, and say why",
        "Verdict saved: rejected; your change was not sent: send it again to add it",
        false,
        true,
      ],
      [null, why, "Keep it, and say why", unknown, false, true],
      [null, why, "Something else", unknown, false, false],
    ];
    for (const [heldWords, last, boxed, status, clear, close] of cases) {
      expect(ending(heldWords, last, boxed)).toEqual({ status, clear, close });
    }
    const message = words("message", "Hi");
    expect(ending(message, message, "Hi").status).toBe("Message sent");
    expect(savedText("comment")).toBe("Comment saved");
    expect(ending(message, message, "Hi", true)).toEqual({
      status: "Message held",
      clear: true,
      close: true,
    });
    expect([savedText("comment", true), savedText("approved", true)]).toEqual([
      "Comment held",
      "Verdict saved: approved",
    ]);
  });

  it("of two sendings keeps the words the feed's line does not show", () => {
    const first = unanswered("approved", "Keep it", "ui-1", 0);
    const sent = Unanswered.after(first, words("approved", "Keep it, and say why"), "ui-1", 0);
    const latest = answered(document(AFTER), "alpha", [["approved", said("Keep it")]]);
    expect(sent.answered(latest, "card:alpha")).toBe(true);
    const heldWords = sent.held(latest, "card:alpha");
    expect(heldWords).toEqual(words("approved", "Keep it"));
    const last = sent.last();
    expect(last && ending(heldWords, last, "Keep it, and say why")).toEqual({
      status: "Verdict saved: approved; your change was not sent: send it again to add it",
      clear: false,
      close: true,
    });
  });

  it("answered as a repeat of other words than the tap holds keeps them", () => {
    const why = words("approved", "Keep it, and say why");
    const sent = Unanswered.after(unanswered("approved", "Keep it", "ui-1", 0), why, "ui-1", 0);
    expect(
      tap(sent, "ui-1\napproved\nKeep it, and say why", "approved\nKeep it, and say why"),
    ).toBe("ui-1");
    const end = ending(words("approved", "Keep it"), why, "Keep it, and say why");
    expect(end).toEqual({
      status: "Verdict saved: approved; your change was not sent: send it again to add it",
      clear: false,
      close: true,
    });
    expect(ending(why, why, "Keep it, and say why").clear).toBe(true);
  });

  it("of a draft reloaded under the first tap keeps it when the tap holds other words", () => {
    const stored = "ui-1\napproved\nKeep it, and say why";
    expect(tap(null, stored, "approved\nKeep it, and say why")).toBe("ui-1");
    const end = ending(
      words("approved", "Keep it"),
      words("approved", "Keep it, and say why"),
      "Keep it, and say why",
    );
    expect([end.clear, end.close]).toEqual([false, true]);
    expect(end.status).toContain("your change was not sent");
  });
});

describe("a card's owner lines", () => {
  it("count the owner's entries in its thread's thread blocks alone", () => {
    const doc = thread(document(BEFORE), "alpha", 2, 3);
    expect(ownerLines(doc, "card:alpha")).toBe(2);
    expect(ownerLines(doc, "card:bravo")).toBe(0);
    expect(ownerLines(doc, "alpha")).toBe(0);
    const mixed = { ...doc, threads: { alpha: { blocks: [{ type: "rule" as const }] } } };
    expect(ownerLines(mixed, "card:alpha")).toBe(0);
    expect(ownerLines({ ...doc, threads: {} }, "card:constructor")).toBe(0);
  });
});

describe("a document's bodies", () => {
  it("are its drawn sections with their cards and threads", () => {
    const doc = {
      ...thread(
        document([
          ["intro", "i1"],
          ["card:alpha", "a1"],
          ["card:bravo", "b1"],
        ]),
        "alpha",
        1,
        1,
      ),
      cards: [card("alpha", "pending", "Alpha's whole text"), card("bravo", "approved", "Bravo")],
    };
    const bravo = doc.sections[2];
    if (bravo !== undefined) doc.sections[2] = { ...bravo, blocks: [] };
    const taken = bodies(doc, keys("intro", "card:alpha", "card:bravo", "gone"));
    expect(taken.map((body) => body.section.key)).toEqual(["intro", "card:alpha"]);
    expect(taken[0]).toEqual({ section: doc.sections[0] });
    expect(taken[1]?.card?.text).toBe("Alpha's whole text");
    expect(taken[1]?.thread).toBe(doc.threads.alpha);
    expect(bodies(doc, NONE)).toEqual([]);
    const bare = bodies({ ...doc, cards: [], threads: {} }, keys("card:alpha"));
    expect(bare).toEqual([{ section: doc.sections[1] }]);
  });

  it("drawn into an outline keep its places and states and bring their threads", () => {
    const full = {
      ...thread(
        document([
          ["intro", "i1"],
          ["card:alpha", "a1"],
          ["card:bravo", "b1"],
        ]),
        "alpha",
        2,
        0,
      ),
      cards: [card("alpha", "pending", "Alpha's whole text"), card("bravo", "pending", "Bravo")],
    };
    const kept = bodies(full, keys("intro", "card:alpha", "card:bravo"));
    // Since they were kept the sections moved, alpha was answered and intro went.
    const outline = outlined(full);
    outline.sections = outline.sections
      .slice(1)
      .map((one, at) => (at === 0 ? { ...one, start: 40 } : one));
    outline.cards = outline.cards.map((one, at) =>
      at === 0 ? { ...one, state: "approved" } : one,
    );
    const drawn = drawBodies(outline, kept);
    expect(drawn.sections.map((one) => [one.key, one.start])).toEqual([
      ["card:alpha", 40],
      ["card:bravo", 0],
    ]);
    expect(drawn.sections.every((one) => one.blocks.length > 0)).toBe(true);
    expect(drawn.cards[0]?.state).toBe("approved");
    expect(drawn.cards[0]?.text).toBe("Alpha's whole text");
    expect(drawn.threads.alpha).toBe(full.threads.alpha);
    expect(Object.hasOwn(drawn.threads, "bravo")).toBe(false);
    // A body with no thread takes away one the document held.
    const heldThread = { ...drawn, threads: { ...drawn.threads, bravo: { blocks: [] } } };
    expect(
      Object.hasOwn(drawBodies(heldThread, bodies(drawn, keys("card:bravo"))).threads, "bravo"),
    ).toBe(false);
    // A card body without its card keeps the outline's text.
    const alpha = bodies(drawn, keys("card:alpha")).map(({ section }) => ({ section }));
    expect(drawBodies(outline, alpha).cards[0]?.text).toBe(outline.cards[0]?.text);
    // A card body whose card the outline does not hold changes no card.
    const carded = drawBodies({ ...outline, cards: [] }, kept);
    expect(carded.cards).toEqual([]);
    // A section that is no card's takes its blocks alone.
    const whole = outlined(full);
    const intro = drawBodies(whole, bodies(full, keys("intro")));
    expect(intro.sections.map((one) => one.blocks)).toEqual([full.sections[0]?.blocks, [], []]);
    expect(intro.cards).toEqual(whole.cards);
    expect(intro.threads).toEqual({});
    // Nothing drawn leaves the outline as it was.
    expect(drawBodies(outline, [])).toBe(outline);
  });

  it("are a section's blocks as a fragment", () => {
    const one = section("intro", "i1");
    expect(fragment(one)).toEqual({ blocks: one.blocks });
  });
});

describe("a register's parts", () => {
  it("have edges at each run's ends, absent and repeated parts among them", () => {
    const sections = [null, null, 0, 0, null].map((part) => ({ ...section("", ""), part }));
    expect(sections.map((_, at) => partEdges(sections, at))).toEqual([
      [true, false],
      [false, true],
      [true, false],
      [false, true],
      [true, true],
    ]);
    expect(partEdges([], 0)).toEqual([false, false]);
    expect(partEdges(sections, Number.MAX_SAFE_INTEGER)).toEqual([false, false]);
    expect(partEdges(sections, -1)).toEqual([false, false]);
  });

  it("are known by their form before their bodies are read", () => {
    const outline = { ...section("card:a", ""), blocks: [] };
    const grouped = (group: string) => ({ ...card("a", "proposed", "A"), group });
    for (const group of ["decide", "confirm", "proposed", ""]) {
      expect(isSettled(grouped(group))).toBe(false);
      expect(partForm(outline, grouped(group))).toBe(" doc-part-card");
    }
    for (const group of ["resolved", "rejected"]) {
      expect(isSettled(grouped(group))).toBe(true);
      expect(partForm(outline, grouped(group))).toBe("");
    }
    expect(isSettled(null)).toBe(false);
    // Prose kept from before the outline named its ends, or a card the cards do not name, is
    // known by nothing.
    expect(partForm(section("p", ""), null)).toBe("");
    expect(partForm(outline, null)).toBe("");
    expect(partForm({ ...outline, ends: null }, null)).toBe("");
    // A section drawn with its difference from main sits in a frame of its own.
    const ended = { ...outline, ends: ["paragraph", "table"] as [string, string] };
    for (const kind of ["added", "changed", "removed"] as const) {
      const differs = { ...ended, comparison: { kind } };
      expect(partForm(differs, null)).toBe("");
      expect(partForm(differs, grouped("decide"))).toBe("");
    }
    expect(partForm({ ...outline, comparison: null }, grouped("decide"))).toBe(" doc-part-card");
    expect(partForm(ended, grouped("resolved"))).toBe("");
  });

  it("are known by their first and last blocks before their bodies are read", () => {
    const form = (first: string, last: string, open = false) =>
      partForm(
        { ...section("lamps", ""), blocks: [], ends: [first, last] },
        open ? { ...card("lamps", "proposed", "L"), group: "decide" } : null,
      );
    for (const text of ["paragraph", "list", "thread"]) {
      expect(form(text, "code")).toBe(" doc-part-opens-text");
      expect(form("code", text)).toBe(" doc-part-closes");
    }
    for (const box of ["table", "callout"]) {
      expect(form(box, "quote")).toBe(" doc-part-opens-box");
      expect(form("quote", box)).toBe(" doc-part-closes");
    }
    for (const heading of ["heading-2", "heading-3", "heading-4"]) {
      expect(form(heading, heading)).toBe(" doc-part-closes");
    }
    // The title, a lesser heading and what is not text close nothing and open nothing.
    for (const other of ["heading-1", "heading-5", "code", "quote", "image", "media", "figure"]) {
      expect(form(other, other)).toBe("");
    }
    expect(form("card", "card", true)).toBe(" doc-part-card");
    expect(form("paragraph", "table", true)).toBe(
      " doc-part-card doc-part-opens-text doc-part-closes",
    );
    expect(form("callout", "paragraph")).toBe(" doc-part-opens-box doc-part-closes");
  });
});

describe("a folder's pill", () => {
  const folder = (docs: number, entries: [string, number][]): Folder => ({
    path: "docs",
    title: "Docs",
    crumbs: [],
    docs,
    intro: null,
    entries: entries.map(([kind, count], n) => ({
      kind,
      path: `docs/${n}`,
      title: `Entry ${n}`,
      docs: count,
      waiting: 0,
      summary: "",
    })),
  });

  it("adds up with its listing", () => {
    // This repository's docs/: Analysis (15), History (2, not counted) and ten documents.
    const entries: [string, number][] = [
      ["folder", 15],
      ["folder", 2],
      ...Array.from({ length: 10 }, (): [string, number] => ["doc", 0]),
    ];
    expect(folderPill(folder(25, entries))).toBe("25 docs · 2 in history");
    expect(folderPill(folder(27, entries))).toBe("27 docs");
    expect(
      folderPill(
        folder(2, [
          ["doc", 0],
          ["doc", 0],
          ["media", 0],
        ]),
      ),
    ).toBe("2 docs");
    expect(folderPill(folder(1, [["doc", 0]]))).toBe("1 doc");
    expect(
      folderPill(
        folder(1, [
          ["doc", 0],
          ["folder", 3],
        ]),
      ),
    ).toBe("1 doc · 3 in history");
    expect(folderPill(folder(0, []))).toBe("0 docs");
    // A count above the listing says the count alone.
    expect(folderPill(folder(4, [["doc", 0]]))).toBe("4 docs");
  });
});

describe("a document's related tasks", () => {
  /** A task whose title is never its slug, so only a test of the slug tells a verdicts' task. */
  const task = (slug: string, status: string, title = "Answer the owner on lamp-a-pic") => ({
    slug,
    title,
    status,
  });

  it("answer verdicts as crates/symphony-api tells them", () => {
    for (const [slug, answers] of CASES.answersVerdicts) {
      expect(answersVerdicts(task(slug, "ready")), slug).toBe(answers);
    }
  });

  it("are the open, then the finished, in the board's order, with no verdicts' task", () => {
    const tasks = [
      task("app/a", "done"),
      task("app/b", "in_progress"),
      task("verdicts/2026-11-11-1244", "ready"),
      task("app/c", "cancelled"),
      task("app/d", "ready"),
    ];
    expect(related(tasks)).toEqual({
      open: [task("app/b", "in_progress"), task("app/d", "ready")],
      finished: [task("app/a", "done"), task("app/c", "cancelled")],
    });
    expect(related([])).toEqual({ open: [], finished: [] });
  });

  it("are told from a verdicts' task by its slug's area, never by its title", () => {
    const tasks: DocumentTask[] = [
      task("verdicts/2026-11-11-1244", "done", "The cache"),
      task("perf/client-cache", "ready"),
      task("verdicts/2026-11-11-1742", "ready"),
      task("verdict/x", "ready", "verdicts/x"),
      task("app/verdicts/x", "ready"),
    ];
    expect(related(tasks).open.map((one) => one.slug)).toEqual([
      "perf/client-cache",
      "verdict/x",
      "app/verdicts/x",
    ]);
    expect(related(tasks).finished).toEqual([]);
  });
});

describe("a text's words", () => {
  it("are split at Unicode's white space, as Rust splits them", () => {
    expect(collapse(`  a\tb${NEL}c${NBSP}d${LS}e${IDEOGRAPHIC} `)).toBe("a b c d e");
    expect(collapse(`a${BOM}b`)).toBe(`a${BOM}b`);
    expect(collapse("")).toBe("");
  });

  it("are kept under any key", () => {
    const record: Record<string, number> = {};
    put(record, "__proto__", 1);
    expect(Object.keys(record)).toEqual(["__proto__"]);
  });
});
