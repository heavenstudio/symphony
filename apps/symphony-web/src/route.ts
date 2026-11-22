// The page's route as the shell routes it: shell.ts owns the browser's links and history for the
// whole page, and says `review:navigate` once it has moved; a page reads its route again then.

import { useEffect, useState } from "react";
import { isDocumentRoute, isFolderRoute, navPage, project, strip } from "./project";
import type { ProjectView } from "./wire";

export interface Route {
  /** The route under the project's base and its checkout's scope: `queue`, `docs/A.md`. */
  path: string;
  /** The checkout the route names (`@scope/…`), empty for the main checkout's. */
  scope: string;
  /** Whether it is a document or folder page of an entrypoint. */
  document: boolean;
  /** Whether that page is a folder's rather than a document's. */
  folder: boolean;
}

export const HOME: Route = { path: "", scope: "", document: false, folder: false };

/** The route of the browser's `pathname` under `view`, or none for a path no page answers. */
export function parseRoute(view: ProjectView, pathname: string): Route | null {
  let decoded: string;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  let path = strip(view, decoded.replace(/\/{2,}/g, "/")).replace(/^\/+/, "");
  path = path.startsWith("app/") ? path.slice("app/".length) : path;
  let scope = "";
  if (path.startsWith("@")) {
    const slash = path.indexOf("/");
    if (slash < 0) return null;
    scope = path.slice(1, slash);
    path = path.slice(slash + 1);
    path = path.startsWith("app/") ? path.slice("app/".length) : path;
  }
  const slashed = path.startsWith("doc/") ? path.slice("doc/".length) : path;
  const page = slashed.endsWith("/") ? slashed.slice(0, -1) : slashed;
  // The route as the server answers it: a trailing slash names a folder page.
  const named = navPage(view.nav, page)?.document ?? (page === "task" ? "tasks" : slashed);
  const document = isDocumentRoute(view, named);
  const folder = isFolderRoute(view, named);
  const route = named.endsWith("/") ? named.slice(0, -1) : named;
  const answered =
    route === "" ||
    ["tasks", "queue", "reports", "runs", "agents"].includes(route) ||
    route.startsWith("task/") ||
    document;
  return answered ? { path: route, scope, document, folder } : null;
}

/** The browser's route now; the home's when no page answers its path. */
export function currentRoute(): Route {
  return parseRoute(project(), location.pathname) ?? HOME;
}

/** The route, read again each time the shell says the page moved. */
export function useRoute(): Route {
  const [route, setRoute] = useState(currentRoute);
  useEffect(() => {
    const moved = () => setRoute(currentRoute());
    addEventListener("review:navigate", moved);
    return () => removeEventListener("review:navigate", moved);
  }, []);
  return route;
}

