// What the blocks draw that markup cannot: a mockup in its frame, a file in the viewer its plugin's
// renderer names, a gallery of such files, and a document's picture; a board's picture and a
// mockup's frame are media-picture.tsx's, which home draws too. Each keeps its own box until it
// comes into view (`useVisibility`), then says `review:media`, which starts the page's media layer:
// the frames scaled to their width, a viewer's script loaded and its buttons, the picture viewer.
// The Dioxus client's media.rs and viewer.rs, drawn alike.
//
// A viewer is data (symphony_api::Viewer): its element's markup is written here from it, every
// value escaped, as crates/symphony-blocks writes it (`elementHtml`, held to the Rust by the cases
// it writes), and the client holds no viewer's code.

import { useMemo, useState } from "react";
import { Button } from "./button";
import { isName, label, localAsset, MockupFrame, useMediaShown, WAITING } from "./media-picture";
import { href } from "./project";
import { useVisibility } from "./viewport";
import type { GalleryItem, Image, Media, View, Viewer } from "./wire";

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
      return (
        <span className="unknown-block" role="alert">
          Unsupported media: {unknownMedia(media)}
        </span>
      );
  }
}

/**
 * The `kind` of media of no kind the wire types know. No kind they know is left undrawn: a kind the
 * Rust adds fails the type check here. What does reach it is a kind a newer server sent; one whose
 * `kind` is no text, which no read lets through, is `unnamed`.
 */
function unknownMedia(media: never): string {
  const kind = (media as { kind?: unknown }).kind;
  return typeof kind === "string" ? kind : "unnamed";
}

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
  const ref = useMemo(() => pictureRef(watch, style), [watch, style]);
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

/**
 * A picture's one ref, which an element that spreads its presentation and needs a ref of its own
 * takes too: it watches the element until it comes into view (`watch`) and writes its `style` as
 * given, and what it returns undoes both.
 */
export function pictureRef(
  watch: (node: Element | null) => () => void,
  style: string | undefined,
): (node: Element | null) => () => void {
  return (node) => {
    const unwatch = watch(node);
    const unstyle = style === undefined ? undefined : writtenStyle(style)(node);
    return () => {
      unwatch();
      unstyle?.();
    };
  };
}

/** A ref that writes a style attribute as given: one element's, of any kind. */
export type StyleRef = (node: Element | null) => (() => void) | undefined;

/**
 * Each style's ref, so an element drawn again with the same style keeps its ref and is let be. A
 * ref no element holds any more is let go: the map holds the styles the page draws, not every
 * style it ever drew.
 */
const STYLE_REFS = new Map<string, StyleRef>();

/** How many styles have a ref now (`writtenStyle`): the tests read it. */
export function styleRefs(): number {
  return STYLE_REFS.size;
}

/**
 * A ref that writes `css` as the element's style attribute, as given, as the Dioxus client writes
 * it. React's `style` takes an object, which it sets property by property: a value with
 * `!important`, which no property takes, and a declaration after a comment would be lost. As the
 * Dioxus client's interpreter sets a `style` (dioxus-interpreter-js, `setAttributeInner`), each
 * property the element already had (a centred container's `text-align`, which React set first)
 * and the written style does not set is set again after it. The element drawn without it again,
 * or with another style, has its style as it was before this one was written. One ref per style,
 * the same each time while an element holds it, so an element drawn again with it is let be.
 */
export function writtenStyle(css: string): StyleRef {
  let ref = STYLE_REFS.get(css);
  if (ref === undefined) {
    let held = 0;
    const written: StyleRef = (node) => {
      if (node === null) return undefined;
      // Held again after it was let go (another element took it as one let it go): kept again.
      held += 1;
      STYLE_REFS.set(css, written);
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
        held -= 1;
        if (held === 0 && STYLE_REFS.get(css) === written) STYLE_REFS.delete(css);
      };
    };
    ref = written;
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
              <a href={href(src)} download={label(src, "")}>
                {viewer.download}
              </a>
            )}
          </span>
        </span>
      )}
    </span>
  );
}
