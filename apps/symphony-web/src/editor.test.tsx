import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Unanswered, words } from "./document";
import type { DocumentLive } from "./document-live";
import {
  Editor,
  type EditorProps,
  heldBy,
  refusal,
  request,
  SaveControl,
  type SaveField,
} from "./editor";
import { STALE } from "./shared";
import { type Asked, asking, memoryStorage, reply, server } from "./testing";
import type { Document } from "./wire";
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

describe("a save", () => {
  it("waits for a file still uploading, whatever it says", () => {
    for (const action of ["comment", "message", "approved"]) {
      expect(refusal(action, "words", true, true)).toBe("A file is still uploading");
    }
  });

  it("needs words for a comment, words or files for a message, nothing for a verdict", () => {
    expect(refusal("comment", " \n", false, true)).toBe("Type a message first");
    expect(refusal("message", "", false, false)).toBe("Type a message first");
    expect(refusal("message", "", false, true)).toBeNull();
    expect(refusal("comment", "A title", false, false)).toBeNull();
    for (const verdict of ["approved", "rejected", "question", "dismissed", "pending"]) {
      expect(refusal(verdict, "", false, false)).toBeNull();
    }
  });

  it("posts a comment after the section's last line, of its kind", () => {
    expect(request("comment", "A title", "docs/a.md", null, "", 12, "question")).toEqual([
      "/api/comment",
      { path: "docs/a.md", text: "A title", after_line: 12, kind: "question" },
    ]);
  });

  it("posts a card's message, or its verdict under its tap", () => {
    expect(request("message", "Hello", "docs/a.md", "card-1", "", 3, "comment")).toEqual([
      "/api/message",
      { path: "docs/a.md", text: "Hello", slug: "card-1" },
    ]);
    expect(request("approved", "Fine", "docs/a.md", "card-1", "ui-1", 3, "comment")).toEqual([
      "/api/decision",
      {
        path: "docs/a.md",
        text: "Fine",
        slug: "card-1",
        state: "approved",
        note: "Fine",
        tap: "ui-1",
      },
    ]);
  });

  it("holds the words sent, or a repeated tap's as its reply names them", () => {
    const sent = words("approved", "Fine");
    expect(heldBy({ ok: true }, sent)).toBe(sent);
    expect(heldBy(null, sent)).toBe(sent);
    expect(heldBy({ repeat: true, state: "rejected", note: "No" }, sent)).toEqual(
      words("rejected", "No"),
    );
    expect(heldBy({ repeat: true, state: "rejected" }, sent)).toBeNull();
    expect(heldBy({ repeat: true }, sent)).toBeNull();
  });
});

const props = (overrides: Partial<EditorProps> = {}): EditorProps => ({
  path: "docs/a.md",
  scope: "",
  sectionKey: "L12",
  slug: null,
  state: "pending",
  afterLine: 12,
  editing: false,
  setEditing: () => {},
  opener: "opener-docs%2Fa.md%3AL12",
  gone: false,
  ...overrides,
});

const drawn = (overrides: Partial<EditorProps> = {}) =>
  renderToStaticMarkup(<Editor {...props(overrides)} />);

/** The button whose words are `label`, as drawn. */
const button = (html: string, label: string) =>
  html.match(new RegExp(`<button[^>]*>${label.replace(/[?]/g, "\\$&")}</button>`))?.[0] ?? "";

