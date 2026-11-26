import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  bothShown,
  DocumentBody,
  DocumentPage,
  Excerpt,
  ExcerptBody,
  FirstDocument,
  FolderPage,
  GhostDocument,
  HtmlPage,
  HydratedSection,
  Missing,
  movedAddress,
  readMoved,
  SANDBOX,
  Versions,
} from "./document-page";
import { CardOwnersContext } from "./owner";
import { memoryStorage } from "./testing";
import type { Block, Document, DocumentCard, Folder, Section } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
});

const paragraph = (text: string): Block => ({ type: "paragraph", spans: [{ type: "text", text }] });

/** A section `key` from line `start`, its body `blocks` (none read yet when empty). */
const section = (key: string, start: number, blocks: Block[] = []): Section => ({
  start,
  end: start + 1,
  key,
  hash: `${key}-hash`,
  part: null,
  blocks,
});

const card = (slug: string, group: string, state = "pending"): DocumentCard => ({
  slug,
  state,
  text: `${slug}, whole`,
  group,
  open: true,
});

const DOCUMENT: Document = {
  version: 3,
  path: "docs/LAMPS.md",
  title: "Lamps",
  waiting: [],
  crumbs: [],
  headings: [
    { level: 2, anchor: "colour", text: "Colour" },
    { level: 2, anchor: "size", text: "Size" },
    { level: 3, anchor: "base", text: "Base" },
  ],
  sections: [section("colour", 1, [paragraph("Warm white.")]), section("card:lamp", 4)],
  cards: [card("lamp", "decide")],
  threads: {},
  tasks: [],
};

describe("an HTML document", () => {
  it("is drawn in a frame that runs its scripts alone, from its file's address", () => {
    for (const [scope, src] of [
      ["", "/docs/analysis/readout.html?raw"],
      ["feature-x", "/@feature-x/docs/analysis/readout.html?raw"],
    ] as const) {
      const html = renderToStaticMarkup(
        <HtmlPage path="docs/analysis/readout.html" scope={scope} />,
      );
      expect(html).toContain('<div class="document-page html-document" data-ready="html">');
      expect(html).toContain('<p class="eyebrow">docs/analysis/readout.html</p>');
      expect(html).toContain(`src="${src}"`);
      expect(html).toContain('sandbox="allow-scripts"');
      expect(html).toContain('title="readout.html"');
      expect(html).toContain('referrerPolicy="no-referrer"');
      expect(html.match(/sandbox=/g)).toHaveLength(1);
      for (const allowed of ["same-origin", "top-navigation", "forms", "popups", "modals"]) {
        expect(html).not.toContain(allowed);
      }
    }
    expect(SANDBOX).toBe("allow-scripts");
  });
});

describe("a document before its read", () => {
  it("is its head, its outline and its body's card where they will be", () => {
    const html = renderToStaticMarkup(<GhostDocument toc head />);
    expect(html).toMatch(/^<div class="ghost-doc"><div class="ghost-doc-head">/);
    expect(html).toContain('<div class="ghost-title"><span class="bone" style="--w:58%">');
    expect(html.match(/<div><span class="bone"><\/span><\/div>/g)).toHaveLength(7);
    expect(html).toContain('<div class="ghost-doc-body">');
    const intro = renderToStaticMarkup(<GhostDocument toc={false} head={false} />);
    expect(intro).not.toContain("ghost-doc-head");
    expect(intro).not.toContain("ghost-toc");
    expect(intro).toContain("ghost-doc-body");
  });

  it("says it is loading, under the ghost of the page", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    const html = renderToStaticMarkup(<DocumentPage path="docs/LAMPS.md" scope="" />);
    expect(html).toContain('<div class="document-page" data-ready="loading" data-feed="stale"');
    expect(html).toContain('aria-busy="true"');
    expect(html).toContain('<p role="status" class="status-text">Loading document…</p>');
    expect(html).toContain('<div class="ghost-doc"><div class="ghost-doc-head">');
    const intro = renderToStaticMarkup(<DocumentPage path="docs/LAMPS.md" scope="" embedded />);
    expect(intro).toContain("Loading document…");
    expect(intro).not.toContain("ghost-doc-head");
  });
});

