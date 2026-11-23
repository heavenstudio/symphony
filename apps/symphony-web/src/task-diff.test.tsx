import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Parts, PartsContext } from "./parts";
import { DiffFiles, diffPart, loadFileList, readDiff, TaskDiffPanel } from "./task-diff";
import { PatchText, readPatch } from "./task-diff-files";
import type { TaskEvent } from "./task-live";
import { reply, until } from "./testing";
import type { TaskDiff } from "./wire";

const DIFF: TaskDiff = {
  source: "branch",
  base: "a".repeat(40),
  head: "b".repeat(40),
  files: [
    { path: "one.ts", added: 2, removed: 1 },
    { path: "binary.dat", added: null, removed: null },
  ],
  truncated: false,
  reason: null,
};

afterEach(() => vi.unstubAllGlobals());

// The files' chunk, come as it comes once a page drew a comparison with files: the panel then draws
// them at once (`a comparison's files before their chunk` draws them before).
beforeAll(async () => {
  await loadFileList();
});

it("draws the independent part's pending, answered, failed and missing states", async () => {
  expect(renderToStaticMarkup(<TaskDiffPanel scope="" slug="tools/x" />)).toContain(
    "Reading changed files",
  );
  for (const [status, value, words] of [
    [200, DIFF, "one.ts"],
    [200, {}, "not readable"],
    [503, { error: "Board unavailable" }, "Board unavailable"],
    [404, { error: "gone" }, "No comparison is available"],
  ] as const) {
    vi.stubGlobal("document", { getElementById: () => null });
    vi.stubGlobal("fetch", async () => reply(status, value));
    const mount = new AbortController();
    const parts = new Parts<TaskEvent>(mount.signal);
    const part = diffPart("", "tools/x");
    parts.open(part);
    await until(() =>
      expect(parts.shown(part).answered || parts.shown(part).error !== null).toBe(true),
    );
    const html = renderToStaticMarkup(
      <PartsContext.Provider value={parts}>
        <TaskDiffPanel scope="" slug="tools/x" />
      </PartsContext.Provider>,
    );
    mount.abort();
    expect(html).toContain(words);
  }
});

it("keeps a refresh failure visible beside the last files until a retry succeeds", async () => {
  for (const [status, value, words] of [
    [503, { error: "Board unavailable" }, "Board unavailable"],
    [200, {}, "not readable"],
  ] as const) {
    vi.stubGlobal("document", { getElementById: () => null });
    const fetch = vi.fn().mockResolvedValue(reply(200, DIFF));
    vi.stubGlobal("fetch", fetch);
    const mount = new AbortController();
    const parts = new Parts<TaskEvent>(mount.signal);
    const part = diffPart("", "tools/x");
    const draw = () =>
      renderToStaticMarkup(
        <PartsContext.Provider value={parts}>
          <TaskDiffPanel scope="" slug="tools/x" />
        </PartsContext.Provider>,
      );
    try {
      parts.open(part);
      await until(() => expect(parts.shown(part).answered).toBe(true));
      expect(draw()).toContain("one.ts");
      fetch.mockResolvedValue(reply(status, value));
      await parts.read(part);
      expect(draw()).toContain(words);
      expect(draw()).toContain("Showing files from the last successful read");
      expect(draw()).toContain("one.ts");

      let resolve: (response: Response) => void = () => {};
      fetch.mockReturnValue(
        new Promise<Response>((done) => {
          resolve = done;
        }),
      );
      const retry = parts.read(part);
      expect(draw()).toContain(words);
      expect(draw()).toContain('disabled="">Reading files…</button>');
      resolve(reply(200, { ...DIFF, files: [{ path: "two.ts", added: 3, removed: 0 }] }));
      await retry;
      const html = draw();
      expect(html).toContain("two.ts");
      expect(html).not.toContain("one.ts");
      expect(html).not.toContain(words);
      expect(html).not.toContain("last successful read");
      expect(html).toContain(">Refresh files</button>");
    } finally {
      mount.abort();
    }
  }
});

