// `report.sh OUT` (`npm.sh run report -- OUT`): the bundle report of the build in OUT, what each
// kind makes a browser fetch and its bytes, as built and gzipped at the server's level. What it
// says is src/bundle-report.ts's, typed and tested; this is the Node around it, which the pinned
// Node runs as it is, stripping its types.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import { bundleReport } from "./src/bundle-report.ts";

const out = process.argv[2];
if (process.argv.length !== 3 || !out) {
  console.error("usage: report.sh OUT");
  process.exit(2);
}
const web = join(out, ".modules", "web");
try {
  const manifest = JSON.parse(readFileSync(join(web, ".vite", "manifest.json"), "utf8"));
  const report = bundleReport(manifest, (file) => {
    const bytes = readFileSync(join(web, file));
    return { raw: bytes.length, gzip: gzipSync(bytes, { level: 9 }).length };
  });
  process.stdout.write(report);
} catch (error) {
  console.error(`symphony-web: no bundle report of ${out}: ${(error as Error).message}`);
  process.exit(2);
}
