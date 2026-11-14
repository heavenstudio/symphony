// A document's contents column ("On this page") folds on its own once a table of the document needs
// the width it takes, as the Dioxus client's media runtime folded it: bodies and pictures arrive
// late, so it looks again as the tables are drawn, resized and scrolled to. A click on its summary
// is the reader's choice, which this never overrides.

/** Whether a table of `layout`'s body is wider than its box. */
export function cramped(layout: Element): boolean {
  return [...layout.querySelectorAll(".doc-body table")].some(
    (table) => table.clientWidth > 0 && table.scrollWidth > table.clientWidth + 2,
  );
}

/** Fold each contents of `root` the reader did not choose for whose document a table is cramped. */
export function foldContents(root: ParentNode): void {
  for (const toc of root.querySelectorAll(".doc-toc details:not([data-toc-chosen])")) {
    const layout = toc.closest(".doc-layout");
    if (layout === null || !cramped(layout)) continue;
    toc.setAttribute("data-toc-chosen", "automatic");
    toc.removeAttribute("open");
  }
}

/** A click on a contents' summary: the reader chose, and the contents stay as they leave them. */
export function chosen(event: Event): void {
  const target = event.target as Element | null;
  if (typeof target?.closest !== "function") return;
  target.closest(".doc-toc summary")?.parentElement?.setAttribute("data-toc-chosen", "manual");
}

/**
 * Watch the page's document tables until `signal` ends: each new one observed for its size, and
 * the contents folded as they are drawn, resized and scrolled.
 */
export function watchContents(signal: AbortSignal): void {
  if (signal.aborted || typeof ResizeObserver === "undefined") return;
  const root = document.documentElement;
  const fold = () => foldContents(root);
  const resize = new ResizeObserver(fold);
  const observed = new Set<Element>();
  const scan = () => {
    for (const table of observed) {
      if (table.isConnected) continue;
      resize.unobserve(table);
      observed.delete(table);
    }
    for (const table of root.querySelectorAll(".doc-body table:not([data-toc-observed])")) {
      table.setAttribute("data-toc-observed", "true");
      observed.add(table);
      resize.observe(table);
    }
    fold();
  };
  const changed = new MutationObserver(scan);
  changed.observe(root, { childList: true, subtree: true });
  document.addEventListener("scroll", fold);
  document.addEventListener("click", chosen);
  scan();
  signal.addEventListener(
    "abort",
    () => {
      resize.disconnect();
      changed.disconnect();
      document.removeEventListener("scroll", fold);
      document.removeEventListener("click", chosen);
      for (const table of observed) table.removeAttribute("data-toc-observed");
    },
    { once: true },
  );
}
