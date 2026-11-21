// Document content as data: the server sends Block and Span trees (symphony_api, from
// crates/symphony-blocks) and every element is chosen here, one component per kind, as the Dioxus
// client's block_view.rs draws them. A plugin renders to these blocks, never HTML, so a plugin's
// content is drawn by them whatever plugin made it. Each switch over the kinds ends in a value of
// type `never`: a kind the Rust adds fails the type check until it is drawn here. A kind a newer
// server sends this client (the kit's lists grow in a minor, docs/PLUGIN_PROTOCOL.md) is said
// where it stands, as unsupported content, and the rest of the page is drawn.

import { createContext, type ReactNode, useContext, useState } from "react";
import { Button } from "./button";
import { DocumentPicture, MediaView, writtenStyle } from "./media";
import { href } from "./project";
import { NearViewport, useVisibility } from "./viewport";
import type {
  Block,
  Cell,
  ContainerKind,
  FigureSize,
  Image,
  InlineKind,
  LinkTarget,
  Presentation,
  Span,
  ThreadEntry,
  VectorKind,
  VectorShape,
} from "./wire";

/**
 * How a card draws a link's excerpt: a link with an anchor gets a button beside it that opens the
 * excerpt under it and closes it again. Without one (null) a link is an ordinary link.
 */
export const ExcerptsContext = createContext<((target: LinkTarget) => ReactNode) | null>(null);

/**
 * The card whose outer `details` holds its anchor while its body is read: that card's own
 * decision leaves the id to it. Null where no card does.
 */
export const CardAnchorContext = createContext<string | null>(null);

/** Blocks drawn away from their place keep their cards' names without their anchors. */
const HistoricalContext = createContext(false);

/**
 * Blocks shown away from their place (a card's earlier version, a link's excerpt): they keep their
 * names, but neither own the current card's anchor nor open excerpts.
 */
export function HistoricalBlocks({ blocks }: { blocks: Block[] }) {
  return (
    <HistoricalContext.Provider value={true}>
      <ExcerptsContext.Provider value={null}>
        <BlocksView blocks={blocks} />
      </ExcerptsContext.Provider>
    </HistoricalContext.Provider>
  );
}

/** `blocks` with each run of threads beside each other joined into one: one history per entry. */
export function joinThreads(blocks: Block[]): Block[] {
  const joined: Block[] = [];
  for (const block of blocks) {
    const last = joined[joined.length - 1];
    if (block.type === "thread" && last?.type === "thread") {
      joined[joined.length - 1] = { type: "thread", entries: [...last.entries, ...block.entries] };
    } else {
      joined.push(block);
    }
  }
  return joined;
}

/** Blocks, each drawn by its kind's component. */
export function BlocksView({ blocks }: { blocks: Block[] }) {
  return joinThreads(blocks).map((block, at) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: a block has no identity but its place
    <BlockView key={at} block={block} />
  ));
}

/** Spans, each drawn by its kind's component. */
export function SpansView({ spans }: { spans: Span[] }) {
  return spans.map((span, at) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: a span has no identity but its place
    <SpanView key={at} span={span} />
  ));
}

/**
 * The `type` of a block or span of no kind the wire types know. No kind they know is left undrawn:
 * a kind the Rust adds fails the type check where this is called, as what reaches it then is no
 * longer `never`. What does reach it is a kind a newer server sent; one whose `type` is no text,
 * which no read lets through, is `unnamed`.
 */
function unknownKind(kind: never): string {
  const type = (kind as { type?: unknown }).type;
  return typeof type === "string" ? type : "unnamed";
}

