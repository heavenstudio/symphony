// One history adapter; switching page kinds awaits the old runtime's cleanup.
//
// The shell's script is cache.ts and then this file, each with its types stripped on its own
// (strip.ts), as one module (package.sh), the manifest's line first: a name this one declares at its
// top, the store's cannot.
import type { Ahead, Store } from "./cache.ts";

/** A kind's files in the manifest package.sh writes: its script, the chunks it imports, its styles. */
interface Files {
  script: string;
  imports: string[];
  styles: string[];
}

/** The kinds' manifest, which package.sh writes in the place of the line that names it. */
declare const __MODULES__: Record<string, Files>;

/** A kind's script: a module exporting `run`, which mounts the kind, and `stop`. */
interface Kind {
  run(): unknown;
  stop(): unknown;
}

/** A request to the menus (`reviewOverlay`, apps/symphony-web/src/overlay.tsx), as the shell reads it. */
interface OverlayRequest {
  kind: string;
  owner?: unknown;
  callback?: ((answer: unknown) => void) | undefined;
}

/** What the shell and the page kinds say to each other on the window (apps/symphony-web/src/shared.ts). */
interface Page {
  symphonyCache?: Store;
  reviewShell: { gone: () => Promise<boolean>; alarm: typeof alarm };
  reviewOverlay: (request: OverlayRequest) => void;
  reviewOverlayRequest?: OverlayRequest | null;
  reviewHoldPlace: (rows: string) => void;
  reviewLand: (target: Element) => void;
  reviewPanelReady?: boolean;
  reviewOverlayReady?: boolean;
  reviewActivityOpen?: boolean;
  reviewAppearanceOpen?: boolean;
  reviewQuickDraft?: { open?: boolean };
  reviewPanelOpener?: HTMLElement | null;
  reviewPanelFocus?: unknown;
}

declare const window: Window & typeof globalThis & Page;
/** The link the browser says it reads over (Chrome's Network Information), where it says one. */
declare const navigator: Navigator & {
  connection?: { saveData?: boolean; effectiveType?: string };
};
/** The device cache's store (cache.ts), on the window before this script runs. */
declare const symphonyCache: Store;

const modules = __MODULES__;

try {
  const css = localStorage.getItem(stored("studio.cssVars"));
  if (css) {
    const style = document.createElement("style");
    style.id = "studio-theme";
    style.textContent = css;
    document.head.append(style);
    document.documentElement.setAttribute("data-studio-theme", "");
  }
} catch (_) {
  /* Storage can be unavailable in a private browser. */
}
let active: { kind: string; imported: Kind; mount: HTMLElement } | undefined;
let requested = 0;
let switching = Promise.resolve();
const styleLoads = new Map<string, Promise<unknown>>();
const preloaded = new Set(
  [...document.querySelectorAll('link[rel="modulepreload"]')].map((link) =>
    link.getAttribute("href"),
  ),
);
// The page's own reads in flight (its data) and when the last one answered: the idle warm-up waits
// until none has been in flight for half a second, and after a read that got no answer (its time
// ran out, the link dropped), until one does. A read the page gave up (an
// `AbortError`) is neither: the web client's reads whose time ran out fail with a `TimeoutError`
// (apps/symphony-web/src/request.ts).
let reads = 0,
  answered = 0;
const read = window.fetch;
window.fetch = (...args) => {
  reads++;
  return read(...args)
    .then(
      (response) => {
        answered = Date.now();
        return response;
      },
      (error) => {
        if (error?.name !== "AbortError") answered = Infinity;
        throw error;
      },
    )
    .finally(() => reads--);
};
// A lookup in this device's cache is one of the page's reads too: a page waits on what it kept
// before it asks the server for the rest (the board its counts and columns), and the store can take
// seconds to open and answer on a loaded machine or a slow phone, with no fetch in flight. Its end
// is an answer, but never one after a read that got none.
const lookup = symphonyCache.read;
symphonyCache.read = (...args) => {
  reads++;
  return lookup(...args).finally(() => {
    reads--;
    answered = Math.max(answered, Date.now());
  });
};

// The modules every page kind shares, each loaded once on its first use and kept mounted: the
// chat and activity panel (apps/symphony-web/src/panel.tsx), and the overlay host that draws a
// select's popup or the picture viewer for any module (apps/symphony-web/src/overlay.tsx). A select
// never waits on the chat's module.
// Each has its root, its ready flag (`reviewPanelReady`, `reviewOverlayReady`) and event
// (`review:panel-ready`...), and its failure notice (`#panel-error`...) with a retry.
type Persistent = "panel" | "overlay";
interface Shared {
  root: string;
  ready: "reviewPanelReady" | "reviewOverlayReady";
  said: string;
  retry: string;
}
const PERSISTENT: Record<Persistent, Shared> & Partial<Record<string, Shared>> = {
  panel: {
    root: "quick-root",
    ready: "reviewPanelReady",
    said: "Conversation could not be loaded. ",
    retry: "Retry loading conversation",
  },
  overlay: {
    root: "overlay-root",
    ready: "reviewOverlayReady",
    said: "Menus and pictures could not be loaded. ",
    retry: "Retry loading menus",
  },
};
const persistent = new Map<Persistent, Promise<unknown>>();

