import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DocumentPicture,
  elementHtml,
  encoded,
  FileViewer,
  MediaView,
  pictureRef,
  styleRefs,
  writtenStyle,
} from "./media";
import {
  decode,
  isMockup,
  label,
  localAsset,
  MockupFrame,
  PictureView,
  WAITING,
} from "./media-picture";
import { CASES } from "./testing";
import type { Image, Media, Viewer } from "./wire";

const VIEWER: Viewer = {
  element: "orbit-view",
  script: "/static/orbit.js",
  attributes: [["controls", ""]],
  views: [
    {
      label: "Above",
      title: "From above",
      set: [
        ["angle", "0deg 25deg"],
        ["zoom", "2"],
      ],
      call: "settle",
    },
  ],
  download: ".item",
  open_label: null,
  close_label: null,
};

beforeEach(() => {
  // A page with no project element: the plain project, whose links are as written.
  vi.stubGlobal("document", { getElementById: () => null });
});

afterEach(() => vi.unstubAllGlobals());

describe("what crates/symphony-blocks writes for a viewer", () => {
  it.each(CASES.elementHtml)("draws %j's element for %j", (viewer, src, caption, poster, html) => {
    expect(elementHtml(viewer, src, caption, poster)).toBe(html);
  });

  it.each(CASES.encoded)("encodes %j's attributes as %j", (view, value) => {
    expect(encoded(view)).toBe(value);
  });

  it.each(CASES.decode)("reads %j back as View::decode does", (value, pairs) => {
    expect(decode(value)).toEqual(pairs);
  });

  it.each(CASES.encoded)(
    "reads what it encodes for %j back as the pairs it kept",
    (view, value) => {
      const kept = view.set.filter(
        ([name, value]) => /^[a-z][a-z0-9-]*$/.test(name) && !/[\n\r]/.test(value),
      );
      expect(decode(value)).toEqual(kept);
    },
  );
});

describe("media a frame or a viewer runs", () => {
  it("is the project's own file alone", () => {
    for (const src of ["/ui/hud.html", "/@topic/model.item?v=2#view", "/a/b.HTM"]) {
      expect(localAsset(src), src).toBe(true);
    }
    for (const src of [
      "",
      "ui/hud.html",
      "//example.com/a.html",
      "https://example.com/a.html",
      "javascript:alert(1)",
      "data:text/html,hi",
      "/\\example.com/a.html",
      "/a\n.html",
      "/a\u0000.item",
      "/a\u007f.item",
      "/a\u0085.item",
    ]) {
      expect(localAsset(src), JSON.stringify(src)).toBe(false);
    }
    expect(localAsset("/a b.item")).toBe(true);
  });

  it("is a mockup by its path's extension, and is called by its caption or its file", () => {
    for (const src of ["/a.html", "/a.HTM?version=1", "/a.HTML#view"]) {
      expect(isMockup(src), src).toBe(true);
    }
    for (const src of ["/a.png?x=.html", "/a.item#x.html", "/html", ""]) {
      expect(isMockup(src), src).toBe(false);
    }
    expect(label("/ui/hud.html?v=2#frame", "")).toBe("hud.html");
    expect(label("/ui/hud.html", "Flight HUD")).toBe("Flight HUD");
  });
});

describe("a mockup", () => {
  it("runs in a frame that allows its scripts alone, linked full size", () => {
    const html = renderToStaticMarkup(<PictureView src="/ui/hud.HTML?v=2" caption="Flight HUD" />);
    for (const part of [
      'class="mockup-shell"',
      'sandbox="allow-scripts"',
      'width="1280"',
      'height="800"',
      'tabindex="-1"',
      'referrerPolicy="no-referrer"',
      'rel="noopener noreferrer"',
      'aria-label="Open Flight HUD full size"',
      '<span class="mockup-bar"><span>Flight HUD</span>',
    ]) {
      expect(html).toContain(part);
    }
    expect(html).not.toContain("allow-same-origin");
  });

  it("as a thumbnail has no bar and no link over it", () => {
    const html = renderToStaticMarkup(<MockupFrame src="/ui/hud.html" caption="" thumb />);
    expect(html).toContain('class="mockup-shell thumb"');
    expect(html).toContain('class="mockup mockup-thumb"');
    expect(html).toContain('title="hud.html"');
    expect(html).not.toContain("mockup-open");
    expect(html).not.toContain("mockup-bar");
  });

  it("loads its file's address and links its route", () => {
    const html = renderToStaticMarkup(
      <MockupFrame src="/docs/board/a.html?state=b#x" caption="Board" />,
    );
    expect(html).toContain('src="/docs/board/a.html?state=b&amp;raw#x"');
    expect(html.match(/href="\/docs\/board\/a\.html\?state=b#x"/g)).toHaveLength(2);
  });

  it("is not framed when it is no project's HTML file", () => {
    for (const src of ["/picture.svg", "https://x/a.html", "//x/a.html"]) {
      expect(renderToStaticMarkup(<MockupFrame src={src} caption="bad" />), src).toBe("");
    }
  });
});