function BlockView({ block }: { block: Block }) {
  switch (block.type) {
    case "heading":
      return (
        <Heading level={block.level} anchor={block.anchor}>
          <SpansView spans={block.spans} />
        </Heading>
      );
    case "paragraph":
      return (
        <p>
          <SpansView spans={block.spans} />
        </p>
      );
    case "list":
      return <ListView ordered={block.ordered} start={block.start} items={block.items} />;
    case "table":
      return (
        <TableView
          head={block.head}
          rows={block.rows}
          presentation={block.presentation ?? NO_PRESENTATION}
        />
      );
    case "code":
      return (
        <pre>
          <code className={`language-${block.language}`}>{block.text}</code>
        </pre>
      );
    case "quote":
      return (
        <blockquote>
          <BlocksView blocks={block.blocks} />
        </blockquote>
      );
    case "rule":
      return <hr />;
    case "callout":
      return (
        <Callout
          tone={block.kind}
          done={block.resolved}
          title={<SpansView spans={block.title.filter((span) => !isKindWord(span))} />}
        >
          <BlocksView blocks={block.blocks} />
        </Callout>
      );
    case "card":
      return <CardView card={block} />;
    case "component":
      return <ComponentView kind={block.kind} blocks={block.blocks} />;
    case "media":
      return <MediaView media={block.media} />;
    case "image":
      return <DocumentPicture image={block.image} />;
    case "figure":
      return (
        <FigureView
          picture={block.picture ?? null}
          caption={block.caption}
          href={block.href ?? null}
          badge={block.badge ?? null}
          size={block.size}
        />
      );
    case "caption":
      return (
        <figcaption>
          <BlocksView blocks={block.blocks} />
        </figcaption>
      );
    case "comparison":
      return (
        <div className="compare">
          <BlocksView blocks={block.figures} />
        </div>
      );
    case "group":
      return <BlocksView blocks={block.blocks} />;
    case "container":
      return (
        <ContainerView tag={block.tag} presentation={block.presentation} blocks={block.blocks} />
      );
    case "details":
    case "settled-card":
      return (
        <details open={block.open}>
          <summary>
            <SpansView spans={block.summary} />
          </summary>
          <NearViewport>
            <BlocksView blocks={block.blocks} />
          </NearViewport>
        </details>
      );
    case "thread":
      return <ThreadView entries={block.entries} />;
    case "comment":
      // A comment draws nothing, as in Markdown; its source stays in the data.
      return null;
    case "unknown":
      // An element is named alone, what it holds drawn after it: no empty paragraph for it.
      return (
        <Unknown description={block.tag} attributes={[]}>
          {block.text !== "" && <p>{block.text}</p>}
        </Unknown>
      );
    default:
      return <Unknown description={unknownKind(block)} attributes={[]} />;
  }
}

function SpanView({ span }: { span: Span }) {
  switch (span.type) {
    case "text":
      return span.text;
    case "emphasis":
      return (
        <em>
          <SpansView spans={span.spans} />
        </em>
      );
    case "strong":
      return (
        <strong>
          <SpansView spans={span.spans} />
        </strong>
      );
    case "code":
      return <code>{span.text}</code>;
    case "link":
      return (
        <LinkView
          href={span.href}
          target={span.target ?? null}
          title={span.title}
          spans={span.spans}
          presentation={NO_PRESENTATION}
        />
      );
    case "wiki-link":
      return (
        <LinkView
          href={span.href}
          target={span.target ?? null}
          title=""
          spans={span.spans}
          presentation={NO_PRESENTATION}
        />
      );
    case "line-break":
      return <br />;
    case "image":
      return <DocumentPicture image={span.image} />;
    case "media":
      return <MediaView media={span.media} />;
    case "component":
      return <ComponentView kind={span.kind} blocks={span.blocks} inline />;
    case "icon":
      return (
        <IconView shapes={span.shape} label={span.label} named={span.named} tip={span.tip ?? ""} />
      );
    case "styled":
      return <InlineView tag={span.tag} presentation={span.presentation} spans={span.spans} />;
    case "graphic":
      return (
        <GraphicView
          viewBox={span.view_box}
          presentation={span.presentation}
          shapes={span.shapes}
        />
      );
    case "blocks":
      return <BlocksView blocks={span.blocks} />;
    case "unknown":
      return (
        <span className="unknown-block" role="alert">
          Unsupported {span.tag}: {span.text}
        </span>
      );
    default:
      return (
        <span className="unknown-block" role="alert">
          Unsupported {unknownKind(span)}
        </span>
      );
  }
}

const NO_PRESENTATION: Presentation = {};

