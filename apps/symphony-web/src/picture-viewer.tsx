// The picture viewer, as the overlay module draws it for the media layer of any page
// (media-layer.ts asks for it through the shell's `reviewOverlay`): a modal dialog that behaves as
// the pinned primitive's does (dioxus-primitives `dialog`, with its focus trap, `focus-trap.js`),
// holding the picture. Its buttons, and Escape, a press outside the picture's box or the focus
// leaving it, answer the page with a direction: -1 and 1 move through the pictures, 0 closes.
// Opened, it takes the focus to its first button and keeps Tab within itself; closed, it gives the
// focus back to what held it.

import { useEffect, useRef } from "react";
import { Button } from "./button";

/** A picture as the viewer shows it: its address and its caption. */
export interface Photo {
  src: string;
  caption: string;
}

/** Whether the focus trap stops at `element`: a control that takes the focus by Tab. */
export function focusable(element: {
  tabIndex: number;
  tagName: string;
  getAttribute(name: string): string | null;
  href?: string;
  rel?: string;
  type?: string;
}): boolean {
  if (element.tabIndex < 0 || element.getAttribute("disabled")) return false;
  switch (element.tagName) {
    case "A":
      return Boolean(element.href) && element.rel !== "ignore";
    case "INPUT":
      return element.type !== "hidden";
    case "BUTTON":
    case "SELECT":
    case "TEXTAREA":
      return true;
    default:
      return false;
  }
}

/**
 * The focus kept within `container`, as the primitives' focus trap keeps it: its first control
 * takes the focus now, and Tab and Shift+Tab move it to the next and the previous control, round
 * the container, never out. The cleanup returned stops it.
 */
export function trapFocus(container: HTMLElement): () => void {
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_ELEMENT, {
    acceptNode: (node) =>
      node instanceof HTMLElement && focusable(node)
        ? NodeFilter.FILTER_ACCEPT
        : NodeFilter.FILTER_SKIP,
  });
  const next = () => {
    let node = walker.nextNode();
    if (!node) {
      walker.currentNode = container;
      node = walker.nextNode();
    }
    (node as HTMLElement | null)?.focus();
  };
  const previous = () => {
    let node = walker.previousNode();
    if (!node) {
      walker.currentNode = container;
      node = walker.lastChild();
    }
    (node as HTMLElement | null)?.focus();
  };
  const keys = (event: KeyboardEvent) => {
    if (event.key !== "Tab") return;
    if (event.shiftKey) previous();
    else next();
    event.preventDefault();
  };
  next();
  container.addEventListener("keydown", keys);
  return () => container.removeEventListener("keydown", keys);
}

export interface PictureViewerProps {
  photos: Photo[];
  /** The place of the picture shown. */
  at: number;
  /** What a button, or a way out, says to the page: -1 or 1 to move, 0 to close. */
  command: (direction: number) => void;
}

export function PictureViewer({ photos, at, command }: PictureViewerProps) {
  const content = useRef<HTMLDivElement>(null);
  const said = useRef(command);
  said.current = command;
  const open = photos.length > 0;

  // Opened: the focus goes in and stays, Escape anywhere closes it, and so does a press or a focus
  // outside the picture's box. Closed or gone: the focus goes back to what held it.
  useEffect(() => {
    const box = content.current;
    if (!open || !box) return;
    const before = document.activeElement;
    const release = trapFocus(box);
    const leave = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      said.current(0);
    };
    const outside = (event: Event) => {
      if (event.target instanceof Node && !box.contains(event.target)) said.current(0);
    };
    document.addEventListener("keydown", leave);
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("focusin", outside, true);
    return () => {
      release();
      document.removeEventListener("keydown", leave);
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("focusin", outside, true);
      if (before instanceof HTMLElement) before.focus();
    };
  }, [open]);

  if (!open) return null;
  const photo = photos[at];
  const multiple = photos.length > 1;
  return (
    <div id="media-lightbox" data-state="open" className="dx-dialog-backdrop">
      <div
        ref={content}
        id="media-lightbox-content"
        role="dialog"
        aria-modal="true"
        aria-label="Picture viewer"
        className="dx-dialog"
      >
        {photo && (
          <>
            <div className="lightbox-controls">
              {multiple && (
                <Button type="button" aria-label="Previous picture" onClick={() => command(-1)}>
                  ←
                </Button>
              )}
              <Button type="button" onClick={() => command(0)}>
                Close
              </Button>
              {multiple && (
                <Button type="button" aria-label="Next picture" onClick={() => command(1)}>
                  →
                </Button>
              )}
            </div>
            <img src={photo.src} alt={photo.caption} />
            <div className="lightbox-caption">
              {photo.caption}
              {multiple && ` · ${at + 1} / ${photos.length}`}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