describe("a board's picture", () => {
  it("is a square picture, lazily loaded", () => {
    const html = renderToStaticMarkup(<PictureView src="/picture.png" caption="Picture" thumb />);
    expect(html).toBe(
      '<span class="picture-shell thumb"><img class="media-picture" src="/picture.png" ' +
        'alt="Picture" width="540" height="540" loading="lazy" decoding="async"/></span>',
    );
  });

  it("keeps its box empty until it comes into view", () => {
    vi.stubGlobal("IntersectionObserver", class {});
    expect(renderToStaticMarkup(<PictureView src="/picture.png" caption="Picture" />)).toBe(
      '<span class="picture-shell"></span>',
    );
  });
});

describe("a document's picture", () => {
  const image: Image = {
    src: "/thumb.png",
    alt: "Lamp",
    presentation: {
      id: "picture",
      attributes: {
        width: "640",
        height: "480",
        "data-full": "/original.png",
        style: "max-width: 100%; --tint: red",
        tabindex: "0",
        "aria-label": "A lamp",
      },
    },
  };

  it("keeps the server's sizes, original and presentation", () => {
    const html = renderToStaticMarkup(<DocumentPicture image={image} />);
    expect(html).not.toContain("data:image");
    for (const part of [
      'src="/thumb.png"',
      'alt="Lamp"',
      'id="picture"',
      'width="640"',
      'height="480"',
      'data-full="/original.png"',
      'loading="lazy"',
      'decoding="async"',
      'tabindex="0"',
      'aria-label="A lamp"',
    ]) {
      expect(html).toContain(part);
    }
  });

  it("holds the empty picture until it comes into view", () => {
    vi.stubGlobal("IntersectionObserver", class {});
    const html = renderToStaticMarkup(<DocumentPicture image={image} />);
    expect(html).toContain(`src="${WAITING.replaceAll("'", "&#x27;")}"`);
    expect(html).toContain('width="640"');
  });

  it("loads lazily unless the server says otherwise", () => {
    const eager: Image = {
      src: "/a.png",
      alt: "",
      presentation: { attributes: { loading: "eager", decoding: "sync" } },
    };
    const html = renderToStaticMarkup(<DocumentPicture image={eager} />);
    expect(html).toContain('loading="eager"');
    expect(html).toContain('decoding="sync"');
    expect(html).not.toContain("data-full");
  });
});

