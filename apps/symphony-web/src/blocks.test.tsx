import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  attributes,
  BlocksView,
  CardAnchorContext,
  ExcerptsContext,
  HistoricalBlocks,
  joinThreads,
  SpansView,
  unsupported,
} from "./blocks";
import { writtenStyle } from "./media";
import { CASES } from "./testing";
import type { NearViewportProps } from "./viewport";
import type {
  Block,
  ContainerKind,
  FigureSize,
  Image,
  InlineKind,
  Media,
  Span,
  VectorKind,
  VectorShape,
} from "./wire";

// What waits to come near the screen is drawn as its content alone, as the Dioxus client's tests
// draw it (`near.shown`), or as it is until it comes near: a test reads either.
const near = vi.hoisted(() => ({ shown: true }));
vi.mock(import("./viewport"), async (original) => {
  const real = await original();
  const { createElement: element, Fragment } = await import("react");
  return {
    ...real,
    NearViewport: (props: NearViewportProps) =>
      near.shown ? element(Fragment, null, props.children) : real.NearViewport(props),
  };
});

/** What `blocks` draw, inside what `wrap` puts around them. */
function drawn(blocks: Block[], wrap: (node: ReactNode) => ReactNode = (node) => node): string {
  return renderToStaticMarkup(wrap(<BlocksView blocks={blocks} />));
}

const text = (words: string): Span => ({ type: "text", text: words });
const paragraph = (words: string): Block => ({ type: "paragraph", spans: [text(words)] });
const LUMEN: Image = { src: "/k.png", alt: "Lumen", presentation: {} };
const figure = (
  picture: Image | null,
  href: string | null,
  badge: string | null,
  size: FigureSize,
): Block => ({ type: "figure", picture, caption: [text("NOR-L01 Lumen")], href, badge, size });

beforeEach(() => {
  near.shown = true;
  // A page with no project element: the plain project, whose links are as written.
  vi.stubGlobal("document", { getElementById: () => null });
});

afterEach(() => vi.unstubAllGlobals());

/** Every kind the wire types know, each named once, which the type check holds. */
const BLOCK_KINDS: Record<Block["type"], true> = {
  heading: true,
  paragraph: true,
  list: true,
  table: true,
  code: true,
  quote: true,
  rule: true,
  callout: true,
  card: true,
  component: true,
  media: true,
  image: true,
  figure: true,
  caption: true,
  comparison: true,
  group: true,
  container: true,
  details: true,
  "settled-card": true,
  thread: true,
  comment: true,
  unknown: true,
};
const SPAN_KINDS: Record<Span["type"], true> = {
  text: true,
  emphasis: true,
  strong: true,
  code: true,
  link: true,
  "wiki-link": true,
  "line-break": true,
  image: true,
  media: true,
  component: true,
  icon: true,
  styled: true,
  graphic: true,
  blocks: true,
  unknown: true,
};
const MEDIA_KINDS: Record<Media["kind"], true> = { mockup: true, viewer: true, gallery: true };
const CONTAINER_TAGS: Record<ContainerKind, true> = {
  div: true,
  section: true,
  figure: true,
  figcaption: true,
  aside: true,
  header: true,
  footer: true,
  address: true,
  center: true,
};
const INLINE_TAGS: Record<InlineKind, true> = {
  span: true,
  small: true,
  sub: true,
  sup: true,
  time: true,
  mark: true,
  kbd: true,
  samp: true,
  delete: true,
  insert: true,
  underline: true,
  abbreviation: true,
};
const SHAPE_KINDS: Record<VectorKind, true> = {
  path: true,
  circle: true,
  ellipse: true,
  rect: true,
  line: true,
  polyline: true,
  polygon: true,
};
const SIZES: Record<FigureSize, true> = {
  small: true,
  row: true,
  large: true,
  tile: true,
  cell: true,
  slot: true,
  wide: true,
};

