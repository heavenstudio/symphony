// Document content as typed data (`symphony_api::Block` and `Span`), drawn element for element as
// the Dioxus client draws it: `block_view.rs` (BlocksView, SpansView), `doc_components.rs` (the
// headings, code, callouts, decision cards, lists and the unsupported-content notice) and
// `media.rs` / `viewer.rs` (document pictures, mockups, file viewers and galleries).
//
// Exports `Blocks({ blocks })` and `Spans({ spans })`, `HistoricalBlocks` (blocks shown away from
// their place, owning no card anchor and no excerpts), and the two contexts a document page sets
// for a card's section: `ExcerptContext` (how a link's excerpt is drawn) and `CardAnchorContext`
// (the card whose anchor its folded summary owns).
//
// Simplified or left out (named in the comparison):
// - the picture viewer dialog (`media_live::MediaRuntime`: a picture opening at its `data-full`,
//   the zoom and table-of-contents markers it sets): a picture is its sized img with `data-full`,
//   its src set once it comes near the screen, and opens nothing;
// - a file viewer's custom element is drawn (its markup from the viewer's data, escaped, as
//   `Viewer::element_html` writes it) with its caption, view buttons and download link, but the
//   module script that defines the element is never loaded and the view buttons do nothing;
// - a gallery's grid, its cards and the one open viewer are drawn; a mockup's sandboxed frame is
//   drawn; nothing sizes a mounted frame (the media runtime's adapter);
// - Dioxus's `SuspenseBoundary` around each block list ("Loading content…") has no counterpart:
//   the blocks render at once.
import { createContext, memo, useContext, useState } from "react";
import { Near, useVisible } from "./near.jsx";
import { href } from "./project.js";

/** `Excerpts`: a card's sections draw a link's excerpt with it; elsewhere it is null. */
export const ExcerptContext = createContext(null);
/** `CardAnchor`: a settled card's outer details owns its anchor while its body is fetched. */
export const CardAnchorContext = createContext(null);
const HistoricalContext = createContext(false);

/** Blocks shown away from their place (a link's excerpt, main's version of a section). */
export function HistoricalBlocks({ blocks }) {
  return (
    <HistoricalContext.Provider value={true}>
      <ExcerptContext.Provider value={null}>
        <Blocks blocks={blocks} />
      </ExcerptContext.Provider>
    </HistoricalContext.Provider>
  );
}

/** Detached history comes as one Thread per entry: adjacent entries join into one timeline. */
function joined(blocks) {
  const out = [];
  for (const block of blocks ?? []) {
    const last = out[out.length - 1];
    if (block.type === "thread" && last?.type === "thread") {
      out[out.length - 1] = { type: "thread", entries: [...last.entries, ...block.entries] };
    } else out.push(block);
  }
  return out;
}

export function Blocks({ blocks }) {
  return joined(blocks).map((block, index) => <Block key={index} block={block} />);
}

export function Spans({ spans }) {
  return (spans ?? []).map((span, index) => <Span key={index} span={span} />);
}

/** `Deferred`: children mounted once near the screen, a placeholder of `height` px until then. */
function Deferred({ height = 160, ghost, children }) {
  return (
    <Near height={height} ghost={ghost}>
      {children}
    </Near>
  );
}