/** Whether `span` is a callout's kind word, which the callout draws itself before its title. */
function isKindWord(span: Span): boolean {
  return (
    span.type === "styled" && (span.presentation.class ?? "").split(/\s+/).includes("callout-kind")
  );
}

const HEADINGS = ["h1", "h2", "h3", "h4", "h5"] as const;

function Heading({
  level,
  anchor,
  children,
}: {
  level: number;
  anchor: string;
  children: ReactNode;
}) {
  const Tag = HEADINGS[level - 1] ?? "h6";
  return <Tag id={anchor}>{children}</Tag>;
}

function ListView({
  ordered,
  start,
  items,
}: {
  ordered: boolean;
  start: number;
  items: Block[][];
}) {
  const drawn = items.map((blocks, at) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: an item has no identity but its place
    <li key={at}>
      <NearViewport height={32}>
        <BlocksView blocks={blocks} />
      </NearViewport>
    </li>
  ));
  return ordered ? <ol start={start}>{drawn}</ol> : <ul>{drawn}</ul>;
}

/** A callout: its kind's word and title, then its blocks; a resolved one folds under its title. */
function Callout({
  tone,
  title,
  done,
  children,
}: {
  tone: string;
  title: ReactNode;
  done: boolean;
  children: ReactNode;
}) {
  const heading = (
    <>
      <span className="callout-kind">{tone}</span> {title}
    </>
  );
  if (done) {
    return (
      <details className={`callout callout-${tone} callout-done`}>
        <summary className="callout-title">{heading}</summary>
        {children}
      </details>
    );
  }
  return (
    <div className={`callout callout-${tone}`}>
      <p className="callout-title">{heading}</p>
      {children}
    </div>
  );
}

/**
 * A decision card: its slug, when it was posted, its body (its title when it has none), its state,
 * then its comparison and its thread. It owns its anchor unless it is drawn away from its place or
 * its outer `details` holds it.
 */
function CardView({ card }: { card: Extract<Block, { type: "card" }> }) {
  const historical = useContext(HistoricalContext);
  const suppressed = useContext(CardAnchorContext);
  const anchor = !historical && suppressed !== card.slug;
  const posted = card.verdict.posted ?? "";
  const decided = card.verdict.decided ?? "";
  return (
    <div
      className="decision"
      id={anchor ? card.slug : undefined}
      data-slug={card.slug}
      data-state={card.mark}
      data-group={card.verdict.group}
      data-open={`${card.verdict.open}`}
      data-decided={decided}
    >
      <div className="decision-text">
        <code>{card.slug}</code>{" "}
        {posted !== "" && (
          <>
            <time className="posted">{posted}</time>{" "}
          </>
        )}
        {card.body.length === 0 ? card.title : <BlocksView blocks={card.body} />}
        <span className="state">
          {card.mark}
          {decided !== "" && ` · ${decided}`}
        </span>
      </div>
      <BlocksView blocks={card.comparison} />
      {card.thread.length > 0 && <ThreadView entries={card.thread} />}
    </div>
  );
}

/** What the client cannot draw, said where it stands, with the attributes it could not apply. */
function Unknown({
  description,
  attributes,
  children,
}: {
  description: string;
  attributes: string[];
  children?: ReactNode;
}) {
  return (
    <aside className="unknown-block" role="alert">
      <strong>Unsupported document content: {description}</strong>
      {attributes.length > 0 && (
        <p>
          Attributes: <code>{attributes.join(", ")}</code>
        </p>
      )}
      {children}
    </aside>
  );
}

interface LinkProps {
  href: string;
  target: LinkTarget | null;
  title: string;
  spans: Span[];
  presentation: Presentation;
}

