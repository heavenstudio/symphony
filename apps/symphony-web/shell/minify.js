// The shell's minifier (package.sh): a module script read from stdin, written minified to stdout by
// the minifier Vite ships and minifies the kinds with. Anything it cannot read is said on stderr
// and fails the package: a shell half minified is never published.
import { text } from "node:stream/consumers";
import { minify } from "vite";

const { code, errors } = await minify("shell.js", await text(process.stdin), { module: true });
if (errors.length > 0) {
  for (const error of errors)
    console.error(`symphony-web: the shell does not minify: ${error.message}`);
  process.exit(1);
}
process.stdout.write(code.endsWith("\n") ? code : `${code}\n`);