describe("the task's changed files", () => {
  it("checks every displayed field and accepts newer fields", () => {
    expect(readDiff(DIFF)).toEqual(DIFF);
    expect(readDiff({ ...DIFF, later: true })).toMatchObject(DIFF);
    expect(readDiff({ ...DIFF, source: "landing" })?.source).toBe("landing");
    expect(
      readDiff({
        source: "unavailable",
        base: null,
        head: null,
        files: [],
        truncated: false,
        reason: "Branch gone",
      })?.reason,
    ).toBe("Branch gone");
    for (const fields of [
      { source: "later" },
      { base: null },
      { head: null },
      { reason: 1 },
      { truncated: "no" },
      { files: null },
      { files: [{ ...DIFF.files[0], path: 1 }] },
      ...[-1, 1.5, "1", undefined, Number.NaN].map((added) => ({
        files: [{ path: "x", added, removed: 0 }],
      })),
    ])
      expect(readDiff({ ...DIFF, ...fields })).toBeNull();
    expect(readDiff(null)).toBeNull();
  });

  it("draws counts before any hunks, escaping paths and explaining unavailable and bounded reads", () => {
    const draw = (diff: TaskDiff) =>
      renderToStaticMarkup(<DiffFiles diff={diff} scope="" slug="tools/x" />);
    const html = draw(DIFF);
    expect(html).toContain("Branch from its merge base");
    expect(html).toContain("+2");
    expect(html).toContain("−1");
    expect(html).toContain("Binary file");
    expect(html).toContain("<details>");
    expect(html).not.toContain("<pre");
    expect(draw({ ...DIFF, source: "landing" })).toContain("Landing merge");
    expect(draw({ ...DIFF, files: [] })).toContain("No changed files");
    expect(
      draw({ ...DIFF, files: [], truncated: true, reason: "At most 500 files" }),
    ).not.toContain("No changed files");
    expect(
      draw({
        ...DIFF,
        source: "unavailable",
        base: null,
        head: null,
        files: [],
        reason: "Branch gone",
      }),
    ).toContain("Branch gone");
    expect(draw({ ...DIFF, files: [{ path: "<script>.ts", added: 0, removed: 0 }] })).toContain(
      "&lt;script&gt;.ts",
    );
  });

  it("refreshes for branch changes and the task's landing, leaving unrelated posts alone", () => {
    const part = diffPart("branch~one", "tools/x");
    expect(part.url).toContain("/@branch~one/api/task-diff?slug=tools%2Fx");
    const hear = (changes: TaskEvent["changes"]) => part.hear?.(DIFF, { changes, page: null });
    expect(hear([])).toBeNull();
    expect(hear([{ region: "branches", moved: ["tools/x"] }])).toEqual({
      value: DIFF,
      again: true,
    });
    expect(hear([{ region: "task", change: { key: "tools/x", removed: true } }])?.again).toBe(true);
    const entry = {
      message: {
        id: 1,
        slug: "tools/x",
        sender: "land",
        recipient: null,
        kind: "verdict",
        text: "landed",
        ts: 1,
        person: false,
      },
      stamp: "now",
      blocks: [],
    };
    expect(hear([{ region: "message", entry }])?.again).toBe(true);
    expect(
      hear([
        { region: "message", entry: { ...entry, message: { ...entry.message, kind: "msg" } } },
      ]),
    ).toBeNull();
  });
});

describe("a comparison's files before their chunk", () => {
  it("say they are read, a region the first screen waits for, then come as their chunk does", async () => {
    vi.resetModules();
    const fresh = await import("./task-diff");
    const draw = () =>
      renderToStaticMarkup(<fresh.DiffFiles diff={DIFF} scope="" slug="tools/x" />);
    const before = draw();
    expect(before).toContain("Branch from its merge base");
    expect(before).toContain(
      '<p role="status" aria-busy="true" data-pending="true">Reading changed files…</p>',
    );
    expect(before).not.toContain("one.ts");
    await fresh.loadFileList();
    expect(draw()).toContain("one.ts");
    expect(draw()).not.toContain("Reading changed files");
  });

  it("are not asked for where the comparison has none to list", async () => {
    vi.resetModules();
    const fresh = await import("./task-diff");
    for (const diff of [
      { ...DIFF, files: [] },
      { ...DIFF, source: "unavailable", base: null, head: null, files: [] },
    ] as TaskDiff[]) {
      const html = renderToStaticMarkup(<fresh.DiffFiles diff={diff} scope="" slug="tools/x" />);
      expect(html).not.toContain("Reading changed files");
    }
  });
});

describe("a single file's patch", () => {
  it("requires the requested immutable comparison and path", () => {
    const patch = { base: "a", head: "b", path: "x", patch: "@@\n+added\n", truncated: false };
    expect(readPatch(patch, "a", "b", "x")).toEqual(patch);
    for (const fields of [
      { base: "c" },
      { head: "c" },
      { path: "y" },
      { patch: 1 },
      { truncated: "false" },
    ]) {
      expect(readPatch({ ...patch, ...fields }, "a", "b", "x")).toBeNull();
    }
    expect(readPatch(null, "a", "b", "x")).toBeNull();
  });

  it("keeps literal patch text in a keyboard-scrollable region and states truncation", () => {
    const html = renderToStaticMarkup(<PatchText patch={"@@\n+<script>\n-old\n"} truncated />);
    expect(html).toContain('tabindex="0"');
    expect(html).toContain('aria-label="File patch"');
    expect(html).toContain("+&lt;script&gt;");
    expect(html).toContain("Patch stops at 256 KiB");
    expect(renderToStaticMarkup(<PatchText patch="binary" truncated={false} />)).not.toContain(
      "stops at",
    );
  });
});
