// The web client's dev server (`just symphony-ui-dev SERVER [PORT]`, which runs `dev.ts`): Vite's
// own, over vite.config.ts, serving this package's kinds from their sources, a page reloaded on
// each save, and handing every other request to a Symphony server of the developer's own. No kind
// imports this module, so no build carries it: Node runs it under dev.ts, and Vitest beside it.

import type { InlineConfig, Plugin } from "vite";

/** Where Vite serves its own files: the one prefix the dev server keeps from the Symphony server. */
export const BASE = "/@symphony-web/";

/** The shared server's port (`just symphony`): never the dev server's, nor the one it reads. */
export const SHARED_PORT = 8765;

/** What the dev server proxies: every request outside [`BASE`]. */
export const PROXIED = "^/(?!@symphony-web/)";

/** The shell's sources (package.sh's order), from this package's folder. */
export const SHELL_SOURCES = {
  cache: "shell/cache.ts",
  shell: "shell/shell.ts",
} as const;

/** The shell's line package.sh writes the kinds' manifest in. */
const MANIFEST_LINE = "const modules = __MODULES__;";

/** A page's shell script, as package.sh names it. */
const SHELL_PATH = /^\/assets\/symphony-shell-[0-9a-f]{16}\.js$/;

export const USAGE =
  "usage: just symphony-ui-dev SERVER [PORT]: SERVER is the port of a server of your own (`just symphony --port 0` prints it), PORT the dev server's (0, the default, for any)";

export interface DevArgs {
  /** The port of the Symphony server it reads from. */
  server: number;
  /** Its own port on the loopback address, 0 for one the system picks. */
  port: number;
}

/** The arguments `dev.ts` takes, SERVER and an optional PORT, or why they cannot be. */
export function devArgs(args: readonly string[]): DevArgs {
  const [server, port = "0", ...rest] = args;
  if (server === undefined || rest.length > 0) throw new Error(USAGE);
  const parsed = { server: portOf(server, "SERVER"), port: portOf(port, "PORT") };
  if (parsed.server === 0) {
    throw new Error(`SERVER is the port a server listens on, not 0; ${USAGE}`);
  }
  for (const [name, value] of Object.entries(parsed)) {
    if (value === SHARED_PORT) {
      throw new Error(
        `${name.toUpperCase()} ${SHARED_PORT} is the shared server's port, which the dev server never reads from or takes: start a server of your own (\`just symphony --port 0\`)`,
      );
    }
  }
  return parsed;
}

function portOf(word: string, name: string): number {
  const port = /^\d{1,5}$/.test(word) ? Number(word) : Number.NaN;
  if (!(port <= 65535)) throw new Error(`${name} ${JSON.stringify(word)} is not a port; ${USAGE}`);
  return port;
}

/** The address of the Symphony server on port `port`, on this machine. */
export function origin(port: number): string {
  return `http://127.0.0.1:${port}`;
}

/** Why the server at `served` cannot be read from, or null once it answers with a Symphony page. */
export async function unreadable(served: string, wait = 5000): Promise<string | null> {
  try {
    const answer = await fetch(`${served}/`, { signal: AbortSignal.timeout(wait) });
    const page = await answer.text();
    if (answer.ok && /src="\/assets\/symphony-shell-[0-9a-f]{16}\.js"/.test(page)) return null;
    return `${served}/ answered ${answer.status} with no Symphony page`;
  } catch (error) {
    return `nothing answers at ${served}/ (${error instanceof Error ? error.message : String(error)})`;
  }
}

/** A kind's files in a shell's manifest: package.sh puts the shared stylesheet first in `styles`. */
type Files = Record<string, unknown> & { styles: string[] };

type Manifest = Record<string, Files>;

/** The kinds' manifest a built shell holds on its first line (package.sh). */
export function readManifest(shell: string): Manifest {
  const json = /^const modules = (.*);$/.exec(shell.split("\n", 1)[0] ?? "")?.[1];
  let manifest: unknown;
  try {
    manifest = json === undefined ? undefined : JSON.parse(json);
  } catch {
    manifest = undefined;
  }
  if (
    typeof manifest !== "object" ||
    manifest === null ||
    Array.isArray(manifest) ||
    !Object.values(manifest).every((files) => typeof files === "object" && files !== null)
  ) {
    throw new Error("the server's shell holds no manifest of its page kinds on its first line");
  }
  return manifest as Manifest;
}

/**
 * The manifest of the shell the dev server serves: each kind of this checkout's `kinds` as Vite
 * serves its entry under `base`, with the stylesheet every kind of the server's build shares
 * (package.sh puts it first in each) as its one style, since Vite puts each kind's own in the page
 * as its module asks. A kind the server's build has and the checkout does not is no more.
 */
export function devManifest(
  served: Manifest,
  kinds: Record<string, string>,
  base: string,
): Manifest {
  const shared = Object.values(served)[0]?.styles.slice(0, 1) ?? [];
  const manifest: Manifest = {};
  for (const [kind, entry] of Object.entries(kinds)) {
    manifest[kind] = { script: `${base}${entry}`, imports: [], styles: shared };
  }
  return manifest;
}

/**
 * Vite puts a kind's own styles at the end of the head, where the built shell puts them before the
 * project's theme package, which wins the cascade (shell.ts, `styles`): each is moved there.
 */