// A failed load's alert `id` in the notice region: what failed, and a button that takes the
// alert down and tries `again`.
function alarm(id: string, said: string, retry: string, again: () => void) {
  let notice = document.getElementById(id);
  if (!notice) {
    notice = document.createElement("div");
    notice.id = id;
    notice.className = "shell-notice";
    notice.setAttribute("role", "alert");
    // The page's own (index.html, package.sh).
    (document.getElementById("shell-notices") as HTMLElement).append(notice);
  }
  const button = document.createElement("button");
  button.textContent = retry;
  button.onclick = () => {
    notice.remove();
    again();
  };
  notice.replaceChildren(said, button);
}

// Whether this page's build is gone, and the page is loading again onto the one the server serves
// now. A published client keeps a build two publications more (KEEP in
// crates/symphony/src/cli/publish.rs), so a page open longer asks for a module the server no
// longer has: its import fails. The page then asks for its own shell, bypassing the cache, and a
// 404 says the build is gone; it loads again, once per build it was opened on (kept in the tab's
// storage, without which it never does), and otherwise says the failure as before.
async function gone(): Promise<boolean> {
  const own = new URL(import.meta.url).pathname;
  try {
    if ((await read(own, { cache: "no-store" })).status !== 404) return false;
    const key = stored("symphony-ui-gone-build");
    if (sessionStorage.getItem(key) === own) return false;
    sessionStorage.setItem(key, own);
  } catch {
    return false;
  }
  location.reload();
  return true;
}

// The two a kind's own late loads (a task page's timeline, apps/symphony-web/src/timeline-slot.tsx)
// say a failure with: what the shell's own failures use.
window.reviewShell = { gone, alarm };

// Each of `hrefs` asked for once, by a link of `attributes`, beside the module that needs it.
function preload(hrefs: string[], attributes: string) {
  for (const href of hrefs) {
    if (href && !preloaded.has(href)) {
      preloaded.add(href);
      document.head.insertAdjacentHTML("beforeend", `<link ${attributes} href="${href}">`);
    }
  }
}

// A kind (apps/symphony-web) is its script, which exports its own `run` and `stop`. The script
// asks for the chunks it imports only once it is in: a module preload asks for them beside it.
function initialize(kind: string): Promise<Kind> {
  const spec = modules[kind] as Files;
  preload(spec.imports, 'rel="modulepreload"');
  return import(spec.script);
}

function mountFor(kind: string): HTMLElement {
  const shared = PERSISTENT[kind];
  const id = shared ? shared.root : `review-root-${kind}`;
  let mount = document.getElementById(id);
  if (!mount) {
    mount = document.createElement("div");
    mount.id = id;
    mount.hidden = !shared;
    // The page's own `#main` (index.html, package.sh).
    (shared ? document.body : (document.getElementById("main") as HTMLElement)).append(mount);
  }
  return mount;
}

function styles(spec: Files): Promise<unknown[]> {
  const pending: Promise<unknown>[] = [];
  for (const href of spec.styles) {
    let loaded = styleLoads.get(href);
    if (!loaded) {
      const link = [...document.querySelectorAll<HTMLLinkElement>("link[rel=stylesheet]")].find(
        (link) => link.getAttribute("href") === href,
      );
      if (link?.sheet) loaded = Promise.resolve();
      else {
        const fresh = !link;
        const element = link || document.createElement("link");
        loaded = new Promise((resolve, reject) => {
          element.addEventListener("load", resolve, { once: true });
          element.addEventListener(
            "error",
            () => {
              styleLoads.delete(href);
              element.remove();
              reject(new Error(`Styles could not load: ${href}`));
            },
            { once: true },
          );
        });
        // Page styles go before the project's theme package, which wins the cascade.
        if (fresh) {
          element.rel = "stylesheet";
          element.href = href;
          document.head.insertBefore(element, document.head.querySelector("[data-symphony-theme]"));
        }
      }
      styleLoads.set(href, loaded);
    }
    pending.push(loaded);
  }
  return Promise.all(pending);
}

// A persistent module's script, the chunks it imports and its styles are requested together; it
// mounts once all are in. A failure says so with a retry, and the next use tries again. `focus`: a
// click opened the panel's sheet, which takes the focus.
function ensure(kind: Persistent, focus?: boolean): Promise<unknown> {
  const shared = PERSISTENT[kind];
  const failure = `${kind}-error`;
  let loading = persistent.get(kind);
  if (!loading) {
    loading = (async () => {
      mountFor(kind);
      const [imported] = await Promise.all([initialize(kind), styles(modules[kind] as Files)]);
      await imported.run();
      document.getElementById(failure)?.remove();
      window[shared.ready] = true;
      dispatchEvent(new Event(`review:${kind}-ready`));
      return { imported };
    })().catch(async (error) => {
      persistent.delete(kind);
      if (await gone()) return;
      console.error(shared.said, error);
      if (kind === "panel") loadingSheets();
      alarm(failure, shared.said, shared.retry, () => ensure(kind));
    });
    persistent.set(kind, loading);
  }
  if (kind === "panel") loadingSheets(focus);
  return loading;
}

