import { describe, expect, it } from "vitest";
import { Backoff, keepsPanelBaseline, MetadataReads, reloadFor, tryAgain } from "./state";

describe("the waits between tries of a lost feed", () => {
  it("double from a second to half a minute while streams end at once", () => {
    const backoff = new Backoff();
    const waits = Array.from({ length: 7 }, () => backoff.after(0));
    expect(waits).toEqual([1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000]);
  });

  it("start again after a stream that held half a minute", () => {
    const backoff = new Backoff();
    backoff.after(0);
    backoff.after(29_999);
    expect(backoff.after(30_000)).toBe(1_000);
    expect(backoff.wait({ lost: 0 })).toBe(2_000);
  });

  it("are none after a page that went away, and start again", () => {
    const backoff = new Backoff();
    backoff.after(0);
    backoff.after(0);
    expect(backoff.wait("away")).toBe(0);
    expect(backoff.wait({ lost: 10 })).toBe(1_000);
  });
});

describe("a failed read tried again by itself", () => {
  /** `tryAgain` with its waits recorded rather than slept, `answers` reads before it is answered;
   * `left` ends the page at that wait (1 for the first). */
  async function tried(answers: number, left = 0, signal = new AbortController()) {
    const waited: number[] = [];
    let reads = 0;
    await tryAgain(
      () => reads < answers,
      async () => {
        reads += 1;
      },
      signal.signal,
      async (ms, given) => {
        expect(given).toBe(signal.signal);
        waited.push(ms);
        if (waited.length === left) signal.abort();
      },
    );
    return { waited, reads };
  }

  it("reads again after each of the feed's waits, longer each time, until it is answered", async () => {
    expect(await tried(7)).toEqual({
      waited: [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000],
      reads: 7,
    });
  });

  it("does nothing for a read already answered, or one the page no longer waits for", async () => {
    expect(await tried(0)).toEqual({ waited: [], reads: 0 });
    // The page moved on while it waited (another route, a newer read, a 404): no read after it.
    let waiting = true;
    let reads = 0;
    await tryAgain(
      () => waiting,
      async () => {
        reads += 1;
      },
      new AbortController().signal,
      async () => {
        waiting = false;
      },
    );
    expect(reads).toBe(0);
  });

  it("stops once the page leaves, the wait it was in with it", async () => {
    expect(await tried(5, 2)).toEqual({ waited: [1_000, 2_000], reads: 1 });
    const gone = new AbortController();
    gone.abort();
    expect(await tried(5, 0, gone)).toEqual({ waited: [], reads: 0 });
  });
});

describe("a page loaded again for another build", () => {
  it("is loaded for another build, once", () => {
    expect(reloadFor("/a.js", "/b.js", null)).toBe(true);
    expect(reloadFor("/a.js", "/b.js", "/c.js")).toBe(true);
    expect(reloadFor("/a.js", "/b.js", "/b.js")).toBe(false);
  });

  it("is not for its own build, nor when it cannot name its own", () => {
    expect(reloadFor("/a.js", "/a.js", null)).toBe(false);
    expect(reloadFor(null, "/b.js", null)).toBe(false);
  });
});

describe("the panel's baseline", () => {
  it("stays while an earlier read already announced an arrival the panel has not shown", () => {
    expect(keepsPanelBaseline(["a"], ["a", "b"], false)).toBe(true);
  });

  it("moves once the panel is ready, with nothing new, or before any read", () => {
    expect(keepsPanelBaseline(["a"], ["a", "b"], true)).toBe(false);
    expect(keepsPanelBaseline(["a", "b"], ["a"], false)).toBe(false);
    expect(keepsPanelBaseline(null, ["a"], false)).toBe(false);
    expect(keepsPanelBaseline(["a"], null, false)).toBe(false);
  });
});

describe("the frame's metadata reads", () => {
  it("let the first connection reuse an initial read that is active or done", () => {
    let reads = new MetadataReads();
    let started = reads.start();
    expect(reads.connected()).toBe(false);
    expect(reads.finished(started, true)).toBe(false);
    expect(reads.connected()).toBe(true);

    reads = new MetadataReads();
    started = reads.start();
    expect(reads.finished(started, true)).toBe(false);
    expect(reads.connected()).toBe(false);
  });

  it("try a failed initial read once more on either side of the handshake", () => {
    let reads = new MetadataReads();
    let started = reads.start();
    expect(reads.finished(started, false)).toBe(false);
    expect(reads.connected()).toBe(true);

    reads = new MetadataReads();
    started = reads.start();
    expect(reads.connected()).toBe(false);
    expect(reads.finished(started, false)).toBe(true);
    const retried = reads.start();
    expect(reads.finished(retried, false)).toBe(false);
    expect(reads.connected()).toBe(true);
  });

  it("read again on a reconnect, even while an older read is active", () => {
    const reads = new MetadataReads();
    expect(reads.connected()).toBe(true);
    reads.start();
    expect(reads.connected()).toBe(true);
    const replacement = reads.start();
    expect(reads.finished(replacement, true)).toBe(false);
  });
});
