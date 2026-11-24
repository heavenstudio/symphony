// The project the page belongs to: its name, its plugins' navigation and its entrypoints, which
// the server inlines in the page it serves (PROJECT_ELEMENT), so the first paint asks for nothing.
// A page without it is a plain project with no plugin pages and the convention's one entrypoint.
//
// What crates/symphony-api's `ProjectView` answers (`here`, `strip`, the document routes, its own
// pages) is carried here in TypeScript, and held to the Rust by the cases it writes
// (wire.cases.json, project.test.ts). That file holds the Rust it carries too, so a change there
// fails `just client-lint` until it is ported here, with its cases.

import {
  FILE_QUERY,
  type NavItem,
  PAGE_NAMES,
  type PersonName,
  PROJECT_ELEMENT,
  type ProjectView,
  SYMPHONY_ROUTES,
} from "./wire";

/** The plain project: no name, no plugin pages, the convention's one entrypoint. */
export const PLAIN: ProjectView = { name: "", register: "", nav: [], entrypoints: [] };

/**
 * `path` under the project's base: a path of the site (one slash first, `/tasks`) takes the base;
 * a URL, a protocol-relative one, a fragment and a relative path stay as they are.
 */
export function here(view: ProjectView, path: string): string {
  const base = view.base ?? "";
  if (base === "" || !path.startsWith("/") || path.startsWith("//")) return path;
  return `${base}${path}`;
}

/** The browser's `path` without the project's base; a path the base is not above stays. */
export function strip(view: ProjectView, path: string): string {
  const base = view.base ?? "";
  if (base === "" || !path.startsWith(base)) return path;
  const rest = path.slice(base.length);
  if (rest === "") return "/";
  return rest.startsWith("/") ? rest : path;
}

/** The routes documents are served at: the project's entrypoints, or the convention's. */
export function entrypoints(view: ProjectView): string[] {
  return view.entrypoints.length === 0 ? ["docs"] : view.entrypoints;
}

/** Whether the route `path` is `route` or lies under it; every path lies under the empty route. */
function within(route: string, path: string): boolean {
  return route === "" || path === route || path.startsWith(`${route}/`);
}

/**
 * What follows the last dot of the last part of `name`, a route or a file name: none when that part
 * holds no dot, or only the one it starts with (`.html` names no page).
 */
function extension(name: string): string | null {
  const last = name.split("/").pop() ?? "";
  const dot = last.lastIndexOf(".");
  return dot > 0 ? last.slice(dot + 1) : null;
}

/** `html` or `htm`, in any case of its ASCII letters. */
function htmlExtension(extension: string): boolean {
  return /^html?$/i.test(extension);
}

/** Whether `name`, a route or a file name, names a document: Markdown (`.md`, its case kept) or HTML. */
function isPage(name: string): boolean {
  const found = extension(name);
  return found !== null && (found === "md" || htmlExtension(found));
}

/** Whether `name`, a route or a file name, names an HTML page: `.html` or `.htm`, in any case. */
export function isHtml(name: string): boolean {
  const found = extension(name);
  return found !== null && htmlExtension(found);
}

/** Whether `query`, a URL's query without its `?`, asks for the file: it holds FILE_QUERY. */
export function asksForFile(query: string): boolean {
  return query.split("&").some((pair) => pair.split("=")[0] === FILE_QUERY);
}

/**
 * `src`, an HTML document's address (its route, or a path of the site), asking for the file:
 * FILE_QUERY added to its query, before its fragment, unless it asks already. A document's frame
 * and a mockup's frame load it: without it the route is the app's page.
 */
export function fileAddress(src: string): string {
  const hash = src.indexOf("#");
  const at = hash < 0 ? src.length : hash;
  const [address, fragment] = [src.slice(0, at), src.slice(at)];
  const question = address.indexOf("?");
  if (question < 0) return `${address}?${FILE_QUERY}${fragment}`;
  if (asksForFile(address.slice(question + 1))) return src;
  return `${address}&${FILE_QUERY}${fragment}`;
}

/**
 * Whether `path` (a route, no leading slash) is a document or folder page of an entrypoint: under
 * one, on no route the Symphony or a plugin's page answers, and a folder (a trailing slash, an
 * entrypoint's own route) or a name with no dot, a Markdown one or an HTML one.
 */