// Until the panel's module mounts, the sheet or activity list a click opened is drawn here at
// once, with the page frame's own styles and a status line, never a blank gutter. A click's sheet
// takes the focus (`+` hides at desktop width), and keeps it when redrawn; Escape closes it and
// gives the focus back to its opener (`reviewPanelOpener`, as the frame recorded it). Its status
// is filled a frame after the sheet is in the page, so a screen reader announces it. The real
// panel replaces it and takes the focus into its own sheet (apps/symphony-web/src/panel-focus.ts);
// a failed load takes it down beside the retry notice. (A spinner's markup alone would cost every
// first visit some 400 gzip bytes of the owner's budget.)
function loadingSheets(focus?: boolean) {
  const old = document.getElementById("panel-loading");
  focus ||= old?.contains(document.activeElement);
  old?.remove();
  if (!persistent.has("panel") || window.reviewPanelReady) return;
  const sheet = (open: boolean, id: string, label: string, inner: string, what: string) =>
    open
      ? `<aside id="${id}" aria-label="${label}" tabindex="-1">${inner}<p role="status" data-what="${what}"></p></aside>`
      : "";
  const sheets =
    sheet(
      window.reviewActivityOpen === true,
      "activity-panel",
      "Recent activity",
      `<div class="panel-heading"><h2>Recent activity</h2></div>`,
      "activity",
    ) +
    sheet(
      window.reviewQuickDraft?.open === true,
      "quick-sheet",
      "Chat or add a task",
      "",
      "conversation",
    );
  if (!sheets) return;
  document.body.insertAdjacentHTML("beforeend", `<div id="panel-loading">${sheets}</div>`);
  // The sheet just put in the page.
  if (focus) (document.querySelector("#panel-loading aside") as HTMLElement).focus();
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      for (const status of document.querySelectorAll<HTMLElement>("#panel-loading [data-what]"))
        status.textContent = `Loading ${status.dataset.what}…`;
    }),
  );
}
addEventListener("review:panel-ready", () => document.getElementById("panel-loading")?.remove());
addEventListener("keydown", (event) => {
  const sheet =
    event.key === "Escape" &&
    event.target instanceof Element &&
    event.target.closest("#panel-loading aside");
  if (!sheet) return;
  const quick = sheet.id === "quick-sheet",
    opener = window.reviewPanelOpener;
  window.reviewPanelOpener = window.reviewPanelFocus = null;
  // A `+` sheet is drawn here only while the draft says it is open (`loadingSheets`).
  if (quick) (window.reviewQuickDraft as { open?: boolean }).open = false;
  else window.reviewActivityOpen = false;
  dispatchEvent(new Event(quick ? "review:quick" : "review:activity"));
  requestAnimationFrame(() =>
    (opener?.isConnected
      ? opener
      : document.getElementById(quick ? "quick-open" : "bell")
    )?.focus(),
  );
});

// Selects' popups and the picture viewer are data-only requests to the overlay module's host.
window.reviewOverlay = (request) => {
  if (request.kind === "close") {
    if (window.reviewOverlayRequest?.owner !== request.owner) return;
    window.reviewOverlayRequest = null;
    dispatchEvent(new Event("review:overlay"));
    return;
  }
  window.reviewOverlayRequest = request;
  ensure("overlay").then(() => dispatchEvent(new Event("review:overlay")));
};

// The box that scrolls around `node`: its nearest ancestor that scrolls, or none for the page.
function scrollerOf(node: Element): HTMLElement | null {
  let box = node.parentElement;
  while (
    box &&
    !(box.scrollHeight > box.clientHeight && /auto|scroll/.test(getComputedStyle(box).overflowY))
  )
    box = box.parentElement;
  return box;
}

// Whether `r` meets the window, cut to `clip` (a scroller's box) when there is one, and 600 px
// above and below it.
function reaches(r: DOMRect, clip?: DOMRect): boolean {
  return (
    r.bottom > r.top &&
    r.right > r.left &&
    r.bottom > Math.max(0, clip?.top ?? 0) - 600 &&
    r.top < Math.min(innerHeight, clip?.bottom ?? innerHeight) + 600 &&
    r.right > Math.max(0, clip?.left ?? 0) &&
    r.left < Math.min(innerWidth, clip?.right ?? innerWidth)
  );
}

// Whether a box `selector` names is within a sentinel's reach: 600 px above and below what its own
// scroller shows of the window (`.viewport-sentinel`, as `viewport::waits_near` decides).
function near(selector: string): boolean {
  return [...document.querySelectorAll(selector)].some((box) => {
    const r = box.getBoundingClientRect();
    return reaches(r) && reaches(r, scrollerOf(box)?.getBoundingClientRect());
  });
}

// Whether a reserved box waits to draw within its sentinel's reach, for a box past it never draws.
function pendingNear() {
  return near("[data-pending=true]");
}

// Call `stop` on the reader's own scroll, click or key (typing in a field aside); what it returns
// stops listening.
function onGesture(stop: () => void): () => void {
  const gestures = ["wheel", "touchstart", "touchmove", "mousedown", "keydown"];
  const gesture = (event: Event) => {
    if (
      event.type !== "keydown" ||
      // A window or a document has no `matches`.
      !(event.target as Partial<Element>).matches?.("input, textarea, select, [contenteditable]")
    )
      stop();
  };
  for (const kind of gestures) addEventListener(kind, gesture, true);
  return () => {
    for (const kind of gestures) removeEventListener(kind, gesture, true);
  };
}

