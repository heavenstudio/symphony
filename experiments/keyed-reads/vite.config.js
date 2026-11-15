import { defineConfig } from "vite";

// One build per page (`--mode NAME`, `src/NAME.jsx`), each a single entry, so a library's bytes
// are what one page of it ships, React's included; measure.mjs subtracts the baseline's. The build
// settings are the web client's (apps/symphony-web/vite.config.ts): es2023, no module preload.
export default defineConfig(({ mode }) => ({
  publicDir: false,
  build: {
    target: "es2023",
    modulePreload: false,
    reportCompressedSize: false,
    outDir: `dist/${mode}`,
    emptyOutDir: true,
    rolldownOptions: {
      input: { page: `src/${mode}.jsx` },
      preserveEntrySignatures: "strict",
      output: { entryFileNames: "page.js", chunkFileNames: "[name].js" },
    },
  },
}));