describe("a section's editor", () => {
  it("is closed with nothing drafted: no hold on the section, its form hidden", () => {
    const html = drawn();
    expect(html).toContain('<div class="editor" data-busy="false" data-draft="false">');
    expect(html).toContain(
      '<form class="comment" hidden="" data-attach="review-draft:docs/a.md:L12">',
    );
    expect(html).not.toContain("Resume draft");
    expect(html).toContain('id="draft-review-draft%3Adocs%2Fa.md%3AL12"');
    expect(html).toContain('aria-label="Message or verdict note"');
    expect(html).toContain('placeholder="Write a message, or add a note to your verdict…"');
    expect(html).not.toContain('role="alert"');
    expect(html).not.toContain('role="status"');
  });

  it("of a comment offers its kind and saves it", () => {
    const html = drawn({ editing: true });
    expect(html).toContain('<div class="editor" data-busy="true" data-draft="false">');
    expect(html).toContain('<form class="comment" data-attach=');
    expect(html).toContain("Comment kind");
    expect(button(html, "Save comment")).toContain("primary");
    expect(button(html, "Discard draft")).toContain("discard");
    expect(html).not.toContain("verdict-choices");
    expect(html).not.toContain("Send message");
  });

  it("of a card offers its verdicts, the card's own marked current, then its Later", () => {
    const html = drawn({
      slug: "card-1",
      state: "rejected",
      editing: true,
      later: <button type="button">Later</button>,
    });
    expect(html).toContain('<form class="verdict" data-attach=');
    expect(html).not.toContain("Comment kind");
    expect(button(html, "✓ Approve")).toContain("v-approved primary");
    expect(button(html, "✓ Approve")).not.toContain("aria-current");
    expect(button(html, "✗ Reject")).toContain('aria-current="true"');
    expect(button(html, "? Question")).toContain("v-question");
    expect(html.indexOf("? Question")).toBeLessThan(html.indexOf(">Later</button>"));
    expect(html.indexOf(">Later</button>")).toBeLessThan(html.indexOf("Send message"));
    expect(button(html, "Send message")).not.toContain("primary");
    expect(button(html, "⊘ Dismiss")).toContain("v-dismissed");
    expect(button(html, "↺ Reset to pending")).toContain(
      'title="Take back the verdict: the card waits for yours again"',
    );
  });

  it("of a pending card has no verdict to take back, and marks Dismiss on a dismissed one", () => {
    expect(drawn({ slug: "card-1", state: "pending" })).not.toContain("Reset to pending");
    expect(button(drawn({ slug: "card-1", state: "dismissed" }), "⊘ Dismiss")).toContain(
      'aria-current="true"',
    );
  });

  it("closed over a draft offers it again and holds its card's words", () => {
    storage.setItem("review-draft:docs/a.md:L12", "Half a thought");
    const html = drawn();
    expect(html).toContain('<div class="editor" data-busy="true" data-draft="true">');
    expect(button(html, "Resume draft")).not.toBe("");
    expect(html).toContain(">Half a thought</textarea>");
    expect(drawn({ editing: true })).toContain('data-busy="true" data-draft="false"');
  });

  it("waits while the page may be out of date, its draft still editable", () => {
    STALE.set(true);
    const html = drawn({ slug: "card-1", state: "approved", editing: true });
    for (const label of [
      "✓ Approve",
      "✗ Reject",
      "? Question",
      "⊘ Dismiss",
      "↺ Reset to pending",
    ]) {
      expect(html).not.toContain(label);
    }
    expect(html.match(new RegExp(`disabled=""[^>]*>${CHECKING}</button>`, "g"))).toHaveLength(6);
    expect(button(html, "Discard draft")).not.toContain("disabled");
    expect(html).not.toMatch(/<textarea[^>]*disabled/);
  });
});

/** What a save showed, each state's values in the order it set them. */
function shownOf() {
  return {
    text: vi.fn<(value: string) => void>(),
    pending: vi.fn<(value: boolean) => void>(),
    error: vi.fn<(value: string | null) => void>(),
    saved: vi.fn<(value: string | null) => void>(),
    unanswered: vi.fn<(value: Unanswered | null) => void>(),
  };
}

/** A box's files: none, unless `over` says otherwise; its words sent as they are. */
const attachOf = (over: Partial<SaveField["attach"]> = {}) => ({
  busy: false,
  empty: true,
  compose: (text: string) => text,
  clear: vi.fn(),
  ...over,
});

/** A document whose card `slug`'s thread holds the owner's `lines`, each a verdict and words. */
function withLines(slug: string, lines: [string, string][]): Document {
  const entries = lines.map(([verdict, text]) => ({
    who: "owner" as const,
    timestamp: null,
    verdict,
    blocks: [{ type: "paragraph" as const, spans: [{ type: "text" as const, text }] }],
  }));
  return {
    version: 7,
    path: "docs/a.md",
    title: "A",
    waiting: [],
    crumbs: [],
    headings: [],
    sections: [],
    cards: [],
    threads: { [slug]: { blocks: [{ type: "thread", entries }] } },
    tasks: [],
  };
}

/** A page whose latest copy is `latest`, and the cards it released. */
const pageOf = (latest: Document) =>
  ({ state: { latest }, released: new Map<string, number>() }) as unknown as DocumentLive;

/** A save of the box's `typed` words in the section `over` names (a comment after L12 else). */
function saving(over: Partial<SaveField> = {}, typed = "Keep it") {
  const shown = shownOf();
  const left = new AbortController();
  const field: SaveField = {
    path: "docs/a.md",
    scope: "",
    sectionKey: "L12",
    slug: null,
    afterLine: 12,
    kind: "comment",
    gone: false,
    page: null,
    attach: attachOf(),
    setEditing: vi.fn(),
    ...over,
  };
  return { control: new SaveControl(typed, field, left.signal, shown), shown, field, left };
}

