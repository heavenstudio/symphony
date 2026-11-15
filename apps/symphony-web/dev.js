// `just symphony-ui-dev SERVER [PORT]` (`npm.sh run dev -- SERVER PORT`): the web client's dev
// server, Vite's own over a Symphony server of the developer's own on port SERVER. What it does is
// src/dev-server.ts's, typed and tested; this is the Node around it. It starts nothing else, and
// Ctrl-C ends it.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createServer } from "vite";
import {
  devArgs,
  devConfig,
  origin,
  SHELL_SOURCES,
  shellPlugin,
  unreadable,
} from "./src/dev-server.ts";

/** Says why the dev server cannot run, and ends it. */
function refuse(why) {
  console.error(`✖ symphony-ui-dev: ${why}`);
  process.exit(2);
}

const root = import.meta.dirname;
let args;
try {
  args = devArgs(process.argv.slice(2));
} catch (error) {
  refuse(error.message);
}
const served = origin(args.server);
const problem = await unreadable(served);
if (problem !== null) {
  refuse(
    `${problem}: start a server of your own (\`just symphony --port 0\`) and name the port it prints`,
  );
}
const sources = {
  cache: join(root, SHELL_SOURCES.cache),
  shell: join(root, SHELL_SOURCES.shell),
  read: (path) => readFileSync(path, "utf8"),
};
try {
  const server = await createServer(devConfig(root, args, [shellPlugin(served, sources)]));
  await server.listen();
  console.log(
    `→ symphony-ui-dev at http://127.0.0.1:${server.httpServer.address().port}/ over the Symphony at ${served}/ (pid ${process.pid}; Ctrl-C stops it)`,
  );
} catch (error) {
  refuse(error.message);
}