export const THEME_LAST = `new MutationObserver((records) => {
  const theme = document.head.querySelector("[data-symphony-theme]");
  for (const node of records.flatMap((record) => [...record.addedNodes])) {
    if (theme && node.matches?.("style[data-vite-dev-id]") && theme.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) {
      document.head.insertBefore(node, theme);
    }
  }
}).observe(document.head, { childList: true });`;

/**
 * The shell the dev server serves in place of the built one, `served`: as package.sh assembles it
 * from this checkout's `cache` and `shell` sources, unminified, with [`devManifest`]'s manifest, and
 * Vite's client first, which loads the page again when a module of a kind changes.
 */
export function devShell(
  served: string,
  kinds: Record<string, string>,
  base: string,
  cache: string,
  shell: string,
): string {
  const lines = shell.split("\n");
  if (lines.filter((line) => line === MANIFEST_LINE).length !== 1) {
    throw new Error(`the checkout's shell.ts needs its manifest line, ${MANIFEST_LINE}`);
  }
  const manifest = devManifest(readManifest(served), kinds, base);
  return [
    `import "${base}@vite/client";`,
    THEME_LAST,
    `const modules = ${JSON.stringify(manifest)};`,
    cache,
    lines.filter((line) => line !== MANIFEST_LINE).join("\n"),
  ].join("\n");
}

/**
 * A request as the dev server's middleware reads it. The middleware takes Node's request as
 * `unknown` and reads these alone, so a test hands it a plain object.
 */
export interface ShellRequest {
  method?: string | undefined;
  url?: string | undefined;
}

/** A response as the dev server's middleware writes it. */
export interface ShellResponse {
  statusCode: number;
  setHeader(name: string, value: string): unknown;
  end(body?: string): unknown;
}

/** Where the shell's sources are (absolute paths), and how they are read: as JavaScript, their
 * types stripped (dev.ts). */
export interface Sources {
  cache: string;
  shell: string;
  read(path: string): string;
}

/**
 * The middleware that answers a page's shell script with [`devShell`]'s, made of the one the
 * Symphony server at `served` serves at that path and this checkout's sources, read again on each
 * request; every other request goes on to Vite and the proxy.
 */
export function shellMiddleware(
  served: string,
  kinds: () => Record<string, string>,
  sources: Sources,
): (request: unknown, response: ShellResponse, next: () => void) => Promise<void> {
  return async (request, response, next) => {
    const { method, url } = request as ShellRequest;
    const path = (url ?? "").split("?")[0] ?? "";
    if (method !== "GET" || !SHELL_PATH.test(path)) {
      next();
      return;
    }
    try {
      const answer = await fetch(`${served}${path}`);
      if (!answer.ok) throw new Error(`${served}${path} answered ${answer.status}`);
      const body = devShell(
        await answer.text(),
        kinds(),
        BASE,
        sources.read(sources.cache),
        sources.read(sources.shell),
      );
      response.statusCode = 200;
      response.setHeader("Content-Type", "text/javascript; charset=utf-8");
      response.setHeader("Cache-Control", "no-store");
      response.end(body);
    } catch (error) {
      response.statusCode = 502;
      response.setHeader("Content-Type", "text/plain; charset=utf-8");
      response.end(
        `The dev server could not make the page's shell: ${error instanceof Error ? error.message : String(error)}\n`,
      );
    }
  };
}

/**
 * The plugin that gives the dev server its shell: the kinds from Vite's resolved build, the
 * middleware before Vite's own and its proxy, and a page reload when a source of the shell changes.
 */
export function shellPlugin(served: string, sources: Sources): Plugin {
  let kinds: Record<string, string> = {};
  return {
    name: "symphony-dev-shell",
    configResolved(config) {
      // vite.config.ts names each kind's entry, `src/kinds/NAME.tsx`.
      kinds = config.build.rolldownOptions.input as Record<string, string>;
    },
    configureServer(server) {
      const watched = [sources.cache, sources.shell];
      server.watcher.add(watched);
      server.watcher.on("change", (file) => {
        if (watched.includes(file)) server.ws.send({ type: "full-reload" });
      });
      server.middlewares.use(shellMiddleware(served, () => kinds, sources));
    },
  };
}

/**
 * The dev server's settings over vite.config.ts in `root`: Vite's files under [`BASE`], on the
 * loopback address alone (it hands what it proxies to the server as this machine's), at `port`,
 * every request outside them, the feed's WebSocket among them (`ws`; Vite's own is under its base),
 * proxied to the server with its Host and Origin unchanged (so a save or the feed meets the server's
 * origin check as on the server itself), and the shell from `plugins`. No CORS
 * header of Vite's (its default lets a page on any loopback port read every answer, the server's
 * too): the dev page and all it loads are of its own origin, and another's reads through it no
 * more than from the server.
 */
export function devConfig(root: string, args: DevArgs, plugins: Plugin[]): InlineConfig {
  return {
    root,
    configFile: `${root}/vite.config.ts`,
    base: BASE,
    appType: "custom",
    clearScreen: false,
    server: {
      host: "127.0.0.1",
      port: args.port,
      strictPort: true,
      cors: false,
      proxy: {
        [PROXIED]: {
          target: origin(args.server),
          changeOrigin: false,
          ws: true,
          headers: { "x-symphony-dev": "1" },
        },
      },
      fs: { allow: ["."] },
    },
    plugins,
  };
}