describe("a file in its viewer", () => {
  it("is the element, its views and its download, drawn from the viewer alone", () => {
    const html = renderToStaticMarkup(
      <FileViewer src="/a.item" caption="Item" viewer={VIEWER} poster="/a.png" />,
    );
    for (const part of [
      '<span class="viewer-stage" data-viewer-script="/static/orbit.js">',
      '<orbit-view src="/a.item" alt="Item" controls="" poster="/a.png"></orbit-view>',
      'data-viewer-set="angle=0deg 25deg\nzoom=2"',
      'data-viewer-call="settle"',
      'title="From above"',
      ">Above</button>",
      '<a href="/a.item" download="a.item">.item</a>',
    ]) {
      expect(html).toContain(part);
    }
  });

  it.each([
    ["/models/final.item", "final.item"],
    ["/models/final.item?v=/other.item", "final.item"],
    ["/models/final.item#view/other.item", "final.item"],
    ["/models/final.item?v=2#view", "final.item"],
    ["/models/模型 final", "模型 final"],
    ['/models/a & "b".item', "a &amp; &quot;b&quot;.item"],
  ])("downloads %s under its file name", (src, filename) => {
    const html = renderToStaticMarkup(
      <FileViewer src={src} caption="A different caption" viewer={VIEWER} />,
    );
    expect(html).toContain(`download="${filename}">.item</a>`);
  });

  it("draws nothing for a foreign file or script, or an element that is no custom element", () => {
    for (const src of ["//external/a.item", "https://external/a.item"]) {
      expect(renderToStaticMarkup(<FileViewer src={src} caption="" viewer={VIEWER} />)).toBe("");
    }
    const foreign = { ...VIEWER, script: "https://external/orbit.js" };
    expect(renderToStaticMarkup(<FileViewer src="/a.item" caption="" viewer={foreign} />)).toBe("");
    const bad = { ...VIEWER, element: "script" };
    expect(renderToStaticMarkup(<FileViewer src="/a.item" caption="" viewer={bad} />)).toBe("");
  });

  it("leaves out a foreign poster, and buttons and a link it was not given", () => {
    const html = renderToStaticMarkup(
      <FileViewer src="/a.item" caption="" viewer={VIEWER} poster="https://x/a.png" />,
    );
    expect(html).toContain('<orbit-view src="/a.item" alt="a.item" controls=""></orbit-view>');
    const plain = { ...VIEWER, views: [], download: null };
    const bare = renderToStaticMarkup(<FileViewer src="/a.item" caption="" viewer={plain} />);
    expect(bare).not.toContain("<a ");
    expect(bare).not.toContain("<button");
  });
});

describe("a gallery", () => {
  const gallery: Media = {
    kind: "gallery",
    items: [
      { name: "FIRST", src: "/first.item", poster: "/first.png" },
      { name: "SECOND", src: "/second.item", poster: null },
    ],
    viewer: VIEWER,
  };

  it("is a card per file, none open, so no viewer is drawn", () => {
    const html = renderToStaticMarkup(<MediaView media={gallery} />);
    expect(html).toContain('<div class="gallery-grid">');
    expect(html.match(/<article class="gallery-card" data-busy="false">/g)).toHaveLength(2);
    expect(html).toContain(
      '<button class="dx-button gallery-pick" data-style="outline" data-size="default" ' +
        'type="button" aria-expanded="false" aria-controls="viewer-FIRST">',
    );
    expect(html).toContain(
      '<img src="/first.png" width="80" height="80" alt="FIRST" loading="lazy" data-no-lightbox="true"/>',
    );
    expect(html.match(/<small>Open<\/small>/g)).toHaveLength(2);
    expect(html).not.toContain("<orbit-view");
    expect(html).not.toContain("/first.item");
  });

  it("says what its viewer names for opening a file", () => {
    const named = { ...gallery, viewer: { ...VIEWER, open_label: "Take it down" } };
    const html = renderToStaticMarkup(<MediaView media={named} />);
    expect(html).toContain("<small>Take it down</small>");
    expect(html).not.toContain(">Open<");
  });

  it("without a viewer, as an older server sends it, opens none", () => {
    const { viewer: _, ...bare } = gallery;
    const html = renderToStaticMarkup(<MediaView media={bare as Media} />);
    expect(html.match(/<small>Open<\/small>/g)).toHaveLength(2);
  });
});

describe("media as the blocks hold them", () => {
  it("draws a mockup in its frame and a file in its viewer", () => {
    const mockup = renderToStaticMarkup(
      <MediaView media={{ kind: "mockup", src: "/ui/a.html", label: "Frame" }} />,
    );
    expect(mockup).toContain("<iframe");
    const viewer = renderToStaticMarkup(
      <MediaView media={{ kind: "viewer", src: "/model.item", label: "Model", viewer: VIEWER }} />,
    );
    expect(viewer).toContain('<orbit-view src="/model.item" alt="Model"');
  });
});

