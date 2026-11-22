// Mount costly content once, just before it scrolls into view, and keep it mounted after that
// first visit (open forms and media stay as they are). The sentinel reaches 600 px each way
// (`.viewport-sentinel`). A page's ghost draws each part it drew before at the height it took.

import {
  type CSSProperties,
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { anchor, anchorIds, scrollAnchor } from "./route";
import { load, save } from "./storage";
// biome-ignore lint/style/noRestrictedImports: no-timed-waits baseline: rememberDrawn measures what it keeps every 100 ms, where a ResizeObserver of each part is the event
import { deadline, sleep } from "./timing";

/** The height of a line of body text, in px (16 px at a line height of 1.55, and its gap). */
const LINE = 26;

/** How many lines of text a placeholder `height` px tall stands for: one at least, six at most. */
export function linesFor(height: number): number {
  return Math.min(Math.max(Math.floor(height / LINE), 1), 6);
}

export interface SkeletonProps {
  lines?: number;
  label?: string;
}

/**
 * A paragraph waiting for its read: a bar where each of its lines will be, and `label` for a reader
 * of the page's text instead.
 */
export function Skeleton({ lines = 3, label = "" }: SkeletonProps) {
  return (
    <div className="skeleton">
      {label !== "" && (
        <span className="status-text" role="status">
          {label}
        </span>
      )}
      {Array.from({ length: lines }, (_, line) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: the bars are alike, and only their count changes
        <span key={line} className="bone" aria-hidden="true" />
      ))}
    </div>
  );
}

/**
 * Whether what is drawn inside mounts at once rather than as it nears the screen: content the
 * reader asked for by opening it (a link's excerpt), whose scrolling box would clip the sentinel's
 * look-ahead and leave it waiting until the box itself is on screen.
 */
export const AtOnce = createContext(false);

export interface NearViewportProps {
  children: ReactNode;
  /** The box's height until its content mounts, in px. */
  height?: number;
  /** The outline of what the content will be; lines of text when none is given. */
  ghost?: ReactNode;
  /** Ids the placeholder holds, so a link to one finds its place before the content mounts. */
  anchors?: string[];
  id?: string;
  className?: string;
  /** Called once, the first time the content mounts. */
  onEnter?: () => void;
  /** Whether the box is a list's item (`li`) rather than a `div`. */
  listItem?: boolean;
  /** Mount the content now, near the screen or not (a part the focus is given to), and keep it. */
  eager?: boolean;
  /**
   * Whether a part the address names mounts at once and is landed, as it mounts: false once the
   * reader has acted since following the address, the part then waiting as any other, its anchors
   * still holding the address's place.
   */
  lands?: boolean;
}

/**
 * `children` mounted once they come near the screen, or once `eager`; until then `ghost`, in a box
 * of `height` px. A part the address's fragment names mounts at once, and is scrolled to, while it
 * `lands`; one under `AtOnce` mounts at once too, and is not.
 */
export function NearViewport({
  children,
  height = 160,
  ghost,
  anchors = [],
  id = "",
  className = "",
  onEnter,
  listItem = false,
  eager = false,
  lands = true,
}: NearViewportProps) {
  const [addressed] = useState(
    () => lands && anchorIds(anchor()).some((name) => anchors.includes(name) || name === id),
  );
  const atOnce = useContext(AtOnce);
  const [latched, setShown] = useState(addressed || atOnce);
  // An eager part draws its content in the very draw that made it eager, and stays drawn after.
  const shown = latched || eager;
  const sentinel = useRef<HTMLDivElement>(null);
  const entered = useRef(onEnter);
  entered.current = onEnter;
  useEffect(() => {
    if (shown && !latched) {
      setShown(true);
      return;
    }
    if (shown) {
      if (addressed) scrollAnchor();
      entered.current?.();
      return;
    }
    const node = sentinel.current;
    if (!node) return;
    if (typeof IntersectionObserver === "undefined") {
      // A browser without it must still show the content.
      setShown(true);
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) setShown(true);
    });
    observer.observe(node);
    return () => {
      observer.disconnect();
      observer.takeRecords();
    };
  }, [shown, latched, addressed]);
  const style: CSSProperties = {
    containIntrinsicSize: `auto ${height}px`,
    minHeight: `${shown ? 0 : height}px`,
  };
  const Box = listItem ? "li" : "div";
  return (
    <Box id={id === "" ? undefined : id} style={style} className={`viewport-slot ${className}`}>
      {shown ? (
        children
      ) : (
        <>
          <div className="viewport-sentinel" aria-hidden="true" ref={sentinel} />
          <div
            className="viewport-placeholder"
            style={{ minHeight: `${height}px` }}
            aria-hidden="true"
            data-pending="true"
          >
            {anchors.map((name) => (
              <span key={name} id={name} />
            ))}
            {ghost ?? <Skeleton lines={linesFor(height)} />}
          </div>
        </>
      )}
    </Box>
  );
}

