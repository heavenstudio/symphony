// The shell's minifier (package.sh): the TypeScript files named, in their order, each with its types
// stripped on its own (strip.ts) and joined as one module script, written minified to stdout by the
// minifier Vite ships and minifies the kinds with. `-` names stdin. Anything it cannot read is said
// on stderr and fails the package: a shell half minified is never published. The join is read as
// the page reads it first, a module, so a name two files both declare is refused here, where the
// minifier would keep one of them.
//
//     node shell/minify.ts FILE…      package.sh: cache.ts, then shell.ts without its manifest line

import { readFileSync } from "node:fs";
import { text } from "node:stream/consumers";
import { minify, parseSync } from "vite";
import { strip } from "./strip.ts";

const files = process.argv.slice(2);
if (files.length === 0) {
  console.error("usage: node shell/minify.ts FILE…");
  process.exit(2);
}
const sources: string[] = [];
for (const file of files) {
  const named = file === "-" ? "stdin" : file;
  let source: string;
  try {
    source = file === "-" ? await text(process.stdin) : readFileSync(file, "utf8");
  } catch (error) {
    console.error(`symphony-web: the shell's ${named} cannot be read: ${(error as Error).message}`);
    process.exit(1);
  }
  try {
    sources.push(strip(source));
  } catch (error) {
    console.error(`symphony-web: the shell's ${named} does not strip: ${(error as Error).message}`);
    process.exit(1);
  }
}
const joined = sources.join("");
const read = parseSync("shell.js", joined, { sourceType: "module", showSemanticErrors: true });
if (read.errors.length > 0) {
  for (const error of read.errors)
    console.error(`symphony-web: the shell is no module: ${error.message}`);
  process.exit(1);
}
const { code, errors } = await minify("shell.js", joined, { module: true });
if (errors.length > 0) {
  for (const error of errors)
    console.error(`symphony-web: the shell does not minify: ${error.message}`);
  process.exit(1);
}
process.stdout.write(code.endsWith("\n") ? code : `${code}\n`);