// A touch that moves owns the scrolling until its scrollers finish their momentum. A refresh
// arriving after touchstart must not begin a position hold in the middle of that gesture. Track
// each scroller: an unrelated box ending its scroll must not end the page's flick.
let touchScroll: {
  moved: boolean;
  down: boolean;
  scrollers: Set<Node>;
  release: AbortController;
} | null = null;
function clearTouchScroll() {
  touchScroll?.release.abort();
  touchScroll = null;
}
addEventListener(
  "touchstart",
  (event) => {
    clearTouchScroll();
    const release = new AbortController();
    touchScroll = { moved: false, down: true, scrollers: new Set(), release };
    // Releases still target where each contact began, even after that node is removed. Keep
    // listeners there as well as at the window, for every contact that remains down.
    const targets = new Set([
      event.target as EventTarget,
      ...Array.from(event.touches, (touch) => touch.target),
    ]);
    for (const target of targets) {
      for (const type of ["touchend", "touchcancel"]) {
        target.addEventListener(type, releaseTouch as EventListener, {
          capture: true,
          passive: true,
          signal: release.signal,
        });
      }
    }
  },
  { capture: true, passive: true },
);
addEventListener(
  "touchmove",
  () => {
    if (touchScroll) touchScroll.moved = true;
  },
  { capture: true, passive: true },
);
addEventListener(
  "scroll",
  (event) => {
    // An element, or the document for the page.
    if (touchScroll?.moved) touchScroll.scrollers.add(event.target as Node);
  },
  { capture: true, passive: true },
);
function releaseTouch(event: TouchEvent) {
  const touch = touchScroll;
  if (!touch || event.touches.length > 0) return;
  touch.down = false;
  touch.release.abort();
  // Scroll events are delivered before the next frame. A touch that scrolled nothing gets no
  // scrollend, so let it go once those events have had their turn.
  requestAnimationFrame(() => {
    if (touchScroll === touch && touch.scrollers.size === 0) clearTouchScroll();
  });
}
addEventListener("touchend", releaseTouch, { capture: true, passive: true });
addEventListener("touchcancel", releaseTouch, { capture: true, passive: true });
addEventListener(
  "scrollend",
  (event) => {
    // Only the end of one of the touch's own scrollers ends it: another box's, the page's own
    // after a script scrolled it, can come between a release and the touch's first scroll event.
    if (!touchScroll?.scrollers.delete(event.target as Node)) return;
    if (!touchScroll.down && touchScroll.scrollers.size === 0) clearTouchScroll();
  },
  { capture: true, passive: true },
);
addEventListener("pagehide", clearTouchScroll);

// Updates sharing a scrolling box share its original anchor until that hold ends. Starting a
// second hold after a row moved would make the two correct toward different positions each frame.
const placeHolds = new Map<
  HTMLElement | null,
  { anchor: Element; stop: () => void; renew: () => void }
>();
addEventListener("pagehide", () => {
  for (const held of placeHolds.values()) held.stop();
});

// A live update about to be drawn keeps what the reader looks at in place: the focused field on
// the screen, else the first of `rows` on it. Only where the reader has scrolled: the box that
// scrolls around it once it is off its top, else the page once it is. One at its top shows
// what lands at the top. Each frame the box and then the page scroll by however far it moved, until
// four pass with nothing moved and no reserved box waiting to draw within its sentinel's reach
// (`pendingNear`). However many frames that takes, as on a slow phone, and 1800 at most (half a
// minute at sixty a second; a hidden tab draws none), a backstop only a box that never draws
// reaches. A move the box and the page cannot take back (one at its end) counts as nothing moved
// once it stays. The reader's own scroll, click or key ends the hold, including a drag or its
// momentum already underway when the update arrived.
window.reviewHoldPlace = (rows) => {
  if (touchScroll && !touchScroll.down) {
    // A removed box's scrollend cannot reach the window. It no longer owns any scrolling,
    // while connected boxes must still finish their momentum before a hold can begin.
    for (const scroller of touchScroll.scrollers) {
      if (!scroller.isConnected) touchScroll.scrollers.delete(scroller);
    }
    if (touchScroll.scrollers.size === 0) clearTouchScroll();
  }
  if (touchScroll?.moved) return;
  const shows = (node: Element) => {
    const top = node.getBoundingClientRect().top,
      box = scrollerOf(node)?.getBoundingClientRect();
    return top >= 0 && top < innerHeight && (!box || (top >= box.top && top < box.bottom));
  };
  const field = document.activeElement;
  const anchor =
    field?.matches("input, textarea, select") && shows(field)
      ? field
      : [...document.querySelectorAll(rows)].find(shows);
  if (!anchor) return;
  const scroller = scrollerOf(anchor),
    page = scrollY > 0;
  if (scroller ? scroller.scrollTop <= 0 : !page) return;
  const held = placeHolds.get(scroller);
  if (held?.anchor.isConnected) {
    held.renew();
    return;
  }
  held?.stop();
  const top = anchor.getBoundingClientRect().top;
  let stopped = false,
    frame = 0,
    quiet = 0,
    stuck = 0;
  const stop = () => {
    stopped = true;
    listening();
    if (placeHolds.get(scroller)?.stop === stop) placeHolds.delete(scroller);
  };
  const listening = onGesture(stop);
  placeHolds.set(scroller, { anchor, stop, renew: () => (quiet = 0) });
  const hold = () => {
    if (!stopped && anchor.isConnected) {
      const moved = anchor.getBoundingClientRect().top - top;
      let left = moved;
      if (Math.abs(left) >= 0.5 && scroller) {
        const before = scroller.scrollTop;
        scroller.scrollTop = before + Math.round(left);
        left -= scroller.scrollTop - before;
      }
      if (Math.abs(left) >= 0.5 && page) {
        const before = scrollY;
        scrollBy(0, left);
        left -= scrollY - before;
      }
      quiet = Math.abs(moved - stuck) >= 0.5 || pendingNear() ? 0 : quiet + 1;
      stuck = left;
      if (quiet < 4 && ++frame < 1800) return requestAnimationFrame(hold);
    }
    stop();
  };
  requestAnimationFrame(hold);
};

