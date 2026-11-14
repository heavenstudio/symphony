// What a page draws that markup cannot: a mockup in its frame, a file in the viewer its plugin's
// renderer names, a gallery of such files, a board's picture and a document's. Each keeps its own
// box until it comes into view (`useVisibility`), then says `review:media`, which starts the page's
// media layer: the frames scaled to their width, a viewer's script loaded and its buttons, the
// picture viewer. The Dioxus client's media.rs and viewer.rs, drawn alike.
//
// A viewer is data (symphony_api::Viewer): its element's markup is written here from it, every
// value escaped, as crates/symphony-blocks writes it (`elementHtml`, held to the Rust by the cases
// it writes), and the client holds no viewer's code.

import { useCallback, useEffect, useState } from "react";
import { Button } from "./button";
import { fileAddress, href } from "./project";
import { useVisibility } from "./viewport";
import type { GalleryItem, Image, Media, View, Viewer } from "./wire";

/** Says a part of the page shows media once `shown`, so the page's media layer starts. */
function useMediaShown(shown: boolean): void {
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
function isName(name: string): boolean {
  return /^[a-z][a-z0-9-]*$/.test(name);
}

/** Whether `name` is a custom element's: an attribute's name holding a hyphen. */
function isElement(name: string): boolean {
  return isName(name) && name.includes("-");
}

const ESCAPES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#x27;",
};

