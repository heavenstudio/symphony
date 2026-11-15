import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LaterButton, PostponeControls, postpone, press, seen } from "./postpone";
import { STALE } from "./shared";
import { asking, reply, server } from "./testing";

describe("a card's Later", () => {
  afterEach(() => STALE.set(false));
  const controls = (postponed: boolean) =>
    renderToStaticMarkup(<PostponeControls path="docs/A.md" slug="s" postponed={postponed} />);

  it("is Later, or Undo Later on a postponed card, which says so", () => {
    expect(controls(false)).toContain(">Later</button>");
    expect(controls(false)).not.toContain("postponed");
    expect(controls(true)).toContain('<small class="postponed">postponed</small>');
    expect(controls(true)).toContain(">Undo Later</button>");
  });

  it("waits, saying so, while the page may show what is out of date", () => {
    STALE.set(true);
    expect(controls(false)).toMatch(/disabled="".*>Checking…<\/button>/);
  });
});

describe("the button of Later", () => {
  const button = (pending: boolean, checking: boolean, postponed = false) =>
    renderToStaticMarkup(
      <LaterButton
        pending={pending}
        checking={checking}
        postponed={postponed}
        onPress={() => {}}
      />,
    );

  it("takes a press: Later, or Undo Later on a postponed card", () => {
    for (const [postponed, label] of [
      [false, "Later"],
      [true, "Undo Later"],
    ] as const) {
      const html = button(false, false, postponed);
      expect(html).toContain(`>${label}</button>`);
      expect(html).not.toContain("disabled");
    }
  });

  it("on its way says so and takes no press, but keeps the focus: never disabled", () => {
    for (const checking of [false, true]) {
      const html = button(true, checking);
      expect(html).toContain('aria-disabled="true"');
      expect(html).not.toContain('disabled=""');
      expect(html).toContain(">Saving…</button>");
    }
  });

  it("is disabled while the page may show what is out of date", () => {
    const html = button(false, true, true);
    expect(html).toMatch(/disabled="".*>Checking…<\/button>/);
    expect(html).not.toContain("aria-disabled");
  });
});

describe("Later and its Undo", () => {
  afterEach(() => vi.unstubAllGlobals());
  const frame = () => ({ refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() });

  it("post the card, read the frame again, and after Later go to the card named next", async () => {
    const asked = server();
    const refreshed = frame();
    const go = vi.fn();
    const card = { path: "docs/A.md", slug: "s", postponed: false };
    const done = postpone(card, new AbortController().signal, refreshed, go);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/postpone");
    expect(asked[0]?.body).toEqual({ path: "docs/A.md", slug: "s", action: "later" });
    asked[0]?.answer(reply(200, { next: "/docs/B.md#t" }));
    expect(await done).toBeNull();
    expect(refreshed.refresh).toHaveBeenCalledTimes(1);
    expect(go).toHaveBeenCalledWith("/docs/B.md#t");
  });

  it("stay on the page after Undo, and say what failed", async () => {
    const asked = server();
    const go = vi.fn();
    const undo = postpone(
      { path: "docs/A.md", slug: "s", postponed: true },
      new AbortController().signal,
      frame(),
      go,
    );
    await asking(asked, 1);
    expect(asked[0]?.body).toMatchObject({ action: "undo" });
    asked[0]?.answer(reply(200, { next: "/docs/B.md#t" }));
    expect(await undo).toBeNull();
    const refused = postpone(
      { path: "docs/A.md", slug: "s", postponed: false },
      new AbortController().signal,
      frame(),
      go,
    );
    await asking(asked, 2);
    asked[1]?.answer(reply(409, { error: "The card moved." }));
    expect(await refused).toBe("The card moved.");
    expect(go).not.toHaveBeenCalled();
  });

  it("of a checkout's card post under its scope", async () => {
    const asked = server();
    const card = { path: "docs/A.md", slug: "s", postponed: false, scope: "design~policy" };
    void postpone(card, new AbortController().signal, frame(), vi.fn());
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/@design~policy/api/postpone");
  });

  it("do nothing more once the page is gone before the server answers", async () => {
    const asked = server();
    const refreshed = frame();
    const go = vi.fn();
    const mount = new AbortController();
    const done = postpone(
      { path: "docs/A.md", slug: "s", postponed: false },
      mount.signal,
      refreshed,
      go,
    );
    await asking(asked, 1);
    // The reader leaves the queue: the shell stops its kind, which aborts its mount.
    mount.abort();
    asked[0]?.answer(reply(200, { next: "/docs/B.md#t" }));
    expect(await done).toBeNull();
    expect(go).not.toHaveBeenCalled();
    expect(refreshed.refresh).not.toHaveBeenCalled();
  });
});