// The id the address's fragment names, decoded: empty for none, null for one that does not decode.
function addressed() {
  try {
    return decodeURIComponent(location.hash.slice(1));
  } catch {
    return null;
  }
}

// The keys that scroll the page outside a field (and Space outside a button it would press).
const SCROLL_KEYS = ["PageUp", "PageDown", "Home", "End", "ArrowUp", "ArrowDown", " "];

// Call `stop` on the reader's own input that scrolls the page: a wheel, a touch that moves, a press
// on the page's own scrollbar (its target the root element) or a scroll key. A click, a tap, a
// selection or any other key scrolls nothing. What it returns stops listening.
function onScrollInput(stop: () => void): () => void {
  const kinds = ["wheel", "touchmove", "mousedown", "keydown"];
  const takes = (event: KeyboardEvent) =>
    `input, textarea, select, [contenteditable]${event.key === " " ? ", button, summary" : ""}`;
  const input = (event: Event) => {
    if (
      event.type === "mousedown"
        ? event.target === document.documentElement
        : event.type !== "keydown" ||
          (SCROLL_KEYS.includes((event as KeyboardEvent).key) &&
            // A window or a document has no `matches`.
            !(event.target as Partial<Element>).matches?.(takes(event as KeyboardEvent)))
    )
      stop();
  };
  for (const kind of kinds) addEventListener(kind, input, { capture: true, passive: true });
  return () => {
    for (const kind of kinds) removeEventListener(kind, input, { capture: true });
  };
}

// The landing on the element the address names: one hold for each navigation (`requested`), from
// the first call to land it (`land`, from `scrollAnchor` here and from the web client's route.ts:
// at the page's scroll padding, just below the header). Each frame the element is scrolled into
// view again once something drawn moved it, or the page's length changed (one at the page's end
// may go higher now). The browser's own scroll anchoring holds none of this while the page is
// under its ghost (`.settle`, hidden), and the sections that draw above a card once the landing
// brings them near grow it by their pictures. It is off while the hold runs (`overflow-anchor:
// none` on the root, back once the landing ends): its adjustment for a box that moves what it
// anchors on and not the element (the ghost lifting before the sections near it come, on a loaded
// machine) scrolls the page and the element alike, which the hold would take for a scroll away,
// leaving the element to those sections as they draw above it. A value the root's own style had
// as the landing began is the one put back. The landing ends when it goes
// quiet: four frames with nothing moved, no reserved box waiting within its sentinel's reach
// (`pendingNear`) and no region busy within that reach (`aria-busy`: a ghost not lifted, a read
// not in), or 1800 at most, a backstop as `reviewHoldPlace`'s. It ends too once no element has
// its id (one drawn again in its place within the frame is held in turn), at the reader's input
// that scrolls (`onScrollInput`), and at any scroll that moved the page and the element alike
// (find in the page, a focus brought into view, a script: not a draw, the browser's own pull back
// at the page's end aside). Only the next navigation lands again: a later call (a section's read
// in, the page's last, an outline read again) opens what holds the element while the hold runs,
// and does nothing once it has ended, leaving the page where the reader, or what they did, put it.
/** Where an element and the page are (`placeOf`). */
type Place = [top: number, y: number, inner: number, length: number];
interface Landing {
  generation: number;
  ended: boolean;
  place: Place | null;
  /** The root's own inline `overflow-anchor` as the landing began. */
  anchoring: string;
  listening: () => void;
}
let landing: Landing = {
  generation: 0,
  ended: true,
  place: null,
  anchoring: "",
  listening: () => {},
};

// Where `target` and the page are: its top, the page's scroll and its scroller's, the page's length.
function placeOf(target: Element): Place {
  return [
    target.getBoundingClientRect().top,
    scrollY,
    scrollerOf(target)?.scrollTop ?? 0,
    document.documentElement.scrollHeight,
  ];
}

// Whether the page went from `before` to `now` (each `placeOf`) by a scroll that moved the element
// alike.
function scrolledAway([top, y, inner]: Place, [now, nowY, nowInner, nowLength]: Place): boolean {
  const scrolled = nowY - y + nowInner - inner;
  const pulledBack = nowY < y && nowY >= nowLength - document.documentElement.clientHeight - 1;
  return Math.abs(scrolled) >= 0.5 && !pulledBack && Math.abs(now - top + scrolled) < 1;
}

// End the landing that holds, if one does: the root's own `overflow-anchor` is back.
function endLanding() {
  if (landing.ended) return;
  landing.ended = true;
  landing.listening();
  document.documentElement.style.overflowAnchor = landing.anchoring;
}

