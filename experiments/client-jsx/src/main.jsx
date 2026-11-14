// The page: one history adapter over three page kinds, each its own chunk with the style sheets
// the Dioxus client's kind of that page loads. A link under the project's base to one of them is
// followed in place; any other is left to the browser. A kind's chunk is imported by hand, not
// through `lazy` and `Suspense`: React holds a boundary's content back for up to 300 ms after its
// fallback showed, which a first visit would wait out.
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { Frame } from "./frame.jsx";
import { entrypoints, href, strip } from "./project.js";

const loaded = new Set([__STYLES__.board[0]]);

/** The kind's style sheets in the page, resolved once each has loaded. */
function styles(list) {
  return Promise.all(
    list
      .filter((sheet) => !loaded.has(sheet))
      .map(
        (sheet) =>
          new Promise((resolve) => {
            loaded.add(sheet);
            const link = document.createElement("link");
            link.rel = "stylesheet";
            link.href = sheet;
            link.onload = link.onerror = resolve;
            document.head.append(link);
          }),
      ),
  );
}

const kinds = {
  board: () => Promise.all([import("./pages/board.jsx"), styles(__STYLES__.board)]),
  task: () => Promise.all([import("./pages/task.jsx"), styles(__STYLES__.task)]),
  document: () => Promise.all([import("./pages/document.jsx"), styles(__STYLES__.document)]),
};

/** Each kind's page once its chunk and sheets are in, and the import that brings them. */
const pages = {};
const loading = {};
const load = (kind) =>
  (loading[kind] ??= kinds[kind]().then(
    ([module]) => (pages[kind] = module.default),
    () => (pages[kind] = Unloaded),
  ));

function Unloaded() {
  return (
    <main>
      <p>This page's code could not be loaded. Reload the page to try again.</p>
    </main>
  );
}

/** The route at `pathname`: its path under the base, and the kind that draws it. */
function routeAt(pathname) {
  const path = decodeURIComponent(strip(pathname));
  const page = path.replace(/^\//, "");
  const document = entrypoints().some((root) => page.startsWith(`${root}/`)) && page.endsWith(".md");
  const kind = path === "/tasks" ? "board" : path.startsWith("/task/") ? "task" : document ? "document" : null;
  return { path, kind, document, slug: kind === "task" ? path.slice("/task/".length) : null };
}

const route = () => routeAt(location.pathname);

function Elsewhere({ path }) {
  return (
    <main>
      <h1>Not in this experiment</h1>
      <p>
        {path} is a page of the Dioxus client only. This client draws <a href={href("/tasks")}>the board</a>, a task's page and a document.
      </p>
    </main>
  );
}

function App() {
  const [here, setHere] = useState(route);
  useEffect(() => {
    const moved = () => setHere(route());
    const click = (event) => {
      if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      const link = event.target.closest?.("a[href]");
      if (!link || link.target || link.hasAttribute("download") || link.dataset.native === "true") return;
      const url = new URL(link.href, location.href);
      if (url.origin !== location.origin) return;
      if (url.pathname === location.pathname && url.hash) return;
      if (!routeAt(url.pathname).kind) return;
      event.preventDefault();
      history.pushState(null, "", url.pathname + url.search + url.hash);
      window.scrollTo(0, 0);
      setHere(route());
    };
    window.addEventListener("popstate", moved);
    document.addEventListener("click", click);
    return () => {
      window.removeEventListener("popstate", moved);
      document.removeEventListener("click", click);
    };
  }, []);
  const [, setLoaded] = useState(0);
  const Page = here.kind ? pages[here.kind] : null;
  // Drawn without its page: draw again once the import is in, even if it came in since this draw.
  useEffect(() => {
    if (here.kind && !Page) load(here.kind).then(() => setLoaded((n) => n + 1));
  }, [here.kind, Page]);
  return (
    <Frame route={here}>
      {Page ? <Page key={here.path} route={here} /> : here.kind ? <main aria-busy="true" /> : <Elsewhere path={here.path} />}
    </Frame>
  );
}

const main = document.getElementById("main");
const root = document.createElement("div");
root.id = "app";
main.append(root);
const first = route().kind;
if (first) load(first);
createRoot(root).render(<App />);
