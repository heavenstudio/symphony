import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Asks, AskThread, concerns, selection } from "./asks";
import { STALE } from "./shared";
import { memoryStorage } from "./testing";
import type { Ask } from "./wire";
import { CHECKING } from "./words";

let storage: Storage;

beforeEach(() => {
  storage = memoryStorage();
  vi.stubGlobal("localStorage", storage);
});

afterEach(() => {
  STALE.set(false);
  vi.unstubAllGlobals();
});

describe("what the feed hears of the asks", () => {
  it("concerns a document when the stream opened again, the recipient changed or its thread moved", () => {
    expect(concerns(true, "docs/a.md")).toBe(true);
    expect(concerns({ recipient: null }, "docs/a.md")).toBe(true);
    expect(concerns({ threads: [{ path: "docs/b.md" }, { path: "docs/a.md" }] }, "docs/a.md")).toBe(
      true,
    );
  });

  it("is no concern of a document none of its threads is about, or that it cannot read", () => {
    for (const moved of [
      false,
      null,
      undefined,
      "docs/a.md",
      {},
      { threads: [{ path: "docs/b.md" }] },
      { threads: "docs/a.md" },
    ]) {
      expect(concerns(moved, "docs/a.md")).toBe(false);
    }
  });
});

/** A page's element, as `selection` reads it: its closest matches and its headings. */
class Element {
  id = "";
  constructor(
    readonly inside: string[] = [],
    readonly headings: Element[] = [],
    readonly order = 0,
  ) {}
  get parentElement() {
    return this;
  }
  closest(css: string) {
    return this.inside.some((name) => css.includes(name)) ? this : null;
  }
  querySelectorAll() {
    return this.headings;
  }
  contains(node: Element) {
    return node === this;
  }
  compareDocumentPosition(node: Element) {
    return node.order > this.order ? 4 : 2;
  }
}

/** A heading `id` at `order` in the document. */
const heading = (id: string, order: number) => Object.assign(new Element([], [], order), { id });

describe("the passage a question is about", () => {
  beforeEach(() => {
    vi.stubGlobal("Element", Element);
    vi.stubGlobal("Node", { DOCUMENT_POSITION_FOLLOWING: 4 });
    vi.stubGlobal("location", { hash: "#intro" });
  });

  const picked = (text: string, node: unknown) =>
    vi.stubGlobal("getSelection", () => ({ toString: () => text, anchorNode: node }));

  it("is the address's section when nothing is selected", () => {
    vi.stubGlobal("getSelection", () => null);
    expect(selection()).toEqual(["intro", ""]);
    picked("", null);
    expect(selection()).toEqual(["intro", ""]);
  });

  it("is the quote under the last heading before it", () => {
    const main = new Element(
      ["main"],
      [heading("intro", 1), heading("plan", 3), heading("end", 9)],
    );
    const node = new Element(["main"], [], 5);
    node.closest = (css) => (css === "main" ? main : null);
    picked("the passage", node);
    expect(selection()).toEqual(["plan", "the passage"]);
    // A heading itself selected is its own section.
    const inHeading = heading("plan", 3);
    inHeading.closest = (css) => (css === "main" ? main : null);
    main.headings[1] = inHeading;
    picked("Plan", inHeading);
    expect(selection()).toEqual(["plan", "Plan"]);
  });

  it("is at most 4000 characters, a character never cut in half", () => {
    const main = new Element(["main"], []);
    const node = new Element(["main"], [], 5);
    node.closest = (css) => (css === "main" ? main : null);
    picked("😀".repeat(4001), node);
    const [anchor, quote] = selection();
    expect(anchor).toBe("intro");
    expect([...quote]).toHaveLength(4000);
  });

  it("is never one outside the document or in a form", () => {
    picked("typed words", new Element([".asks"]));
    expect(selection()).toEqual(["", ""]);
    picked("the header", new Element([]));
    expect(selection()).toEqual(["", ""]);
    // A text node's element is its parent.
    picked("text", { parentElement: null });
    expect(selection()).toEqual(["intro", ""]);
  });
});