// Land on `target`, the element the address names, once for each navigation: open every `details`
// around it, scroll it into view and hold it there (above).
function land(target: Element) {
  const held = landing.generation === requested;
  if (held && landing.ended) return;
  for (let parent: Element | null = target; parent; parent = parent.parentElement) {
    if (parent instanceof HTMLDetailsElement) parent.open = true;
  }
  // The hold running brings it back into view, from where it now is.
  if (held) return;
  endLanding();
  const current: Landing = {
    generation: requested,
    ended: false,
    place: null,
    anchoring: document.documentElement.style.overflowAnchor,
    listening: () => {},
  };
  landing = current;
  document.documentElement.style.overflowAnchor = "none";
  target.scrollIntoView();
  current.place = placeOf(target);
  current.listening = onScrollInput(endLanding);
  const id = target.id,
    href = location.href;
  let frame = 0,
    quiet = 0;
  const hold = () => {
    if (current.ended || landing !== current) return;
    if (location.href !== href) return endLanding();
    // An element drawn again in its place is held in turn; none at all is nothing left to hold.
    const now = document.getElementById(id);
    if (!now) return endLanding();
    // Set as the landing began, before its first frame.
    const before = current.place as Place,
      there = placeOf(now);
    if (scrolledAway(before, there)) return endLanding();
    if (Math.abs(there[0] - before[0]) >= 0.5 || there[3] !== before[3]) now.scrollIntoView();
    current.place = placeOf(now);
    const moved =
      Math.abs(there[0] - before[0]) >= 0.5 || Math.abs(current.place[0] - before[0]) >= 0.5;
    quiet = moved || pendingNear() || near("[aria-busy=true]") ? 0 : quiet + 1;
    if (quiet < 4 && ++frame < 1800) requestAnimationFrame(hold);
    else endLanding();
  };
  requestAnimationFrame(hold);
}
window.reviewLand = land;

// `then`, once the page has drawn its data (none of its reads in flight for half a second, the
// last of them answered) and the browser is idle.
function settled(then: () => void) {
  // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: a browser with no idle callback (Safari) goes on at once
  (window.requestIdleCallback || setTimeout)(
    function next() {
      // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: looks again each 500 ms whether the page's reads went quiet, where the last read's answer is the event
      if (reads || Date.now() - answered < 500) return setTimeout(next, 500);
      then();
    },
    { timeout: 5000 },
  );
}

// Once the first page has drawn its data (none of its reads in flight for half a second, the last
// of them answered) and the browser is idle, the persistent modules' files go into the HTTP cache
// at the lowest priority, nothing evaluated, so a first open waits on no network: the chat's first,
// then, once its script (or its own load, if a click came first) is in, the menus', each its
// script, the chunks it imports that the page has not, and its styles, so no two share a slow link.
// A browser asking to save data, or a 2g link, skips it; the first open then fetches them. A
// browser without idle callbacks warms them at once; one without prefetch (Safari) ignores it.
let warmed = false;
function warm() {
  const link = navigator.connection,
    kinds = Object.keys(PERSISTENT) as Persistent[];
  if (warmed || link?.saveData || /2g/.test(link?.effectiveType ?? "")) return;
  warmed = true;
  // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: a browser with no idle callback (Safari) goes on at once
  (window.requestIdleCallback || setTimeout)(
    function next(): unknown {
      // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: looks again each 500 ms whether the page's reads went quiet, where the last read's answer is the event
      if (reads || Date.now() - answered < 500) return setTimeout(next, 500);
      const kind = kinds.shift();
      if (!kind) return;
      const loading = persistent.get(kind);
      if (loading) return loading.then(next);
      const { script, styles, imports } = modules[kind] as Files;
      for (const href of [script, ...imports, ...styles]) {
        if (!document.querySelector(`link[href="${href}"]`))
          document.head.insertAdjacentHTML("beforeend", `<link rel="prefetch" href="${href}">`);
      }
      const last = document.querySelector<HTMLLinkElement>(`link[rel=prefetch][href="${script}"]`);
      if (last) last.onload = last.onerror = next;
      else next();
    },
    { timeout: 5000 },
  );
}