/** A link under the project's base; beside it, in a card, the button that opens its excerpt. */
function LinkView({ href: path, target, title, spans, presentation }: LinkProps) {
  const excerpt = useContext(ExcerptsContext);
  const [shown, setShown] = useState(false);
  const anchored = target != null && target.anchor != null ? target : null;
  return (
    <>
      <a href={href(path)} title={title} {...attributes(presentation)}>
        <SpansView spans={spans} />
      </a>
      {excerpt !== null && anchored !== null && (
        <>
          <Button
            className="show-link"
            type="button"
            aria-expanded={shown}
            onClick={(event) => {
              event.stopPropagation();
              setShown((now) => !now);
            }}
          >
            {shown ? "▾ hide" : "▸ show"}
          </Button>
          {shown && excerpt(anchored)}
        </>
      )}
      <PresentationWarnings presentation={presentation} />
    </>
  );
}

/**
 * A plugin's component: its blocks inside a wrapper naming its kind, which lays out nothing itself
 * and is there for a project's stylesheet to select. In a line of text, one paragraph's spans stay
 * in that line.
 */
function ComponentView({
  kind,
  blocks,
  inline = false,
}: {
  kind: string;
  blocks: Block[];
  inline?: boolean;
}) {
  if (!inline) {
    return (
      <div data-component={kind}>
        <BlocksView blocks={blocks} />
      </div>
    );
  }
  const [only] = blocks;
  return (
    <span data-component={kind}>
      {blocks.length === 1 && only?.type === "paragraph" ? (
        <SpansView spans={only.spans} />
      ) : (
        <BlocksView blocks={blocks} />
      )}
    </span>
  );
}

/** The sizes an item takes, each drawn as a link card, its badge standing in for its picture. */
const ITEM_SIZES: FigureSize[] = ["small", "row", "large", "tile"];

interface FigureProps {
  picture: Image | null;
  caption: Span[];
  href: string | null;
  badge: string | null;
  size: FigureSize;
}

/**
 * A picture with its caption. An item's sizes (`small`, `row`, `large`, `tile`) draw a link card,
 * its badge standing in for a missing picture; the others draw a figure, `missing` without one,
 * its picture's box filled until it comes near.
 */
function FigureView({ picture, caption, href: link, badge, size }: FigureProps) {
  const className = `figure figure-${size}`;
  if (ITEM_SIZES.includes(size)) {
    const content = (
      <>
        {picture !== null ? (
          <DocumentPicture image={picture} />
        ) : (
          badge !== null && <span className="figure-badge">{badge}</span>
        )}
        <span className="figure-caption">
          <SpansView spans={caption} />
        </span>
      </>
    );
    return link === null ? (
      <span className={className}>{content}</span>
    ) : (
      <a className={className} href={href(link)}>
        {content}
      </a>
    );
  }
  return (
    <figure className={picture === null ? `${className} missing` : className}>
      {picture !== null ? (
        <NearViewport ghost={<span className="bone ghost-picture" />}>
          {link === null ? (
            <DocumentPicture image={picture} />
          ) : (
            <a href={href(link)}>
              <DocumentPicture image={picture} />
            </a>
          )}
        </NearViewport>
      ) : (
        badge !== null && <span className="figure-badge">{badge}</span>
      )}
      {caption.length > 0 && (
        <figcaption>
          <SpansView spans={caption} />
        </figcaption>
      )}
    </figure>
  );
}

/**
 * An icon on the 24-unit stroke grid: its label is its accessible name, its tip shows on hover and
 * focus, and `named` shows the label after it.
 */
function IconView({
  shapes,
  label,
  named,
  tip,
}: {
  shapes: VectorShape[];
  label: string;
  named: boolean;
  tip: string;
}) {
  const tipped = tip === "" ? undefined : tip;
  return (
    <>
      <span
        className="icon"
        data-tip={tipped}
        title={tipped}
        // biome-ignore lint/a11y/noNoninteractiveTabindex: its tip shows on focus as on hover
        tabIndex={0}
        role="img"
        aria-label={label}
      >
        <svg
          viewBox="0 0 24 24"
          width="18"
          height="18"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <Shapes shapes={shapes} />
        </svg>
        {named && <span className="icon-name">{label}</span>}
      </span>
      <ShapeWarnings shapes={shapes} />
    </>
  );
}

/** The elements a container's tag names; `center` is a `div` that centres its text. */
const CONTAINERS = {
  div: "div",
  section: "section",
  figure: "figure",
  figcaption: "figcaption",
  aside: "aside",
  header: "header",
  footer: "footer",
  address: "address",
  center: "div",
} as const satisfies Record<ContainerKind, string>;

