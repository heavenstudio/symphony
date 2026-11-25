import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  HeldContext,
  HeldOne,
  HeldUnder,
  type HeldValue,
  passOn,
  placeHeld,
  readHeld,
  readHeldComment,
} from "./document-held";
import type { HeldComment, Section } from "./wire";

const CALLOUT: HeldComment = {
  id: 4,
  author: "ana",
  kind: "question",
  text: "Too many lamps\nShow the thirty that matter.",
  card: null,
  key: "section:topic:1",
  line: 3,
};
const MESSAGE: HeldComment = {
  id: 5,
  author: "ana",
  kind: "message",
  text: "And why?",
  card: "one",
  key: null,
  line: null,
};

/** A section of the page, `key` from `start` to `end`. */
const section = (key: string, start: number, end: number) =>
  ({ key, start, end, hash: "", blocks: [] }) as unknown as Section;

describe("a document's held comments, as the server sends them", () => {
  it("reads each with its place, a card's message or a callout's section", () => {
    const sent = {
      held: [
        {
          id: 4,
          author: "ana",
          kind: "question",
          text: CALLOUT.text,
          key: "section:topic:1",
          line: 3,
        },
        { id: 5, author: "ana", kind: "message", text: "And why?", card: "one" },
      ],
      passes: true,
    };
    expect(readHeld(sent)).toEqual({ held: [CALLOUT, MESSAGE], passes: true });
    expect(readHeld({ held: [], passes: false })).toEqual({ held: [], passes: false });
  });

  it("reads none where a field it draws is not one, or the read is not this one", () => {
    for (const broken of [
      { ...CALLOUT, id: 1.5 },
      { ...CALLOUT, id: "4" },
      { ...CALLOUT, author: null },
      { ...CALLOUT, kind: 3 },
      { ...CALLOUT, text: undefined },
      { ...CALLOUT, card: 7 },
      { ...CALLOUT, key: [] },
      { ...CALLOUT, line: "3" },
      null,
    ]) {
      expect(readHeldComment(broken)).toBeNull();
    }
    for (const broken of [
      { held: [{ ...CALLOUT, id: "4" }], passes: true },
      { held: {}, passes: true },
      { held: [], passes: "yes" },
      {},
      null,
    ]) {
      expect(readHeld(broken)).toBeNull();
    }
    // A field a newer server adds changes nothing.
    expect(readHeldComment({ ...CALLOUT, at: 1 })).toEqual(CALLOUT);
  });
});

describe("where each held comment is drawn", () => {
  const sections = [
    section("section:topic:0", 1, 1),
    section("section:topic:1", 3, 3),
    section("card:one", 6, 6),
    section("section:lamps:1", 9, 12),
  ];

  it("goes under its card, else the section of its key, else the one ending on its line", () => {
    const moved = { ...CALLOUT, id: 6, key: "section:gone:2", line: 12 };
    const placed = placeHeld([CALLOUT, MESSAGE, moved], sections);
    expect([...placed.entries()]).toEqual([
      ["section:topic:1", [CALLOUT]],
      ["card:one", [MESSAGE]],
      ["section:lamps:1", [moved]],
    ]);
  });

  it("goes at the document's end where nothing of the page holds its place, in the order sent", () => {
    const lost = { ...CALLOUT, id: 7, key: "section:gone:1", line: 40 };
    const cardless = { ...MESSAGE, id: 8, card: "gone" };
    const placed = placeHeld([lost, cardless, { ...lost, id: 9, key: null, line: null }], sections);
    expect(placed.get("")?.map((comment) => comment.id)).toEqual([7, 8, 9]);
    expect(placed.size).toBe(1);
  });
});

describe("a held comment drawn", () => {
  const none = async () => {};

  it("says who wrote it, its kind, held and its words, with Pass on for one who may", () => {
    const html = renderToStaticMarkup(<HeldOne comment={CALLOUT} passes pass={none} />);
    expect(html).toContain('id="held-4"');
    expect(html).toContain("<b>ana</b>");
    expect(html).toContain('<span class="held-kind">question</span>');
    expect(html).toContain('<b class="entry-held">held</b>');
    expect(html).toContain("Too many lamps\nShow the thirty that matter.");
    expect(html).toContain("Pass on");
  });

  it("draws no Pass on for one who may not, and no kind for a card's message", () => {
    const html = renderToStaticMarkup(<HeldOne comment={MESSAGE} passes={false} pass={none} />);
    expect(html).not.toContain("Pass on");
    expect(html).not.toContain("held-kind");
    expect(html).toContain("And why?");
  });

  it("draws a section's own and nothing where none goes under it", () => {
    const value: HeldValue = {
      placed: new Map([["section:topic:1", [CALLOUT]]]),
      passes: false,
      pass: none,
    };
    const under = (key: string) =>
      renderToStaticMarkup(
        <HeldContext.Provider value={value}>
          <HeldUnder sectionKey={key} />
        </HeldContext.Provider>,
      );
    expect(under("section:topic:1")).toContain('id="held-4"');
    expect(under("card:one")).toBe("");
    expect(renderToStaticMarkup(<HeldUnder sectionKey="" />)).toBe("");
  });

  it("says nothing once passed, and a refusal in the server's words", async () => {
    const asked: number[] = [];
    expect(await passOn(4, async (id) => void asked.push(id))).toBe("");
    expect(asked).toEqual([4]);
    const refused = async () => {
      throw new Error("comment #4 was passed on already, by bo: nothing was written");
    };
    expect(await passOn(4, refused)).toBe(
      "comment #4 was passed on already, by bo: nothing was written",
    );
  });
});
