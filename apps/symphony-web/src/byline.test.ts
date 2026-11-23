import { describe, expect, it } from "vitest";
import { byline } from "./byline";
import type { Block, ThreadEntry } from "./wire";

const paragraph = (text: string): Block => ({ type: "paragraph", spans: [{ type: "text", text }] });

/** A first span styled `class`, holding `spans`. */
const styled = (cls: string, spans: unknown[]): Block =>
  ({
    type: "paragraph",
    spans: [{ type: "styled", tag: "span", presentation: { class: cls }, spans }],
  }) as Block;

const entry = (blocks: Block[]): ThreadEntry => ({
  who: "owner",
  timestamp: "2026-11-23 18:00",
  verdict: "approved",
  blocks,
});

describe("a thread entry's name", () => {
  it("is its first block's, a span classed entry-by holding the name, and the rest are its words", () => {
    const ana = styled("entry-by", [{ type: "text", text: "ana" }]);
    expect(byline(entry([ana, paragraph("A")]))).toEqual(["ana", [paragraph("A")]]);
    expect(byline(entry([ana]))).toEqual(["ana", []]);
  });

  it("is none for the owner's and an agent's line, and for a first span of another kind", () => {
    expect(byline(entry([paragraph("Warm?")]))).toEqual([null, [paragraph("Warm?")]]);
    expect(byline(entry([]))).toEqual([null, []]);
    const other = styled("x", [{ type: "text", text: "ana" }]);
    expect(byline(entry([other]))).toEqual([null, [other]]);
    // A name span holding more than one text, or none, is the entry's own words.
    const two = styled("entry-by", [
      { type: "text", text: "a" },
      { type: "text", text: "b" },
    ]);
    expect(byline(entry([two]))).toEqual([null, [two]]);
    const code = styled("entry-by", [{ type: "code", text: "ana" }]);
    expect(byline(entry([code]))).toEqual([null, [code]]);
    const many: Block = {
      type: "paragraph",
      spans: [
        { type: "text", text: "a" },
        { type: "text", text: "b" },
      ],
    };
    expect(byline(entry([many]))).toEqual([null, [many]]);
  });
});
