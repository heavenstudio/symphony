import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  type AskDraft,
  AskForm,
  Asks,
  AskThread,
  concerns,
  type Draft,
  RETRY_MS,
  readAsksAgain,
  readAsksUntilAnswered,
  selection,
  sendDraft,
} from "./asks";
import { STALE } from "./shared";
import { asking, fire, memoryStorage, reply, server } from "./testing";
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

describe("a draft's form", () => {
  const KEY = "review-ask-reply:7";

  /** A draft of `text` with no file, as its hook gives one; `over` says otherwise. */
  function aDraft(text = "", over: Partial<AskDraft> = {}): AskDraft {
    return {
      key: KEY,
      text,
      sending: false,
      trouble: null,
      stale: false,
      attach: {
        zone: KEY,
        files: [],
        pending: [],
        trouble: null,
        busy: false,
        empty: true,
        remove: () => {},
        compose: (words: string) => words,
        clear: () => {},
      },
      mount: new AbortController().signal,
      setText: vi.fn<(text: string) => void>(),
      setSending: () => {},
      setTrouble: () => {},
      onSent: () => {},
      ...over,
    };
  }

  /** The form of `draft`, labelled Follow up, whose submit says Send. */
  const form = (draft: AskDraft, onSend = async () => {}) => (
    <AskForm draft={draft} id="reply-7" label="Follow up" word="Send" onSend={onSend} />
  );

  it("draws its box under its label by its id, and its submit by its word", () => {
    const html = renderToStaticMarkup(form(aDraft("More, please")));
    expect(html).toContain('<form data-attach="review-ask-reply:7">');
    expect(html).toContain('<label for="reply-7">Follow up</label>');
    expect(html).toMatch(/<textarea[^>]*id="reply-7"[^>]*maxLength="4000"[^>]*>More, please</);
    expect(html).toMatch(/<button[^>]*type="submit"[^>]*>Send<\/button>/);
    expect(html).not.toMatch(/<button[^>]*type="submit"[^>]*disabled=""/);
    expect(html).not.toContain('role="alert"');
  });

  it("cannot be sent with nothing to send, and needs words while it holds no file", () => {
    const empty = renderToStaticMarkup(form(aDraft("  ")));
    expect(empty).toMatch(/<textarea[^>]*required=""/);
    expect(empty).toMatch(/<button[^>]*type="submit"[^>]*disabled=""[^>]*>Send<\/button>/);
    const draft = aDraft("");
    const filed = renderToStaticMarkup(
      form({ ...draft, attach: { ...draft.attach, empty: false } }),
    );
    expect(filed).not.toMatch(/<textarea[^>]*required=""/);
    expect(filed).not.toMatch(/<button[^>]*type="submit"[^>]*disabled=""/);
  });

  it("waits while it is sent, a file uploads or the page may be out of date", () => {
    const sending = renderToStaticMarkup(form(aDraft("More", { sending: true })));
    expect(sending).toMatch(/<textarea[^>]*disabled=""/);
    expect(sending).toMatch(/<button[^>]*type="submit"[^>]*disabled=""[^>]*>Sending…<\/button>/);
    const stale = renderToStaticMarkup(form(aDraft("More", { stale: true })));
    expect(stale).not.toMatch(/<textarea[^>]*disabled=""/);
    expect(stale).toMatch(
      new RegExp(`<button[^>]*type="submit"[^>]*disabled=""[^>]*>${CHECKING}</button>`),
    );
    const draft = aDraft("More");
    const busy = renderToStaticMarkup(form({ ...draft, attach: { ...draft.attach, busy: true } }));
    expect(busy).toMatch(/<button[^>]*type="submit"[^>]*disabled=""[^>]*>Send<\/button>/);
  });

  it("says what a send that failed said", () => {
    const html = renderToStaticMarkup(form(aDraft("More", { trouble: "The board is busy." })));
    expect(html).toContain('<p role="alert">The board is busy.</p>');
  });

  it("sends on its submit, the browser's own left out", () => {
    const send = vi.fn(async () => {});
    const drawn = AskForm({ draft: aDraft("More"), id: "r", label: "L", word: "W", onSend: send });
    const event = { preventDefault: vi.fn() };
    (drawn.props.onSubmit as (event: unknown) => void)(event);
    expect(event.preventDefault).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("keeps each change of its words in the draft and on this device", () => {
    const draft = aDraft("Mor");
    const drawn = AskForm({ draft, id: "r", label: "L", word: "W", onSend: async () => {} });
    fire(drawn, (element) => element.props.id === "r", "onChange", { target: { value: "More" } });
    expect(draft.setText).toHaveBeenCalledWith("More");
    expect(storage.getItem(KEY)).toBe("More");
  });
});

const ENDPOINT = "/api/asks?format=blocks&path=docs%2Fa.md";

/** What a read of the asks showed, each in the order it was said. */
const shownOf = () => ({
  asks: vi.fn<(asks: Ask[]) => void>(),
  recipient: vi.fn<(recipient: string | null) => void>(),
  trouble: vi.fn<(trouble: string | null) => void>(),
});

describe("a read of a document's asks", () => {
  afterEach(() => vi.useRealTimers());

  it("shows the threads and the chat's recipient once the server answers", async () => {
    const asked = server();
    const shown = shownOf();
    const done = readAsksUntilAnswered(ENDPOINT, new AbortController().signal, shown);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe(ENDPOINT);
    asked[0]?.answer(reply(200, { asks: [ask()], recipient: "teal-lynx" }));
    await done;
    expect(shown.asks).toHaveBeenCalledWith([ask()]);
    expect(shown.recipient).toHaveBeenCalledWith("teal-lynx");
    expect(shown.trouble.mock.calls).toEqual([[null]]);
  });

  it("says why each read failed and asks again 3 s later, until one answers", async () => {
    vi.useFakeTimers();
    const asked = server();
    const shown = shownOf();
    const done = readAsksUntilAnswered(ENDPOINT, new AbortController().signal, shown);
    await vi.advanceTimersByTimeAsync(0);
    asked[0]?.answer(reply(500, { error: "the disk is full" }));
    await vi.advanceTimersByTimeAsync(RETRY_MS - 1);
    expect(shown.trouble).toHaveBeenLastCalledWith(expect.stringContaining("the disk is full"));
    expect(asked).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(asked).toHaveLength(2);
    asked[1]?.answer(reply(200, { asks: "none" }));
    await vi.advanceTimersByTimeAsync(0);
    expect(shown.trouble).toHaveBeenLastCalledWith("The server's asks could not be read");
    await vi.advanceTimersByTimeAsync(RETRY_MS);
    expect(asked).toHaveLength(3);
    asked[2]?.answer(reply(200, { asks: [] }));
    await done;
    expect(shown.asks).toHaveBeenCalledWith([]);
    // A reply naming no recipient is a chat nobody receives.
    expect(shown.recipient).toHaveBeenCalledWith(null);
    expect(shown.trouble).toHaveBeenLastCalledWith(null);
    // The read that answered waits for the feed's news: no retry, and no read's own deadline, is
    // left set.
    expect(vi.getTimerCount()).toBe(0);
  });

  it("leaves no timer set once its first read answered", async () => {
    vi.useFakeTimers();
    const asked = server();
    const done = readAsksUntilAnswered(ENDPOINT, new AbortController().signal, shownOf());
    await vi.advanceTimersByTimeAsync(0);
    asked[0]?.answer(reply(200, { asks: [ask()], recipient: "teal-lynx" }));
    await done;
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(RETRY_MS);
    expect(asked).toHaveLength(1);
  });

  it("shows and asks nothing more once its page left, while it read or while it waited", async () => {
    vi.useFakeTimers();
    const asked = server();
    for (const answer of [reply(200, { asks: [ask()] }), reply(500, { error: "gone" })]) {
      const shown = shownOf();
      const left = new AbortController();
      const done = readAsksUntilAnswered(ENDPOINT, left.signal, shown);
      await vi.advanceTimersByTimeAsync(0);
      left.abort();
      asked.at(-1)?.answer(answer);
      await done;
      expect(shown.asks).not.toHaveBeenCalled();
      expect(shown.trouble).not.toHaveBeenCalled();
    }
    const shown = shownOf();
    const left = new AbortController();
    const done = readAsksUntilAnswered(ENDPOINT, left.signal, shown);
    await vi.advanceTimersByTimeAsync(0);
    asked.at(-1)?.answer(reply(500, { error: "the disk is full" }));
    await vi.advanceTimersByTimeAsync(1_000);
    left.abort();
    await done;
    await vi.advanceTimersByTimeAsync(RETRY_MS);
    expect(asked).toHaveLength(3);
    expect(shown.trouble).toHaveBeenCalledTimes(1);
  });
});

describe("a read of the asks after a send", () => {
  it("shows the threads it read, and nothing when it fails or its page left", async () => {
    const asked = server();
    const shown = vi.fn<(asks: Ask[]) => void>();
    const read = readAsksAgain(ENDPOINT, new AbortController().signal, shown);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, { asks: [ask()] }));
    await read;
    expect(shown).toHaveBeenCalledWith([ask()]);
    shown.mockClear();
    for (const [at, answer] of [
      reply(500, { error: "the disk is full" }),
      reply(200, { asks: "none" }),
    ].entries()) {
      const again = readAsksAgain(ENDPOINT, new AbortController().signal, shown);
      await asking(asked, at + 2);
      asked[at + 1]?.answer(answer);
      await expect(again).resolves.toBeUndefined();
    }
    const left = new AbortController();
    const away = readAsksAgain(ENDPOINT, left.signal, shown);
    await asking(asked, 4);
    left.abort();
    asked[3]?.answer(reply(200, { asks: [ask()] }));
    await away;
    expect(shown).not.toHaveBeenCalled();
  });
});

