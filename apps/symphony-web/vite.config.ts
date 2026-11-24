import { defineConfig } from "vitest/config";
import { BACKSTOP_MS, UnitRunReport } from "./src/unit-run.ts";

// The web client's kinds, each an entry the shell imports by the name the shell's manifest gives it
// (package.sh): `src/kinds/NAME.tsx` exports the kind's `run` and `stop`. The menus (`overlay`) and
// the chat (`panel`) are no page's but the shell's persistent kinds every page mounts beside its
// own.
const PAGES = [
  "agents",
  "board",
  "docs",
  "people",
  "queue",
  "reports",
  "review",
  "runs",
  "settings",
  "task",
];
const KINDS = [...PAGES, "overlay", "panel"].sort();
// The stylesheet every kind shares, an entry of its own and no kind: its sheets in the order
// `src/styles/shared.css` writes, one minified file package.sh puts first in every kind's styles.
const SHARED_STYLES = "src/styles/shared.css";

// Vite wraps each `import()` in its preload helper, which asks for the chunk's sheets and module
// preloads beside it. Neither is wanted here: preloads are off (`modulePreload`), and a chunk a page
// imports when it needs it puts its own sheet in the page (timeline.tsx, flow.tsx). So the helper
// is `src/shared.ts`'s, which only calls the import, in the chunk every page shares: Vite's own
// would be a file of its own beside each page that imports such a chunk.
const PRELOAD_HELPER = "\0vite/preload-helper.js";
/** `src/shared.ts`'s path, beside this file (its URL's path decoded: no Node types are taken). */
const SHARED_MODULE = decodeURIComponent(new URL("./src/shared.ts", import.meta.url).pathname);
const preloadHelper = {
  name: "symphony:preload-helper",
  enforce: "pre" as const,
  resolveId: (id: string) => (id === PRELOAD_HELPER ? SHARED_MODULE : null),
};

export default defineConfig({
  plugins: [preloadHelper],
  // No public folder: the server serves the files package.sh publishes (the dev server's own
  // settings, src/dev-server.ts, go over these).
  publicDir: false,
  // No `.env` file is read, so none can change the bytes (its NODE_ENV would make React's
  // development build). The build's own NODE_ENV is production, set by its script (package.json).
  envDir: false,
  build: {
    // The browsers the shell already asks for (ES modules, top-level await).
    target: "es2023",
    // Vite's manifest, which package.sh reads to put each kind's script and styles in the shell's.
    manifest: true,
    // The actual bundled packages and their installed license text, consumed by package.sh.
    license: { fileName: ".vite/licenses.json" },
    // The shell imports one script per kind, and preloads the chunks it imports beside it (package.sh
    // names them in the shell's manifest from this one): no preload helper of Vite's.
    modulePreload: false,
    reportCompressedSize: false,
    rolldownOptions: {
      input: {
        ...Object.fromEntries(KINDS.map((kind) => [kind, `src/kinds/${kind}.tsx`])),
        shared: SHARED_STYLES,
      },
      // A kind's entry keeps its exports, which the shell calls (an app build drops them, as a page
      // loaded by a script tag has no use for them).
      preserveEntrySignatures: "strict",
      output: {
        // Sixteen hex digits, as package.sh names its own files: the server caches a file so named
        // for good (apps/symphony-server/src/api/client.rs, `content_hashed`).
        hashCharacters: "hex",
        entryFileNames: "assets/[name]-[hash:16].js",
        // A chunk the kinds share is named so, not for whichever of its modules Rolldown would name
        // it after (a kind's, which another kind's page would then seem to load); a chunk a kind
        // imports when it needs it keeps its own name.
        chunkFileNames: (chunk) =>
          chunk.isDynamicEntry ? "assets/[name]-[hash:16].js" : "assets/shared-[hash:16].js",
        assetFileNames: "assets/[name]-[hash:16][extname]",
        // Eight page kinds share the frame and the device cache/live module. Compress them as
        // one chunk, avoiding a separate request on each first visit. Runs takes the cache code
        // too; the menus reuse this chunk already loaded by whichever page opened them.
        //
        // Keep cache.ts with live.tsx even if their importers differ: Reports uses the cache and
        // the page's notices, so splitting them would add a file to every page following the board.
        // So too a task's kind (kind.ts, which Reports reads as well) and whose a task is
        // (owner-read.ts, which a document's cards read as well): each a few hundred bytes that
        // two pages share, which as a file of its own costs a first visit more in its answer's
        // headers than in its code.
        codeSplitting: {
          groups: [
            { name: "shared", minShareCount: PAGES.length - 1 },
            { name: "kept", test: /\/src\/(cache|live|kind|owner-read)\.tsx?$/ },
          ],
        },
      },
    },
  },
  test: {
    include: ["src/**/*.test.{ts,tsx}", "shell/*.test.ts"],
    // A test waits on its condition, never for a time of its own (time is Vitest's fake timers, a
    // wait for a condition testing.ts's `until`). The one time a test, a hook or a wait has is a
    // backstop only a stall reaches (a machine asleep, a process stopped). The report beside
    // Vitest's says each one that ran out as a stall, with the load average, and names each test
    // file the run did not run (src/unit-run.ts).
    testTimeout: BACKSTOP_MS,
    hookTimeout: BACKSTOP_MS,
    reporters: ["default", new UnitRunReport()],
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      // The generated types, each kind's lines that mount it (the browser tests run them), and the
      // tests with what they share.
      exclude: ["src/wire.ts", "src/kinds/**", "src/**/*.test.{ts,tsx}", "src/testing.ts"],
      reporter: ["text"],
    },
  },
});
