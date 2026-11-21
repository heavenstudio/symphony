import { createServer, type ViteDevServer } from "vite";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  BASE,
  devArgs,
  devConfig,
  devManifest,
  devShell,
  origin,
  PROXIED,
  readManifest,
  SHARED_PORT,
  type ShellResponse,
  shellMiddleware,
  shellPlugin,
  THEME_LAST,
  USAGE,
  unreadable,
} from "./dev-server";

afterEach(() => vi.unstubAllGlobals());

const SHARED = "/assets/shared-cb43a0c990f14660.css";
const SHELL = "/assets/symphony-shell-c8d2bbdc7691eafb.js";

/** A built shell's manifest, as package.sh writes it: three kinds. */
const SERVED = {
  panel: {
    script: "/assets/panel-7d67008fbebc0e13.js",
    imports: ["/assets/shared-c98795c5e95033c5.js"],
    styles: [SHARED, "/assets/panel-1ccad27589f1f569.css"],
  },
  queue: {
    script: "/assets/queue-c951b76db6e1dad8.js",
    imports: ["/assets/shared-c98795c5e95033c5.js"],
    styles: [SHARED, "/assets/queue-b185f2f18d8a251c.css"],
  },
  reports: {
    script: "/assets/reports-fc3977db0bc032e7.js",
    imports: ["/assets/shared-c98795c5e95033c5.js"],
    styles: [SHARED, "/assets/reports-16f1fbd8f24caefb.css"],
  },
};

/** A built shell: its manifest line, then the minified rest. */
const BUILT = `const modules = ${JSON.stringify(SERVED)};\nlet a=1;navigate();\n`;

const KINDS = { board: "src/kinds/board.tsx", queue: "src/kinds/queue.tsx" };

const CACHE = "// the cache's store\nwindow.symphonyCache = {};";
const SOURCE = "// One history adapter\nconst modules = __MODULES__;\nnavigate();";

describe("the dev server's arguments", () => {
  it("are a server's port and its own, none naming any port it picks", () => {
    expect(devArgs(["4321"])).toEqual({ server: 4321, port: 0 });
    expect(devArgs(["4321", "5173"])).toEqual({ server: 4321, port: 5173 });
    expect(devArgs(["65535", "0"])).toEqual({ server: 65535, port: 0 });
  });

  it("are refused without a server or with more than two", () => {
    expect(() => devArgs([])).toThrow(USAGE);
    expect(() => devArgs(["1", "2", "3"])).toThrow(USAGE);
  });

  it("never name the shared server's port, as the server read or its own", () => {
    expect(() => devArgs([String(SHARED_PORT)])).toThrow(/^SERVER 8765 is the shared server's/);
    expect(() => devArgs(["4321", String(SHARED_PORT)])).toThrow(
      /^PORT 8765 is the shared server's/,
    );
  });

  it("name a server that listens, never port 0", () => {
    expect(() => devArgs(["0"])).toThrow(/^SERVER is the port a server listens on, not 0/);
  });

  it("are ports, whole numbers to 65535", () => {
    for (const word of ["abc", "65536", "-1", "12.5", " 80", "123456", "", "0x10"]) {
      expect(() => devArgs([word]), word).toThrow(`SERVER ${JSON.stringify(word)} is not a port`);
      expect(() => devArgs(["4321", word]), word).toThrow(
        `PORT ${JSON.stringify(word)} is not a port`,
      );
    }
  });
});

describe("the server the dev server reads", () => {
  it("is on this machine's loopback address", () => {
    expect(origin(4321)).toBe("http://127.0.0.1:4321");
  });

  it("is readable once its page names a shell, asked with a deadline", async () => {
    const fetched = vi.fn(
      async (_url: string, _init?: RequestInit) =>
        new Response(`<script type="module" src="${SHELL}"></script>`),
    );
    vi.stubGlobal("fetch", fetched);
    expect(await unreadable("http://127.0.0.1:4321")).toBeNull();
    expect(fetched).toHaveBeenCalledOnce();
    expect(fetched.mock.calls[0]?.[0]).toBe("http://127.0.0.1:4321/");
    expect(fetched.mock.calls[0]?.[1]?.signal).toBeInstanceOf(AbortSignal);
  });

  it("is not, when it answers something else or fails", async () => {
    vi.stubGlobal("fetch", async () => new Response("<p>hello</p>"));
    expect(await unreadable("http://127.0.0.1:1")).toBe(
      "http://127.0.0.1:1/ answered 200 with no Symphony page",
    );
    vi.stubGlobal("fetch", async () => new Response(`src="${SHELL}"`, { status: 503 }));
    expect(await unreadable("http://127.0.0.1:1")).toBe(
      "http://127.0.0.1:1/ answered 503 with no Symphony page",
    );
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("fetch failed")));
    expect(await unreadable("http://127.0.0.1:1")).toBe(
      "nothing answers at http://127.0.0.1:1/ (fetch failed)",
    );
    vi.stubGlobal("fetch", () => Promise.reject("refused"));
    expect(await unreadable("http://127.0.0.1:1")).toBe(
      "nothing answers at http://127.0.0.1:1/ (refused)",
    );
  });

  it("is given up on past its deadline", async () => {
    vi.stubGlobal(
      "fetch",
      (_url: string, init?: RequestInit) =>
        new Promise((_, refuse) =>
          init?.signal?.addEventListener("abort", () => refuse(init.signal?.reason)),
        ),
    );
    expect(await unreadable("http://127.0.0.1:1", 1)).toMatch(
      /^nothing answers at http:\/\/127\.0\.0\.1:1\/ \(.+\)$/,
    );
  });
});