/** The longest a page stays under its skeleton waiting for its first screen's reads, in ms. */
export const SETTLE_MS = 3_000;

let settles = 0;

/**
 * Whether `root` has not drawn its page yet (only its cover is in it), or a region of it on the
 * screen, the part of the window its page covers, still waits for its read.
 */
export function unsettled(root: Element): boolean {
  if (root.childElementCount < 2) return true;
  const page = root.getBoundingClientRect();
  const screen = [0, innerHeight, Math.max(page.left, 0), Math.min(page.right, innerWidth)];
  return [...root.querySelectorAll("[data-pending=true]")].some((region) => {
    const box = region.getBoundingClientRect();
    return onScreen([box.top, box.bottom, box.left, box.right], screen);
  });
}

/**
 * `settled` once nothing under `root` waits on its first screen (`unsettled`), or SETTLE_MS
 * passed (its deadline), once: it looks at once, then at each change that can settle it or move
 * its regions on or off the screen. Those are a region drawn or marked in it, its box resized,
 * the page's own size changed (what draws above it, a task's gates and timeline on a phone,
 * moves it with no change of its own), the page scrolled, or the window resized. The function it
 * gives stops it unsettled.
 */
export function watchSettle(root: Element, settled: () => void): () => void {
  const watching = new AbortController();
  const drawn = new MutationObserver(() => look());
  const resized = new ResizeObserver(() => look());
  const stop = () => {
    watching.abort();
    drawn.disconnect();
    resized.disconnect();
    ended();
  };
  const settle = () => {
    stop();
    settled();
  };
  const look = () => {
    if (!watching.signal.aborted && !unsettled(root)) settle();
  };
  const ended = deadline(SETTLE_MS, settle);
  drawn.observe(root, { subtree: true, childList: true, attributeFilter: ["data-pending"] });
  resized.observe(root);
  resized.observe(document.documentElement);
  addEventListener("scroll", look, { passive: true, signal: watching.signal });
  addEventListener("resize", look, { signal: watching.signal });
  look();
  return stop;
}

export interface SettleProps {
  children: ReactNode;
  /** The outline of the page `children` make, drawn over them until they settle. */
  cover: ReactNode;
  failed?: boolean;
}

/**
 * Draw `children` hidden under `cover` until no region they reserve on the first screen
 * (`[data-pending=true]`) waits for its read any longer, until `failed`, or for SETTLE_MS at most
 * (its deadline): the reader's first screen arrives whole, and no read that lands moves it
 * (`watchSettle`).
 */
export function Settle({ children, cover, failed = false }: SettleProps) {
  const [id] = useState(() => `web-${settles++}`);
  const [settled, setSettled] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const done = settled || failed;
  useEffect(() => {
    if (failed) setSettled(true);
  }, [failed]);
  useEffect(() => {
    const node = root.current;
    if (done || node === null) return;
    return watchSettle(node, () => setSettled(true));
  }, [done]);
  return (
    <div
      className="settle"
      data-settle={id}
      data-settled={`${done}`}
      aria-busy={`${!done}`}
      ref={root}
    >
      {!done && <div className="settle-cover">{cover}</div>}
      {children}
    </div>
  );
}

/**
 * The latch of a part that keeps its own box until it comes into view (a picture, a mockup, a
 * viewer, a table's row): whether it has, and the ref that names the element to watch. Once shown
 * it stays shown, and a new element in its place is watched in turn. Under `AtOnce`, or where
 * there is no IntersectionObserver (a browser without one, or the unit tests' Node), it shows at
 * once.
 */
export function useVisibility(initial = false): [boolean, (node: Element | null) => () => void] {
  const atOnce = useContext(AtOnce);
  const [shown, setShown] = useState(
    () => initial || atOnce || typeof IntersectionObserver === "undefined",
  );
  const watch = useCallback(
    (node: Element | null) =>
      node === null || shown ? () => {} : watchUntilShown(node, () => setShown(true)),
    [shown],
  );
  return [shown, watch];
}

/** Watch `node` until it comes into view, then call `reveal`; what it returns stops the watch. */
export function watchUntilShown(node: Element, reveal: () => void): () => void {
  const observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) reveal();
  });
  observer.observe(node);
  return () => {
    observer.disconnect();
    observer.takeRecords();
  };
}

/** Where this browser keeps the heights parts of pages were last drawn at, newest first. */
const DRAWN = "drawn-heights";
/** How many parts' heights a browser keeps. */
const DRAWN_MAX = 128;

/**
 * The height `kept` (lines of `WIDTH HEIGHT KEY`) gives the part `key` in a window `width` px wide;
 * for a part never drawn at that width, the one the last part of its kind took there (the kind is
 * the key before its last space). A key with no space has no kind.
 */