describe("a press of Later", () => {
  afterEach(() => {
    STALE.set(false);
    vi.unstubAllGlobals();
  });
  const frame = () => ({ refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() });
  const card = { path: "docs/A.md", slug: "s", postponed: false };
  const shownOf = () => ({
    pending: vi.fn<(pending: boolean) => void>(),
    error: vi.fn<(error: string | null) => void>(),
  });

  it("is on its way until the server answers, then goes to the card named next", async () => {
    const asked = server();
    const shown = shownOf();
    const go = vi.fn();
    const done = press(card, false, new AbortController().signal, frame(), shown, go);
    expect(shown.pending.mock.calls).toEqual([[true]]);
    expect(shown.error.mock.calls).toEqual([[null]]);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, { next: "/docs/B.md#t" }));
    await done;
    expect(go).toHaveBeenCalledWith("/docs/B.md#t");
    expect(shown.error.mock.calls).toEqual([[null], [null]]);
    expect(shown.pending).toHaveBeenLastCalledWith(false);
  });

  it("says what failed, its button pressable again", async () => {
    const asked = server();
    const shown = shownOf();
    const done = press(card, false, new AbortController().signal, frame(), shown, vi.fn());
    await asking(asked, 1);
    asked[0]?.answer(reply(409, { error: "The card moved." }));
    await done;
    expect(shown.error).toHaveBeenLastCalledWith("The card moved.");
    expect(shown.pending).toHaveBeenLastCalledWith(false);
  });

  it("sends nothing while one is on its way or the page may be out of date", async () => {
    const asked = server();
    const shown = shownOf();
    await press(card, true, new AbortController().signal, frame(), shown, vi.fn());
    STALE.set(true);
    await press(card, false, new AbortController().signal, frame(), shown, vi.fn());
    expect(shown.pending).not.toHaveBeenCalled();
    await asking(asked, 0);
  });

  it("shows nothing more once the page is gone before the server answers", async () => {
    const asked = server();
    for (const [at, answer] of [reply(200, {}), reply(409, { error: "moved" })].entries()) {
      const shown = shownOf();
      const left = new AbortController();
      const done = press(card, false, left.signal, frame(), shown, vi.fn());
      await asking(asked, at + 1);
      left.abort();
      asked[at]?.answer(answer);
      await done;
      expect(shown.pending.mock.calls).toEqual([[true]]);
      expect(shown.error.mock.calls).toEqual([[null]]);
    }
  });
});

describe("a document's card seen", () => {
  afterEach(() => vi.unstubAllGlobals());
  const frame = () => ({ refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() });
  const card = { path: "docs/A.md", slug: "s", postponed: false };

  it("says so, and reads the frame again only when that ended its Later", async () => {
    const asked = server();
    const refreshed = frame();
    const unchanged = seen(card, new AbortController().signal, refreshed);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/postpone");
    expect(asked[0]?.body).toEqual({ path: "docs/A.md", slug: "s", action: "seen" });
    asked[0]?.answer(reply(200, { changed: false }));
    expect(await unchanged).toBeNull();
    expect(refreshed.refresh).not.toHaveBeenCalled();
    const scoped = { ...card, scope: "design~policy" };
    const changed = seen(scoped, new AbortController().signal, refreshed);
    await asking(asked, 2);
    expect(asked[1]?.url).toBe("/@design~policy/api/postpone");
    asked[1]?.answer(reply(200, { changed: true }));
    expect(await changed).toBeNull();
    expect(refreshed.refresh).toHaveBeenCalledTimes(1);
  });

  it("says what failed, unless the page is gone", async () => {
    const asked = server();
    const refreshed = frame();
    const refused = seen(card, new AbortController().signal, refreshed);
    await asking(asked, 1);
    asked[0]?.answer(reply(409, { error: "The card moved." }));
    expect(await refused).toBe("The card moved.");
    const page = new AbortController();
    const gone = seen(card, page.signal, refreshed);
    await asking(asked, 2);
    page.abort();
    asked[1]?.answer(reply(200, { changed: true }));
    expect(await gone).toBeNull();
    const failed = new AbortController();
    const lost = seen(card, failed.signal, refreshed);
    await asking(asked, 3);
    failed.abort();
    asked[2]?.answer(reply(409, { error: "The card moved." }));
    expect(await lost).toBeNull();
    expect(refreshed.refresh).not.toHaveBeenCalled();
  });
});