describe("a built shell's manifest", () => {
  it("is its first line's", () => {
    expect(readManifest(BUILT)).toEqual(SERVED);
    expect(readManifest("const modules = {};")).toEqual({});
  });

  it("is none in a shell that does not start with it", () => {
    for (const shell of [
      "// a comment\nconst modules = {};",
      "const modules = {;\n",
      "const modules = [];\n",
      "const modules = null;\n",
      'const modules = {"queue": 1};\n',
      "const modules = {}\n",
      "",
    ]) {
      expect(() => readManifest(shell), shell).toThrow("the server's shell holds no manifest");
    }
  });
});

describe("the dev server's manifest", () => {
  it("serves the checkout's kinds from their sources, with the build's shared stylesheet", () => {
    const manifest = devManifest(SERVED, { ...KINDS, agents: "src/kinds/agents.tsx" }, BASE);
    expect(manifest).toEqual({
      board: { script: "/@symphony-web/src/kinds/board.tsx", imports: [], styles: [SHARED] },
      queue: { script: "/@symphony-web/src/kinds/queue.tsx", imports: [], styles: [SHARED] },
      agents: { script: "/@symphony-web/src/kinds/agents.tsx", imports: [], styles: [SHARED] },
    });
    // A kind the checkout no longer has is gone, whatever the server's build holds.
    expect(manifest).not.toHaveProperty("reports");
    expect(manifest).not.toHaveProperty("panel");
  });

  it("serves a kind the server's build has from the checkout's source", () => {
    expect(devManifest(SERVED, { panel: "src/kinds/panel.tsx" }, BASE).panel).toEqual({
      script: "/@symphony-web/src/kinds/panel.tsx",
      imports: [],
      styles: [SHARED],
    });
  });

  it("gives the checkout's kinds no shared stylesheet when the server's build has no kind", () => {
    expect(devManifest({}, KINDS, BASE)).toEqual({
      board: { script: "/@symphony-web/src/kinds/board.tsx", imports: [], styles: [] },
      queue: { script: "/@symphony-web/src/kinds/queue.tsx", imports: [], styles: [] },
    });
  });
});

describe("the dev server's shell", () => {
  it("is the checkout's, Vite's client first, then the theme's place, its manifest and sources", () => {
    const shell = devShell(BUILT, KINDS, BASE, CACHE, SOURCE);
    expect(shell.split("\n")).toEqual([
      'import "/@symphony-web/@vite/client";',
      ...THEME_LAST.split("\n"),
      `const modules = ${JSON.stringify(devManifest(SERVED, KINDS, BASE))};`,
      "// the cache's store",
      "window.symphonyCache = {};",
      "// One history adapter",
      "navigate();",
    ]);
  });

  it("is refused from a shell.ts without its one manifest line", () => {
    for (const source of ["navigate();", `${SOURCE}\nconst modules = __MODULES__;`]) {
      expect(() => devShell(BUILT, KINDS, BASE, CACHE, source)).toThrow(
        "the checkout's shell.ts needs its manifest line",
      );
    }
  });
});

