import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type Draft, NEW_DRAFT } from "./panel-model";
import { loadDraft, priorityOf, QuickPanel, saveDraft } from "./quick";
import { HOME, type Route } from "./route";
import { memoryStorage } from "./testing";

let storage: Storage;

/** The window the panel shares with the frame, holding `values`, on a page with no project. */
function page(values: Record<string, unknown> = {}) {
  const window = Object.assign(new EventTarget(), values);
  vi.stubGlobal("window", window);
  vi.stubGlobal("dispatchEvent", window.dispatchEvent.bind(window));
  vi.stubGlobal("document", { getElementById: () => null });
  return window as EventTarget & Record<string, unknown>;
}

beforeEach(() => {
  storage = memoryStorage();
  vi.stubGlobal("localStorage", storage);
});

afterEach(() => vi.unstubAllGlobals());

describe("the sheet's draft", () => {
  it("is the window's, else this device's, else a new one", () => {
    page({ reviewQuickDraft: { open: true, text: "on the window" } });
    storage.setItem("review-quick-draft", JSON.stringify({ text: "kept" }));
    expect(loadDraft()).toMatchObject({ open: true, text: "on the window", priority: "" });
    page();
    expect(loadDraft()).toMatchObject({ open: false, text: "kept" });
    storage.setItem("review-quick-draft", "{");
    expect(loadDraft()).toEqual(NEW_DRAFT);
    storage.removeItem("review-quick-draft");
    expect(loadDraft()).toEqual(NEW_DRAFT);
    page({ reviewQuickDraft: { open: "yes" } });
    expect(loadDraft()).toEqual(NEW_DRAFT);
  });

  it("is left on the window, a copy of its own, and kept on this device", () => {
    const window = page();
    const draft: Draft = { ...NEW_DRAFT, open: true, title: "T" };
    saveDraft(draft);
    expect(window.reviewQuickDraft).toEqual(draft);
    expect(window.reviewQuickDraft).not.toBe(draft);
    expect(JSON.parse(storage.getItem("review-quick-draft") ?? "null")).toEqual(draft);
  });
});

describe("a task's priority", () => {
  it("is the number the draft holds, else p2", () => {
    expect(["0", "4", "1.5", "-1", "1e1", ".5"].map(priorityOf)).toEqual([0, 4, 1.5, -1, 10, 0.5]);
    expect(["", "p1", "1.2.3", " 1", "NaN"].map(priorityOf)).toEqual([2, 2, 2, 2, 2]);
  });
});

const route: Route = { ...HOME, path: "docs/a.md", document: true };
const draw = (draft: Partial<Draft>) =>
  renderToStaticMarkup(
    <QuickPanel route={route} draft={{ ...NEW_DRAFT, ...draft }} change={() => {}} />,
  );

describe("the sheet", () => {
  it("is hidden while closed, on the chat's tab, asking about the page", () => {
    page();
    const markup = draw({});
    expect(markup).toMatch(
      /^<aside id="quick-sheet" hidden="" aria-label="Chat or add a task" data-busy="false" data-posting="false">/,
    );
    expect(markup).toContain(
      '<button role="tab" id="quick-chat-tab" class="dx-tabs-trigger" tabindex="0" type="button" aria-selected="true"',
    );
    expect(markup).toContain('class="dx-tooltip-trigger collapse"');
    expect(markup).toContain('aria-label="Collapse panel"');
    expect(markup).toContain(
      '<div class="quick-thread" aria-live="polite"><p class="muted">Ask about this page.</p></div>',
    );
    expect(markup).toContain('<label for="quick-text">Message</label>');
    expect(markup).toContain('required="" maxLength="4000"');
    expect(markup).not.toContain("quick-title");
    expect(markup).not.toContain("New conversation");
    expect(markup).toMatch(/type="submit" disabled="">Send<\/button>/);
  });

  it("open on a conversation, loads it with its restart beside", () => {
    page();
    const markup = draw({ open: true, ask: 7, text: "more" });
    expect(markup).not.toContain('hidden=""><div class="dx-tabs"');
    expect(markup).toMatch(
      /^<aside id="quick-sheet" aria-label="Chat or add a task" data-busy="true"/,
    );
    expect(markup).toContain(
      '<span class="status-text" role="status">Loading conversation…</span>',
    );
    expect(markup.match(/class="ghost-message"/g)).toHaveLength(2);
    expect(markup).toContain('<div class="quick-restart"><button');
    expect(markup).toContain(">New conversation</button>");
    expect(markup).toMatch(/type="submit">Send<\/button>/);
    expect(markup).not.toMatch(/type="submit" disabled=""/);
  });

  it("on the Add task tab is the form, its thread quiet", () => {
    page();
    const markup = draw({ open: true, task: true, title: "Fix the bell" });
    expect(markup).toContain(
      '<button role="tab" id="quick-task-tab" class="dx-tabs-trigger" tabindex="0" type="button" aria-selected="true"',
    );
    expect(markup).toContain('<div class="quick-thread quiet" aria-live="polite"></div>');
    expect(markup).toContain('<label for="quick-title">Title</label>');
    expect(markup).toContain(
      '<input class="dx-input" id="quick-title" required="" maxLength="200" aria-describedby="quick-title-limit" value="Fix the bell"/>',
    );
    expect(markup).toContain(
      '<p id="quick-title-limit" role="status">12 / 200 characters</p><label for="quick-text">Details</label>',
    );
    expect(markup).toContain("<label>Slug (optional)<input");
    expect(markup).toContain("<label>Priority<");
    expect(markup).toContain(">p2<");
  });

  it("holds the form while a save's outcome is unknown, which a send checks", () => {
    page();
    const markup = draw({ open: true, task: true, title: "T", request: "ab" });
    expect(markup).toContain('data-orientation="horizontal" data-disabled="true"');
    expect(markup).toContain(
      '<div class="quick-thread" aria-live="polite"><p role="status">Check the previous save to continue; your text is kept.</p></div>',
    );
    expect(markup).toMatch(/<input class="dx-input" id="quick-title"[^>]* disabled=""/);
    expect(markup).toMatch(/type="submit">Check save<\/button>/);
  });

  it("refuses a task with no title", () => {
    page();
    expect(draw({ open: true, task: true, title: "  " })).toMatch(
      /type="submit" disabled="">Send<\/button>/,
    );
  });
});
