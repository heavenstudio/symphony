import { describe, expect, it } from "vitest";
import { byline, entryWriter } from "./byline";
import { drawnName } from "./project";
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

const stefano = { name: "stefano", shown: "Stefano" };

describe("a person's name as a line draws it", () => {
  it("is the person at the machine's shown name under owner and their short name, titled with it", () => {
    expect(drawnName("owner", stefano)).toEqual({ text: "Stefano", title: "owner" });
    expect(drawnName("stefano", stefano)).toEqual({ text: "Stefano", title: "stefano" });
  });

  it("is any other name, and every name while the person at the machine gave none, as written", () => {
    expect(drawnName("ana", stefano)).toEqual({ text: "ana" });
    expect(drawnName("Stefano", stefano)).toEqual({ text: "Stefano" });
    for (const machine of [null, undefined]) {
      expect(drawnName("owner", machine)).toEqual({ text: "owner" });
      expect(drawnName("stefano", machine)).toEqual({ text: "stefano" });
    }
  });

  it("carries no title where their shown name is the name the line carries", () => {
    const plain = { name: "stefano", shown: "stefano" };
    expect(drawnName("stefano", plain)).toEqual({ text: "stefano" });
    expect(drawnName("owner", plain)).toEqual({ text: "stefano", title: "owner" });
  });
});

describe("a card entry's writer", () => {
  const named = styled("entry-by", [{ type: "text", text: "stefano" }]);

  it("is the owner's line drawn as the person at the machine once they gave a name, and none before", () => {
    const owners = entry([paragraph("Warm?")]);
    expect(entryWriter(owners, stefano)).toEqual([
      { text: "Stefano", title: "owner" },
      [paragraph("Warm?")],
    ]);
    expect(entryWriter(owners, null)).toEqual([null, [paragraph("Warm?")]]);
  });

  it("is the name a person's line carries, drawn by the one rule", () => {
    expect(entryWriter(entry([named, paragraph("A")]), stefano)).toEqual([
      { text: "Stefano", title: "stefano" },
      [paragraph("A")],
    ]);
    expect(entryWriter(entry([named]), undefined)).toEqual([{ text: "stefano" }, []]);
  });

  it("is none for an agent's line, named or not", () => {
    const agents: ThreadEntry = { ...entry([paragraph("Done")]), who: "agent" };
    expect(entryWriter(agents, stefano)).toEqual([null, [paragraph("Done")]]);
  });
});