/** A card's save: the card `alpha`, whose thread has one line of the owner's. */
const cardSave = (typed = "Keep it") => {
  const page = pageOf(withLines("alpha", [["question", "Why?"]]));
  return { page, ...saving({ slug: "alpha", sectionKey: "card:alpha", page }, typed) };
};

const TAP = "review-tap:docs/a.md:card:alpha";

/** The tap a request's body names, if any. */
const tapOf = (one: Asked | undefined) => (one?.body as { tap?: string } | undefined)?.tap;

describe("a section's draft", () => {
  it("is kept on this device as it is typed, and Discard empties it and closes the form", () => {
    const { control, shown, field } = saving({}, "");
    control.type("Half a thought");
    expect(storage.getItem("review-draft:docs/a.md:L12")).toBe("Half a thought");
    expect(control.typed).toBe("Half a thought");
    expect(shown.text).toHaveBeenLastCalledWith("Half a thought");
    control.discard();
    expect(storage.getItem("review-draft:docs/a.md:L12")).toBe("");
    expect(control.typed).toBe("");
    expect(shown.text).toHaveBeenLastCalledWith("");
    expect(field.attach.clear).toHaveBeenCalledTimes(1);
    expect(field.setEditing).toHaveBeenCalledWith(false);
  });
});

describe("a section's save", () => {
  it("posts a comment after the section's last line, and once saved empties the box and closes", async () => {
    const asked = server();
    storage.setItem("review-draft:docs/a.md:L12", "Keep it");
    const { control, shown, field } = saving({ kind: "question" });
    const done = control.send("comment");
    expect(shown.pending.mock.calls).toEqual([[true]]);
    expect(shown.error.mock.calls).toEqual([[null]]);
    expect(shown.saved.mock.calls).toEqual([[null]]);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/comment");
    expect(asked[0]?.body).toEqual({
      path: "docs/a.md",
      text: "Keep it",
      after_line: 12,
      kind: "question",
    });
    // A second press while the first is on its way sends nothing.
    await control.send("comment");
    expect(asked).toHaveLength(1);
    asked[0]?.answer(reply(200, { ok: true }));
    await done;
    expect(shown.text).toHaveBeenLastCalledWith("");
    expect(storage.getItem("review-draft:docs/a.md:L12")).toBe("");
    expect(field.attach.clear).toHaveBeenCalledTimes(1);
    expect(shown.saved).toHaveBeenLastCalledWith("Comment saved");
    expect(field.setEditing).toHaveBeenCalledWith(false);
    expect(shown.pending).toHaveBeenLastCalledWith(false);
    expect(control.sending).toBe(false);
    // No card: no tap kept, nothing released.
    expect(storage.getItem(TAP)).toBeNull();
  });

  it("of a checkout's section posts under its scope, with its files' lines", async () => {
    const asked = server();
    const compose = (text: string) => `${text}\nFile: /uploads/a.png`;
    const files = attachOf({ empty: false, compose });
    const { control } = saving({ scope: "feature", attach: files }, "A title");
    const done = control.send("comment");
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/@feature/api/comment");
    expect(asked[0]?.body).toMatchObject({ text: "A title\nFile: /uploads/a.png" });
    asked[0]?.answer(reply(200, {}));
    await done;
  });

  it("sends nothing for a block gone, words missing, a file uploading or a page out of date", async () => {
    const asked = server();
    const gone = saving({ gone: true });
    await gone.control.send("comment");
    expect(gone.shown.error).toHaveBeenCalledWith("This block was removed; your draft is kept.");
    const wordless = saving({}, " ");
    await wordless.control.send("comment");
    expect(wordless.shown.error).toHaveBeenCalledWith("Type a message first");
    const uploading = saving({ attach: attachOf({ busy: true }) });
    await uploading.control.send("comment");
    expect(uploading.shown.error).toHaveBeenCalledWith("A file is still uploading");
    STALE.set(true);
    const stale = saving();
    await stale.control.send("comment");
    // The page may show what is out of date: the save waits, saying nothing of its own.
    expect(stale.shown.error).not.toHaveBeenCalled();
    for (const { shown } of [gone, wordless, uploading, stale]) {
      expect(shown.pending).not.toHaveBeenCalled();
    }
    await asking(asked, 0);
  });

  it("sends a card's verdict under a tap kept until a reply answers it, and releases the card", async () => {
    const asked = server();
    const { control, shown, page } = cardSave();
    const done = control.send("approved");
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/decision");
    const body = asked[0]?.body as Record<string, unknown>;
    expect(body).toMatchObject({ slug: "alpha", state: "approved", note: "Keep it" });
    expect(body.tap).toMatch(/^ui-/);
    // Kept before the reply: a retry after a reload goes under the same tap.
    expect(storage.getItem(TAP)).toBe(`${body.tap}\napproved\nKeep it`);
    asked[0]?.answer(reply(200, { ok: true }));
    await done;
    expect(storage.getItem(TAP)).toBe("");
    // The card had one line of the owner's: it shows the answer once it has two.
    expect(page.released.get("card:alpha")).toBe(2);
    expect(shown.saved).toHaveBeenLastCalledWith("Verdict saved: approved");
    expect(shown.unanswered.mock.calls).toEqual([[null]]);
  });

  it("sends a card's message under no tap", async () => {
    const asked = server();
    const { control, shown } = cardSave("Hello");
    const done = control.send("message");
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/message");
    expect(asked[0]?.body).toEqual({ path: "docs/a.md", text: "Hello", slug: "alpha" });
    expect(storage.getItem(TAP)).toBeNull();
    asked[0]?.answer(reply(200, {}));
    await done;
    expect(shown.saved).toHaveBeenLastCalledWith("Message sent");
  });

  it("says a comment and a card's message the board held, and waits on no line for them", async () => {
    const asked = server();
    const comment = saving({ kind: "question" });
    const commented = comment.control.send("comment");
    await asking(asked, 1);
    asked[0]?.answer(reply(200, { ok: true, held: 4 }));
    await commented;
    expect(comment.shown.saved).toHaveBeenLastCalledWith("Comment held");
    expect(comment.shown.text).toHaveBeenLastCalledWith("");
    expect(comment.field.setEditing).toHaveBeenCalledWith(false);
    const { control, shown, page } = cardSave("Hello");
    const messaged = control.send("message");
    await asking(asked, 2);
    asked[1]?.answer(reply(200, { ok: true, held: 5 }));
    await messaged;
    expect(shown.saved).toHaveBeenLastCalledWith("Message held");
    expect(page.released.has("card:alpha")).toBe(false);
  });

  it("takes a tap kept for the same words across a reload, never for others", async () => {
    const asked = server();
    storage.setItem(TAP, "ui-kept\napproved\nKeep it");
    const same = cardSave().control.send("approved");
    await asking(asked, 1);
    expect(asked[0]?.body).toMatchObject({ tap: "ui-kept" });
    asked[0]?.answer(reply(200, {}));
    await same;
    storage.setItem(TAP, "ui-kept\napproved\nKeep it");
    const other = cardSave("Changed my mind").control.send("rejected");
    await asking(asked, 2);
    expect(tapOf(asked[1])).toMatch(/^ui-\d/);
    asked[1]?.answer(reply(200, {}));
    await other;
  });

  it("says a refusal in the server's words, and leaves nothing unanswered", async () => {
    const asked = server();
    const { control, shown } = cardSave();
    const done = control.send("approved");
    await asking(asked, 1);
    asked[0]?.answer(reply(409, { error: "the card moved" }));
    await done;
    expect(shown.error).toHaveBeenLastCalledWith(expect.stringContaining("the card moved"));
    expect(shown.unanswered).not.toHaveBeenCalled();
    expect(control.waiting).toBeNull();
    expect(shown.pending).toHaveBeenLastCalledWith(false);
    // The tap stays for a retry of the same words.
    expect(storage.getItem(TAP)).toMatch(/^ui-.*\napproved\nKeep it$/);
  });

  it("leaves a comment no reply answered as it is: only a card's save waits on the feed", async () => {
    const asked = server();
    const { control, shown } = saving();
    const done = control.send("comment");
    await asking(asked, 1);
    asked[0]?.answer(reply(500, { error: "the disk is full" }));
    await done;
    expect(shown.error).toHaveBeenLastCalledWith(expect.stringContaining("the disk is full"));
    expect(shown.unanswered).not.toHaveBeenCalled();
  });

  it("retries a card's save no reply answered under its tap, and its repeat releases the card", async () => {
    const asked = server();
    const { control, shown, page } = cardSave();
    const first = control.send("approved");
    await asking(asked, 1);
    const tapped = tapOf(asked[0]);
    asked[0]?.answer(reply(500, { error: "the disk is full" }));
    await first;
    expect(control.waiting).toEqual(
      new Unanswered([words("approved", "Keep it")], tapped ?? null, 1),
    );
    expect(shown.unanswered).toHaveBeenLastCalledWith(control.waiting);
    // The owner changes the words and the verdict: the retry goes under the same tap.
    control.type("No");
    const retry = control.send("rejected");
    await asking(asked, 2);
    expect(asked[1]?.body).toMatchObject({ state: "rejected", note: "No", tap: tapped });
    // The server had written the first sending: it names those words.
    asked[1]?.answer(reply(200, { repeat: true, state: "approved", note: "Keep it" }));
    await retry;
    expect(control.waiting).toBeNull();
    expect(page.released.get("card:alpha")).toBe(2);
    expect(shown.saved).toHaveBeenLastCalledWith(
      "Verdict saved: approved; your change was not sent: send it again to add it",
    );
    // Words sent and not held stay as the card's draft, the form closed.
    expect(shown.text).not.toHaveBeenCalledWith("");
  });

  it("releases nothing for a repeat of a tap this page never left unanswered", async () => {
    const asked = server();
    storage.setItem(TAP, "ui-kept\napproved\nKeep it");
    const { control, page } = cardSave();
    const done = control.send("approved");
    await asking(asked, 1);
    asked[0]?.answer(reply(200, { repeat: true, state: "approved", note: "Keep it" }));
    await done;
    expect(page.released.has("card:alpha")).toBe(false);
  });

  it("does nothing more once the page left while the server answered", async () => {
    const asked = server();
    const answers = [reply(200, { ok: true }), reply(500, { error: "the disk is full" })];
    for (const [at, answer] of answers.entries()) {
      storage.setItem("review-draft:docs/a.md:card:alpha", "Keep it");
      const { control, shown, page, field, left } = cardSave();
      const done = control.send("approved");
      await asking(asked, at + 1);
      const tapped = storage.getItem(TAP);
      left.abort();
      asked[at]?.answer(answer);
      await done;
      expect(shown.pending.mock.calls).toEqual([[true]]);
      expect(shown.saved.mock.calls).toEqual([[null]]);
      expect(shown.error.mock.calls).toEqual([[null]]);
      expect(shown.text).not.toHaveBeenCalled();
      expect(shown.unanswered).not.toHaveBeenCalled();
      expect(field.setEditing).not.toHaveBeenCalled();
      expect(page.released.size).toBe(0);
      // What this device keeps stays for the page that comes back: the draft and the tap.
      expect(storage.getItem("review-draft:docs/a.md:card:alpha")).toBe("Keep it");
      expect(storage.getItem(TAP)).toBe(tapped);
    }
  });
});

