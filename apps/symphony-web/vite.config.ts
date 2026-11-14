import { defineConfig } from "vitest/config";

// The web client's page kinds, each an entry the shell imports by the name the shell's manifest
// gives it (apps/symphony-ui/package.sh): `src/kinds/NAME.tsx` exports the kind's `run` and `stop`.
const KINDS = ["docs", "queue", "reports", "review"];

export default defineConfig({
  // No dev server and no public folder: the server serves the files package.sh publishes.
  publicDir: false,
  // No `.env` file is read, so none can change the bytes (its NODE_ENV would make React's
  // development build). The build's own NODE_ENV is production, set by its script (package.json).
  envDir: false,
  build: {
    // The browsers the Dioxus kinds already ask for (wasm, ES modules, top-level await).
    target: "es2023",
    // Vite's manifest, which package.sh reads to put each kind's script and styles in the shell's.
    manifest: true,
    // The shell imports one script per kind; it preloads nothing on a module's behalf.
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