describe("a style", () => {
  /**
   * An element as a ref sees it: the attributes written on it, by name, and its style's
   * properties, read from its style attribute (`name: value`, by `;`) and written back to it as a
   * browser writes them, each `name: value;`.
   */
  function element(own: [string, string][] = []) {
    const attributes = new Map<string, string>();
    const properties = new Map<string, string>();
    const serialize = () =>
      attributes.set(
        "style",
        [...properties].map(([name, value]) => `${name}: ${value};`).join(" "),
      );
    const parse = (css: string) => {
      properties.clear();
      for (const [name, value] of css.split(";").map((part) => part.split(":"))) {
        if (name !== undefined && value !== undefined) properties.set(name.trim(), value.trim());
      }
    };
    for (const [name, value] of own) properties.set(name, value);
    if (own.length > 0) serialize();
    const node = {
      getAttribute: (name: string) => attributes.get(name) ?? null,
      setAttribute: (name: string, value: string) => {
        attributes.set(name, value);
        if (name === "style") parse(value);
      },
      removeAttribute: (name: string) => {
        attributes.delete(name);
        if (name === "style") properties.clear();
      },
      style: {
        get length() {
          return properties.size;
        },
        item: (at: number) => [...properties.keys()][at] ?? "",
        getPropertyValue: (name: string) => properties.get(name) ?? "",
        setProperty: (name: string, value: string) => {
          properties.set(name, value);
          serialize();
        },
      },
    };
    return { node: node as unknown as Element, attributes, properties };
  }

  it("is written as given, important values and comments kept, and taken away with its element", () => {
    const css = "color: red !important; /* a; b */ width: 3em";
    const { node, attributes } = element();
    const undo = writtenStyle(css)(node);
    expect(attributes.get("style")).toBe(css);
    undo?.();
    expect(attributes.has("style")).toBe(false);
  });

  it("keeps each property the element had that it does not set, as the Dioxus client does", () => {
    const { node, attributes, properties } = element([
      ["text-align", "center"],
      ["outline", "none"],
    ]);
    const before = attributes.get("style");
    const undo = writtenStyle("outline: 1px dashed red; color: blue")(node);
    expect([...properties]).toEqual([
      ["outline", "1px dashed red"],
      ["color", "blue"],
      ["text-align", "center"],
    ]);
    expect(attributes.get("style")).toBe(
      "outline: 1px dashed red; color: blue; text-align: center;",
    );
    // Drawn without it, or with another, the element's style is as it was before.
    undo?.();
    expect(attributes.get("style")).toBe(before);
    expect([...properties]).toEqual([
      ["text-align", "center"],
      ["outline", "none"],
    ]);
  });

  it("is the same ref each time for the same words, and none for no element", () => {
    expect(writtenStyle("color: red")).toBe(writtenStyle("color: red"));
    expect(writtenStyle("color: red")).not.toBe(writtenStyle("color: blue"));
    expect(writtenStyle("color: red")(null)).toBeUndefined();
  });

  it("keeps its ref while an element holds it, and lets it go once none does", () => {
    const before = styleRefs();
    const ref = writtenStyle("width: 7px");
    const [first, second] = [element(), element()];
    const undoFirst = ref(first.node);
    const undoSecond = ref(second.node);
    expect(writtenStyle("width: 7px")).toBe(ref);
    undoFirst?.();
    // The second element holds it still: drawn again, it is let be.
    expect(writtenStyle("width: 7px")).toBe(ref);
    undoSecond?.();
    expect(styleRefs()).toBe(before);
    // A style drawn again later takes a ref of its own; the one let go, held again, is kept again.
    expect(writtenStyle("width: 7px")).not.toBe(ref);
    const again = ref(first.node);
    expect(writtenStyle("width: 7px")).toBe(ref);
    again?.();
    expect(styleRefs()).toBe(before);
  });

  it("is written with the picture's watch by its one ref, and both undone together", () => {
    const watched: (Element | null)[] = [];
    let unwatched = 0;
    const watch = (node: Element | null) => {
      watched.push(node);
      return () => {
        unwatched += 1;
      };
    };
    const { node, attributes } = element();
    const undo = pictureRef(watch, "color: red !important")(node);
    expect(watched).toEqual([node]);
    expect(attributes.get("style")).toBe("color: red !important");
    undo();
    expect(unwatched).toBe(1);
    expect(attributes.has("style")).toBe(false);
    // A picture with no style is watched alone.
    const plain = element();
    pictureRef(watch, undefined)(plain.node)();
    expect(watched).toEqual([node, plain.node]);
    expect(unwatched).toBe(2);
    expect(plain.attributes.has("style")).toBe(false);
  });

  it("is no attribute React draws on a document's picture: the ref writes it", () => {
    const styled: Image = {
      src: "/a.png",
      alt: "",
      presentation: { attributes: { style: "x: 1" } },
    };
    expect(renderToStaticMarkup(<DocumentPicture image={styled} />)).not.toContain("style=");
  });
});
