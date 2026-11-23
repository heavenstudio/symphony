// What every page that shows media draws of it: a board's picture in its box, or the mockup it
// names in its frame, which home's cards draw (home.tsx) as the blocks do (media.tsx). Each keeps
// its own box until it comes into view (`useVisibility`), then says `review:media`, which starts the
// page's media layer (media-layer.ts), which reads a viewer's views here (`decode`). Apart from
// media.tsx, the blocks' media, so a page that draws only pictures carries none of a file's viewer
// or a gallery.

import { useEffect } from "react";
import { fileAddress, href } from "./project";
import { useVisibility } from "./viewport";

/** Says a part of the page shows media once `shown`, so the page's media layer starts. */
export function useMediaShown(shown: boolean): void {
  useEffect(() => {
    if (shown) dispatchEvent(new Event("review:media"));
  }, [shown]);
}

/** Whether `src` is the project's own: a path of the site, the only kind a frame or viewer runs. */
export function localAsset(src: string): boolean {
  return (
    src.startsWith("/") &&
    !src.startsWith("//") &&
    ![...src].some((char) => {
      const code = char.codePointAt(0) ?? 0;
      return code < 0x20 || (code >= 0x7f && code <= 0x9f) || char === "\\";
    })
  );
}

/** `src`'s path without its query and fragment, its ASCII letters in lower case. */
export function pathOf(src: string): string {
  return (src.split(/[?#]/)[0] ?? "").replace(/[A-Z]/g, (letter) => letter.toLowerCase());
}

/** Whether `src` names an HTML mockup: its path ends in `.html` or `.htm`, in any case. */
export function isMockup(src: string): boolean {
  const path = pathOf(src);
  return path.endsWith(".html") || path.endsWith(".htm");
}

/** What a piece of media is called: its caption, else its file's name. */
export function label(src: string, caption: string): string {
  if (caption !== "") return caption;
  return (src.split(/[?#]/)[0] ?? src).split("/").pop() ?? src;
}

/** Whether `name` is an attribute's name as viewers write them: a-z, then a-z, 0-9 and hyphens. */
export function isName(name: string): boolean {
  return /^[a-z][a-z0-9-]*$/.test(name);
}

/**
 * The pairs an `encoded` value holds, in order; a line without `=` or naming no attribute is passed
 * over. Its lines are Rust's: a `\r` before a line's `\n` is no part of it, and a last `\n` ends a
 * line rather than starting one.
 */
export function decode(value: string): [string, string][] {
  const pieces = value.split("\n");
  const last = pieces.pop() ?? "";
  const lines = [...pieces.map((line) => line.replace(/\r$/, "")), ...(last === "" ? [] : [last])];
  return lines.flatMap((line): [string, string][] => {
    const at = line.indexOf("=");
    const name = line.slice(0, at);
    return at >= 0 && isName(name) ? [[name, line.slice(at + 1)]] : [];
  });
}

export interface MockupFrameProps {
  src: string;
  caption: string;
  thumb?: boolean;
}

/**
 * An HTML mockup in a frame that runs its scripts alone, at 1280 by 800 and scaled to its box by
 * the media layer, with a link to it full size. Only a project's own file is framed.
 */
export function MockupFrame({ src, caption, thumb = false }: MockupFrameProps) {
  const [shown, watch] = useVisibility();
  useMediaShown(shown);
  if (!localAsset(src) || !isMockup(src)) return null;
  const named = label(src, caption);
  // A sandboxed frame sends no Referer: its address names the project itself. The frame loads the
  // file's address, which an HTML document's route needs (its route is the app's page); the
  // links open the route.
  const route = href(src);
  return (
    <span className={thumb ? "mockup-shell thumb" : "mockup-shell"} ref={watch}>
      {shown && (
        <span className={thumb ? "mockup mockup-thumb" : "mockup"}>
          <span className="mockup-frame">
            <iframe
              src={fileAddress(route)}
              sandbox="allow-scripts"
              loading="lazy"
              width="1280"
              height="800"
              title={named}
              tabIndex={-1}
              referrerPolicy="no-referrer"
            />
            {!thumb && (
              // biome-ignore lint/a11y/useAnchorContent: its label names it, over the frame it opens
              <a
                className="mockup-open"
                href={route}
                target="_blank"
                rel="noopener noreferrer"
                aria-label={`Open ${named} full size`}
              />
            )}
          </span>
          {!thumb && (
            <span className="mockup-bar">
              <span>{named}</span>
              <a href={route} target="_blank" rel="noopener noreferrer">
                full size
              </a>
            </span>
          )}
        </span>
      )}
    </span>
  );
}

export interface PictureViewProps {
  src: string;
  caption: string;
  thumb?: boolean;
}

/** A board's picture in a square box, or its mockup when it names one. */
export function PictureView({ src, caption, thumb = false }: PictureViewProps) {
  const [shown, watch] = useVisibility();
  useMediaShown(shown);
  if (isMockup(src)) return <MockupFrame src={src} caption={caption} thumb={thumb} />;
  return (
    <span className={thumb ? "picture-shell thumb" : "picture-shell"} ref={watch}>
      {shown && (
        <img
          className="media-picture"
          src={href(src)}
          alt={caption}
          width="540"
          height="540"
          loading="lazy"
          decoding="async"
        />
      )}
    </span>
  );
}

/**
 * The src a document picture holds until it comes into view: an empty picture of no size, drawn
 * with no request. Its width and height of 0 give it no aspect ratio, so the img's own width and
 * height attributes size its box.
 */
export const WAITING =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='0' height='0'/%3E";
