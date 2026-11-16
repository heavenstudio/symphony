import { describe, expect, it } from "vitest";
import { bundleReport, kinds, type Manifest, SHARED, type Size } from "./bundle-report";

/** A build of two kinds: both import one chunk, and `task` defers a timeline that brings another. */
const MANIFEST: Manifest = {
  [SHARED]: { file: "assets/shared-1.css", isEntry: true },
  "src/kinds/task.tsx": {
    file: "assets/task-1.js",
    name: "task",
    isEntry: true,
    imports: ["_live-1.js", "_media-1.js"],
    dynamicImports: ["src/timeline.tsx"],
    css: ["assets/task-1.css"],
  },
  "src/kinds/board.tsx": {
    file: "assets/board-1.js",
    name: "board",
    isEntry: true,
    imports: ["_live-1.js"],
  },
  "_live-1.js": { file: "assets/live-1.js", imports: ["_react-1.js"] },
  "_react-1.js": { file: "assets/react-1.js" },
  "_media-1.js": {
    file: "assets/media-1.js",
    imports: ["_react-1.js"],
    css: ["assets/media-1.css"],
  },
  "src/timeline.tsx": {
    file: "assets/timeline-1.js",
    imports: ["_live-1.js", "_rows-1.js"],
    dynamicImports: ["src/attach.tsx", "src/kinds/task.tsx"],
    css: ["assets/timeline-1.css"],
  },
  "_rows-1.js": { file: "assets/rows-1.js" },
  "src/attach.tsx": { file: "assets/attach-1.js", css: ["assets/attach-1.css"] },
};

/** A file's bytes: its name's length a hundred times over, a tenth of it gzipped. */
function size(file: string): Size {
  return { raw: file.length * 100, gzip: file.length * 10 };
}

describe("kinds", () => {
  it("names each kind's first visit, shared sheet first, then its CSS and scripts once each", () => {
    expect(kinds(MANIFEST)).toEqual([
      {
        kind: "board",
        first: [
          "assets/shared-1.css",
          "assets/react-1.js",
          "assets/live-1.js",
          "assets/board-1.js",
        ],
        later: [],
      },
      {
        kind: "task",
        first: [
          "assets/shared-1.css",
          "assets/media-1.css",
          "assets/task-1.css",
          "assets/react-1.js",
          "assets/live-1.js",
          "assets/media-1.js",
          "assets/task-1.js",
        ],
        later: [
          "assets/timeline-1.css",
          "assets/attach-1.css",
          "assets/rows-1.js",
          "assets/timeline-1.js",
          "assets/attach-1.js",
        ],
      },
    ]);
  });

  it("names a kind by its entry's path when the build gave it no name", () => {
    const manifest: Manifest = {
      [SHARED]: { file: "assets/shared-1.css", isEntry: true },
      "src/kinds/queue.tsx": { file: "assets/queue-1.js", isEntry: true },
    };
    expect(kinds(manifest).map(({ kind }) => kind)).toEqual(["src/kinds/queue.tsx"]);
  });

  it("refuses a manifest with no shared stylesheet, no kind, or an import it does not hold", () => {
    expect(() => kinds({ "src/kinds/queue.tsx": { file: "q.js", isEntry: true } })).toThrow(
      "the manifest has no shared stylesheet (src/styles/shared.css)",
    );
    expect(() => kinds({ [SHARED]: { file: "s.css", isEntry: true } })).toThrow(
      "the manifest has no kind",
    );
    expect(() =>
      kinds({
        [SHARED]: { file: "s.css", isEntry: true },
        "src/kinds/queue.tsx": {
          file: "q.js",
          name: "queue",
          isEntry: true,
          imports: ["_gone.js"],
        },
      }),
    ).toThrow("the manifest names _gone.js and holds no such chunk");
  });
});

describe("bundleReport", () => {
  it("writes each kind's totals and files, the deferred ones apart, then the build's", () => {
    expect(bundleReport(MANIFEST, size)).toBe(
      [
        "board: first visit 4 files, 6900 bytes, 690 gzipped",
        "  assets/shared-1.css  1900 bytes, 190 gzipped",
        "  assets/react-1.js  1700 bytes, 170 gzipped",
        "  assets/live-1.js  1600 bytes, 160 gzipped",
        "  assets/board-1.js  1700 bytes, 170 gzipped",
        "task: first visit 7 files, 12000 bytes, 1200 gzipped; later: 5 files, 9400 bytes, 940 gzipped",
        "  assets/shared-1.css  1900 bytes, 190 gzipped",
        "  assets/media-1.css  1800 bytes, 180 gzipped",
        "  assets/task-1.css  1700 bytes, 170 gzipped",
        "  assets/react-1.js  1700 bytes, 170 gzipped",
        "  assets/live-1.js  1600 bytes, 160 gzipped",
        "  assets/media-1.js  1700 bytes, 170 gzipped",
        "  assets/task-1.js  1600 bytes, 160 gzipped",
        "  later assets/timeline-1.css  2100 bytes, 210 gzipped",
        "  later assets/attach-1.css  1900 bytes, 190 gzipped",
        "  later assets/rows-1.js  1600 bytes, 160 gzipped",
        "  later assets/timeline-1.js  2000 bytes, 200 gzipped",
        "  later assets/attach-1.js  1800 bytes, 180 gzipped",
        "the build: 13 files, 23100 bytes, 2310 gzipped",
        "",
      ].join("\n"),
    );
  });

  it("says one file as one file", () => {
    const manifest: Manifest = {
      [SHARED]: { file: "s.css", isEntry: true },
      "src/kinds/queue.tsx": { file: "s.css", name: "queue", isEntry: true },
    };
    expect(bundleReport(manifest, size)).toBe(
      "queue: first visit 1 file, 500 bytes, 50 gzipped\n  s.css  500 bytes, 50 gzipped\nthe build: 1 file, 500 bytes, 50 gzipped\n",
    );
  });
});