/** Each kind and word the sample holds, by its family: `block heading`, `inline kbd`. */
function census(blocks: Block[]): Set<string> {
  const found = new Set<string>();
  const shapes = (list: VectorShape[]) => {
    for (const shape of list) found.add(`shape ${shape.kind}`);
  };
  const media = (one: Media) => found.add(`media ${one.kind}`);
  const spans = (list: Span[]) => {
    for (const span of list) {
      found.add(`span ${span.type}`);
      if ("spans" in span) spans(span.spans);
      if ("blocks" in span) walk(span.blocks);
      if (span.type === "media") media(span.media);
      if (span.type === "styled") found.add(`inline ${span.tag}`);
      if (span.type === "icon") shapes(span.shape);
      if (span.type === "graphic") shapes(span.shapes);
    }
  };
  const walk = (list: Block[]) => {
    for (const block of list) {
      found.add(`block ${block.type}`);
      switch (block.type) {
        case "heading":
        case "paragraph":
          spans(block.spans);
          break;
        case "list":
          for (const item of block.items) walk(item);
          break;
        case "table":
          for (const cell of [...block.head, ...block.rows.flat()]) spans(cell.spans);
          break;
        case "callout":
          spans(block.title);
          walk(block.blocks);
          break;
        case "card":
          walk([...block.body, ...block.comparison, ...block.thread.flatMap((e) => e.blocks)]);
          break;
        case "media":
          media(block.media);
          break;
        case "figure":
          found.add(`size ${block.size}`);
          spans(block.caption);
          break;
        case "comparison":
          walk(block.figures);
          break;
        case "container":
          found.add(`container ${block.tag}`);
          walk(block.blocks);
          break;
        case "details":
        case "settled-card":
          spans(block.summary);
          walk(block.blocks);
          break;
        case "thread":
          walk(block.entries.flatMap((entry) => entry.blocks));
          break;
        case "quote":
        case "component":
        case "caption":
        case "group":
          walk(block.blocks);
          break;
        default:
          break;
      }
    }
  };
  walk(blocks);
  return found;
}

describe("the blocks crates/symphony-api writes", () => {
  const SAMPLE = CASES.samples.blocks;

  it("hold every kind and word the wire types know, so each is drawn here", () => {
    const families: [string, object][] = [
      ["block", BLOCK_KINDS],
      ["span", SPAN_KINDS],
      ["media", MEDIA_KINDS],
      ["container", CONTAINER_TAGS],
      ["inline", INLINE_TAGS],
      ["shape", SHAPE_KINDS],
      ["size", SIZES],
    ];
    const known = families.flatMap(([family, words]) =>
      Object.keys(words).map((word) => `${family} ${word}`),
    );
    expect([...census(SAMPLE)].sort()).toEqual(known.sort());
  });

  it("are each drawn by its kind's element", () => {
    const html = drawn(SAMPLE);
    for (const part of [
      '<h2 id="lamps">Lamps</h2>',
      "<em>emphasis</em><strong>strong</strong><code>code</code>",
      '<ol start="3"><li><p>third</p></li><li><p>fourth</p></li></ol>',
      "<ul><li><p>one</p></li></ul>",
      '<pre><code class="language-rust">fn main() {}</code></pre>',
      "<blockquote><p>Quoted</p></blockquote>",
      "<hr/>",
      '<div data-component="lamp"><p>A lamp</p></div>',
      '<span data-component="lamp">NOR-L01</span>',
      "<figcaption><p>A caption</p></figcaption>",
      '<div class="compare"><figure class="figure figure-slot">',
      "<p>Grouped</p>",
      '<details open=""><summary>More</summary><p>Detail</p></details>',
      "<details><summary>lamp-size · approved</summary><p>Settled</p></details>",
      '<aside class="unknown-block" role="alert"><strong>Unsupported document content: marquee</strong><p>Scrolling</p></aside>',
      '<span class="unknown-block" role="alert">Unsupported blink: old</span>',
      "<br/>",
      '<span class="mockup-shell">',
      '<orbit-view src="/models/a.item" alt="Model" controls="" poster="/models/a.png"></orbit-view>',
      '<div class="gallery-grid">',
    ]) {
      expect(html).toContain(part);
    }
    expect(html).not.toContain("a note");
    for (const tag of ["div", "section", "figure", "figcaption", "aside", "header", "footer"]) {
      expect(html).toContain(`<${tag} data-kind="lamp" class="box"><p>Inside</p></${tag}>`);
    }
    expect(html).toContain('<address data-kind="lamp" class="box"><p>Inside</p></address>');
    expect(html).toContain(
      '<div style="text-align:center" data-kind="lamp" class="box"><p>Inside</p></div>',
    );
    for (const [tag, element] of [
      ["span", "span"],
      ["small", "small"],
      ["sub", "sub"],
      ["sup", "sup"],
      ["time", "time"],
      ["mark", "mark"],
      ["kbd", "kbd"],
      ["samp", "samp"],
      ["delete", "del"],
      ["insert", "ins"],
      ["underline", "u"],
      ["abbreviation", "abbr"],
    ]) {
      expect(html).toContain(`<${element} class="${tag}">${tag}</${element}>`);
    }
    for (const shape of ["path", "circle", "ellipse", "rect", "line", "polyline", "polygon"]) {
      expect(html).toContain(`<${shape} `);
    }
  });
});

