// One source, two builds: `--mode react` (React 19) and `--mode preact` (the same JSX on Preact
// through its compat alias). Each writes a folder the Symphony server serves as it serves the
// Dioxus client (`SYMPHONY_UI_DIST=dist/MODE`): an `index.html` and content-hashed `assets/`
// (16 hex digits, the server's rule for a file it caches for good), so both clients are measured
// through the same static handler, gzip and cache headers.
//
// The pages draw with the Dioxus client's own style sheets, copied byte for byte from a built
// distribution (`SYMPHONY_DIOXUS_DIST`, default the committed one), each page loading the sheets
// that client's page kind loads: the comparison is of the code, not the CSS.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import fs from "node:fs";
import path from "node:path";

const here = path.dirname(new URL(import.meta.url).pathname);
const repo = path.resolve(here, "../..");

/// The kinds' style sheets in the Dioxus distribution at `dist` (its `public/` folder), read from
/// the module manifest on the first line of the shell its `index.html` names.
function dioxusStyles(dist) {
  const index = fs.readFileSync(path.join(dist, "index.html"), "utf8");
  const shell = index.match(/src="\/assets\/(symphony-shell-[0-9a-f]{16}\.js)"/)[1];
  const first = fs.readFileSync(path.join(dist, "assets", shell), "utf8").split("\n")[0];
  const modules = JSON.parse(first.replace(/^const modules = /, "").replace(/;$/, ""));
  return { board: modules.board.styles, task: modules.tasks.styles, document: modules.docs.styles };
}

function styles(dist, kinds) {
  const files = [...new Set(Object.values(kinds).flat())];
  return {
    name: "dioxus-styles",
    transformIndexHtml(html) {
      // The sheet every page loads goes in the head, as a page's first paint needs it.
      return html.replace("</head>", `<link rel="stylesheet" href="${kinds.board[0]}">\n</head>`);
    },
    generateBundle() {
      for (const file of files) {
        this.emitFile({ type: "asset", fileName: file.slice(1), source: fs.readFileSync(path.join(dist, file)) });
      }
    },
  };
}

export default defineConfig(({ mode, command }) => {
  const candidate = mode === "preact" ? "preact" : "react";
  const dist = process.env.SYMPHONY_DIOXUS_DIST || path.join(repo, "apps/symphony-server/client/public");
  const kinds = dioxusStyles(dist);
  const server = process.env.SYMPHONY_URL || "http://127.0.0.1:8000";
  return {
    plugins: [react(), styles(dist, kinds)],
    define: { __STYLES__: JSON.stringify(kinds), __CANDIDATE__: JSON.stringify(candidate) },
    resolve: {
      alias:
        candidate === "preact"
          ? {
              "react-dom/client": "preact/compat/client",
              "react-dom": "preact/compat",
              "react/jsx-runtime": "preact/jsx-runtime",
              react: "preact/compat",
            }
          : {},
    },
    build: {
      outDir: `dist/${candidate}`,
      emptyOutDir: true,
      assetsDir: "assets",
      // No preload polyfill: every browser measured supports modulepreload.
      modulePreload: { polyfill: false },
      rolldownOptions: {
        output: {
          hashCharacters: "hex",
          entryFileNames: "assets/[name]-[hash:16].js",
          chunkFileNames: "assets/[name]-[hash:16].js",
          assetFileNames: "assets/[name]-[hash:16][extname]",
        },
      },
    },
    // `npm run dev`: the API, the feed and the style sheets from a Symphony server of one's own.
    server: command === "serve" ? { proxy: { "/api": server, "/assets/": server } } : undefined,
  };
});