const Block = memo(function Block({ block }) {
  switch (block.type) {
    case "heading":
      return (
        <Heading level={block.level} anchor={block.anchor}>
          <Spans spans={block.spans} />
        </Heading>
      );
    case "paragraph":
      return (
        <p>
          <Spans spans={block.spans} />
        </p>
      );
    case "list": {
      const items = block.items.map((blocks, index) => (
        <li key={index}>
          <Deferred height={32}>
            <Blocks blocks={blocks} />
          </Deferred>
        </li>
      ));
      return block.ordered ? <ol start={block.start}>{items}</ol> : <ul>{items}</ul>;
    }
    case "table":
      return <Table head={block.head} rows={block.rows} presentation={block.presentation} />;
    case "code":
      return <Code language={block.language} text={block.text} block />;
    case "quote":
      return (
        <blockquote>
          <Blocks blocks={block.blocks} />
        </blockquote>
      );
    case "rule":
      return <hr />;
    case "callout": {
      const title = block.title.filter(
        (span) => !(span.type === "styled" && (span.presentation?.class ?? "").split(/\s+/).includes("callout-kind")),
      );
      return (
        <Callout tone={block.kind} done={block.resolved} title={<Spans spans={title} />}>
          <Blocks blocks={block.blocks} />
        </Callout>
      );
    }
    case "card":
      return <Card block={block} />;
    case "component":
      return <Component kind={block.kind} blocks={block.blocks} />;
    case "media":
      return <MediaView media={block.media} />;
    case "image":
      return <DocumentPicture image={block.image} />;
    case "figure":
      return <Figure {...block} />;
    case "caption":
      return (
        <figcaption>
          <Blocks blocks={block.blocks} />
        </figcaption>
      );
    case "comparison":
      return (
        <div className="compare">
          <Blocks blocks={block.figures} />
        </div>
      );
    case "group":
      return <Blocks blocks={block.blocks} />;
    case "container":
      return <Container tag={block.tag} presentation={block.presentation} blocks={block.blocks} />;
    case "details":
    case "settled-card":
      return (
        <details open={block.open}>
          <summary>
            <Spans spans={block.summary} />
          </summary>
          <Deferred>
            <Blocks blocks={block.blocks} />
          </Deferred>
        </details>
      );
    case "thread":
      return <Thread entries={block.entries} />;
    // Comments have no visual content, as in Markdown.
    case "comment":
      return null;
    case "unknown":
      return (
        <Unknown description={block.tag}>
          <p>{block.text}</p>
        </Unknown>
      );
    default:
      return (
        <Unknown description={String(block.type)}>
          <p />
        </Unknown>
      );
  }
});

const Span = memo(function Span({ span }) {
  switch (span.type) {
    case "text":
      return span.text;
    case "emphasis":
      return (
        <em>
          <Spans spans={span.spans} />
        </em>
      );
    case "strong":
      return (
        <strong>
          <Spans spans={span.spans} />
        </strong>
      );
    case "code":
      return <Code text={span.text} language="" />;
    case "link":
      return <Link href={span.href} target={span.target} title={span.title} spans={span.spans} presentation={EMPTY} />;
    case "wiki-link":
      return <Link href={span.href} target={span.target} title="" spans={span.spans} presentation={EMPTY} />;
    case "line-break":
      return <br />;
    case "image":
      return <DocumentPicture image={span.image} />;
    case "media":
      return <MediaView media={span.media} />;
    case "component":
      return <Component kind={span.kind} blocks={span.blocks} inline />;
    case "icon":
      return <Icon shape={span.shape} label={span.label} named={span.named} tip={span.tip ?? ""} />;
    case "styled":
      return <Inline tag={span.tag} presentation={span.presentation} spans={span.spans} />;
    case "graphic":
      return <Graphic viewBox={span.view_box} presentation={span.presentation} shapes={span.shapes} />;
    case "blocks":
      return <Blocks blocks={span.blocks} />;
    default:
      return (
        <span className="unknown-block" role="alert">
          Unsupported {span.tag ?? span.type}: {span.text ?? ""}
        </span>
      );
  }
});

const EMPTY = {};

// ---- doc_components.rs ----

function Heading({ level, anchor, children }) {
  const Tag = `h${Math.min(Math.max(level, 1), 6)}`;
  return <Tag id={anchor}>{children}</Tag>;
}

function Code({ text, language, block = false }) {
  return block ? (
    <pre>
      <code className={`language-${language}`}>{text}</code>
    </pre>
  ) : (
    <code>{text}</code>
  );
}

function Callout({ tone, title, done, children }) {
  const heading = (
    <>
      <span className="callout-kind">{tone}</span> {title}
    </>
  );
  return done ? (
    <details className={`callout callout-${tone} callout-done`}>
      <summary className="callout-title">{heading}</summary>
      {children}
    </details>
  ) : (
    <div className={`callout callout-${tone}`}>
      <p className="callout-title">{heading}</p>
      {children}
    </div>
  );
}