describe("a heading", () => {
  it("is of its level, h6 past the fifth, and holds its anchor", () => {
    const heading = (level: number): Block => ({
      type: "heading",
      level,
      anchor: "a",
      spans: [text("A")],
    });
    expect(drawn([heading(1)])).toBe('<h1 id="a">A</h1>');
    expect(drawn([heading(5)])).toBe('<h5 id="a">A</h5>');
    expect(drawn([heading(6)])).toBe('<h6 id="a">A</h6>');
    expect(drawn([heading(0)])).toBe('<h6 id="a">A</h6>');
  });
});

describe("a list", () => {
  it("draws each item once it comes near the screen, a line's height until then", () => {
    near.shown = false;
    const list: Block = { type: "list", ordered: false, start: 1, items: [[paragraph("first")]] };
    const html = drawn([list]);
    expect(html).toContain('<ul><li><div style="contain-intrinsic-size:auto 32px;min-height:32px"');
    expect(html).not.toContain("first");
  });
});

describe("a figure", () => {
  it("of an item's size is a link card, its badge standing in for a missing picture", () => {
    for (const size of ["small", "row", "large", "tile"] as const) {
      const className = `figure figure-${size}`;
      let html = drawn([figure(LUMEN, "/docs/lamps/north.md#nor-l01", null, size)]);
      expect(html).toContain(`<a class="${className}" href="/docs/lamps/north.md#nor-l01">`);
      expect(html).toContain('<img src="/k.png" alt="Lumen"');
      expect(html).toContain('<span class="figure-caption">NOR-L01 Lumen</span>');
      expect(html).not.toContain("figure-badge");
      html = drawn([figure(null, null, "coming", size)]);
      expect(html).toContain(`<span class="${className}">`);
      expect(html).toContain('<span class="figure-badge">coming</span>');
      expect(html).not.toContain("<img");
      expect(html).not.toContain("<a ");
    }
  });

  it("of any other size is a figure, marked missing without its picture", () => {
    let html = drawn([figure(LUMEN, "/screens/title.png", null, "wide")]);
    expect(html.startsWith('<figure class="figure figure-wide">')).toBe(true);
    expect(html).toContain('<a href="/screens/title.png"><img src="/k.png"');
    expect(html).toContain("<figcaption>NOR-L01 Lumen</figcaption>");
    html = drawn([figure(LUMEN, null, null, "slot")]);
    expect(html.startsWith('<figure class="figure figure-slot"><img')).toBe(true);
    html = drawn([figure(null, null, "not captured", "cell")]);
    expect(html.startsWith('<figure class="figure figure-cell missing">')).toBe(true);
    expect(html).toContain('<span class="figure-badge">not captured</span>');
    const bare: Block = {
      type: "figure",
      picture: null,
      caption: [],
      href: null,
      badge: null,
      size: "wide",
    };
    expect(drawn([bare])).toBe('<figure class="figure figure-wide missing"></figure>');
  });

  it("fills its picture's box until it comes near the screen", () => {
    near.shown = false;
    const html = drawn([figure(LUMEN, null, null, "wide")]);
    expect(html).toContain('<span class="bone ghost-picture"></span>');
    expect(html).not.toContain("<img");
  });
});

