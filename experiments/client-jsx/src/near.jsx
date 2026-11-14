// `viewport::NearViewport`: children mounted once they come near the screen, and kept mounted.
// Until then a placeholder of `height` px, with a sentinel the shared style sheet extends 600 px
// around it (`.viewport-sentinel`), watched by one IntersectionObserver for the whole page.
import { useEffect, useRef, useState } from "react";

const waiting = new Map();
let observer = null;

function watch(element, reveal) {
  if (!observer) {
    observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        const shown = waiting.get(entry.target);
        if (shown) {
          waiting.delete(entry.target);
          observer.unobserve(entry.target);
          shown();
        }
      }
    });
  }
  waiting.set(element, reveal);
  observer.observe(element);
  return () => {
    waiting.delete(element);
    observer.unobserve(element);
  };
}

/**
 * `viewport::use_visibility`: the latch for an element with its own reserved layout (a picture, a
 * table row, a media shell), watched itself, with no sentinel. Returns `[shown, ref]`; once shown
 * it stays shown.
 */
export function useVisible() {
  const [shown, setShown] = useState(false);
  const node = useRef(null);
  useEffect(() => {
    if (shown || !node.current) return;
    if (typeof IntersectionObserver === "undefined") return setShown(true);
    return watch(node.current, () => setShown(true));
  }, [shown]);
  return [shown, node];
}

/** A page's sections waiting near the screen, as a line of text each. */
export function Skeleton({ lines = 3, label = "" }) {
  return (
    <div className="skeleton">
      {label ? <span className="status-text" role="status">{label}</span> : null}
      {Array.from({ length: lines }, (_, line) => (
        <span key={line} className="bone" aria-hidden="true" />
      ))}
    </div>
  );
}

const LINE = 26;
export const linesFor = (height) => Math.min(6, Math.max(1, Math.floor(height / LINE)));

export function Near({ id, height = 160, className = "", listItem = false, anchors = [], onEnter, ghost, children }) {
  const addressed = anchors.length > 0 && anchors.includes(decodeURIComponent(location.hash.slice(1)));
  const [shown, setShown] = useState(addressed);
  const sentinel = useRef(null);
  const entered = useRef(false);
  useEffect(() => {
    if (shown || !sentinel.current) return;
    return watch(sentinel.current, () => setShown(true));
  }, [shown]);
  useEffect(() => {
    if (shown && !entered.current) {
      entered.current = true;
      onEnter?.();
    }
  }, [shown, onEnter]);
  const content = shown ? (
    children
  ) : (
    <>
      <div className="viewport-sentinel" aria-hidden="true" ref={sentinel} />
      <div className="viewport-placeholder" style={{ minHeight: `${height}px` }} aria-hidden="true" data-pending="true">
        {anchors.map((anchor) => (
          <span key={anchor} id={anchor} />
        ))}
        {ghost ?? <Skeleton lines={linesFor(height)} />}
      </div>
    </>
  );
  const style = { containIntrinsicSize: `auto ${height}px`, minHeight: `${shown ? 0 : height}px` };
  const Tag = listItem ? "li" : "div";
  return (
    <Tag id={id || undefined} style={style} className={`viewport-slot ${className}`.trim()}>
      {content}
    </Tag>
  );
}