/** `Block::Card` as `doc_components::Decision` draws it, its thread under its text. */
function Card({ block }) {
  const historical = useContext(HistoricalContext);
  const suppressed = useContext(CardAnchorContext);
  const anchor = !historical && suppressed !== block.slug;
  const { verdict } = block;
  const posted = verdict.posted ?? "";
  const decided = verdict.decided ?? "";
  return (
    <div
      className="decision"
      id={anchor ? block.slug : undefined}
      data-slug={block.slug}
      data-state={block.mark}
      data-group={verdict.group}
      data-open={String(verdict.open)}
      data-decided={decided}
    >
      <div className="decision-text">
        <code>{block.slug}</code>{" "}
        {posted ? (
          <>
            <time className="posted">{posted}</time>{" "}
          </>
        ) : null}
        {block.body.length === 0 ? block.title : <Blocks blocks={block.body} />}
        <span className="state">
          {block.mark}
          {decided ? ` · ${decided}` : null}
        </span>
      </div>
      <Blocks blocks={block.comparison} />
      {block.thread.length ? <Thread entries={block.thread} /> : null}
    </div>
  );
}

function Unknown({ description, attributes = [], children }) {
  return (
    <aside className="unknown-block" role="alert">
      <strong>Unsupported document content: {description}</strong>
      {attributes.length ? (
        <p>
          Attributes: <code>{attributes.join(", ")}</code>
        </p>
      ) : null}
      {children}
    </aside>
  );
}

// ---- block_view.rs ----

/** A link; in a card, one naming a section opens that section's excerpt beside it. */
function Link({ href: to, target, title, spans, presentation }) {
  const excerpt = useContext(ExcerptContext);
  const [shown, setShown] = useState(false);
  const opens = excerpt && target?.anchor ? target : null;
  return (
    <>
      <a href={href(to)} title={title} {...attributes(presentation)}>
        <Spans spans={spans} />
      </a>
      {opens ? (
        <>
          <button
            type="button"
            className="dx-button show-link"
            data-style="outline"
            data-size="default"
            aria-expanded={shown}
            onClick={(event) => {
              event.stopPropagation();
              setShown((open) => !open);
            }}
          >
            {shown ? "▾ hide" : "▸ show"}
          </button>
          {shown ? excerpt(opens) : null}
        </>
      ) : null}
      <Warnings values={presentation.attributes} />
    </>
  );
}

/** A plugin's component: its blocks in a wrapper naming its kind; in a line, one paragraph's spans. */
function Component({ kind, blocks, inline = false }) {
  if (inline) {
    if (blocks.length === 1 && blocks[0].type === "paragraph") {
      return (
        <span data-component={kind}>
          <Spans spans={blocks[0].spans} />
        </span>
      );
    }
    return (
      <span data-component={kind}>
        <Blocks blocks={blocks} />
      </span>
    );
  }
  return (
    <div data-component={kind}>
      <Blocks blocks={blocks} />
    </div>
  );
}

const ITEM_SIZES = ["small", "row", "large", "tile"];

/** A picture with its caption: an item's sizes draw a link card, the others a figure. */
function Figure({ picture, caption, href: to, badge, size }) {
  const className = `figure figure-${size}`;
  if (ITEM_SIZES.includes(size)) {
    const content = (
      <>
        {picture ? <DocumentPicture image={picture} /> : badge != null ? <span className="figure-badge">{badge}</span> : null}
        <span className="figure-caption">
          <Spans spans={caption} />
        </span>
      </>
    );
    return to != null ? (
      <a className={className} href={href(to)}>
        {content}
      </a>
    ) : (
      <span className={className}>{content}</span>
    );
  }
  return (
    <figure className={picture ? className : `${className} missing`}>
      {picture ? (
        // A picture still to come is its box, filled.
        <Deferred ghost={<span className="bone ghost-picture" />}>
          {to != null ? (
            <a href={href(to)}>
              <DocumentPicture image={picture} />
            </a>
          ) : (
            <DocumentPicture image={picture} />
          )}
        </Deferred>
      ) : badge != null ? (
        <span className="figure-badge">{badge}</span>
      ) : null}
      {caption.length ? (
        <figcaption>
          <Spans spans={caption} />
        </figcaption>
      ) : null}
    </figure>
  );
}