describe("an icon", () => {
  const path: VectorShape = { kind: "path", attributes: { d: "M4 4L20 20" } };
  const icon = (named: boolean, tip?: string): Block => ({
    type: "paragraph",
    spans: [
      {
        type: "icon",
        shape: [path],
        label: "Cloning bay",
        named,
        ...(tip === undefined ? {} : { tip }),
      },
    ],
  });

  it("is its vector, named by its label and tipped on hover", () => {
    const html = drawn([icon(true, "Cloning bay: brings a lost crew back")]);
    for (const part of [
      'class="icon"',
      'data-tip="Cloning bay: brings a lost crew back"',
      'title="Cloning bay: brings a lost crew back"',
      'tabindex="0"',
      'role="img"',
      'aria-label="Cloning bay"',
      'viewBox="0 0 24 24"',
      'stroke="currentColor"',
      'stroke-width="1.8"',
      'aria-hidden="true"',
      '<path d="M4 4L20 20"',
      '<span class="icon-name">Cloning bay</span>',
    ]) {
      expect(html).toContain(part);
    }
  });

  it("without a tip or its name shows neither", () => {
    for (const html of [drawn([icon(false, "")]), drawn([icon(false)])]) {
      expect(html).toContain('aria-label="Cloning bay"');
      expect(html).not.toContain("data-tip");
      expect(html).not.toContain("title=");
      expect(html).not.toContain("icon-name");
    }
  });
});

describe("a component", () => {
  it("keeps its kind for the stylesheet and stays in its line", () => {
    expect(drawn([{ type: "component", kind: "lamp-matrix", blocks: [paragraph("inside")] }])).toBe(
      '<div data-component="lamp-matrix"><p>inside</p></div>',
    );
    const inline: Block = {
      type: "paragraph",
      spans: [
        text("see "),
        { type: "component", kind: "task", blocks: [paragraph("sim/jump")] },
        text(" now"),
      ],
    };
    expect(drawn([inline])).toBe('<p>see <span data-component="task">sim/jump</span> now</p>');
    // An inline component of several blocks keeps them, still inside its line's wrapper.
    const several: Block = {
      type: "paragraph",
      spans: [{ type: "component", kind: "lamps", blocks: [paragraph("a"), paragraph("b")] }],
    };
    expect(drawn([several])).toBe('<p><span data-component="lamps"><p>a</p><p>b</p></span></p>');
  });
});

describe("a table", () => {
  const cell = (spans: Span[]) => ({ header: false, spans, presentation: {} });

  it("names its look and holds figures in its cells", () => {
    const html = drawn([
      {
        type: "table",
        head: [{ header: true, spans: [text("Lamp")], presentation: {} }],
        rows: [[cell([{ type: "blocks", blocks: [figure(LUMEN, null, null, "cell")] }])]],
        presentation: { class: "matrix shape-matrix" },
      },
    ]);
    expect(
      html.startsWith('<table class="matrix shape-matrix"><thead><tr><th>Lamp</th></tr></thead>'),
    ).toBe(true);
    expect(html).toContain('<td><figure class="figure figure-cell"><img src="/k.png"');
    const plain = drawn([{ type: "table", head: [], rows: [[cell([text("x")])]] }]);
    expect(plain).toBe("<table><tbody><tr><td>x</td></tr></tbody></table>");
  });

  it("keeps a row's height in one cell until the row comes into view", () => {
    vi.stubGlobal("IntersectionObserver", class {});
    const html = drawn([{ type: "table", head: [], rows: [[cell([text("x")]), cell([])]] }]);
    expect(html).toBe(
      '<table><tbody><tr><td colSpan="2" style="height:48px" aria-hidden="true"></td></tr></tbody></table>',
    );
  });

  it("applies its cells' attributes", () => {
    const html = drawn([
      {
        type: "table",
        head: [],
        rows: [[{ header: true, spans: [], presentation: { attributes: { colspan: "2" } } }]],
      },
    ]);
    expect(html).toContain('<th colSpan="2"></th>');
  });
});

describe("a code block, a quote and a rule", () => {
  it("are their elements", () => {
    expect(drawn([{ type: "code", language: "", text: "<b>" }])).toBe(
      '<pre><code class="language-">&lt;b&gt;</code></pre>',
    );
    expect(drawn([{ type: "quote", blocks: [paragraph("q")] }])).toBe(
      "<blockquote><p>q</p></blockquote>",
    );
    expect(drawn([{ type: "rule" }])).toBe("<hr/>");
  });
});

describe("a callout", () => {
  it("says its kind once, before its title", () => {
    const html = drawn([
      {
        type: "callout",
        kind: "question",
        title: [
          {
            type: "styled",
            tag: "span",
            presentation: { class: "x callout-kind" },
            spans: [text("question")],
          },
          text("Warm?"),
        ],
        resolved: false,
        blocks: [paragraph("Which?")],
      },
    ]);
    expect(html).toBe(
      '<div class="callout callout-question"><p class="callout-title">' +
        '<span class="callout-kind">question</span> Warm?</p><p>Which?</p></div>',
    );
  });

  it("folds under its title once resolved", () => {
    const html = drawn([
      { type: "callout", kind: "comment", title: [text("Done")], resolved: true, blocks: [] },
    ]);
    expect(html).toBe(
      '<details class="callout callout-comment callout-done"><summary class="callout-title">' +
        '<span class="callout-kind">comment</span> Done</summary></details>',
    );
  });
});