describe("a card's save no reply answered", () => {
  /** A card's save the server gave no reply to, the feed still to answer it. */
  async function unanswered(typed = "Keep it") {
    const asked = server();
    const save = cardSave(typed);
    const done = save.control.send("approved");
    await asking(asked, 1);
    asked[0]?.answer(reply(500, { error: "the disk is full" }));
    await done;
    save.shown.error.mockClear();
    return save;
  }

  const answeredLine = () =>
    withLines("alpha", [
      ["question", "Why?"],
      ["approved", "Keep it"],
    ]);

  it("is answered by the feed once the card shows the owner's line it brought", async () => {
    const { control, shown, page, field } = await unanswered();
    // The card as it was answers nothing.
    control.heard(withLines("alpha", [["question", "Why?"]]));
    expect(control.waiting).not.toBeNull();
    expect(shown.error).not.toHaveBeenCalled();
    control.heard(answeredLine());
    expect(control.waiting).toBeNull();
    expect(shown.unanswered).toHaveBeenLastCalledWith(null);
    expect(page.released.get("card:alpha")).toBe(2);
    expect(storage.getItem(TAP)).toBe("");
    expect(shown.error).toHaveBeenLastCalledWith(null);
    expect(shown.saved).toHaveBeenLastCalledWith("Verdict saved: approved");
    expect(shown.text).toHaveBeenLastCalledWith("");
    expect(field.setEditing).toHaveBeenCalledWith(false);
  });

  it("keeps words the owner wrote since sending, the form open", async () => {
    const { control, shown, field } = await unanswered();
    control.type("Keep it, and more");
    control.heard(answeredLine());
    expect(shown.saved).toHaveBeenLastCalledWith("Verdict saved: approved");
    expect(shown.text).toHaveBeenLastCalledWith("Keep it, and more");
    expect(field.setEditing).not.toHaveBeenCalled();
  });

  it("waits while another save is on its way, and is nothing to a page with no model", async () => {
    const { control, field } = await unanswered();
    control.sending = true;
    control.heard(answeredLine());
    expect(control.waiting).not.toBeNull();
    control.sending = false;
    control.field = { ...field, page: null };
    control.heard(answeredLine());
    expect(control.waiting).not.toBeNull();
  });
});