function ContainerView({
  tag,
  presentation,
  blocks,
}: {
  tag: ContainerKind;
  presentation: Presentation;
  blocks: Block[];
}) {
  const Tag = CONTAINERS[tag];
  const centred = tag === "center" ? { style: { textAlign: "center" as const } } : {};
  return (
    <>
      <Tag {...centred} {...attributes(presentation)}>
        <BlocksView blocks={blocks} />
      </Tag>
      <PresentationWarnings presentation={presentation} />
    </>
  );
}

/** The elements an inline tag names. */
const INLINES = {
  span: "span",
  small: "small",
  sub: "sub",
  sup: "sup",
  time: "time",
  mark: "mark",
  kbd: "kbd",
  samp: "samp",
  delete: "del",
  insert: "ins",
  underline: "u",
  abbreviation: "abbr",
} as const satisfies Record<InlineKind, string>;

/** Whether a styled span's presentation belongs to the link it wraps (its target, its task). */
function linkPresentation(presentation: Presentation): boolean {
  return (
    Object.keys(presentation.attributes ?? {}).some((name) =>
      ["target", "rel", "download", "data-task"].includes(name),
    ) || (presentation.class ?? "").split(/\s+/).includes("tasklink")
  );
}

function InlineView({
  tag,
  presentation,
  spans,
}: {
  tag: InlineKind;
  presentation: Presentation;
  spans: Span[];
}) {
  // The parser wraps attributes that belong to a link in a styled span: they go on the link
  // itself, not around it.
  const [only] = spans;
  if (
    tag === "span" &&
    spans.length === 1 &&
    linkPresentation(presentation) &&
    (only?.type === "link" || only?.type === "wiki-link")
  ) {
    return (
      <LinkView
        href={only.href}
        target={only.target ?? null}
        title={only.type === "link" ? only.title : ""}
        spans={only.spans}
        presentation={presentation}
      />
    );
  }
  const Tag = INLINES[tag];
  return (
    <>
      <Tag {...attributes(presentation)}>
        <SpansView spans={spans} />
      </Tag>
      <PresentationWarnings presentation={presentation} />
    </>
  );
}

function TableView({
  head,
  rows,
  presentation,
}: {
  head: Cell[];
  rows: Cell[][];
  presentation: Presentation;
}) {
  return (
    <>
      <table {...attributes(presentation)}>
        {head.length > 0 && (
          <thead>
            <tr>
              {head.map((cell, at) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: a cell has no identity but its place
                <CellView key={at} cell={cell} />
              ))}
            </tr>
          </thead>
        )}
        <tbody>
          {rows.map((cells, at) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: a row has no identity but its place
            <TableRow key={at} cells={cells} />
          ))}
        </tbody>
      </table>
      <PresentationWarnings presentation={presentation} />
    </>
  );
}

/** A table's row, one cell of a row's height until it comes into view. */
function TableRow({ cells }: { cells: Cell[] }) {
  const [shown, watch] = useVisibility();
  return (
    <tr ref={watch}>
      {shown ? (
        cells.map((cell, at) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: a cell has no identity but its place
          <CellView key={at} cell={cell} />
        ))
      ) : (
        <td colSpan={cells.length} style={{ height: "48px" }} aria-hidden="true" />
      )}
    </tr>
  );
}

function CellView({ cell }: { cell: Cell }) {
  const Tag = cell.header ? "th" : "td";
  return (
    <Tag {...attributes(cell.presentation)}>
      <SpansView spans={cell.spans} />
      <PresentationWarnings presentation={cell.presentation} />
    </Tag>
  );
}