describe("a card", () => {
  const card = (body: Block[]): Block => ({
    type: "card",
    slug: "lamp",
    mark: "pending",
    title: "Which lamp?",
    body,
    verdict: { group: "decide", open: true, posted: "2026-11-14", decided: null },
    thread: [],
    line: 3,
    line_offset: null,
    comparison: [],
  });

  it("says its slug, when it was posted, its body and its state, and owns its anchor", () => {
    expect(drawn([card([paragraph("Warm.")])])).toBe(
      '<div class="decision" id="lamp" data-slug="lamp" data-state="pending" data-group="decide" ' +
        'data-open="true" data-decided=""><div class="decision-text"><code>lamp</code> ' +
        '<time class="posted">2026-11-14</time> <p>Warm.</p><span class="state">pending</span>' +
        "</div></div>",
    );
  });

  it("without a body says its title, and its decision's date beside its state", () => {
    const decided: Block = {
      ...(card([]) as Extract<Block, { type: "card" }>),
      mark: "approved",
      verdict: { group: "resolved", open: false, posted: null, decided: "2026-11-15" },
    };
    const html = drawn([decided]);
    expect(html).toContain('data-open="false" data-decided="2026-11-15"');
    expect(html).toContain(
      '<code>lamp</code> Which lamp?<span class="state">approved · 2026-11-15</span>',
    );
    expect(html).not.toContain("posted");
  });

  it("leaves its anchor to its outer details, and drawn away from its place", () => {
    const anchored = (slug: string | null) =>
      drawn([card([])], (node) => (
        <CardAnchorContext.Provider value={slug}>{node}</CardAnchorContext.Provider>
      ));
    expect(anchored("lamp")).not.toContain('id="lamp"');
    expect(anchored("other")).toContain('id="lamp"');
    expect(anchored(null)).toContain('id="lamp"');
    const away = renderToStaticMarkup(<HistoricalBlocks blocks={[card([])]} />);
    expect(away).toContain('data-slug="lamp"');
    expect(away).not.toContain('id="lamp"');
  });
});

describe("a thread", () => {
  const entry = (who: "owner" | "agent" | "unknown", verdict: string | null, words: string) => ({
    who,
    timestamp: "2026-11-14 08:32",
    verdict,
    blocks: [paragraph(words)],
  });

  it("is one history of the threads beside each other, said awaiting after it when the owner spoke last", () => {
    const blocks: Block[] = [
      { type: "thread", entries: [entry("agent", null, "Asked.")] },
      { type: "thread", entries: [entry("owner", null, "Warm?")] },
      paragraph("after"),
      { type: "thread", entries: [entry("owner", "approved", "Warm.")] },
    ];
    expect(joinThreads(blocks).map((block) => block.type)).toEqual([
      "thread",
      "paragraph",
      "thread",
    ]);
    expect(drawn(blocks)).toBe(
      '<ul class="history"><li class="entry agent"><time>2026-11-14 08:32</time> ' +
        '<span class="entry-text"><p>Asked.</p></span></li><li class="entry owner">' +
        '<time>2026-11-14 08:32</time> <span class="entry-text"><p>Warm?</p></span></li>' +
        '</ul><p class="awaiting">Waiting for an answer…</p><p>after</p>' +
        '<ul class="history"><li class="entry owner"><time>2026-11-14 08:32</time> ' +
        '<b class="entry-verdict">approved</b> <span class="entry-text"><p>Warm.</p></span>' +
        "</li></ul>",
    );
  });

  it("says an entry's author is unknown", () => {
    const html = drawn([
      { type: "thread", entries: [{ ...entry("unknown", null, "Who?"), timestamp: null }] },
    ]);
    expect(html).toBe(
      '<ul class="history"><li class="entry unknown"><span class="entry-text"><p>Who?</p>' +
        '</span><span role="alert">Unknown thread author</span></li></ul>',
    );
  });
});

