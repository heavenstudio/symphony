// Builds every page (`vite build --mode NAME`) and prints what each ships: its script minified,
// gzipped at the best level as the Symphony's server sends it, and Brotli at quality 11, then each
// page's gzip beyond the baseline's (React and the page read as a whole).
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

const PAGES = ["baseline", "store", "suspense", "swr", "tanstack", "tanstack-persist"];

const sizes = {};
for (const page of PAGES) {
  execFileSync("npx", ["vite", "build", "--mode", page, "--logLevel", "error"], {
    stdio: "inherit",
  });
  const dir = join("dist", page);
  let raw = 0;
  let gzip = 0;
  let brotli = 0;
  for (const file of readdirSync(dir).filter((name) => name.endsWith(".js"))) {
    const bytes = readFileSync(join(dir, file));
    raw += bytes.length;
    gzip += gzipSync(bytes, { level: 9 }).length;
    brotli += brotliCompressSync(bytes, {
      params: { [constants.BROTLI_PARAM_QUALITY]: 11 },
    }).length;
  }
  sizes[page] = { raw, gzip, brotli };
}

const base = sizes.baseline;
console.log("page              minified    gzip  brotli   minified and gzip beyond the baseline");
for (const page of PAGES) {
  const { raw, gzip, brotli } = sizes[page];
  const added = page === "baseline" ? "" : `${raw - base.raw} ${gzip - base.gzip}`;
  console.log(
    `${page.padEnd(16)} ${String(raw).padStart(9)} ${String(gzip).padStart(7)} ${String(brotli).padStart(7)}   ${added}`,
  );
}