export function recallHeight(kept: string, key: string, width: number): number | null {
  const kindOf = (part: string) => {
    const space = part.lastIndexOf(" ");
    return space < 0 ? null : part.slice(0, space);
  };
  const kind = kindOf(key);
  let alike: number | null = null;
  for (const line of kept.split("\n")) {
    const [drawnAt, tall, ...rest] = line.split(" ");
    const part = rest.join(" ");
    if (rest.length === 0 || !/^\d+$/.test(drawnAt ?? "") || !/^\d+$/.test(tall ?? "")) continue;
    if (Number(drawnAt) !== width) continue;
    if (part === key) return Number(tall);
    if (alike === null && kind !== null && kindOf(part) === kind) alike = Number(tall);
  }
  return alike;
}

/** `kept` with the part `key` drawn `height` px tall at `width` first, its old line dropped. */
export function keepHeight(kept: string, key: string, width: number, height: number): string {
  const others = kept
    .split("\n")
    .filter((line) => line !== "" && line.split(" ").slice(2).join(" ") !== key);
  return [`${width} ${height} ${key}`, ...others].slice(0, DRAWN_MAX).join("\n");
}

/** The window's width in whole px. */
export function windowWidth(): number {
  return Math.round(innerWidth);
}

/** The height this browser last drew the part `key` at, in a window as wide as this one. */
export function drawnHeight(key: string): number | null {
  const kept = load(DRAWN);
  return kept === null ? null : recallHeight(kept, key, windowWidth());
}

/** The style that draws a ghost's box as tall as the part it stands for was drawn. */
export function drawnStyle(height: number | null): CSSProperties | undefined {
  return height === null
    ? undefined
    : { boxSizing: "border-box", height: `${height}px`, overflow: "hidden" };
}

/** How far above and below the screen a placeholder's sentinel reaches, in px. */
export const REACH = 600;

/** The longest a keep for the next visit's ghost watches what is near the screen draw, in ms. */
export const KEEP_MS = 30_000;

/** Whether a region's box (`[top, bottom, left, right]`) shows on `screen`, in the same terms. */
export function onScreen(region: number[], screen: number[]): boolean {
  const [top = 0, bottom = 0, left = 0, right = 0] = region;
  const [above = 0, below = 0, start = 0, end = 0] = screen;
  return (
    bottom > top && right > left && bottom > above && top < below && right > start && left < end
  );
}

/**
 * Whether any region still waiting for its read inside `part` is within reach of the screen: the
 * window, cut to the box of `part`'s closest ancestor `scroller` names when it sits in one.
 */
export function waitsNear(part: Element, scroller?: string): boolean {
  let view = [0, innerHeight, 0, innerWidth];
  const clip = scroller === undefined ? null : part.closest(scroller);
  if (clip) {
    const box = clip.getBoundingClientRect();
    const [top = 0, bottom = 0, left = 0, right = 0] = view;
    view = [
      Math.max(top, box.top),
      Math.min(bottom, box.bottom),
      Math.max(left, box.left),
      Math.min(right, box.right),
    ];
  }
  const [top = 0, bottom = 0, left = 0, right = 0] = view;
  const screen = [top - REACH, bottom + REACH, left, right];
  return [...part.querySelectorAll("[data-pending=true]")].some((region) => {
    const box = region.getBoundingClientRect();
    return onScreen([box.top, box.bottom, box.left, box.right], screen);
  });
}

/**
 * Keep, from `after` ms on, the height of each of `parts` (`[key, selector]`) for its ghost on the
 * next visit: measured every 100 ms and kept again when one changed, until no region still
 * waiting for its read in them is within reach of the screen, or for KEEP_MS at most, or until
 * `signal` aborts.
 */
export async function rememberDrawn(
  parts: [string, string][],
  after: number,
  signal: AbortSignal,
): Promise<void> {
  await sleep(after, signal);
  let waited = after;
  let saved = "[]";
  while (!signal.aborted) {
    const heights: [string, number][] = [];
    let waiting = false;
    for (const [key, selector] of parts) {
      const part = document.querySelector(selector);
      if (part) {
        heights.push([key, Math.round(part.getBoundingClientRect().height)]);
        waiting ||= waitsNear(part);
      }
    }
    const measured = JSON.stringify(heights);
    if (measured !== saved) {
      const width = windowWidth();
      save(
        DRAWN,
        heights.reduce(
          (kept, [key, height]) => keepHeight(kept, key, width, height),
          load(DRAWN) ?? "",
        ),
      );
      saved = measured;
    }
    if (!waiting || waited >= KEEP_MS) return;
    await sleep(100, signal);
    waited += 100;
  }
}