export function isDocumentRoute(view: ProjectView, path: string): boolean {
  const page = path.replace(/\/+$/, "");
  const pages = view.nav.map((item) => item.href.replace(/^\/+|\/+$/g, ""));
  const urls = entrypoints(view);
  const name = page.split("/").pop() ?? page;
  return (
    ![...SYMPHONY_ROUTES, ...pages].some((route) => within(route, page)) &&
    urls.some((url) => within(url, page)) &&
    (path.endsWith("/") || urls.includes(page) || !name.includes(".") || isPage(name))
  );
}

/**
 * Whether `path` is a document route naming a folder page rather than a document: a trailing
 * slash, an entrypoint's own route, or a name that is neither Markdown nor HTML.
 */
export function isFolderRoute(view: ProjectView, path: string): boolean {
  const page = path.replace(/\/+$/, "");
  return (
    isDocumentRoute(view, path) &&
    (path.endsWith("/") || entrypoints(view).includes(page) || !isPage(page))
  );
}

/** The URL of the first entrypoint's folder page: `/docs/`, or `/` at the root. */
export function documentsHref(view: ProjectView): string {
  const first = entrypoints(view)[0];
  return first ? `/${first}/` : "/";
}

/** The route of the first entrypoint's README: `docs/README.md`, or `README.md` at the root. */
export function readmeRoute(view: ProjectView): string {
  const first = entrypoints(view)[0];
  return first ? `${first}/README.md` : "README.md";
}

/** The Symphony's own pages in the header, in its order, each as its path and its name. */
export function ownPages(view: ProjectView): [string, string][] {
  return [
    ["/", PAGE_NAMES.HOME],
    ["/queue", PAGE_NAMES.QUEUE],
    [documentsHref(view), PAGE_NAMES.DOCS],
    ["/tasks", PAGE_NAMES.BOARD],
    ["/reports", PAGE_NAMES.REPORTS],
    ["/settings", PAGE_NAMES.SETTINGS],
    ["/people", PAGE_NAMES.PEOPLE],
    ["/runs", PAGE_NAMES.RUNS],
    ["/agents", PAGE_NAMES.AGENTS],
  ];
}

/** The plugin page `path` opens (`shelf`, from `/shelf`): the item whose link it is. */
export function navPage(nav: NavItem[], path: string): NavItem | undefined {
  const page = path.replace(/^\/+|\/+$/g, "");
  return nav.find((item) => page !== "" && item.href.replace(/^\/+|\/+$/g, "") === page);
}

/** The item whose page shows `document`, the route's path. */
export function navForDocument(nav: NavItem[], document: string): NavItem | undefined {
  return nav.find((item) => item.document === document);
}

/** A link of the header: where it goes, its name, and whether it is the page shown. */
export interface HeaderLink {
  href: string;
  label: string;
  active: boolean;
}

/**
 * The header's links on the page at `path`, a route without its leading slash, which `document`
 * says is a document or folder page: the Symphony's own pages, then the plugins'. A task's page is
 * under the board, and a plugin's page is its own, not the documents'.
 */
export function headerLinks(view: ProjectView, path: string, document: boolean): HeaderLink[] {
  const plugin = navForDocument(view.nav, path);
  const active = [
    path === "" && !document,
    path === "queue",
    document && plugin === undefined,
    path === "tasks" || path.startsWith("task/"),
    path === "reports",
    path === "settings",
    path === "people",
    path === "runs",
    path === "agents",
  ];
  return [
    ...ownPages(view).map(([href, label], at) => ({ href, label, active: active[at] === true })),
    ...view.nav.map((item) => ({
      href: item.href,
      label: item.label,
      active: item.document === path,
    })),
  ];
}

/**
 * The crumbs after the header's links on the page at `path`: what the lit link does not already
 * say. A document or folder shows each folder of its route, the last its page; a task's page its
 * slug alone. None where the route is a header link's own page.
 */
export function crumbs(view: ProjectView, path: string): string[] {
  const trimmed = path.replace(/\/+$/, "");
  if (trimmed.startsWith("task/")) return [trimmed.slice("task/".length)];
  if (trimmed === "" || navForDocument(view.nav, path) !== undefined) return [];
  const trail = trimmed.split("/");
  const linked = ownPages(view).some(([href]) => href.replace(/^\/+|\/+$/g, "") === trail[0]);
  return trail.length === 1 && linked ? [] : trail;
}