/** An icon on the 24-unit stroke grid: its label its accessible name, its tip on hover. */
function Icon({ shape, label, named, tip }) {
  return (
    <>
      <span className="icon" data-tip={tip || undefined} title={tip || undefined} tabIndex={0} role="img" aria-label={label}>
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          {shape.map((one, index) => (
            <Shape key={index} shape={one} />
          ))}
        </svg>
        {named ? <span className="icon-name">{label}</span> : null}
      </span>
      {shape.map((one, index) => (
        <Warnings key={index} values={one.attributes} />
      ))}
    </>
  );
}

const CONTAINERS = { div: "div", section: "section", figure: "figure", figcaption: "figcaption", aside: "aside", header: "header", footer: "footer", address: "address" };

function Container({ tag, presentation, blocks }) {
  const attrs = attributes(presentation);
  const content = <Blocks blocks={blocks} />;
  // `center` is a div whose own style a presentation's style replaces, as the later attribute does.
  const shown =
    tag === "center" ? (
      <div {...attrs} style={attrs.style ?? CENTER}>
        {content}
      </div>
    ) : (
      (() => {
        const Tag = CONTAINERS[tag] ?? "div";
        return <Tag {...attrs}>{content}</Tag>;
      })()
    );
  return (
    <>
      {shown}
      <Warnings values={presentation.attributes} />
    </>
  );
}

const CENTER = { textAlign: "center" };

const INLINES = { span: "span", small: "small", sub: "sub", sup: "sup", time: "time", mark: "mark", kbd: "kbd", samp: "samp", delete: "del", insert: "ins", underline: "u", abbreviation: "abbr" };

function Inline({ tag, presentation, spans }) {
  // The parser wraps a link's own attributes in a presentation span: they go on the link itself.
  const linkAttributes =
    Object.keys(presentation.attributes ?? {}).some((key) => ["target", "rel", "download", "data-task"].includes(key)) ||
    (presentation.class ?? "").split(/\s+/).includes("tasklink");
  if (tag === "span" && linkAttributes && spans.length === 1) {
    const [only] = spans;
    if (only.type === "link") return <Link href={only.href} target={only.target} title={only.title} spans={only.spans} presentation={presentation} />;
    if (only.type === "wiki-link") return <Link href={only.href} target={only.target} title="" spans={only.spans} presentation={presentation} />;
  }
  const Tag = INLINES[tag] ?? "span";
  return (
    <>
      <Tag {...attributes(presentation)}>
        <Spans spans={spans} />
      </Tag>
      <Warnings values={presentation.attributes} />
    </>
  );
}

function Table({ head, rows, presentation = EMPTY }) {
  return (
    <>
      <table {...attributes(presentation)}>
        {head.length ? (
          <thead>
            <tr>
              {head.map((cell, index) => (
                <Cell key={index} cell={cell} />
              ))}
            </tr>
          </thead>
        ) : null}
        <tbody>
          {rows.map((cells, index) => (
            <TableRow key={index} cells={cells} />
          ))}
        </tbody>
      </table>
      <Warnings values={presentation.attributes} />
    </>
  );
}

/** A row drawn once it comes on the screen, a cell of 48 px across it until then. */
const TableRow = memo(function TableRow({ cells }) {
  const [shown, ref] = useVisible();
  return (
    <tr ref={ref}>
      {shown ? (
        cells.map((cell, index) => <Cell key={index} cell={cell} />)
      ) : (
        <td colSpan={cells.length} style={ROW_RESERVE} aria-hidden="true" />
      )}
    </tr>
  );
});

const ROW_RESERVE = { height: "48px" };