describe("a document no file has at its route", () => {
  it("reads the routes of its name the server answers, each a path of the app, and nothing else", () => {
    expect(readMoved(["docs/workflow/RUNS.md", "docs/product/RUNS.md"])).toEqual([
      "docs/workflow/RUNS.md",
      "docs/product/RUNS.md",
    ]);
    expect(
      readMoved(["docs/A.md", 3, null, "/evil.example/A.md", "docs\\A.md", "docs/\nA.md"]),
    ).toEqual(["docs/A.md"]);
    for (const answer of [{ moved: ["docs/A.md"] }, "docs/A.md", null, undefined]) {
      expect(readMoved(answer)).toEqual([]);
    }
  });

  it("opens the document of its name at its route in the checkout, the heading kept", () => {
    expect(movedAddress("", "docs/operations/analysis/board-pg-cutover.md", "#The%20order")).toBe(
      "/docs/operations/analysis/board-pg-cutover.md#The%20order",
    );
    expect(movedAddress("feature-x", "docs/A.md", "")).toBe("/@feature-x/docs/A.md");
  });

  it("says it is not found, with nothing to choose when no document or one has its name", () => {
    for (const moved of [[], ["docs/area/GONE.md"]]) {
      const html = renderToStaticMarkup(
        <Missing path="docs/GONE.md" scope="" moved={moved} hash="" retry={() => {}} />,
      );
      expect(html).toContain("<h1>Document not found</h1>");
      expect(html).toContain("<p>There is no document at docs/GONE.md.</p>");
      expect(html).toContain('<a class="trouble-back" href="/docs/">');
      expect(html).toContain(">Retry</button>");
      expect(html).not.toContain("trouble-choices");
    }
  });

  it("offers each document of a name documents share, in the checkout, the heading kept", () => {
    const html = renderToStaticMarkup(
      <Missing
        path="docs/RUNS.md"
        scope="feature-x"
        moved={["docs/product/RUNS.md", "docs/workflow/RUNS.md"]}
        hash="#the-runs-page"
        retry={() => {}}
      />,
    );
    expect(html).toContain("<h1>Document not found</h1>");
    expect(html).toContain(
      '<p>It may be one of the documents named RUNS.md:</p><ul class="trouble-choices"><li><a href="/@feature-x/docs/product/RUNS.md#the-runs-page">docs/product/RUNS.md</a></li><li><a href="/@feature-x/docs/workflow/RUNS.md#the-runs-page">docs/workflow/RUNS.md</a></li></ul><p class="trouble-actions">',
    );
    expect(html).toContain('<a class="trouble-back" href="/@feature-x/docs/">');
  });
});

describe("a folder before its read", () => {
  it("says it is loading", () => {
    const html = renderToStaticMarkup(<FolderPage path="docs" scope="" />);
    expect(html).toContain('<div class="document-page" data-ready="loading" data-feed="live">');
    expect(html).toContain('<p role="status">Loading folder…</p>');
  });
});

describe("a folder with no document", () => {
  const folder: Folder = {
    path: "docs",
    title: "Docs",
    crumbs: [],
    docs: 0,
    intro: null,
    entries: [],
  };
  it("says in one line that none is here, with the guide's link", () => {
    const html = renderToStaticMarkup(<FirstDocument folder={folder} />);
    expect(html).toContain('<p class="first-steps">No document here yet. <a href="');
    expect(html).toContain('/docs/operations/GETTING_STARTED.md" target="_blank"');
  });
  it("says nothing once it lists a document or a folder, or draws an intro", () => {
    const entry = { kind: "doc", path: "docs/A.md", title: "A", docs: 0, waiting: 0, summary: "" };
    expect(renderToStaticMarkup(<FirstDocument folder={{ ...folder, entries: [entry] }} />)).toBe(
      "",
    );
    expect(renderToStaticMarkup(<FirstDocument folder={{ ...folder, intro: DOCUMENT }} />)).toBe(
      "",
    );
  });
});

describe("a document drawn", () => {
  const body = (document: Document, scope = "", embedded = false) =>
    renderToStaticMarkup(
      <DocumentBody
        document={document}
        latest={document}
        scope={scope}
        anchors={{ colour: ["colour"] }}
        embedded={embedded}
        failed={false}
        postponed={[]}
      />,
    );

  it("is its head, its outline of three headings or more, and its body under its ghost", () => {
    const html = body(DOCUMENT);
    expect(html).toContain(
      '<div class="doc-head"><p class="eyebrow">docs/LAMPS.md</p><h1>Lamps</h1>',
    );
    expect(html).toContain('<nav class="doc-tools" aria-label="Document tools"><a href="/">');
    expect(html).toContain(">All docs</a>");
    expect(html).toContain("Versions and diffs");
    expect(html).toContain('<nav class="doc-toc" aria-label="On this page"><details open="">');
    expect(html).toContain('data-settled="false"');
    expect(html).toContain('<article class="doc-body">');
    // Each section waits in its box until it comes near the screen, its anchors and line held.
    expect(html).toContain('<span id="colour"></span><span id="L1"></span>');
    expect(html).toContain('<span id="L4"></span>');
    expect(html).toContain('style="contain-intrinsic-size:auto 100px;min-height:100px"');
  });

  it("of two headings has no outline, and an intro no head", () => {
    const few = { ...DOCUMENT, headings: DOCUMENT.headings.slice(0, 2) };
    expect(body(few)).not.toContain("doc-toc");
    const intro = body(DOCUMENT, "", true);
    expect(intro).not.toContain("doc-head");
    expect(intro).not.toContain("doc-toc");
  });

  it("of a checkout names its path without the checkout, and its comparison's task", () => {
    const html = body(
      {
        ...DOCUMENT,
        path: "@design~policy/docs/LAMPS.md",
        comparison: { task: "design/policy" },
      },
      "design~policy",
    );
    expect(html).toContain('<p class="eyebrow">docs/LAMPS.md</p>');
    expect(html).toContain('href="/@design~policy/docs/"');
    expect(html).toContain('<a href="/task/design/policy">design/policy</a>');
  });
});