/**
 * `key` as the project's pages keep it in the browser: the machine's one server's projects share
 * its origin, so each key takes the project's id first; a server of its own keeps it as it is.
 * shell.ts's `stored` is the same rule.
 */
export function stored(view: ProjectView, key: string): string {
  const id = (view.base ?? "").replace(/^\/+/, "");
  return id === "" ? key : `${id}:${key}`;
}

const isText = (value: unknown): value is string => typeof value === "string";

/**
 * The view the server inlined, as Rust's serde reads it: a value without the fields it always
 * sends is the plain project, and each field it may leave out takes its default.
 */
export function readView(value: unknown): ProjectView {
  if (typeof value !== "object" || value === null) return PLAIN;
  const view = value as Record<string, unknown>;
  const nav = view.nav === undefined ? [] : view.nav;
  const urls = view.entrypoints === undefined ? [] : view.entrypoints;
  const items =
    Array.isArray(nav) &&
    nav.every(
      (item: unknown) =>
        typeof item === "object" &&
        item !== null &&
        ["label", "title", "href", "document"].every((field) =>
          isText((item as Record<string, unknown>)[field]),
        ),
    );
  if (
    !isText(view.name) ||
    !isText(view.register) ||
    !items ||
    !Array.isArray(urls) ||
    !urls.every(isText) ||
    !["guide", "base", "wordmark", "mark"].every(
      (field) => view[field] === undefined || isText(view[field]),
    ) ||
    // Who reads the page and the person at the machine, each a name and how it is shown.
    !["you", "machine"].every((field) => {
      const person = view[field] as Record<string, unknown> | null | undefined;
      return person == null || (isText(person.name) && isText(person.shown));
    })
  ) {
    return PLAIN;
  }
  return { ...(value as ProjectView), nav: nav as NavItem[], entrypoints: urls };
}

let current: ProjectView | undefined;

/** The page's project, read from its element once. */
export function project(): ProjectView {
  if (current === undefined) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(document.getElementById(PROJECT_ELEMENT)?.textContent ?? "");
    } catch {
      parsed = undefined;
    }
    current = readView(parsed);
  }
  return current;
}

/** A writer as a line draws them: the words shown, and the name the line carries where it differs. */
export interface Drawn {
  text: string;
  title?: string;
}

/**
 * The writer `name` of a person's line as a page draws it: the person at the machine (`machine`)
 * by how they are shown, for a line under `owner` or under the short name they gave themselves,
 * titled with the name the line carries; any other name, and every name while the person at the
 * machine gave none, as it is written. What is written stays as it is.
 */
export function drawnName(name: string, machine: PersonName | null | undefined): Drawn {
  const theirs = machine != null && (name === "owner" || name === machine.name);
  return theirs && machine.shown !== name ? { text: machine.shown, title: name } : { text: name };
}

/** `name`, the writer a line or a row carries, as this page's project draws it (`drawnName`). */
export function writer(name: string): Drawn {
  return drawnName(name, project().machine);
}

/**
 * The person at the machine as they now name themselves (`you`), from a page that wrote it: the
 * header and every thread drawn after it name them so, as a page served after it does.
 */
export function named(you: PersonName): void {
  current = { ...project(), you, machine: you };
}

/**
 * Where who reads the page writes their profile: the person at the machine's You section on the
 * People page, another person's own card there.
 */
export function profilePath(view: ProjectView): string {
  const you = view.you;
  const machine = you == null || you.name === "owner" || you.name === view.machine?.name;
  return machine ? "/people#you" : `/people#person-${you.name}`;
}

/**
 * `path` as a link of the page's project writes it in its `href`: a path of the site takes the
 * project's base, so the link opened in a new tab, copied or long-pressed opens the page a click
 * on it opens.
 */
export function href(path: string): string {
  return here(project(), path);
}

/** `route` (`/api/queue`) under the checkout `scope` names: `/@SCOPE/api/queue`, or as it is. */
export function scoped(scope: string, route: string): string {
  return scope === "" ? route : `/@${scope}${route}`;
}