function Cell({ cell }) {
  const Tag = cell.header ? "th" : "td";
  return (
    <Tag {...attributes(cell.presentation)}>
      <Spans spans={cell.spans} />
      <Warnings values={cell.presentation?.attributes} />
    </Tag>
  );
}

/** A card's or a detached history's thread: one entry each, an owner's last line awaiting. */
function Thread({ entries }) {
  const last = entries[entries.length - 1];
  const awaiting = last?.who === "owner" && last.verdict == null;
  return (
    <>
      <ul className="history">
        {entries.map((entry, index) => (
          <li key={index} className={`entry ${entry.who}`}>
            <Deferred height={60}>
              {entry.timestamp != null ? (
                <>
                  <time>{entry.timestamp}</time>{" "}
                </>
              ) : null}
              {entry.verdict != null ? (
                <>
                  <b className="entry-verdict">{entry.verdict}</b>{" "}
                </>
              ) : null}
              <span className="entry-text">
                <Blocks blocks={entry.blocks} />
              </span>
              {entry.who === "unknown" ? <span role="alert">Unknown thread author</span> : null}
            </Deferred>
          </li>
        ))}
      </ul>
      {awaiting ? <p className="awaiting">Waiting for an answer…</p> : null}
    </>
  );
}

function Graphic({ viewBox, presentation, shapes }) {
  return (
    <>
      <svg viewBox={viewBox} {...attributes(presentation)}>
        {shapes.map((shape, index) => (
          <Shape key={index} shape={shape} />
        ))}
      </svg>
      <Warnings values={presentation.attributes} />
      {shapes.map((shape, index) => (
        <Warnings key={index} values={shape.attributes} />
      ))}
    </>
  );
}

const SHAPES = { path: "path", circle: "circle", ellipse: "ellipse", rect: "rect", line: "line", polyline: "polyline", polygon: "polygon" };

function Shape({ shape }) {
  const Tag = SHAPES[shape.kind] ?? "path";
  return <Tag {...mapAttributes(shape.attributes)} />;
}

/** The attributes the server recognises, by their DOM name, and the prop each is set as. */
const ALLOWED = {
  style: "style",
  role: "role",
  tabindex: "tabIndex",
  "aria-label": "aria-label",
  "aria-hidden": "aria-hidden",
  target: "target",
  rel: "rel",
  download: "download",
  "data-task": "data-task",
  "data-wiki": "data-wiki",
  "data-full": "data-full",
  "data-tip": "data-tip",
  "data-slug": "data-slug",
  "data-state": "data-state",
  "data-open": "data-open",
  "data-group": "data-group",
  "data-decided": "data-decided",
  "data-line": "data-line",
  "data-line-offset": "data-line-offset",
  "data-screen": "data-screen",
  "data-kind": "data-kind",
  "data-media": "data-media",
  "data-arg": "data-arg",
  "data-option": "data-option",
  width: "width",
  height: "height",
  loading: "loading",
  decoding: "decoding",
  scope: "scope",
  align: "align",
  colspan: "colSpan",
  rowspan: "rowSpan",
  fill: "fill",
  stroke: "stroke",
  "stroke-width": "strokeWidth",
  "stroke-dasharray": "strokeDasharray",
  "stroke-linecap": "strokeLinecap",
  "stroke-linejoin": "strokeLinejoin",
  d: "d",
  cx: "cx",
  cy: "cy",
  r: "r",
  rx: "rx",
  ry: "ry",
  x: "x",
  y: "y",
  x1: "x1",
  x2: "x2",
  y1: "y1",
  y2: "y2",
  points: "points",
};

