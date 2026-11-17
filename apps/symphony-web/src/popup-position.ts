// Where a select's popup is drawn: beside its trigger where it can be and inside the visible
// viewport always, as tall as its options: below its trigger, else above it, else over it, and it
// scrolls inside itself only when the viewport cannot hold it, then filling the roomier side of its
// trigger. It is as wide as its longest option, its scrollbar beside it, up to the viewport's width.
// Rectangles are [left, top, width, height], in layout-viewport CSS pixels.

export type Rect = [number, number, number, number];

/** Where the popup goes: `trigger` and `viewport` as rectangles, `content` the list's width and
 * height with nothing clipped. */
export function popupRect(trigger: Rect, viewport: Rect, content: [number, number]): Rect {
  const edge = Math.min(8, viewport[2] / 2, viewport[3] / 2);
  const gap = 4;
  const availableWidth = Math.max(viewport[2] - 2 * edge, 0);
  const availableHeight = Math.max(viewport[3] - 2 * edge, 0);
  const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
  const below = clamp(
    viewport[1] + viewport[3] - edge - trigger[1] - trigger[3] - gap,
    0,
    availableHeight,
  );
  const above = clamp(trigger[1] - gap - viewport[1] - edge, 0, availableHeight);
  const wanted = content[1];
  const flipped = wanted > below && above > below;
  const side = flipped ? above : below;
  // A list neither side holds but the viewport does is drawn whole over its trigger (the clamp
  // below moves it there); one taller than the viewport fills its side and scrolls, attached to
  // its trigger as the page scrolls.
  const height = wanted > side && wanted <= availableHeight ? wanted : Math.min(wanted, side);
  const width = Math.min(Math.max(content[0], trigger[2]), availableWidth);
  const left = clamp(trigger[0], viewport[0] + edge, viewport[0] + edge + availableWidth - width);
  const top = clamp(
    flipped ? trigger[1] - gap - height : trigger[1] + trigger[3] + gap,
    viewport[1] + edge,
    viewport[1] + edge + availableHeight - height,
  );
  return [left, top, width, height];
}

/** The visible viewport (a phone's keyboard shrinks it), else the window. */
function bounds(): Rect {
  const visual = window.visualViewport;
  return [
    visual?.offsetLeft ?? 0,
    visual?.offsetTop ?? 0,
    visual?.width ?? innerWidth,
    visual?.height ?? innerHeight,
  ];
}

/** The scroll that brings the focused option within 4 pixels of the list's edges: 0 for none. */
export function revealBy(item: DOMRect, list: DOMRect): number {
  if (item.top < list.top + 4) return item.top - list.top - 4;
  if (item.bottom > list.bottom - 4) return item.bottom - list.bottom + 4;
  return 0;
}

/** The option focused in `popup`, scrolled into its view. */
export function reveal(popup: HTMLElement): void {
  const active = document.activeElement;
  if (active?.getAttribute("role") !== "option" || !popup.contains(active)) return;
  const by = revealBy(active.getBoundingClientRect(), popup.getBoundingClientRect());
  if (by !== 0) popup.scrollTop += Math.round(by);
}

/** The popup `popupId` placed beside the trigger `triggerId`, marked with the side it opened on,
 * then marked positioned. */
export function position(triggerId: string, popupId: string): void {
  const trigger = document.getElementById(triggerId);
  const popup = document.getElementById(popupId);
  if (!trigger || !popup) return;
  const viewport = bounds();
  const box = trigger.getBoundingClientRect();
  const anchor: Rect = [box.left, box.top, box.width, box.height];
  // Measure the list with nothing clipped before choosing the side: its longest option on one line
  // (no wider than the viewport) and every option's height. Its stylesheet's `min-width:100%` would
  // be the whole window's width once fixed, so it is lifted here. Hidden measurement avoids an
  // unpositioned first frame; no style or focus work is deferred.
  popup.setAttribute(
    "style",
    `position:fixed;visibility:hidden;left:0;top:0;width:max-content;min-width:0;max-width:${Math.max(viewport[2] - 16, 0)}px;overflow:hidden;margin:0;z-index:1100`,
  );
  // offsetWidth rounds fractional text widths down too: fixing that rounded width can wrap the
  // selected label beside its check. The resolved border-box width keeps that fraction without
  // the opening animation's transform, which getBoundingClientRect would include.
  const content: [number, number] = [
    Math.ceil(Number.parseFloat(getComputedStyle(popup).width)),
    popup.scrollHeight,
  ];
  const place = (measured: [number, number]) => {
    const [left, top, width, height] = popupRect(anchor, viewport, measured);
    popup.setAttribute(
      "style",
      `position:fixed;left:${left}px;top:${top}px;width:${width}px;min-width:0;max-height:${height}px;overflow-y:auto;overflow-x:hidden;margin:0;z-index:1100`,
    );
    // The side it opened on, which its sheet grows it from (overlay.css): above its trigger whole,
    // else below or over it.
    popup.setAttribute("data-side", top + height <= anchor[1] ? "top" : "bottom");
  };
  place(content);
  // A list that scrolls draws its scrollbar inside its width where the platform's scrollbars take
  // room: it is wider by that much, so no option wraps beside the bar.
  const bar = popup.offsetWidth - popup.clientWidth;
  if (bar > 0) place([content[0] + bar, content[1]]);
  popup.setAttribute("data-positioned", "true");
  reveal(popup);
}

/**
 * The popup `popupId` kept beside its trigger: placed now, and again on each resize and scroll of
 * the window or the visible viewport (a phone's keyboard), a scroll of the list itself aside; a
 * focus moved within it brings the focused option into its view. The cleanup returned removes the
 * listeners.
 */
export function anchor(triggerId: string, popupId: string): () => void {
  const listener = (event: Event) => {
    const list = document.getElementById(popupId);
    if (list) {
      if (event.type === "focusin") {
        reveal(list);
        return;
      }
      // Scrolling the list must not reset its own scroll position.
      if (event.type === "scroll" && event.target instanceof Node && list.contains(event.target)) {
        return;
      }
    }
    position(triggerId, popupId);
  };
  const visual = window.visualViewport;
  addEventListener("resize", listener);
  document.addEventListener("scroll", listener, true);
  document.addEventListener("focusin", listener);
  visual?.addEventListener("resize", listener);
  visual?.addEventListener("scroll", listener);
  position(triggerId, popupId);
  return () => {
    removeEventListener("resize", listener);
    document.removeEventListener("scroll", listener, true);
    document.removeEventListener("focusin", listener);
    visual?.removeEventListener("resize", listener);
    visual?.removeEventListener("scroll", listener);
  };
}