function escapeText(text: string): string {
  return text.replace(/[&<>"']/g, (char) => ESCAPES[char] ?? char);
}

/**
 * The viewer's element for the file at `src`, captioned `label`, with `poster` shown before it
 * loads: its markup, every value escaped, or null when the viewer names no custom element. An
 * attribute whose name is not an attribute's is left out.
 */
export function elementHtml(
  viewer: Viewer,
  src: string,
  label: string,
  poster: string | null,
): string | null {
  if (!isElement(viewer.element)) return null;
  const given: [string, string][] = [
    ["src", src],
    ["alt", label],
    ...viewer.attributes,
    ...(poster == null ? [] : [["poster", poster] as [string, string]]),
  ];
  const attributes = given
    .filter(([name]) => isName(name))
    .map(([name, value]) => ` ${name}="${escapeText(value)}"`)
    .join("");
  return `<${viewer.element}${attributes}></${viewer.element}>`;
}

/**
 * A view's attributes as one attribute's value: a line `name=value` for each, leaving out a name
 * that is not an attribute's and a value holding a line break.
 */
export function encoded(view: View): string {
  return view.set
    .filter(([name, value]) => isName(name) && !/[\n\r]/.test(value))
    .map(([name, value]) => `${name}=${value}`)
    .join("\n");
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

/** The viewer a gallery sent without one, as Rust's serde reads it: one that draws nothing. */
const NO_VIEWER: Viewer = {
  element: "",
  script: "",
  attributes: [],
  views: [],
  download: null,
  open_label: null,
  close_label: null,
};

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

/** A piece of media as the blocks hold it: a mockup, a file in its viewer, or a gallery. */
export function MediaView({ media }: { media: Media }) {
  switch (media.kind) {
    case "mockup":
      return <MockupFrame src={media.src} caption={media.label} />;
    case "viewer":
      return (
        <FileViewer
          src={media.src}
          caption={media.label}
          viewer={media.viewer}
          poster={media.poster ?? null}
        />
      );
    case "gallery":
      return <Gallery items={media.items} viewer={media.viewer ?? NO_VIEWER} />;
    default:
      return unknownMedia(media);
  }
}

/** No media kind is left undrawn: a kind the Rust adds fails the type check here. */
function unknownMedia(media: never): null {
  void media;
  return null;
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

/**
 * A document's picture with the server's sizes, original and presentation. Until it comes into
 * view its src is WAITING, so it is a sized picture from its first frame and only that node's src
 * changes, which keeps a keyboard's focus on it. Its address and its original's are written under
 * the project's base, as a link's is.
 */
export function DocumentPicture({ image }: { image: Image }) {
  const [shown, watch] = useVisibility();
  useMediaShown(shown);
  const given = image.presentation.attributes ?? {};
  const full = given["data-full"];
  const style = given.style;
  // The server's attributes, each by the name React gives it (`tabindex` as `tabIndex`); the style
  // is written as given, by the ref below.
  const attributes: object = {
    width: given.width,
    height: given.height,
    loading: given.loading ?? "lazy",
    decoding: given.decoding ?? "async",
    role: given.role,
    tabIndex: given.tabindex,
    "aria-label": given["aria-label"],
    "aria-hidden": given["aria-hidden"],
    "data-full": full === undefined ? undefined : href(full),
  };
  const ref = useCallback(
    (node: Element | null) => {
      const unwatch = watch(node);
      const unstyle = style === undefined ? undefined : writtenStyle(style)(node);
      return () => {
        unwatch();
        unstyle?.();
      };
    },
    [watch, style],
  );
  return (
    <img
      src={shown ? href(image.src) : WAITING}
      alt={image.alt}
      className={image.presentation.class ?? ""}
      id={image.presentation.id ?? ""}
      title={image.presentation.title ?? ""}
      {...attributes}
      ref={ref}
    />
  );
}

/** A ref that writes a style attribute as given: one element's, of any kind. */
export type StyleRef = (node: Element | null) => (() => void) | undefined;

/** Each style's ref, so an element drawn again with the same style keeps its ref and is let be. */
const STYLE_REFS = new Map<string, StyleRef>();

/**
 * A ref that writes `css` as the element's style attribute, as given, as the Dioxus client writes
 * it. React's `style` takes an object, which it sets property by property: a value with
 * `!important`, which no property takes, and a declaration after a comment would be lost. As the
 * Dioxus client's interpreter sets a `style` (dioxus-interpreter-js, `setAttributeInner`), each
 * property the element already had (a centred container's `text-align`, which React set first)
 * and the written style does not set is set again after it. The element drawn without it again,
 * or with another style, has its style as it was before this one was written. One ref per style,
 * the same each time, so an element drawn again with it is let be.
 */
export function writtenStyle(css: string): StyleRef {
  let ref = STYLE_REFS.get(css);
  if (ref === undefined) {
    ref = (node) => {
      if (node === null) return undefined;
      const before = node.getAttribute("style");
      const { style } = node as HTMLElement;
      const own: [string, string][] = [];
      for (let at = 0; at < style.length; at += 1) {
        const name = style.item(at);
        own.push([name, style.getPropertyValue(name)]);
      }
      node.setAttribute("style", css);
      for (const [name, value] of own) {
        if (style.getPropertyValue(name) === "") style.setProperty(name, value);
      }
      return () => {
        if (before === null) node.removeAttribute("style");
        else node.setAttribute("style", before);
      };
    };
    STYLE_REFS.set(css, ref);
  }
  return ref;
}

/** Files to pick from: a card each, one open at a time, in `viewer`. */
function Gallery({ items, viewer }: { items: GalleryItem[]; viewer: Viewer }) {
  const [selected, setSelected] = useState<string | null>(null);
  return (
    <div className="gallery-grid">
      {items.map((item) => (
        <GalleryRow
          key={item.name}
          item={item}
          viewer={viewer}
          open={selected === item.name}
          onSelect={(name) => setSelected((now) => (now === name ? null : name))}
        />
      ))}
    </div>
  );
}

interface GalleryRowProps {
  item: GalleryItem;
  viewer: Viewer;
  open: boolean;
  onSelect: (name: string) => void;
}

function GalleryRow({ item, viewer, open, onSelect }: GalleryRowProps) {
  const [shown, watch] = useVisibility();
  const says = open ? (viewer.close_label ?? "Close") : (viewer.open_label ?? "Open");
  return (
    <article
      className={open ? "gallery-card gallery-open" : "gallery-card"}
      data-busy={open}
      ref={watch}
    >
      {shown && (
        <>
          <Button
            className="gallery-pick"
            type="button"
            aria-expanded={open}
            aria-controls={`viewer-${item.name}`}
            onClick={() => onSelect(item.name)}
          >
            {item.poster != null && (
              <img
                src={item.poster}
                width="80"
                height="80"
                alt={item.name}
                loading="lazy"
                data-no-lightbox="true"
              />
            )}
            <span>
              <strong>{item.name}</strong>
              <small>{says}</small>
            </span>
          </Button>
          {open && (
            <div id={`viewer-${item.name}`}>
              <FileViewer src={item.src} caption={item.name} viewer={viewer} />
            </div>
          )}
        </>
      )}
    </article>
  );
}

export interface FileViewerProps {
  src: string;
  caption: string;
  viewer: Viewer;
  poster?: string | null;
}

/**
 * A file in the viewer its plugin's renderer names: the custom element a project's script
 * defines, drawn from the viewer's data alone, with its caption, a button per view and the
 * download link. The media layer loads the element's script once, when the first stage naming it
 * shows, and a view's button sets the attributes it lists on the element and calls its method.
 * Only a project's own file, script and poster are drawn.
 */
export function FileViewer({ src, caption, viewer, poster = null }: FileViewerProps) {
  const [shown, watch] = useVisibility();
  useMediaShown(shown);
  if (!localAsset(src) || !localAsset(viewer.script)) return null;
  const named = label(src, caption);
  const element = elementHtml(
    viewer,
    src,
    named,
    poster != null && localAsset(poster) ? poster : null,
  );
  if (element === null) return null;
  return (
    <span className="viewer-shell" ref={watch}>
      {shown && (
        <span className="media-viewer">
          <span
            className="viewer-stage"
            data-viewer-script={viewer.script}
            // biome-ignore lint/security/noDangerouslySetInnerHtml: the viewer's element, written from its data with every value escaped (elementHtml)
            dangerouslySetInnerHTML={{ __html: element }}
          />
          <span className="viewer-bar">
            <span>{named}</span>
            {(viewer.views ?? []).map((view, at) => (
              <Button
                // biome-ignore lint/suspicious/noArrayIndexKey: the views are the viewer's, in its order
                key={at}
                type="button"
                title={view.title ?? undefined}
                data-viewer-set={encoded(view)}
                data-viewer-call={view.call ?? undefined}
              >
                {view.label}
              </Button>
            ))}
            {viewer.download != null && (
              // As the Dioxus client draws it (`download: true`, written as the value "true"), so
              // the two clients save the same file; an empty value would keep the file's own name.
              <a href={href(src)} download="true">
                {viewer.download}
              </a>
            )}
          </span>
        </span>
      )}
    </span>
  );
}
