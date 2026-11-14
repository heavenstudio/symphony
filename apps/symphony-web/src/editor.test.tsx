import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { words } from "./document";
import { Editor, type EditorProps, heldBy, refusal, request } from "./editor";
import { STALE } from "./shared";
import { memoryStorage } from "./testing";
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