const ask = (overrides: Partial<Ask> = {}): Ask => ({
  id: 7,
  path: "docs/a.md",
  anchor: "plan",
  quote: "the passage",
  question: "Why?",
  asked: 1,
  updated: 2,
  answers: [],
  messages: [
    {
      seq: 1,
      kind: "question",
      by: "owner",
      text: "Why?",
      at: 1,
      blocks: [{ type: "paragraph", spans: [{ type: "text", text: "Why this way?" }] }],
    },
    { seq: 2, kind: "answer", by: "teal-lynx", text: "", at: 2 },
  ],
  status: "open",
  ...overrides,
});

describe("a document's questions", () => {
  it("offer a new question, its draft kept on this device", () => {
    const empty = renderToStaticMarkup(<Asks path="docs/a.md" scope="" />);
    expect(empty).toContain(
      '<section class="asks" id="asks" aria-label="Questions about this document"><h2>Ask about this document</h2>',
    );
    expect(empty).toContain("Nobody is registered to receive chat. Your question is queued.");
    expect(empty).toContain(" Questions go after a day without activity.");
    expect(empty).toContain("Use selected passage");
    expect(empty).not.toContain("Clear passage");
    expect(empty).toContain('<form data-attach="review-ask:/docs/a.md">');
    expect(empty).toContain('<label for="new-ask">Question</label>');
    expect(empty).toMatch(/<button[^>]*type="submit"[^>]*disabled=""[^>]*>Ask<\/button>/);
    storage.setItem("review-ask:@task/docs/a.md", "A typed question");
    const drafted = renderToStaticMarkup(<Asks path="docs/a.md" scope="@task" />);
    expect(drafted).toContain(">A typed question</textarea>");
    expect(drafted).toMatch(/<button[^>]*type="submit"[^>]*>Ask<\/button>/);
    expect(drafted).not.toMatch(/<button[^>]*type="submit"[^>]*disabled=""/);
  });

  it("wait to be sent while the page may be out of date", () => {
    storage.setItem("review-ask:/docs/a.md", "A typed question");
    STALE.set(true);
    const html = renderToStaticMarkup(<Asks path="docs/a.md" scope="" />);
    expect(html).toMatch(new RegExp(`<button[^>]*disabled=""[^>]*>${CHECKING}</button>`));
  });

  it("are each a thread: its place, its quote, its messages and a follow-up", () => {
    const html = renderToStaticMarkup(<AskThread ask={ask()} scope="" onSent={() => {}} />);
    expect(html).toContain('<article class="ask-thread card" id="ask-7" data-deferred="false">');
    expect(html).toContain('<p class="muted">Question 7 · open</p>');
    expect(html).toContain('<a href="#plan">#plan</a>');
    expect(html).toContain("<blockquote>the passage</blockquote>");
    expect(html).toContain('<div class="ask-message"><p class="by">owner</p>');
    expect(html).toContain("Why this way?");
    expect(html).toContain('<div class="ask-message"><p class="by">teal-lynx</p>');
    expect(html).toContain('<form data-attach="review-ask-reply:7">');
    expect(html).toContain('<label for="reply-7">Follow up</label>');
    expect(html).toMatch(/disabled=""[^>]*>Send follow-up<\/button>/);
  });

  it("about a whole document show no place and no quote", () => {
    storage.setItem("review-ask-reply:7", "More, please");
    const html = renderToStaticMarkup(
      <AskThread ask={ask({ anchor: "", quote: "" })} scope="" onSent={() => {}} />,
    );
    expect(html).not.toContain('href="#');
    expect(html).not.toContain("<blockquote>");
    expect(html).toContain(">More, please</textarea>");
    expect(html).not.toMatch(/disabled=""[^>]*>Send follow-up<\/button>/);
  });
});