// Read ahead (docs/engineering/analysis/client-cache.md, "Read ahead"): once a page has drawn its data, its
// feed is open and the browser is idle, the first AHEAD task and document links it shows on the
// screen are read into the client cache one after another, at the lowest priority and outside the
// page's own reads (cache.ts, `ahead`), so opening one draws at once: a task's page and its
// thread's newest page, a document's first part (`view=first`, at most
// symphony_api::AHEAD_BYTES). Not when the browser asks to save data or the link is 3g or slower;
// never a link that acts (a download, `data-native`, another window), a search, a folder,
// another checkout's page or the page itself. A page whose feed is not open within a minute
// reads nothing ahead.
const AHEAD = 8;
function ahead(generation: number, tries = 60) {
  const link = navigator.connection;
  if (link?.saveData || /2g|3g/.test(link?.effectiveType ?? "") || !window.symphonyCache) return;
  settled(() => {
    if (generation !== requested) return;
    const feed = document.querySelector<HTMLElement>("main[data-feed], main [data-feed]")?.dataset
      .feed;
    if (feed !== "live")
      // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: looks again each second, a minute at most, whether the feed is open, where its data-feed turning live is the event
      return feed && tries > 1 && setTimeout(() => ahead(generation, tries - 1), 1000);
    const base = project().base || "",
      id = base.replace(/^\/+/, ""),
      items = new Map<string, [kind: string, reads: Ahead[]]>();
    for (const anchor of document.querySelectorAll<HTMLAnchorElement>("main a[href]")) {
      const box = anchor.getBoundingClientRect(),
        url = inPlace(anchor);
      if (
        !url ||
        url.search ||
        url.pathname === location.pathname ||
        box.bottom <= 0 ||
        box.top >= innerHeight ||
        box.right <= 0 ||
        box.left >= innerWidth ||
        !box.width
      )
        continue;
      const kind = route(url.pathname);
      let path: string;
      try {
        path = decodeURIComponent(url.pathname)
          .slice(base.length)
          .replace(/\/{2,}/g, "/")
          .replace(/^\/(app\/)?/, "");
      } catch {
        continue;
      }
      if (path.startsWith("@")) continue;
      path = path.replace(/^doc\//, "");
      const api = (name: string) => `${base}/api/${name}`,
        encoded = encodeURIComponent;
      if (kind === "task" && /^task\/./.test(path)) {
        const slug = path.slice(5).replace(/\/$/, "");
        items.set(`task:${slug}`, [
          "task",
          [
            ["page", api(`task-page?slug=${encoded(slug)}`)],
            ["thread", api(`task-history?slug=${encoded(slug)}`)],
          ],
        ]);
      } else if (kind === "docs" && path.endsWith(".md")) {
        items.set(`document:${path}`, [
          "document",
          [["outline", api(`document?path=${encoded(path)}&view=first`)]],
        ]);
      }
      if (items.size === AHEAD) break;
    }
    (async () => {
      for (const [item, [kind, reads]] of items) {
        if (generation !== requested) return;
        await symphonyCache.ahead(read, id, item, kind, reads);
      }
    })();
  });
}

// The routes the Symphony answers itself, symphony_api::SYMPHONY_ROUTES word for word (a test
// keeps them equal): they win over every entrypoint.
const SYMPHONY_ROUTES = [
  "tasks",
  "task",
  "agents",
  "queue",
  "reports",
  "runs",
  "api",
  "assets",
  "static",
  "thumb",
  "x",
  "artifacts/uploads",
  "app",
  "doc",
  "history",
  "diff",
  "favicon.ico",
];

function within(route: string, path: string): boolean {
  return route === "" || path === route || path.startsWith(`${route}/`);
}

// The project the server inlines in the page (symphony_api::PROJECT_ELEMENT): its plugins'
// pages and its entrypoints. A page without it is the plain project.
/** The page's project, as far as the shell reads it (symphony_api::ProjectView). */
interface ProjectView {
  base?: string;
  nav?: { href: string }[];
  entrypoints?: string[];
}

function project(): ProjectView {
  try {
    return JSON.parse(document.getElementById("symphony-project")?.textContent || "{}");
  } catch {
    return {};
  }
}

// `key` as the project keeps it in the browser (project::stored): the one server's projects share
// its origin, so each key takes the project's id first; a server of its own keeps it as it is.
function stored(key: string): string {
  const id = (project().base || "").replace(/^\/+/, "");
  return id ? `${id}:${key}` : key;
}

// Whether `path` lies under the project's base (ProjectView::base): the one server serves each
// project at `/<id>/`, a server of its own at `/`. Every link the client draws is written there
// (project::href), so a click follows its address as a new tab or a copy of it does; a link
// elsewhere, the project switch's, loads as the browser loads it.
function ours(path: string): boolean {
  const base = project().base || "";
  return !base || path === base || path.startsWith(`${base}/`);
}

function route(path: string): string | null {
  try {
    path = decodeURIComponent(path);
  } catch {
    return null;
  }
  const base = project().base || "";
  if (base && (path === base || path.startsWith(`${base}/`))) path = path.slice(base.length) || "/";
  // Keep module selection aligned with navigation.rs for scoped URLs and repeated slashes.
  // The empty task route below uses the board module.
  path = path
    .replace(/\/{2,}/g, "/")
    .replace(/^\//, "")
    .replace(/^app\//, "");
  if (path.startsWith("@")) {
    const slash = path.indexOf("/");
    if (slash < 0) return null;
    path = path.slice(slash + 1).replace(/^app\//, "");
  }
  path = path.replace(/^doc\//, "");
  const clean = path.replace(/\/$/, "");
  if (clean === "tasks" || clean === "task") return "board";
  if (path.startsWith("task/")) return "task";
  if (clean === "queue" || clean === "reports" || clean === "runs" || clean === "agents")
    return clean;
  const view = project();
  const pages = (view.nav || []).map((item) => item.href.replace(/^\/+|\/+$/g, "")).filter(Boolean);
  if (pages.includes(clean)) return "docs";
  // A document or folder page of an entrypoint (ProjectView::is_document_route): a trailing
  // slash or an entrypoint's own route names a folder, whatever its name holds; an HTML page (a
  // name before `.html` or `.htm`: `.html` alone names none, as symphony_api::is_html) is a
  // document, its frame drawn in the documents' kind.
  const urls = view.entrypoints?.length ? view.entrypoints : ["docs"];
  // A split gives one word at least.
  const name = clean.split("/").pop() as string;
  if (
    !SYMPHONY_ROUTES.concat(pages).some((taken) => within(taken, clean)) &&
    urls.some((url) => within(url, clean)) &&
    (path.endsWith("/") ||
      urls.includes(clean) ||
      !name.includes(".") ||
      name.endsWith(".md") ||
      /.\.html?$/i.test(name))
  )
    return "docs";
  return clean === "" ? "review" : null;
}

// The id the renderer gives a heading of `text`: src/route.ts's `headingAnchor` word for word (a
// test keeps them equal), so a fragment that names a heading by its text lands it here too.
function headingAnchor(text: string): string {
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

// Land the element the address's fragment names: its id, or, read as a heading's text, that
// heading's (src/route.ts's `anchorIds`).
function scrollAnchor(preserveScroll: boolean) {
  const id = addressed();
  if (id === null) return;
  if (!id) {
    if (!preserveScroll) scrollTo(0, 0);
    return;
  }
  const target = document.getElementById(id) ?? document.getElementById(headingAnchor(id));
  if (target) land(target);
}

async function show(generation: number, scroll: { x: number; y: number; initial: boolean }) {
  if (generation !== requested) return;
  const kind = route(location.pathname);
  if (!kind) {
    location.reload();
    return;
  }
  if (active?.kind !== kind) {
    // Keep the previous page while its replacement loads (its script and the chunks it
    // imports), and under the notice when it does not. Its styles are requested with its
    // script; insets and row geometry must apply before page effects can anchor.
    const spec = modules[kind] as Files;
    const mount = mountFor(kind);
    const [imported] = await Promise.all([initialize(kind), styles(spec)]);
    if (generation !== requested) return;
    if (active) {
      await active.imported.stop();
      active.mount.hidden = true;
    }
    mount.hidden = false;
    await imported.run();
    active = { kind, imported, mount };
  }
  document.getElementById("module-error")?.remove();
  // The page's root element, which scrolls it.
  const root = document.scrollingElement as Element;
  const position = {
    x: scrollX,
    y: scrollY,
    maxX: Math.max(0, root.scrollWidth - root.clientWidth),
    maxY: Math.max(0, root.scrollHeight - root.clientHeight),
  };
  dispatchEvent(new Event("review:navigate"));
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      if (generation !== requested) return;
      // Only shrinking bounds after the mounted baseline can account for a later clamp.
      // Movement while switching away from the old page must not count as a new scroll.
      const maxX = Math.max(0, root.scrollWidth - root.clientWidth);
      const maxY = Math.max(0, root.scrollHeight - root.clientHeight);
      const x = maxX < position.maxX ? Math.min(position.x, maxX) : position.x;
      const y = maxY < position.maxY ? Math.min(position.y, maxY) : position.y;
      const moved = x !== scrollX || y !== scrollY;
      const restored = scroll.initial && (scroll.x !== 0 || scroll.y !== 0);
      scrollAnchor(moved || restored);
      document.documentElement.dataset.navigation = "ready";
      warm();
      ahead(generation);
    }),
  );
}

function navigate() {
  const overlay = window.reviewOverlayRequest;
  if (overlay) {
    window.reviewOverlayRequest = null;
    dispatchEvent(new Event("review:overlay"));
    overlay.callback?.(null);
  }
  endLanding();
  const generation = ++requested;
  const scroll = { x: scrollX, y: scrollY, initial: !active };
  document.documentElement.dataset.navigation = "loading";
  switching = switching
    .catch(() => {})
    .then(() => show(generation, scroll))
    .catch(async (error) => {
      if (await gone()) return;
      if (generation === requested) document.documentElement.dataset.navigation = "failed";
      console.error("Page could not be loaded", error);
      alarm("module-error", "This page could not be loaded. ", "Retry", navigate);
    });
}

// The address a click on `link` takes in place, the shell's to follow, or nothing: a link that
// acts (a download, `data-native`, another window) or leads off the project's pages is the
// browser's.
function inPlace(link: HTMLAnchorElement | null | false): URL | undefined {
  if (
    !link ||
    link.hasAttribute("download") ||
    link.dataset.native === "true" ||
    (link.target && link.target !== "_self")
  )
    return;
  const url = new URL(link.href, location.href);
  if (url.origin === location.origin && ours(url.pathname) && route(url.pathname)) return url;
}

document.addEventListener("click", (event) => {
  if (
    event.defaultPrevented ||
    event.button !== 0 ||
    event.ctrlKey ||
    event.metaKey ||
    event.shiftKey ||
    event.altKey
  )
    return;
  const url = inPlace(event.target instanceof Element && event.target.closest("a[href]"));
  if (!url) return;
  event.preventDefault();
  if (document.querySelector("[data-posting=true]")) return;
  history.pushState(null, "", url);
  navigate();
});
addEventListener("popstate", navigate);
// Opening the "+" sheet or the activity list loads the panel; closing one while it loads takes
// its loading sheet down. The header's Appearance panel is in the same module and has no loading
// sheet: it takes the focus itself once the module mounts.
addEventListener("review:quick", () => {
  if (window.reviewQuickDraft?.open) ensure("panel", true);
  else loadingSheets();
});
addEventListener("review:activity", () => {
  if (window.reviewActivityOpen) ensure("panel", true);
  else loadingSheets();
});
addEventListener("review:appearance", () => {
  if (window.reviewAppearanceOpen) ensure("panel");
});
addEventListener("review:activity-arrived", () => ensure("panel"));
async function pendingStep(step: string) {
  await ensure("panel");
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(`[data-pending-action=${step}]`)?.click();
    }),
  );
}
document.addEventListener("click", (event) => {
  const step =
    event.target instanceof Element &&
    event.target.closest<HTMLElement>("[data-pending-step]")?.dataset.pendingStep;
  if (step) {
    event.preventDefault();
    pendingStep(step);
  }
});
addEventListener("keydown", (event) => {
  if (window.reviewPanelReady || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey)
    return;
  if (
    event.target instanceof Element &&
    event.target.closest(
      "input,textarea,select,[contenteditable],dialog[open],[role=dialog],[role=listbox],[role=menu]",
    )
  )
    return;
  const step = event.key === "[" ? "prev" : event.key === "]" ? "next" : null;
  if (step) {
    event.preventDefault();
    pendingStep(step);
  }
});
addEventListener("beforeunload", (event) => {
  if (document.querySelector("[data-posting=true]")) {
    event.preventDefault();
    event.returnValue = "";
  }
});
navigate();