/** A head as the theme's place sees it: its children in order, a theme among them or not. */
function head(names: string[]) {
  const nodes = names.map((name) => ({
    name,
    matches: (selector: string) =>
      selector === "style[data-vite-dev-id]" && name.startsWith("vite"),
  }));
  return {
    nodes,
    querySelector: (selector: string) =>
      selector === "[data-symphony-theme]"
        ? (nodes.find((node) => node.name === "theme") ?? null)
        : null,
    insertBefore(node: (typeof nodes)[number], before: (typeof nodes)[number]) {
      nodes.splice(nodes.indexOf(node), 1);
      nodes.splice(nodes.indexOf(before), 0, node);
    },
  };
}

/** Runs [`THEME_LAST`] on `page`'s head, then tells it the nodes named `added` were added. */
function themeLast(page: ReturnType<typeof head>, added: string[]): string[] {
  let heard: ((records: { addedNodes: unknown[] }[]) => void) | undefined;
  const observed: unknown[] = [];
  class Observer {
    constructor(callback: typeof heard) {
      heard = callback;
    }
    observe(target: unknown, options: unknown) {
      observed.push(target, options);
    }
  }
  const node = { DOCUMENT_POSITION_FOLLOWING: 4 };
  const document = {
    head: { querySelector: page.querySelector, insertBefore: page.insertBefore.bind(page) },
  };
  // Each node knows where it stands from the head's order, as compareDocumentPosition does.
  for (const each of page.nodes) {
    Object.assign(each, {
      compareDocumentPosition: (other: unknown) =>
        page.nodes.indexOf(other as (typeof page.nodes)[number]) > page.nodes.indexOf(each) ? 4 : 2,
    });
  }
  new Function("document", "Node", "MutationObserver", THEME_LAST)(document, node, Observer);
  expect(observed).toEqual([document.head, { childList: true }]);
  heard?.([
    { addedNodes: page.nodes.filter((each) => added.includes(each.name)) },
    { addedNodes: [{}] },
  ]);
  return page.nodes.map((each) => each.name);
}

describe("the theme's place in the dev server's page", () => {
  it("stays after each style Vite adds, as the built shell keeps it", () => {
    expect(
      themeLast(head(["shared", "theme", "vite-queue", "vite-media"]), [
        "vite-queue",
        "vite-media",
      ]),
    ).toEqual(["shared", "vite-queue", "vite-media", "theme"]);
  });

  it("leaves a style already before it, and what is no style of Vite's", () => {
    expect(themeLast(head(["vite-queue", "theme", "link"]), ["vite-queue", "link"])).toEqual([
      "vite-queue",
      "theme",
      "link",
    ]);
  });

  it("moves nothing in a page with no theme", () => {
    expect(themeLast(head(["shared", "vite-queue"]), ["vite-queue"])).toEqual([
      "shared",
      "vite-queue",
    ]);
  });
});

/** A response that keeps what the middleware wrote. */
function response() {
  const written = { statusCode: 0, headers: {} as Record<string, string>, body: "" };
  const answer: ShellResponse = {
    get statusCode() {
      return written.statusCode;
    },
    set statusCode(code: number) {
      written.statusCode = code;
    },
    setHeader: (name, value) => {
      written.headers[name] = value;
    },
    end: (body) => {
      written.body = body ?? "";
    },
  };
  return { written, answer };
}

const SOURCES = {
  cache: "/checkout/apps/symphony-web/shell/cache.ts",
  shell: "/checkout/apps/symphony-web/shell/shell.ts",
  read: (path: string) => (path.endsWith("cache.ts") ? CACHE : SOURCE),
};

