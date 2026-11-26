import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type Draft, NEW_DRAFT, readAskReply } from "./panel-model";
import {
  afterSent,
  askOf,
  CHECK_MS,
  canSend,
  fragmentOf,
  loadDraft,
  priorityOf,
  QuickPanel,
  QuickThread,
  type QuickThreadProps,
  RETRY_MS,
  readThread,
  requestIdentity,
  saveDraft,
  saveTask,
  taskPage,
} from "./quick";
import { HOME, type Route } from "./route";
import { asking, memoryStorage, reply, server } from "./testing";

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

describe("a task save's request identity", () => {
  it("is 16 bytes in hex, each two digits, and new each time", () => {
    vi.stubGlobal("crypto", {
      getRandomValues: (bytes: Uint8Array) => bytes.fill(10, 0, 8).fill(255, 8),
    });
    expect(requestIdentity()).toBe(`${"0a".repeat(8)}${"ff".repeat(8)}`);
    vi.unstubAllGlobals();
    expect(requestIdentity()).toMatch(/^[0-9a-f]{32}$/);
    expect(requestIdentity()).not.toBe(requestIdentity());
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
  it("is hidden while closed, on the chat's tab, its empty thread quiet", () => {
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
    expect(markup).toMatch(
      /aria-label="Collapse panel"><svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24"[^>]*><path d="m6 9 6 6 6-6"><\/path><\/svg><\/button>/,
    );
    // An empty chat says nothing of itself: its thread is quiet, the Message box under the tabs.
    expect(markup).toContain('<div class="quick-thread quiet" aria-live="polite"></div>');
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

describe("whether the form may send", () => {
  const idle = { busy: false, empty: true };
  const chat = (text: string) => ({ ...NEW_DRAFT, text });
  const task = (title: string) => ({ ...NEW_DRAFT, task: true, title });

  it("asks a message or a file of the chat, and a title of a task", () => {
    expect(canSend(chat("Hello"), false, idle)).toBe(true);
    expect(canSend(chat("  "), false, idle)).toBe(false);
    expect(canSend(chat(""), false, { busy: false, empty: false })).toBe(true);
    expect(canSend(task("Fix the bell"), false, idle)).toBe(true);
    // A task's details are no title, and its files make none.
    expect(canSend({ ...task(" "), text: "Details" }, false, { busy: false, empty: false })).toBe(
      false,
    );
  });

  it("holds while a send is out or a file is still being read", () => {
    expect(canSend(chat("Hello"), true, idle)).toBe(false);
    expect(canSend(task("Fix the bell"), false, { busy: true, empty: false })).toBe(false);
  });
});

describe("what a send names", () => {
  const at = (pathname: string, hash = "") => ({
    pathname,
    hash,
    href: `http://localhost${pathname}${hash}`,
  });

  it("names a task's page: the first send's again, a document with its anchor, else the path", () => {
    page();
    const sent = { ...NEW_DRAFT, task: true };
    expect(taskPage({ ...sent, request_page: "/docs/kept.md" }, route, at("/x"))).toBe(
      "/docs/kept.md",
    );
    expect(taskPage(sent, route, at("/docs/a.md", "#card"))).toBe("/docs/a.md#card");
    expect(taskPage(sent, { ...route, scope: "wt" }, at("/@wt/docs/a.md"))).toBe("/@wt/docs/a.md");
    expect(taskPage(sent, { ...route, folder: true }, at("/docs/", "#top"))).toBe("/docs/#top");
    expect(taskPage(sent, HOME, at("/tasks"))).toBe("/tasks");
  });

  it("asks about a document by its path, else about the readme from the page it was sent on", () => {
    page();
    expect(askOf(route, "Why?", at("/docs/a.md", "#c%C3%A9"))).toEqual({
      path: "docs/a.md",
      question: "Why?",
      anchor: "cé",
    });
    expect(askOf(HOME, "Why?", at("/tasks", "#%E0%A4%A"))).toEqual({
      path: "docs/README.md",
      question: "Why?\n\n(sent from http://localhost/tasks#%E0%A4%A)",
      anchor: "",
    });
  });

  it("reads an address's fragment without its marks, decoded", () => {
    expect(["", "#", "##a", "#a%20b", "#%"].map(fragmentOf)).toEqual(["", "", "a", "a b", ""]);
  });
});

describe("a send that went through", () => {
  it("empties a task's fields, its request with them", () => {
    const now = { ...NEW_DRAFT, task: true, title: "T", request: "ab", request_page: "/p" };
    expect(afterSent(now, { slug: "a/b" }, "")).toEqual({
      text: "",
      title: "",
      slug: "",
      request: "",
      request_page: "",
      request_scope: "",
    });
  });

  it("empties a reply's text, and takes a new conversation's id and scope up", () => {
    expect(afterSent({ ...NEW_DRAFT, ask: 7, text: "more" }, { ok: true }, "")).toEqual({
      text: "",
    });
    expect(afterSent({ ...NEW_DRAFT, text: "Why?" }, { id: 12.9 }, "wt")).toEqual({
      text: "",
      ask: 12,
      ask_scope: "wt",
    });
    expect(afterSent({ ...NEW_DRAFT, text: "Why?" }, { id: "12" }, "")).toMatchObject({ ask: 0 });
  });
});

describe("a task's save", () => {
  afterEach(() => vi.useRealTimers());
  const URL = "/api/quick-task";
  const fields = { request: "ab", title: "T" };

  it("is the slug the server saved it under", async () => {
    const asked = server();
    const checking = vi.fn();
    const saved = saveTask(URL, fields, new AbortController().signal, checking);
    await asking(asked, 1);
    expect([asked[0]?.url, asked[0]?.body]).toEqual([URL, fields]);
    asked[0]?.answer(reply(200, { slug: "ui/fix-the-bell" }));
    expect(await saved).toBe("ui/fix-the-bell");
    expect(checking).not.toHaveBeenCalled();
  });

  it("is asked again each second while the server checks it or its answer was uncertain", async () => {
    vi.useFakeTimers();
    const asked = server();
    const checking = vi.fn();
    const saved = saveTask(URL, fields, new AbortController().signal, checking);
    await vi.advanceTimersByTimeAsync(0);
    asked[0]?.answer(reply(200, { pending: true, error: "the board is busy" }));
    await vi.advanceTimersByTimeAsync(CHECK_MS - 1);
    expect(checking).toHaveBeenLastCalledWith("the board is busy");
    expect(asked).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    asked[1]?.answer(reply(503, { error: "restarting" }));
    await vi.advanceTimersByTimeAsync(CHECK_MS);
    expect(checking).toHaveBeenLastCalledWith(expect.stringContaining("restarting"));
    asked[2]?.answer(reply(200, { slug: "ui/fix-the-bell" }));
    expect(await saved).toBe("ui/fix-the-bell");
    expect(asked.map((each) => each.body)).toEqual([fields, fields, fields]);
  });

  it("throws a refusal, the server having saved nothing", async () => {
    const asked = server();
    const saved = saveTask(URL, fields, new AbortController().signal, vi.fn());
    await asking(asked, 1);
    asked[0]?.answer(reply(400, { error: "the title is too long" }));
    await expect(saved).rejects.toThrow("the title is too long");
  });

  it("is given up once its mount ended, while asked or while it waited", async () => {
    vi.useFakeTimers();
    const asked = server();
    for (const answer of [reply(200, { pending: true }), reply(503, { error: "restarting" })]) {
      const mount = new AbortController();
      const checking = vi.fn();
      const saved = saveTask(URL, fields, mount.signal, checking);
      await vi.advanceTimersByTimeAsync(0);
      mount.abort();
      asked.at(-1)?.answer(answer);
      expect(await saved).toBeNull();
      expect(checking).not.toHaveBeenCalled();
    }
    const mount = new AbortController();
    const saved = saveTask(URL, fields, mount.signal, vi.fn());
    await vi.advanceTimersByTimeAsync(0);
    asked.at(-1)?.answer(reply(200, { pending: true }));
    await vi.advanceTimersByTimeAsync(CHECK_MS / 2);
    mount.abort();
    expect(await saved).toBeNull();
    await vi.advanceTimersByTimeAsync(CHECK_MS);
    expect(asked).toHaveLength(3);
  });
});

describe("the conversation's read", () => {
  afterEach(() => vi.useRealTimers());
  const URL = "/api/ask?format=blocks&id=7";
  const conversation = {
    ask: { id: 7, anchor: "", quote: "", status: "open", messages: [] },
    recipient: "teal-lynx",
  };
  const shownOf = () => ({ thread: vi.fn(), trouble: vi.fn(), missing: vi.fn() });

  it("draws the conversation once the server answers, its trouble cleared", async () => {
    const asked = server();
    const shown = shownOf();
    const done = readThread(URL, new AbortController().signal, shown);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe(URL);
    asked[0]?.answer(reply(200, conversation));
    await done;
    expect(shown.thread).toHaveBeenCalledWith(readAskReply(conversation));
    expect(shown.trouble.mock.calls).toEqual([[null]]);
    expect(shown.missing).not.toHaveBeenCalled();
  });

  it("gives up an expired conversation without retrying its 404", async () => {
    vi.useFakeTimers();
    const asked = server();
    const shown = shownOf();
    const done = readThread(URL, new AbortController().signal, shown);
    await vi.advanceTimersByTimeAsync(0);
    asked[0]?.answer(reply(404, { error: "no ask 7 (asks go after a day without activity)" }));
    await done;
    expect(shown.missing).toHaveBeenCalledOnce();
    expect(shown.thread).not.toHaveBeenCalled();
    expect(shown.trouble.mock.calls).toEqual([[null]]);
    await vi.advanceTimersByTimeAsync(RETRY_MS * 2);
    expect(asked).toHaveLength(1);
  });

  it("says why each read failed and asks again 3 s later, until one answers", async () => {
    vi.useFakeTimers();
    const asked = server();
    const shown = shownOf();
    const done = readThread(URL, new AbortController().signal, shown);
    await vi.advanceTimersByTimeAsync(0);
    asked[0]?.answer(reply(500, { error: "the disk is full" }));
    await vi.advanceTimersByTimeAsync(RETRY_MS - 1);
    expect(shown.trouble).toHaveBeenLastCalledWith(expect.stringContaining("the disk is full"));
    expect(asked).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    asked[1]?.answer(reply(200, { ask: "none" }));
    await vi.advanceTimersByTimeAsync(0);
    expect(shown.trouble).toHaveBeenLastCalledWith("The server's conversation could not be read");
    await vi.advanceTimersByTimeAsync(RETRY_MS);
    asked[2]?.answer(reply(200, conversation));
    await done;
    expect(shown.thread).toHaveBeenCalledOnce();
    expect(shown.trouble).toHaveBeenLastCalledWith(null);
  });

  it("shows nothing once the conversation was given up, while it read or while it waited", async () => {
    vi.useFakeTimers();
    const asked = server();
    for (const answer of [
      reply(200, conversation),
      reply(500, { error: "gone" }),
      reply(404, { error: "no ask 7" }),
    ]) {
      const shown = shownOf();
      const left = new AbortController();
      const done = readThread(URL, left.signal, shown);
      await vi.advanceTimersByTimeAsync(0);
      left.abort();
      asked.at(-1)?.answer(answer);
      await done;
      expect(shown.thread).not.toHaveBeenCalled();
      expect(shown.trouble).not.toHaveBeenCalled();
      expect(shown.missing).not.toHaveBeenCalled();
    }
    const shown = shownOf();
    const left = new AbortController();
    const done = readThread(URL, left.signal, shown);
    await vi.advanceTimersByTimeAsync(0);
    asked.at(-1)?.answer(reply(500, { error: "the disk is full" }));
    await vi.advanceTimersByTimeAsync(RETRY_MS / 2);
    left.abort();
    await done;
    await vi.advanceTimersByTimeAsync(RETRY_MS);
    expect(asked).toHaveLength(4);
    expect(shown.trouble).toHaveBeenCalledOnce();
  });
});

describe("the sheet's thread", () => {
  const thread = (props: Partial<QuickThreadProps>) =>
    renderToStaticMarkup(
      <QuickThread
        draft={NEW_DRAFT}
        shown={null}
        readError={null}
        checking={null}
        created={null}
        {...props}
      />,
    );
  const message = (seq: number, by: string, text: string) => ({
    seq,
    by,
    blocks: [{ type: "paragraph", spans: [{ type: "text", text }] }],
  });
  const conversation = (status: string, recipient: string | null) =>
    readAskReply({
      ask: {
        id: 7,
        anchor: "",
        quote: "",
        status,
        messages: [message(1, "owner", "Why?"), message(2, "teal-lynx", "Because.")],
      },
      recipient,
    });

  it("draws the conversation's messages, and whom chat goes to", () => {
    page();
    const markup = thread({
      draft: { ...NEW_DRAFT, ask: 7 },
      shown: conversation("open", "teal-lynx"),
    });
    expect(markup).toMatch(
      /^<div class="quick-thread" aria-live="polite"><article class="quick-message"><p class="by">owner<\/p>/,
    );
    expect(markup.match(/<article class="quick-message">/g)).toHaveLength(2);
    expect(markup).toContain("Because.");
    expect(markup).toContain('<p class="muted">Chat goes to teal-lynx.</p></div>');
  });

  it("says a message waiting with nobody to receive it is queued, and nothing once answered", () => {
    page();
    const draft = { ...NEW_DRAFT, ask: 7 };
    expect(thread({ draft, shown: conversation("open", null) })).toContain(
      '<p class="muted">Nobody is registered to receive chat; your message is queued.</p></div>',
    );
    expect(thread({ draft, shown: conversation("answered", null) })).not.toContain('class="muted"');
  });

  it("says a failed read over what it holds", () => {
    page();
    expect(thread({ draft: { ...NEW_DRAFT, ask: 7 }, readError: "offline" })).toMatch(
      /^<div class="quick-thread" aria-live="polite"><p role="status">Conversation could not be refreshed: offline<\/p><span class="status-text" role="status">Loading conversation…<\/span>/,
    ); // With no conversation yet, the failure alone, the thread no longer quiet.
    expect(thread({ draft: NEW_DRAFT, readError: "offline" })).toBe(
      '<div class="quick-thread" aria-live="polite"><p role="status">Conversation could not be refreshed: offline</p></div>',
    );
  });

  it("on the Add task tab, says a save being checked, and the task the last one created", () => {
    page();
    const task = { ...NEW_DRAFT, task: true };
    expect(thread({ draft: task, checking: "the board is busy" })).toBe(
      '<div class="quick-thread" aria-live="polite"><p role="status">Checking save… the board is busy</p></div>',
    );
    expect(thread({ draft: task, created: "ui/fix-the-bell" })).toBe(
      '<div class="quick-thread" aria-live="polite"><p role="status">✓ Task <a href="/task/ui/fix-the-bell">ui/fix-the-bell</a> created</p></div>',
    );
    expect(thread({ draft: task })).toBe(
      '<div class="quick-thread quiet" aria-live="polite"></div>',
    );
  });

  it("keeps conversation read failures out of task save status", () => {
    page();
    const draft = { ...NEW_DRAFT, task: true, ask: 7 };
    expect(thread({ draft, readError: "offline" })).toBe(
      '<div class="quick-thread quiet" aria-live="polite"></div>',
    );
    const markup = thread({ draft, readError: "offline", created: "inbox/saved" });
    expect(markup).toContain("inbox/saved</a> created");
    expect(markup).not.toContain("Conversation could not be refreshed");
  });
});
