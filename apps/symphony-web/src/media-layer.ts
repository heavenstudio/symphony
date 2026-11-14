// The page's media layer, the Dioxus client's media_live.rs: started once a part of the page shows
// media (`review:media`, said by each piece of media.tsx as it comes into view, or a picture already
// shown in the chat's panel), it scales each mockup's frame to its box, loads a viewer's script once,
// runs a view's buttons, and opens a picture in the picture viewer, the overlay module's (reached
// through the shell's `reviewOverlay`), on a click, Enter or Space; the viewer's arrows and Escape
// move through and close it. It observes the whole page, the panel's media too, and whatever it
// added ends with the page's mount, the viewer closed.

import { useEffect } from "react";
import { decode, localAsset, pathOf, WAITING } from "./media";
import { useMount } from "./mount";
import { snapshot } from "./shared";

/** A picture as the viewer shows it: its address and its caption. */
export interface Photo {
  src: string;
  caption: string;
}

/** The element an event's target is, or none. */
function element(target: EventTarget | null): Element | null {
  return typeof (target as Element | null)?.closest === "function" ? (target as Element) : null;
}

/**
 * Whether a picture opens in the viewer: not one the page set apart, hid or made decoration, and
 * not inside a link unless the link is to a picture.
 */
export function zoomable(image: Element): boolean {
  if (
    image.hasAttribute("data-no-lightbox") ||
    image.getAttribute("aria-hidden") === "true" ||
    ["presentation", "none"].includes(image.getAttribute("role") ?? "")
  ) {
    return false;
  }
  const link = image.closest("a[href]");
  if (!link) return true;
  const path = pathOf(link.getAttribute("href") ?? "");
  return [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"].some((ext) => path.endsWith(ext));
}

/**
 * A picture for the viewer, from its original or its src; none for a document picture still
 * waiting to come into view, whose src is no picture yet.
 */
export function photo(image: Element): Photo | null {
  const src = image.getAttribute("src");
  const shown = image.getAttribute("data-full") ?? (src === WAITING ? null : src);
  return shown === null ? null : { src: shown, caption: image.getAttribute("alt") ?? "" };
}

/** A mockup's frame, 1280 pixels wide, scaled to its box's width. */
export function scaleFrame(stage: Element): void {
  const width = stage.getBoundingClientRect().width;
  const frame = width > 0 ? stage.querySelector("iframe") : null;
  frame?.setAttribute("style", `transform:scale(${width / 1280})`);
}

/**
 * The module script a viewer's stage names, a project's own, added to the page once: a viewer's
 * module is no part of the page's first requests.
 */
export function loadViewer(stage: Element): void {
  const src = stage.getAttribute("data-viewer-script");
  if (src === null || !localAsset(src)) return;
  const loaded = [...document.querySelectorAll("script[data-viewer-module]")].some(
    (script) => script.getAttribute("data-viewer-module") === src,
  );
  if (loaded) return;
  const script = document.createElement("script");
  script.setAttribute("data-viewer-module", src);
  script.setAttribute("type", "module");
  script.setAttribute("src", src);
  document.head.append(script);
}

/**
 * What the page shows now, readied: each new mockup's frame observed and scaled, each viewer's
 * script loaded, and each new picture of the page that opens in the viewer and is no link made a
 * button that names it (unless the page gave it its own). `targets` are the frames observed: those
 * the page dropped are no longer.
 */
export function scan(resize: ResizeObserver, targets: Set<Element>): void {
  for (const target of targets) {
    if (!target.isConnected) {
      resize.unobserve(target);
      targets.delete(target);
    }
  }
  const root = document.documentElement;
  for (const stage of root.querySelectorAll(".mockup-frame:not([data-media-observed])")) {
    stage.setAttribute("data-media-observed", "true");
    targets.add(stage);
    resize.observe(stage);
    scaleFrame(stage);
  }
  for (const stage of root.querySelectorAll(".viewer-stage[data-viewer-script]")) {
    loadViewer(stage);
  }
  for (const image of root.querySelectorAll("main img:not([data-zoom-ready])")) {
    image.setAttribute("data-zoom-ready", "true");
    if (!zoomable(image) || image.closest("a")) continue;
    if (!image.hasAttribute("tabindex")) image.setAttribute("tabindex", "0");
    if (!image.hasAttribute("role")) image.setAttribute("role", "button");
    if (!image.hasAttribute("aria-label")) {
      image.setAttribute("aria-label", `Open picture ${image.getAttribute("alt") ?? ""}`);
    }
  }
}

/**
 * The pictures a picture opens the viewer on, and its place among them: those of its card's
 * comparison (its card's own, from the card's history), else of the page; itself alone when it is
 * none of them. None for a picture that is no picture yet.
 */
export function pictureSet(image: Element): [Photo[], number] | null {
  const card = image.closest(".decision");
  const scope = card
    ? image.closest(".history")
      ? card
      : (card.querySelector(".compare") ?? card)
    : image.closest("main");
  if (!scope) return null;
  const photos: Photo[] = [];
  let at: number | null = null;
  for (const other of scope.querySelectorAll("img")) {
    const shown = zoomable(other) ? photo(other) : null;
    if (shown === null) continue;
    if (other === image) at = photos.length;
    photos.push(shown);
  }
  if (at !== null) return [photos, at];
  const alone = photo(image);
  return alone === null ? null : [[alone], 0];
}

/** A request to the overlay module's host, which the shell's `reviewOverlay` takes. */
function overlay(request: Record<string, unknown>): void {
  const show = snapshot("reviewOverlay");
  if (typeof show === "function") show(request);
}

/**
 * The picture viewer this page opens, as `owner` (`review-pictures`): the overlay module draws it
 * from the pictures and the place given here, and answers its buttons with a direction (0 closes).
 * Nothing more is asked of it once `signal` ends, but its close.
 */
export class Viewer {
  private photos: Photo[] = [];
  private at = 0;

  constructor(
    private readonly owner: string,
    private readonly signal: AbortSignal,
  ) {}

  /** What the viewer's buttons say: a direction to move by; 0, or anything but a number, closes. */
  readonly answer = (direction: unknown): void => {
    const by =
      typeof direction === "number" && Number.isFinite(direction) ? Math.trunc(direction) : 0;
    if (by === 0) this.close();
    else this.move(by);
  };

  open(photos: Photo[], at: number): void {
    this.photos = photos;
    this.at = at;
    this.show();
  }

  /** The picture `by` places on, round the set. */
  move(by: number): void {
    const count = this.photos.length;
    if (count === 0) return;
    this.at = (((this.at + by) % count) + count) % count;
    this.show();
  }

  close(): void {
    this.photos = [];
    this.show();
  }

  private show(): void {
    if (this.signal.aborted) return;
    if (this.photos.length === 0) {
      overlay({ kind: "close", owner: this.owner });
      return;
    }
    overlay({
      kind: "picture",
      owner: this.owner,
      at: this.at,
      photos: this.photos.map((shown) => ({ ...shown })),
      callback: this.answer,
    });
  }

  /** The viewer closed for good: its page is gone. */
  end(): void {
    overlay({ kind: "close", owner: this.owner });
  }
}

/**
 * A click (or Enter or Space) on the page: on the open viewer's backdrop it closes it; on a view's
 * button it sets the attributes the view lists on its viewer's element and calls its method; on a
 * picture that opens in the viewer (a plain press, no modifier) it opens the viewer on it.
 */
export function pressed(event: Event, viewer: Viewer): void {
  const target = element(event.target);
  if (!target) return;
  if (target.id === "media-lightbox") {
    viewer.close();
    return;
  }
  const button = target.closest("[data-viewer-set]");
  if (button) {
    const shown = button.closest(".media-viewer")?.querySelector(".viewer-stage > *");
    if (shown) {
      for (const [name, value] of decode(button.getAttribute("data-viewer-set") ?? "")) {
        shown.setAttribute(name, value);
      }
      const call = button.getAttribute("data-viewer-call");
      const method =
        call === null ? undefined : (shown as unknown as Record<string, unknown>)[call];
      if (typeof method === "function") method.call(shown);
    }
    return;
  }
  const mouse = event as MouseEvent;
  if (
    "button" in event &&
    (mouse.ctrlKey || mouse.metaKey || mouse.shiftKey || mouse.button !== 0)
  ) {
    return;
  }
  const image = target.closest("main img");
  const set = image && zoomable(image) ? pictureSet(image) : null;
  if (!set) return;
  event.preventDefault();
  viewer.open(...set);
}

/** The open viewer's keys, each by the direction it moves in: 0 closes. */
const VIEWER_KEYS: Record<string, number> = { Escape: 0, ArrowLeft: -1, ArrowRight: 1 };

/** A key on the page: the open viewer's arrows and Escape, else Enter or Space as a press. */
export function keyed(event: KeyboardEvent, viewer: Viewer): void {
  const lightbox = document.getElementById("media-lightbox");
  if (lightbox?.getAttribute("data-state") === "open") {
    const move = VIEWER_KEYS[event.key];
    if (move === undefined) return;
    event.preventDefault();
    if (move === 0) viewer.close();
    else viewer.move(move);
  } else if ((event.key === "Enter" || event.key === " ") && !event.repeat) {
    pressed(event, viewer);
  }
}

/**
 * Start the layer as `owner` until `signal` ends: its observers watch the page, and its listeners
 * hear its clicks, keys and folds (a fold opened is busy and shows what it holds). When `signal`
 * ends they stop, the frames are no longer marked observed, and the viewer closes.
 */
export function startMedia(owner: string, signal: AbortSignal): void {
  if (signal.aborted) return;
  const viewer = new Viewer(owner, signal);
  const targets = new Set<Element>();
  const resize = new ResizeObserver((entries) => {
    for (const entry of entries) scaleFrame(entry.target);
  });
  const changed = new MutationObserver(() => scan(resize, targets));
  changed.observe(document.documentElement, { childList: true, subtree: true });
  const click = (event: Event) => pressed(event, viewer);
  const keys = (event: Event) => keyed(event as KeyboardEvent, viewer);
  const toggle = (event: Event) => {
    const details = element(event.target);
    if (details?.tagName !== "DETAILS") return;
    if (details.hasAttribute("open")) {
      details.setAttribute("data-busy", "true");
      scan(resize, targets);
    } else {
      details.removeAttribute("data-busy");
    }
  };
  document.addEventListener("click", click);
  document.addEventListener("keydown", keys);
  document.addEventListener("toggle", toggle, true);
  scan(resize, targets);
  signal.addEventListener(
    "abort",
    () => {
      resize.disconnect();
      changed.disconnect();
      document.removeEventListener("click", click);
      document.removeEventListener("keydown", keys);
      document.removeEventListener("toggle", toggle, true);
      for (const stage of document.documentElement.querySelectorAll("[data-media-observed]")) {
        stage.removeAttribute("data-media-observed");
      }
      viewer.end();
    },
    { once: true },
  );
}

/** Whether the chat's panel shows a picture, a viewer or a frame already, which said so before. */
export function panelShowsMedia(): boolean {
  return [
    ...document.documentElement.querySelectorAll(
      "#quick-root img, #quick-root .viewer-stage, #quick-root iframe",
    ),
  ].some((node) => {
    const box = node.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  });
}

/**
 * The media layer of the page drawn under it, as `owner`: started the first time the page or the
 * panel shows media, and ended with the page's mount. Drawn before the page, so it hears what the
 * page's first media say.
 */
export function MediaLayer({ owner }: { owner: string }) {
  const mount = useMount();
  useEffect(() => {
    const layer = new AbortController();
    const end = () => layer.abort();
    mount.addEventListener("abort", end);
    const start = () => {
      removeEventListener("review:media", start);
      startMedia(owner, layer.signal);
    };
    addEventListener("review:media", start);
    if (panelShowsMedia()) start();
    return () => {
      removeEventListener("review:media", start);
      mount.removeEventListener("abort", end);
      layer.abort();
    };
  }, [mount, owner]);
  return null;
}