describe("the dev server's shell middleware", () => {
  it("passes on every request but a page's shell script", async () => {
    const fetched = vi.fn();
    vi.stubGlobal("fetch", fetched);
    const handle = shellMiddleware("http://127.0.0.1:4321", () => KINDS, SOURCES);
    for (const [method, url] of [
      ["POST", SHELL],
      ["GET", "/api/board"],
      ["GET", "/assets/program-7d67008fbebc0e13.js"],
      ["GET", "/assets/symphony-shell-c8d2bbdc.js"],
      ["GET", `/alpha${SHELL}`],
      ["GET", undefined],
      [undefined, SHELL],
    ]) {
      const next = vi.fn();
      const { written } = response();
      await handle({ method, url }, response().answer, next);
      expect(next, `${method} ${url}`).toHaveBeenCalledOnce();
      expect(written.statusCode).toBe(0);
    }
    expect(fetched).not.toHaveBeenCalled();
  });

  it("answers the shell with the dev server's, the server's read past its query", async () => {
    const fetched = vi.fn(async (_url: string) => new Response(BUILT));
    vi.stubGlobal("fetch", fetched);
    const read = vi.fn(SOURCES.read);
    let kinds: Record<string, string> = {};
    const handle = shellMiddleware("http://127.0.0.1:4321", () => kinds, { ...SOURCES, read });
    kinds = KINDS;
    const next = vi.fn();
    const { written, answer } = response();
    await handle({ method: "GET", url: `${SHELL}?v=1` }, answer, next);
    expect(next).not.toHaveBeenCalled();
    expect(fetched.mock.calls).toEqual([[`http://127.0.0.1:4321${SHELL}`]]);
    expect(read.mock.calls).toEqual([[SOURCES.cache], [SOURCES.shell]]);
    expect(written).toEqual({
      statusCode: 200,
      headers: { "Content-Type": "text/javascript; charset=utf-8", "Cache-Control": "no-store" },
      body: devShell(BUILT, KINDS, BASE, CACHE, SOURCE),
    });
  });

  it("says why it could not make the shell, as a bad gateway", async () => {
    const cases: [() => Promise<Response>, Partial<typeof SOURCES>, string][] = [
      [
        async () => new Response("gone", { status: 404 }),
        {},
        `http://127.0.0.1:4321${SHELL} answered 404`,
      ],
      [() => Promise.reject(new TypeError("fetch failed")), {}, "fetch failed"],
      [
        async () => new Response("// no manifest"),
        {},
        "the server's shell holds no manifest of its page kinds on its first line",
      ],
      [
        async () => new Response(BUILT),
        {
          read: () => {
            throw new Error("ENOENT: shell.ts");
          },
        },
        "ENOENT: shell.ts",
      ],
      [
        async () => new Response(BUILT),
        {
          read: () => {
            throw "unreadable";
          },
        },
        "unreadable",
      ],
    ];
    for (const [fetched, sources, why] of cases) {
      vi.stubGlobal("fetch", fetched);
      const handle = shellMiddleware("http://127.0.0.1:4321", () => KINDS, {
        ...SOURCES,
        ...sources,
      });
      const { written, answer } = response();
      await handle({ method: "GET", url: SHELL }, answer, vi.fn());
      expect(written.statusCode, why).toBe(502);
      expect(written.headers).toEqual({ "Content-Type": "text/plain; charset=utf-8" });
      expect(written.body).toBe(`The dev server could not make the page's shell: ${why}\n`);
    }
  });
});

describe("the dev server's plugin", () => {
  it("takes the kinds Vite resolved, serves the shell and reloads the page when its sources change", async () => {
    const plugin = shellPlugin("http://127.0.0.1:4321", SOURCES);
    const resolved = plugin.configResolved as unknown as (config: unknown) => void;
    resolved({ build: { rolldownOptions: { input: KINDS } } });
    const added: unknown[] = [];
    let changed: ((file: string) => void) | undefined;
    let handle: ((...args: unknown[]) => Promise<void>) | undefined;
    const sent: unknown[] = [];
    const configure = plugin.configureServer as unknown as (server: unknown) => void;
    configure({
      watcher: {
        add: (paths: unknown) => added.push(paths),
        on: (event: string, listener: (file: string) => void) => {
          expect(event).toBe("change");
          changed = listener;
        },
      },
      ws: { send: (payload: unknown) => sent.push(payload) },
      middlewares: { use: (middleware: typeof handle) => (handle = middleware) },
    });
    expect(plugin.name).toBe("symphony-dev-shell");
    expect(added).toEqual([[SOURCES.cache, SOURCES.shell]]);
    changed?.("/checkout/apps/symphony-web/src/queue.tsx");
    expect(sent).toEqual([]);
    changed?.(SOURCES.shell);
    changed?.(SOURCES.cache);
    expect(sent).toEqual([{ type: "full-reload" }, { type: "full-reload" }]);
    vi.stubGlobal("fetch", async () => new Response(BUILT));
    const { written, answer } = response();
    await handle?.({ method: "GET", url: SHELL }, answer, vi.fn());
    expect(written.body).toBe(devShell(BUILT, KINDS, BASE, CACHE, SOURCE));
  });
});