describe("a link", () => {
  const link: Span = {
    type: "link",
    href: "/docs/a.md#b",
    target: { path: "docs/a.md", anchor: "b" },
    title: "A",
    spans: [text("a")],
  };
  const excerpts = (node: ReactNode) => (
    <ExcerptsContext.Provider value={(target) => <i>{target.anchor}</i>}>
      {node}
    </ExcerptsContext.Provider>
  );

  it("is an ordinary link, outside a card's excerpts", () => {
    expect(renderToStaticMarkup(<SpansView spans={[link]} />)).toBe(
      '<a href="/docs/a.md#b" title="A">a</a>',
    );
    const wiki: Span = { type: "wiki-link", href: "/docs/a.md", target: null, spans: [text("a")] };
    expect(renderToStaticMarkup(<SpansView spans={[wiki]} />)).toBe(
      '<a href="/docs/a.md" title="">a</a>',
    );
  });

  it("to an anchor, in a card, has the button that opens its excerpt, closed", () => {
    const html = renderToStaticMarkup(excerpts(<SpansView spans={[link]} />));
    expect(html).toBe(
      '<a href="/docs/a.md#b" title="A">a</a><button class="dx-button show-link" ' +
        'data-style="outline" data-size="default" type="button" aria-expanded="false">▸ show</button>',
    );
    const unanchored: Span = { ...link, target: { path: "docs/a.md", anchor: null } };
    expect(renderToStaticMarkup(excerpts(<SpansView spans={[unanchored]} />))).not.toContain(
      "button",
    );
    expect(
      renderToStaticMarkup(
        excerpts(<HistoricalBlocks blocks={[{ type: "paragraph", spans: [link] }]} />),
      ),
    ).not.toContain("button");
  });

  it("takes the attributes a styled span around it gives", () => {
    const wrapped: Span = {
      type: "styled",
      tag: "span",
      presentation: { class: "tasklink", attributes: { "data-task": "app/lamps" } },
      spans: [link],
    };
    expect(renderToStaticMarkup(<SpansView spans={[wrapped]} />)).toBe(
      '<a href="/docs/a.md#b" title="A" data-task="app/lamps" class="tasklink">a</a>',
    );
    const targeted: Span = {
      type: "styled",
      tag: "span",
      presentation: { attributes: { target: "_blank" } },
      spans: [{ type: "wiki-link", href: "/x", target: null, spans: [text("x")] }],
    };
    expect(renderToStaticMarkup(<SpansView spans={[targeted]} />)).toBe(
      '<a href="/x" title="" target="_blank">x</a>',
    );
    const styled: Span = { ...wrapped, tag: "small" };
    expect(renderToStaticMarkup(<SpansView spans={[styled]} />)).toContain(
      '<small data-task="app/lamps" class="tasklink"><a',
    );
  });
});

describe("a presentation", () => {
  it("applies the attributes the client knows, its tip as its title when it has none", () => {
    expect(
      attributes({
        class: "a",
        id: "b",
        attributes: { "data-tip": "Tip", tabindex: "0", style: "color: red !important" },
      }),
    ).toEqual({
      "data-tip": "Tip",
      tabIndex: "0",
      // The style is written as given, by its ref.
      ref: writtenStyle("color: red !important"),
      title: "Tip",
      className: "a",
      id: "b",
    });
    expect(attributes({ title: "Own", attributes: { "data-tip": "Tip" } })).toEqual({
      "data-tip": "Tip",
      title: "Own",
    });
    expect(attributes({})).toEqual({});
  });

  it("says each attribute it does not apply, never applying it", () => {
    expect(unsupported({ onclick: "x()", width: "3", constructor: "y" })).toEqual([
      "onclick=x()",
      "constructor=y",
    ]);
    const html = drawn([
      {
        type: "container",
        tag: "div",
        presentation: { attributes: { onclick: "x()", width: "3" } },
        blocks: [],
      },
    ]);
    expect(html).toBe(
      '<div width="3"></div><aside class="unknown-block" role="alert"><strong>Unsupported ' +
        "document content: attributes</strong><p>Attributes: <code>onclick=x()</code></p></aside>",
    );
  });
});

