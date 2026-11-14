// Mount costly content once, just before it scrolls into view, and keep it mounted after that
// first visit (open forms and media stay as they are). The sentinel reaches 600 px each way
// (`.viewport-sentinel`). A page's ghost draws each part it drew before at the height it took.

import { type CSSProperties, type ReactNode, useEffect, useRef, useState } from "react";
import { sleep } from "./feed";
import { anchor, scrollAnchor } from "./route";
import { load, save } from "./storage";

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
}

/**
 * `children` mounted once they come near the screen; until then `ghost`, in a box of `height` px.
 * A part the address's fragment names mounts at once, and is scrolled to.
 */
export function NearViewport({
  children,
  height = 160,
  ghost,
  anchors = [],
  id = "",
  className = "",
}: NearViewportProps) {
  const [addressed] = useState(() => {
    const target = anchor();
    return target !== "" && (anchors.includes(target) || target === id);
  });
  const [shown, setShown] = useState(addressed);
  const sentinel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (shown) {
      if (addressed) scrollAnchor();
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
  }, [shown, addressed]);
  const style: CSSProperties = {
    containIntrinsicSize: `auto ${height}px`,
    minHeight: `${shown ? 0 : height}px`,
  };
  return (
    <div id={id === "" ? undefined : id} style={style} className={`viewport-slot ${className}`}>
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
    </div>
  );
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

/** Whether any region still waiting for its read inside `part` is within reach of the screen. */
function waitsNear(part: Element): boolean {
  const screen = [-REACH, innerHeight + REACH, 0, innerWidth];
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
