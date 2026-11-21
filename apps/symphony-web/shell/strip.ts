// The shell's TypeScript as the JavaScript a page runs (its minifier, minify.ts; the dev server,
// dev.ts; the browser tests' store): its types stripped by the pinned Node's own stripper, the one
// that runs a `.ts` file such as this one. It puts blanks where each type was, so every other byte
// keeps its place, a line of the result is the source's line, and the comments stay. It strips and
// transforms nothing else, so the shell is written in the syntax a type erases alone (no enum,
// namespace or parameter property), which it refuses otherwise. Node warns that the call is
// experimental: its scripts run with `--disable-warning=ExperimentalWarning` (package.json).
//
//     node shell/strip.ts FILE        FILE as JavaScript, on stdout

import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";

/** `source`, TypeScript, as JavaScript: each of its types blanks the length of its text. */
export function strip(source: string): string {
  return stripTypeScriptTypes(source, { mode: "strip" });
}

if (import.meta.main) {
  const [file, ...rest] = process.argv.slice(2);
  if (file === undefined || rest.length > 0) {
    console.error("usage: node shell/strip.ts FILE");
    process.exit(2);
  }
  try {
    process.stdout.write(strip(readFileSync(file, "utf8")));
  } catch (error) {
    console.error(`symphony-web: ${file} does not strip: ${(error as Error).message}`);
    process.exit(1);
  }
}
