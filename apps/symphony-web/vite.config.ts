import { defineConfig } from "vitest/config";

// The web client's kinds, each an entry the shell imports by the name the shell's manifest gives it
// (apps/symphony-ui/package.sh): `src/kinds/NAME.tsx` exports the kind's `run` and `stop`. The
// menus (`overlay`) and the chat (`panel`) are no page's but the shell's persistent kinds every page
// mounts beside its own.
const PAGES = ["agents", "board", "docs", "queue", "reports", "review", "runs", "task"];
const KINDS = [...PAGES, "overlay", "panel"].sort();

export default defineConfig({
  // No public folder: the server serves the files package.sh publishes (the dev server's own
  // settings, src/dev-server.ts, go over these).
  publicDir: false,
  // No `.env` file is read, so none can change the bytes (its NODE_ENV would make React's
  // development build). The build's own NODE_ENV is production, set by its script (package.json).
  envDir: false,
  build: {
    // The browsers the Dioxus kinds already ask for (wasm, ES modules, top-level await).
    target: "es2023",
    // Vite's manifest, which package.sh reads to put each kind's script and styles in the shell's.
    manifest: true,
    // The shell imports one script per kind, and preloads the chunks it imports beside it (package.sh
    // names them in the shell's manifest from this one): no preload helper of Vite's.
    modulePreload: false,
    reportCompressedSize: false,
    rolldownOptions: {
      input: Object.fromEntries(KINDS.map((kind) => [kind, `src/kinds/${kind}.tsx`])),
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
        // What every page kind imports is one chunk. Left to itself, Rolldown splits it by who
        // imports each module, the menus' share (React, the mount) from the pages' alone (the
        // frame), and every first visit pays for two files compressed apart (about 800 bytes).
        // The menus' entry imports that one chunk, which every page has loaded by the time the
        // shell warms or opens them.
        codeSplitting: {
          groups: [{ name: "shared", minShareCount: PAGES.length }],
        },
      },
    },
  },
  test: {
    include: ["src/**/*.test.{ts,tsx}"],
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