describe("a section", () => {
  const drawn = (props: Partial<Parameters<typeof HydratedSection>[0]> = {}) =>
    renderToStaticMarkup(
      <HydratedSection
        section={section("colour", 1, [paragraph("Warm white.")])}
        path="docs/LAMPS.md"
        scope=""
        card={null}
        thread={null}
        gone={false}
        ids={["colour"]}
        postponed={false}
        {...props}
      />,
    );

  it("is its prose, with a comment of its own", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    const html = drawn();
    expect(html).toContain(
      '<section class="doc-section" id="L1" data-doc-key="colour" data-start="1" data-end="2" data-busy="false"><div class="section-prose"><p>Warm white.</p></div>',
    );
    expect(html).toContain('id="opener-docs%2FLAMPS.md%3Acolour"');
    expect(html).toContain(">+ comment</button>");
    expect(html).not.toContain('role="alert"');
  });

  it("not read yet is its ghost, holding its anchors", () => {
    const html = drawn({ section: section("colour", 1) });
    expect(html).toMatch(
      /^<div class="section-loading" id="L1" data-pending="true"><span id="colour"><\/span><div class="ghost-heading">/,
    );
    const ghost = drawn({ section: section("card:lamp", 0), card: card("lamp", "decide") });
    expect(ghost).toMatch(
      /^<div class="section-loading" data-pending="true"><span id="colour"><\/span><div class="ghost-box">/,
    );
  });

  it("of a card waiting on the owner offers its verdict, and its Later", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    const html = drawn({
      section: section("card:lamp", 4, [paragraph("Which lamp?")]),
      card: card("lamp", "decide"),
      postponed: true,
    });
    expect(html).toContain(
      '<section class="doc-section doc-card" id="L4" data-state="pending" data-doc-key="card:lamp"',
    );
    expect(html).toContain('<small class="postponed">postponed</small>');
    expect(html).toContain(">Verdict / message</button>");
    expect(html).toContain(">Undo Later</button>");
    expect(html).toContain('<form class="verdict" hidden=""');
  });

  it("of a card says whose it is where the page read its owners, and nothing before", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    const props = {
      section: section("card:lamp", 4, [paragraph("Which lamp?")]),
      card: card("lamp", "decide"),
    };
    expect(drawn(props)).not.toContain('class="owner"');
    const owner = {
      name: "design",
      kind: "group",
      by: "named",
      said: "named on it",
      hand_to: ["engineering"],
      decides: true,
    };
    const owned = renderToStaticMarkup(
      <CardOwnersContext.Provider value={{ owners: { lamp: owner }, hand: async () => ({}) }}>
        <HydratedSection
          {...props}
          path="docs/LAMPS.md"
          scope=""
          thread={null}
          gone={false}
          ids={["colour"]}
          postponed={false}
        />
      </CardOwnersContext.Provider>,
    );
    expect(owned).toContain(
      '<div class="doc-actions"><span class="owner" data-by="named"><span class="owner-name">group: design</span><small class="owner-by">named on it</small>',
    );
    expect(owned).toContain('id="hand-docs%2FLAMPS.md%3Alamp"');
    expect(owned).toContain(">Verdict / message</button>");
    expect(owned).toContain(">✓ Approve</button>");
    expect(owned).toContain(">Later</button>");
  });

  it("of a card its reader does not decide offers its message alone: no verdict and no Later", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    const owner = {
      name: "design",
      kind: "group",
      by: "map",
      said: "from the folder docs",
      hand_to: [],
      decides: false,
    };
    const theirs = renderToStaticMarkup(
      <CardOwnersContext.Provider value={{ owners: { lamp: owner }, hand: async () => ({}) }}>
        <HydratedSection
          section={section("card:lamp", 4, [paragraph("Which lamp?")])}
          card={card("lamp", "decide")}
          path="docs/LAMPS.md"
          scope=""
          thread={null}
          gone={false}
          ids={["colour"]}
          postponed={false}
        />
      </CardOwnersContext.Provider>,
    );
    expect(theirs).toContain("group: design");
    expect(theirs).toContain(">Message</button>");
    expect(theirs).not.toContain("Verdict / message");
    expect(theirs).toContain('aria-label="Message"');
    expect(theirs).toContain(">Send message</button>");
    for (const control of ["Approve", "Dismiss", "Reset to pending", "verdict-choices", "Later"]) {
      expect(theirs).not.toContain(control);
    }
  });

  it("of a settled card is folded under its summary until opened", () => {
    const summary = [{ type: "text" as const, text: "lamp · approved" }];
    const html = drawn({
      section: section("card:lamp", 4, [
        { type: "settled-card", summary, open: false, blocks: [paragraph("Which lamp?")] },
      ]),
      card: card("lamp", "resolved", "approved"),
      ids: ["lamp", "colour"],
    });
    expect(html).toMatch(
      /^<div class="doc-section folded-slot" id="L4"><details class="settled" id="lamp"><summary>lamp · approved<span id="colour"><\/span><\/summary>/,
    );
    expect(html).not.toContain("data-doc-key");
    const plain = drawn({
      section: section("card:lamp", 4),
      card: card("lamp", "rejected", "rejected"),
      ids: [],
    });
    expect(plain).toContain("<summary><code>lamp</code> · rejected · lamp, whole</summary>");
  });

  it("removed on a branch is main's blocks, read-only, under its label", () => {
    const html = drawn({
      section: {
        ...section("colour", 1, [paragraph("Old")]),
        comparison: { kind: "removed", before: [] },
      },
    });
    expect(html).toContain('data-difference="removed"');
    expect(html).toContain("Old");
    expect(html).not.toContain("+ comment");
  });

  it("gone from the latest copy says its draft cannot be saved there", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    expect(drawn({ gone: true })).toContain(
      '<p role="alert">This block was removed elsewhere. Copy your draft or discard it; saving to its old position is disabled.</p>',
    );
  });
});