describe("the dev server's settings", () => {
  it("serve Vite's files under its base on the loopback address and proxy the rest unchanged", () => {
    const plugins = [shellPlugin("http://127.0.0.1:4321", SOURCES)];
    expect(devConfig("/checkout/apps/symphony-web", { server: 4321, port: 5173 }, plugins)).toEqual(
      {
        root: "/checkout/apps/symphony-web",
        configFile: "/checkout/apps/symphony-web/vite.config.ts",
        base: "/@symphony-web/",
        appType: "custom",
        clearScreen: false,
        server: {
          host: "127.0.0.1",
          port: 5173,
          strictPort: true,
          cors: false,
          proxy: {
            [PROXIED]: {
              target: "http://127.0.0.1:4321",
              changeOrigin: false,
              ws: true,
              headers: { "x-symphony-dev": "1" },
            },
          },
          fs: { allow: ["."] },
        },
        plugins,
      },
    );
  });

  it("proxy every request but those for Vite's own files", () => {
    const proxied = new RegExp(PROXIED);
    for (const url of [
      "/",
      "/api/board",
      "/assets/program.js",
      "/queue",
      "/@tree/tasks",
      "/x/a/",
    ]) {
      expect(proxied.test(url), url).toBe(true);
    }
    for (const url of [`${BASE}src/kinds/queue.tsx`, `${BASE}@vite/client`, `${BASE}?token=a`]) {
      expect(proxied.test(url), url).toBe(false);
    }
  });
});

/** This package's folder, the dev server's root. */
const ROOT = decodeURIComponent(new URL("..", import.meta.url).pathname).replace(/\/$/, "");

/** Where `server` listens, once it does. */
function address(server: ViteDevServer): string {
  const [url] = server.resolvedUrls?.local ?? [];
  if (url === undefined) throw new Error("the server listens nowhere");
  return new URL(url).origin;
}

/** A stand-in for the Symphony server: every request answered with `body`, and no CORS header. */
async function symphony(body: string): Promise<ViteDevServer> {
  const server = await createServer({
    configFile: false,
    root: ROOT,
    appType: "custom",
    logLevel: "silent",
    server: { host: "127.0.0.1", port: 0, cors: false, watch: null, ws: false },
    plugins: [
      {
        name: "symphony",
        configureServer: (served) => {
          served.middlewares.use((_request, response) => response.end(body));
        },
      },
    ],
  });
  return server.listen();
}

describe("the dev server, to a page of another origin", () => {
  it("answers as the server would, letting it read nothing, Vite's files or the server's", async () => {
    const served = await symphony("the server's answer");
    let dev: ViteDevServer | undefined;
    try {
      const settings = devConfig(
        ROOT,
        { server: Number(new URL(address(served)).port), port: 0 },
        [],
      );
      dev = await createServer({
        ...settings,
        configFile: false,
        logLevel: "silent",
        server: { ...settings.server, watch: null },
      });
      const at = address(await dev.listen());
      const foreign = { Origin: "http://localhost:5173" };
      const proxied = await fetch(`${at}/api/front`, { headers: foreign });
      expect(await proxied.text()).toBe("the server's answer");
      expect(proxied.headers.get("access-control-allow-origin")).toBeNull();
      const own = await fetch(`${at}${BASE}src/dev-server.ts`, { headers: foreign });
      expect(own.status).toBe(200);
      expect(own.headers.get("access-control-allow-origin")).toBeNull();
      // A preflight is the server's to answer (it allows none), never Vite's.
      const preflight = await fetch(`${at}/api/comment`, {
        method: "OPTIONS",
        headers: { ...foreign, "Access-Control-Request-Method": "POST" },
      });
      expect(await preflight.text()).toBe("the server's answer");
      expect(preflight.headers.get("access-control-allow-origin")).toBeNull();
    } finally {
      await dev?.close();
      await served.close();
    }
  });
});