/** A card's history: each entry's time, verdict and words, then whether the owner waits. */
function ThreadView({ entries }: { entries: ThreadEntry[] }) {
  const last = entries[entries.length - 1];
  const awaiting = last !== undefined && last.who === "owner" && last.verdict == null;
  return (
    <>
      <ul className="history">
        {entries.map((entry, at) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: an entry has no identity but its place
          <li key={at} className={`entry ${entry.who}`}>
            <NearViewport height={60}>
              {entry.timestamp != null && (
                <>
                  <time>{entry.timestamp}</time>{" "}
                </>
              )}
              {entry.verdict != null && (
                <>
                  <b className="entry-verdict">{entry.verdict}</b>{" "}
                </>
              )}
              <span className="entry-text">
                <BlocksView blocks={entry.blocks} />
              </span>
              {entry.who === "unknown" && <span role="alert">Unknown thread author</span>}
            </NearViewport>
          </li>
        ))}
      </ul>
      {awaiting && <p className="awaiting">Waiting for an answer…</p>}
    </>
  );
}

function GraphicView({
  viewBox,
  presentation,
  shapes,
}: {
  viewBox: string;
  presentation: Presentation;
  shapes: VectorShape[];
}) {
  return (
    <>
      {/* biome-ignore lint/a11y/noSvgWithoutTitle: its presentation names it, as the document wrote it */}
      <svg viewBox={viewBox} {...attributes(presentation)}>
        <Shapes shapes={shapes} />
      </svg>
      <PresentationWarnings presentation={presentation} />
      <ShapeWarnings shapes={shapes} />
    </>
  );
}

/** The SVG element each vector shape is drawn as. */
const SHAPES = {
  path: "path",
  circle: "circle",
  ellipse: "ellipse",
  rect: "rect",
  line: "line",
  polyline: "polyline",
  polygon: "polygon",
} as const satisfies Record<VectorKind, string>;

function Shapes({ shapes }: { shapes: VectorShape[] }) {
  return shapes.map((shape, at) => {
    const Tag = SHAPES[shape.kind];
    // biome-ignore lint/suspicious/noArrayIndexKey: a shape has no identity but its place
    return <Tag key={at} {...mapped(shape.attributes)} />;
  });
}

/**
 * The attributes the client applies, each by the name React gives it (`tabindex` as `tabIndex`):
 * sizing, accessibility, the document's CSS declarations, the data the page's scripts read and an
 * SVG shape's geometry and paint. Any other is said, never applied.
 */
const ATTRIBUTES = new Map<string, string>(
  Object.entries({
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
  }),
);

/**
 * The attributes of `values` the client applies, as React's props, the style as a ref that writes
 * it as given (`writtenStyle`): one bag any element takes, whatever its own props' types.
 */
function mapped(values: Record<string, string>): object {
  const props: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(values)) {
    const prop = ATTRIBUTES.get(name);
    if (prop === "style") props.ref = writtenStyle(value);
    else if (prop !== undefined) props[prop] = value;
  }
  return props;
}

/**
 * A presentation's props: the attributes the client applies, then its class, id and title, which
 * a `data-tip` stands in for when it has none.
 */
export function attributes(presentation: Presentation): object {
  const given = presentation.attributes ?? {};
  const props: Record<string, unknown> = { ...mapped(given) };
  const tip = given["data-tip"];
  if ((presentation.title ?? "") === "" && tip !== undefined) props.title = tip;
  if ((presentation.class ?? "") !== "") props.className = presentation.class;
  if ((presentation.id ?? "") !== "") props.id = presentation.id;
  if ((presentation.title ?? "") !== "") props.title = presentation.title;
  return props;
}

/** The attributes of `values` the client does not apply, as `name=value`. */
export function unsupported(values: Record<string, string>): string[] {
  return Object.entries(values)
    .filter(([name]) => !ATTRIBUTES.has(name))
    .map(([name, value]) => `${name}=${value}`);
}

function AttributeWarnings({ values }: { values: Record<string, string> }) {
  const said = unsupported(values);
  return said.length === 0 ? null : <Unknown description="attributes" attributes={said} />;
}

function PresentationWarnings({ presentation }: { presentation: Presentation }) {
  return <AttributeWarnings values={presentation.attributes ?? {}} />;
}

function ShapeWarnings({ shapes }: { shapes: VectorShape[] }) {
  return shapes.map((shape, at) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: a shape has no identity but its place
    <AttributeWarnings key={at} values={shape.attributes} />
  ));
}