/** The fragment the browser's address names, decoded; empty for none or one that does not decode. */
export function anchor(): string {
  try {
    return decodeURIComponent(location.hash.replace(/^#/, ""));
  } catch {
    return "";
  }
}

/**
 * The id the renderer gives a heading of `text` (crates/symphony/src/edit.rs `heading_anchor`,
 * Python-Markdown's toc slug): accents folded (NFKD, then ASCII only), punctuation dropped, the
 * spaces at its ends trimmed, each run of spaces and hyphens one `-`. The shell lands an address by
 * the same rule (shell/shell.ts, which a test keeps word for word).
 */
export function headingAnchor(text: string): string {
  const space = (c: string) =>
    (c >= "\t" && c <= "\r") || c === " " || (c >= "\x1c" && c <= "\x1f");
  const kept = [...text.normalize("NFKD")].filter((c) => /^[A-Za-z0-9_-]$/.test(c) || space(c));
  while (space(kept[0] ?? "")) kept.shift();
  while (space(kept[kept.length - 1] ?? "")) kept.pop();
  let anchor = "";
  let joining = false;
  for (const c of kept) {
    const joins = c === "-" || space(c);
    if (!joins) anchor += c.toLowerCase();
    else if (!joining) anchor += "-";
    joining = joins;
  }
  return anchor;
}

/**
 * The ids the fragment `target` may name, in the order a page looks for them: its own, then, read
 * as a heading's text (a task's link to a heading stores its text: `#The order`), that heading's
 * id (`headingAnchor`). None for no fragment.
 */
export function anchorIds(target: string): string[] {
  if (target === "") return [];
  const heading = headingAnchor(target);
  return heading === "" || heading === target ? [target] : [target, heading];
}

/** The element the address's fragment names (`anchorIds`), if one is on the page. */
export function anchorElement(): HTMLElement | null {
  for (const id of anchorIds(anchor())) {
    const element = document.getElementById(id);
    if (element) return element;
  }
  return null;
}

/**
 * The address's fragment, read again each time it changes (`hashchange`) and each time the shell
 * says the page moved: a move from card to card changes the fragment alone, and the page stays.
 */
export function useAnchor(): string {
  const [current, setCurrent] = useState(anchor);
  useEffect(() => {
    const moved = () => setCurrent(anchor());
    for (const name of ["hashchange", "review:navigate"]) addEventListener(name, moved);
    return () => {
      for (const name of ["hashchange", "review:navigate"]) removeEventListener(name, moved);
    };
  }, []);
  return current;
}

/**
 * Bring the element the address's fragment names (`anchorElement`) into view, opening every
 * `details` around it. The shell lands it (`reviewLand`, shell/shell.ts), once for each
 * navigation: it holds it there while the page draws around it, as a section drawn above it once
 * the scroll brings it near would push it down, and does nothing once that hold has ended (quiet,
 * or scrolled away). Without a fragment it does nothing: the shell owns the guarded top reset, and
 * a late load must not undo a scroll made since.
 */
export function scrollAnchor(): void {
  const element = anchorElement();
  if (!element) return;
  const land = (globalThis as { reviewLand?: (element: Element) => void }).reviewLand;
  if (land) {
    land(element);
    return;
  }
  for (let node: Element | null = element; node; node = node.parentElement) {
    if (node instanceof HTMLDetailsElement) node.open = true;
  }
  element.scrollIntoView();
}

/** The fields and controls whose focus `engaged` reads as the reader's work. */
const FIELDS =
  "input, textarea, select, button[aria-haspopup=listbox], [role=listbox], [role=option]";

/**
 * Whether the reader is in the middle of something a reload would lose: a busy region, an open
 * history or prose `details`, an open list box, or the focus in a field or a list's control.
 */
export function engaged(): boolean {
  if (
    document.querySelector(
      "[data-busy=true], .section-prose details[open], .history details[open], [role=listbox][data-state=open]",
    )
  ) {
    return true;
  }
  return document.activeElement?.matches(FIELDS) ?? false;
}

/**
 * At the next event that can end what `engaged` reads, or once `signal` ends: the focus leaving a
 * field for anything but another field, a change of a region's `data-busy`, a list box's
 * `data-state` or a `details`' `open` anywhere in the page, or an element added or removed: a
 * busy region, an open `details` or list box, or the focused field gone with what held it, which
 * changes no attribute and moves no focus. What waited reads `engaged` again.
 */
export function whenFree(signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal?.aborted) return resolve();
    const changed = new MutationObserver(() => done());
    const left = (event: Event) => {
      const next = (event as FocusEvent).relatedTarget as Element | null;
      if (!next?.matches?.(FIELDS)) done();
    };
    const done = () => {
      changed.disconnect();
      document.removeEventListener("focusout", left, true);
      signal?.removeEventListener("abort", done);
      resolve();
    };
    changed.observe(document.documentElement, {
      subtree: true,
      childList: true,
      attributeFilter: ["data-busy", "data-state", "open"],
    });
    document.addEventListener("focusout", left, true);
    signal?.addEventListener("abort", done);
  });
}
