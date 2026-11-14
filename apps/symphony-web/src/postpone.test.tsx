import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PostponeControls, postpone, seen } from "./postpone";
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