describe("a link's excerpt", () => {
  it("is its title over its read, loading first", () => {
    const html = renderToStaticMarkup(
      <Excerpt target={{ path: "/docs/NOTES.md", anchor: "open" }} />,
    );
    expect(html).toBe(
      '<div class="excerpt-box" role="region" aria-label="Excerpt of /docs/NOTES.md#open" data-busy="true"><p class="excerpt-title">/docs/NOTES.md#open</p><p role="status">Loading the excerpt…</p></div>',
    );
  });

  it("says a read that failed, and a section with nothing in it", () => {
    expect(renderToStaticMarkup(<ExcerptBody read={{ error: "Not found" }} />)).toBe(
      '<p role="alert">Not found</p>',
    );
    expect(renderToStaticMarkup(<ExcerptBody read={{ fragment: { blocks: [] } }} />)).toBe(
      "<p>This section is empty.</p>",
    );
  });

  it("draws every block it read at once, where the browser could wait for each to come near", () => {
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        observe() {}
        disconnect() {}
        takeRecords() {
          return [];
        }
      },
    );
    const item: Block[] = [paragraph("Is it red?")];
    const html = renderToStaticMarkup(
      <ExcerptBody
        read={{
          fragment: {
            blocks: [
              { type: "list", ordered: true, start: 1, items: [item] },
              { type: "details", open: true, summary: [], blocks: [paragraph("Folded words.")] },
              {
                type: "table",
                head: [],
                rows: [
                  [{ header: false, spans: [{ type: "text", text: "A row" }], presentation: {} }],
                ],
              },
            ],
          },
        }}
      />,
    );
    expect(html).toContain("<p>Is it red?</p>");
    expect(html).toContain("<p>Folded words.</p>");
    expect(html).toContain("<td>A row</td>");
    expect(html).not.toContain("viewport-sentinel");
    expect(html).not.toContain('data-pending="true"');
  });
});

describe("a document's versions", () => {
  it("are read once their fold is opened", () => {
    expect(renderToStaticMarkup(<Versions path="docs/LAMPS.md" scope="" />)).toBe(
      '<details class="versions"><summary>Versions and diffs</summary></details>',
    );
  });
});

describe("a folder's notices", () => {
  it("say the oldest copy this device drew, unreachable when either is", () => {
    expect(bothShown(null, null)).toBeNull();
    expect(bothShown([5, false], null)).toEqual([5, false]);
    expect(bothShown(null, [7, true])).toEqual([7, true]);
    expect(bothShown([5, false], [3, false])).toEqual([3, false]);
    expect(bothShown([5, true], [9, false])).toEqual([5, true]);
  });
});