describe("a composer's send", () => {
  const KEY = "review-ask:/docs/a.md";

  /** A composer holding `text`, its files none unless `over` says, and what it set. */
  function composer(text = "Why this way?", over: Partial<Pick<Draft, "sending" | "attach">> = {}) {
    const left = new AbortController();
    const draft = {
      key: KEY,
      text,
      sending: false,
      attach: { busy: false, empty: true, compose: (words: string) => words, clear: vi.fn() },
      mount: left.signal,
      setText: vi.fn<(text: string) => void>(),
      setSending: vi.fn<(sending: boolean) => void>(),
      setTrouble: vi.fn<(trouble: string | null) => void>(),
      onSent: vi.fn(),
      ...over,
    };
    return { draft, left };
  }

  const question = (words: string) => ({ path: "docs/a.md", question: words, anchor: "plan" });

  it("posts its words, then empties the box and the draft kept and reads the asks again", async () => {
    const asked = server();
    storage.setItem(KEY, "Why this way?");
    const { draft } = composer();
    const done = sendDraft(draft, "/api/ask", question);
    expect(draft.setSending.mock.calls).toEqual([[true]]);
    expect(draft.setTrouble.mock.calls).toEqual([[null]]);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/ask");
    expect(asked[0]?.body).toEqual(question("Why this way?"));
    asked[0]?.answer(reply(200, { id: 8 }));
    await done;
    expect(draft.setText).toHaveBeenCalledWith("");
    expect(draft.attach.clear).toHaveBeenCalledTimes(1);
    expect(storage.getItem(KEY)).toBe("");
    expect(draft.onSent).toHaveBeenCalledTimes(1);
    expect(draft.setSending).toHaveBeenLastCalledWith(false);
  });

  it("sends files with no words, their lines composed into the words", async () => {
    const asked = server();
    const compose = (words: string) => `${words}File: /uploads/a.png`;
    const files = { busy: false, empty: false, compose, clear: vi.fn() };
    const done = sendDraft(composer("", { attach: files }).draft, "/api/ask", question);
    await asking(asked, 1);
    expect(asked[0]?.body).toMatchObject({ question: "File: /uploads/a.png" });
    asked[0]?.answer(reply(200, {}));
    await done;
  });

  it("sends nothing while one is on its way, the page may be out of date, a file uploads, or it holds nothing", async () => {
    const asked = server();
    const uploading = {
      busy: true,
      empty: false,
      compose: (words: string) => words,
      clear: vi.fn(),
    };
    const held = [
      composer("Why?", { sending: true }),
      composer(" \n"),
      composer("Why?", { attach: uploading }),
    ];
    for (const { draft } of held) await sendDraft(draft, "/api/ask", question);
    STALE.set(true);
    const stale = composer();
    await sendDraft(stale.draft, "/api/ask", question);
    for (const { draft } of [...held, stale]) {
      expect(draft.setSending).not.toHaveBeenCalled();
      expect(draft.setTrouble).not.toHaveBeenCalled();
    }
    await asking(asked, 0);
  });

  it("says why a send failed, the draft kept", async () => {
    const asked = server();
    storage.setItem(KEY, "Why this way?");
    const { draft } = composer();
    const done = sendDraft(draft, "/api/ask", question);
    await asking(asked, 1);
    asked[0]?.answer(reply(400, { error: "the question is too long" }));
    await done;
    expect(draft.setTrouble).toHaveBeenLastCalledWith(
      expect.stringContaining("the question is too long"),
    );
    expect(draft.setText).not.toHaveBeenCalled();
    expect(draft.onSent).not.toHaveBeenCalled();
    expect(storage.getItem(KEY)).toBe("Why this way?");
    expect(draft.setSending).toHaveBeenLastCalledWith(false);
  });

  it("sets nothing more once its composer left while the server answered", async () => {
    const asked = server();
    for (const [at, answer] of [reply(200, {}), reply(500, { error: "gone" })].entries()) {
      storage.setItem(KEY, "Why this way?");
      const { draft, left } = composer();
      const done = sendDraft(draft, "/api/ask", question);
      await asking(asked, at + 1);
      left.abort();
      asked[at]?.answer(answer);
      await done;
      expect(draft.setSending.mock.calls).toEqual([[true]]);
      expect(draft.setTrouble.mock.calls).toEqual([[null]]);
      expect(draft.setText).not.toHaveBeenCalled();
      expect(draft.onSent).not.toHaveBeenCalled();
      expect(storage.getItem(KEY)).toBe("Why this way?");
    }
  });
});