/** A declaration list (`a: b; --c: d`) as a style object, custom properties kept by name. */
function styleObject(text) {
  const style = {};
  for (const declaration of text.split(";")) {
    const colon = declaration.indexOf(":");
    if (colon < 0) continue;
    const name = declaration.slice(0, colon).trim();
    const value = declaration.slice(colon + 1).trim();
    if (!name) continue;
    style[name.startsWith("--") ? name : name.toLowerCase().replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = value;
  }
  return style;
}

function mapAttributes(values = {}) {
  const props = {};
  for (const [name, value] of Object.entries(values)) {
    const prop = ALLOWED[name];
    if (!prop) continue;
    props[prop] = prop === "style" ? styleObject(value) : value;
  }
  return props;
}

/** A presentation's recognised attributes, its class, id and title (a tip stands for a title). */
function attributes(presentation = EMPTY) {
  const props = mapAttributes(presentation.attributes);
  if (!presentation.title && presentation.attributes?.["data-tip"]) props.title = presentation.attributes["data-tip"];
  if (presentation.class) props.className = presentation.class;
  if (presentation.id) props.id = presentation.id;
  if (presentation.title) props.title = presentation.title;
  return props;
}

/** The attributes the server sent and the client does not set, said where they would be. */
function Warnings({ values }) {
  const unsupported = Object.entries(values ?? {})
    .filter(([name]) => !ALLOWED[name])
    .map(([name, value]) => `${name}=${value}`);
  return unsupported.length ? <Unknown description="attributes" attributes={unsupported} /> : null;
}

// ---- media.rs and viewer.rs ----

/** Only project assets may run in an embedded frame or a file viewer. */
function localAsset(src) {
  return typeof src === "string" && src.startsWith("/") && !src.startsWith("//") && !/[\u0000-\u001f\u007f-\u009f\\]/.test(src);
}

function isMockup(src) {
  const path = src.split(/[?#]/)[0].toLowerCase();
  return path.endsWith(".html") || path.endsWith(".htm");
}

function label(src, caption) {
  return caption || src.split(/[?#]/)[0].split("/").pop();
}

/**
 * The src a document picture holds until it comes near the screen: an empty picture of no size,
 * drawn with no request, so the img is a sized picture from its first frame.
 */
export const WAITING = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='0' height='0'/%3E";

/** A document image: the server's sizing, presentation and `data-full`, its src once near. */
export function DocumentPicture({ image }) {
  const [shown, ref] = useVisible();
  const attrs = image.presentation?.attributes ?? EMPTY;
  return (
    <img
      ref={ref}
      src={shown ? href(image.src) : WAITING}
      alt={image.alt}
      className={image.presentation?.class ?? ""}
      id={image.presentation?.id ?? ""}
      title={image.presentation?.title ?? ""}
      width={attrs.width}
      height={attrs.height}
      loading={attrs.loading ?? "lazy"}
      decoding={attrs.decoding ?? "async"}
      style={attrs.style != null ? styleObject(attrs.style) : undefined}
      role={attrs.role}
      tabIndex={attrs.tabindex}
      aria-label={attrs["aria-label"]}
      aria-hidden={attrs["aria-hidden"]}
      data-full={attrs["data-full"] != null ? href(attrs["data-full"]) : undefined}
    />
  );
}

export function MediaView({ media }) {
  switch (media.kind) {
    case "mockup":
      return <Mockup src={media.src} caption={media.label} />;
    case "viewer":
      return <FileViewer src={media.src} caption={media.label} viewer={media.viewer} poster={media.poster} />;
    case "gallery":
      return <Gallery items={media.items} viewer={media.viewer ?? {}} />;
    default:
      return null;
  }
}

/** An `.html` mockup in a scripts-only sandbox, at 1280 x 800, once near the screen. */
export function Mockup({ src, caption, thumb = false }) {
  const [shown, ref] = useVisible();
  if (!localAsset(src) || !isMockup(src)) return null;
  const name = label(src, caption);
  // A sandboxed frame sends no Referer: its address names the project itself.
  const at = href(src);
  return (
    <span className={thumb ? "mockup-shell thumb" : "mockup-shell"} ref={ref}>
      {shown ? (
        <span className={thumb ? "mockup mockup-thumb" : "mockup"}>
          <span className="mockup-frame">
            <iframe src={at} sandbox="allow-scripts" loading="lazy" width="1280" height="800" title={name} tabIndex={-1} referrerPolicy="no-referrer" />
            {!thumb ? <a className="mockup-open" href={at} target="_blank" rel="noopener noreferrer" aria-label={`Open ${name} full size`} /> : null}
          </span>
          {!thumb ? (
            <span className="mockup-bar">
              <span>{name}</span>
              <a href={at} target="_blank" rel="noopener noreferrer">
                full size
              </a>
            </span>
          ) : null}
        </span>
      ) : null}
    </span>
  );
}

const isName = (name) => /^[a-z][a-z0-9-]*$/.test(name);
const escapeHtml = (text) =>
  String(text).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#x27;" })[c]);

/** `Viewer::element_html`: the custom element for the file at `src`, every value escaped. */
function elementHtml(viewer, src, caption, poster) {
  if (!isName(viewer.element ?? "") || !viewer.element.includes("-")) return null;
  const pairs = [["src", src], ["alt", caption], ...(viewer.attributes ?? []), ...(poster ? [["poster", poster]] : [])];
  const attrs = pairs
    .filter(([name]) => isName(name))
    .map(([name, value]) => ` ${name}="${escapeHtml(value)}"`)
    .join("");
  return `<${viewer.element}${attrs}></${viewer.element}>`;
}

/** `View::encoded`: a view's attributes as one value, a line `name=value` each. */
function encoded(view) {
  return (view.set ?? [])
    .filter(([name, value]) => isName(name) && !/[\n\r]/.test(value))
    .map(([name, value]) => `${name}=${value}`)
    .join("\n");
}

/** A file in its plugin's viewer: the element, its caption, a button per view, the download. */
export function FileViewer({ src, caption, viewer, poster }) {
  const [shown, ref] = useVisible();
  if (!localAsset(src) || !localAsset(viewer.script)) return null;
  const name = label(src, caption);
  const element = elementHtml(viewer, src, name, poster && localAsset(poster) ? poster : null);
  if (!element) return null;
  return (
    <span className="viewer-shell" ref={ref}>
      {shown ? (
        <span className="media-viewer">
          <span className="viewer-stage" data-viewer-script={viewer.script} dangerouslySetInnerHTML={{ __html: element }} />
          <span className="viewer-bar">
            <span>{name}</span>
            {(viewer.views ?? []).map((view, index) => (
              <button key={index} type="button" className="dx-button" data-style="outline" data-size="default" title={view.title ?? undefined} data-viewer-set={encoded(view)} data-viewer-call={view.call ?? undefined}>
                {view.label}
              </button>
            ))}
            {viewer.download != null ? (
              <a href={href(src)} download>
                {viewer.download}
              </a>
            ) : null}
          </span>
        </span>
      ) : null}
    </span>
  );
}

/** Files to pick from: a card each, one open at a time. */
function Gallery({ items, viewer }) {
  const [selected, setSelected] = useState(null);
  return (
    <div className="gallery-grid">
      {items.map((item) => (
        <GalleryRow key={item.name} item={item} viewer={viewer} open={selected === item.name} onSelect={(name) => setSelected((now) => (now === name ? null : name))} />
      ))}
    </div>
  );
}

function GalleryRow({ item, viewer, open, onSelect }) {
  const [shown, ref] = useVisible();
  const says = open ? (viewer.close_label ?? "Close") : (viewer.open_label ?? "Open");
  return (
    <article className={open ? "gallery-card gallery-open" : "gallery-card"} data-busy={String(open)} ref={ref}>
      {shown ? (
        <>
          <button type="button" className="dx-button gallery-pick" data-style="outline" data-size="default" aria-expanded={open} aria-controls={`viewer-${item.name}`} onClick={() => onSelect(item.name)}>
            {item.poster ? <img src={item.poster} width="80" height="80" alt={item.name} loading="lazy" data-no-lightbox="true" /> : null}
            <span>
              <strong>{item.name}</strong>
              <small>{says}</small>
            </span>
          </button>
          {open ? (
            <div id={`viewer-${item.name}`}>
              <FileViewer src={item.src} caption={item.name} viewer={viewer} />
            </div>
          ) : null}
        </>
      ) : null}
    </article>
  );
}