describe("a graphic", () => {
  it("is its SVG with its shapes, saying each attribute it does not apply", () => {
    const html = renderToStaticMarkup(
      <SpansView
        spans={[
          {
            type: "graphic",
            view_box: "0 0 10 10",
            presentation: { class: "chart", attributes: { role: "img" } },
            shapes: [
              { kind: "rect", attributes: { width: "5", height: "5", "stroke-width": "2" } },
              { kind: "circle", attributes: { r: "1", onload: "x()" } },
            ],
          },
        ]}
      />,
    );
    expect(html).toBe(
      '<svg viewBox="0 0 10 10" role="img" class="chart"><rect width="5" height="5" ' +
        'stroke-width="2"></rect><circle r="1"></circle></svg><aside class="unknown-block" ' +
        'role="alert"><strong>Unsupported document content: attributes</strong><p>Attributes: ' +
        "<code>onload=x()</code></p></aside>",
    );
  });
});

describe("blocks inside a line of text", () => {
  it("are drawn where the line holds them", () => {
    expect(
      renderToStaticMarkup(
        createElement(SpansView, {
          spans: [{ type: "blocks", blocks: [paragraph("x")] }, { type: "line-break" }],
        }),
      ),
    ).toBe("<p>x</p><br/>");
  });
});

// The kit's lists of kinds grow in a minor (docs/engineering/PLUGIN_PROTOCOL.md), so a newer server may send
// this client a kind its types do not know, which the read lets through (`blocks-read.ts`).
describe("a kind the client does not know", () => {
  const later = (value: object) => value as never;
  const SAID =
    '<aside class="unknown-block" role="alert"><strong>Unsupported document content: ' +
    "marquee</strong></aside>";

  it("is said where it stands, by its name, and draws none of its fields", () => {
    expect(drawn([later({ type: "marquee", text: "Scrolling", blocks: [paragraph("in")] })])).toBe(
      SAID,
    );
    expect(
      renderToStaticMarkup(<SpansView spans={[later({ type: "blink", text: "old" })]} />),
    ).toBe('<span class="unknown-block" role="alert">Unsupported blink</span>');
    expect(drawn([{ type: "media", media: later({ kind: "sound", src: "/a.ogg" }) }])).toBe(
      '<span class="unknown-block" role="alert">Unsupported media: sound</span>',
    );
  });

  it("leaves what stands around it drawn", () => {
    expect(drawn([paragraph("before"), later({ type: "marquee" }), paragraph("after")])).toBe(
      `<p>before</p>${SAID}<p>after</p>`,
    );
    expect(
      drawn([
        {
          type: "paragraph",
          spans: [text("a "), later({ type: "blink" }), text(" b")],
        },
      ]),
    ).toBe('<p>a <span class="unknown-block" role="alert">Unsupported blink</span> b</p>');
    expect(drawn([{ type: "quote", blocks: [later({ type: "marquee" })] }])).toBe(
      `<blockquote>${SAID}</blockquote>`,
    );
  });

  it("is said whatever stands for its name, and never throws", () => {
    expect(drawn([later({})])).toContain("Unsupported document content: unnamed");
    expect(drawn([later({ type: "" })])).toContain("Unsupported document content: </strong>");
    // An object with no text of its own, which `String` would throw on.
    expect(drawn([later({ type: { toString: null } })])).toContain(
      "Unsupported document content: unnamed",
    );
    expect(renderToStaticMarkup(<SpansView spans={[later({ type: 3 })]} />)).toBe(
      '<span class="unknown-block" role="alert">Unsupported unnamed</span>',
    );
    expect(drawn([{ type: "media", media: later({}) }])).toContain("Unsupported media: unnamed");
  });
});

// The server names an element of a document it cannot draw by its tag alone and sends what the
// element holds after it (`crates/symphony/src/blocks.rs`): the notice holds no words of its own,
// so the element's are drawn once.
describe("an unsupported element named alone", () => {
  it("is its notice with no empty paragraph, what it holds drawn once after it", () => {
    const html = drawn([
      { type: "unknown", tag: "widget", text: "" },
      { type: "group", blocks: [paragraph("visible")] },
    ]);
    expect(html).toBe(
      '<aside class="unknown-block" role="alert"><strong>Unsupported document content: ' +
        "widget</strong></aside><p>visible</p>",
    );
    expect(html.match(/visible/g)).toHaveLength(1);
  });

  it("still draws the words a notice carries", () => {
    expect(drawn([{ type: "unknown", tag: "p@surprise", text: "yes" }])).toBe(
      '<aside class="unknown-block" role="alert"><strong>Unsupported document content: ' +
        "p@surprise</strong><p>yes</p></aside>",
    );
  });
});
