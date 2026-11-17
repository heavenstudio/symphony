import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  anchorOf,
  focusedLink,
  focusedSlug,
  keptPlace,
  type Place,
  PlaceKeeper,
  RESTS_MS,
  RETURN_FRAMES,
  ROWS,
  readerColumn,
  readerRows,
  rowIn,
  rowOf,
  type Scroller,
  samePlace,
  takeFocus,
  withPlace,
} from "./boardplace";

// The tests run without a DOM: an element here answers only the selectors the module asks it for,
// each as its test sets it up.

interface Box {
  top?: number;
  bottom?: number;
  left?: number;
  right?: number;
}

interface Made {
  id?: string;
  attributes?: Record<string, string>;
  box?: Box;
  /** What `closest` finds, by selector. */
  closest?: Record<string, Element>;
  /** What `querySelectorAll` finds, by selector; `querySelector` finds the first. */
  all?: Record<string, Element[]>;
}

function element({ id = "", attributes = {}, box = {}, closest = {}, all = {} }: Made = {}) {
  const focus = vi.fn();
  const made = {
    id,
    focus,
    getAttribute: (name: string) => attributes[name] ?? null,
    getBoundingClientRect: () => ({ top: 0, bottom: 0, left: 0, right: 0, ...box }),
    closest: (selector: string) => closest[selector] ?? null,
    querySelectorAll: (selector: string) => all[selector] ?? [],
    querySelector: (selector: string) => all[selector]?.[0] ?? null,
  };
  return made as unknown as Element & { focus: typeof focus };
}

const COLUMN = ".board-scroll .column[data-status]";
/** What a row's column is found by. */
const ITS_COLUMN = ".column[data-status]";
const CARD = `.kanban ${ROWS}`;
const FOCUSABLE = "a[href], button, summary, [tabindex]";
const ROW_IDS = '[id^="row-"]';
const rows = (status: string) => `.board-scroll .column[data-status="${status}"] ${ROWS}`;
/** The card `STATUS-N` of the column `status` (`board`), named alone. */
const cardOf = (status: string, n: number) =>
  `.board-scroll .column[data-status="${status}"] [data-slug="${status}-${n}"]`;
const rowsOf = (status: string) => `.column[data-status="${status}"] ${ROW_IDS}`;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("the reader's column", () => {
  const shown: [string, number, number][] = [
    ["concept", 0, 288],
    ["ready", 300, 588],
    ["in_progress", 600, 888],
  ];

  it("is the one holding the focus, before the pointer's and the middle's", () => {
    expect(readerColumn("concept", "ready", shown, 744)).toBe("concept");
  });

  it("is the pointer's when the focus is in none shown, or in no column", () => {
    expect(readerColumn("review", "ready", shown, 744)).toBe("ready");
    expect(readerColumn(null, "ready", shown, 744)).toBe("ready");
  });

  it("is the one nearest the middle when the pointer is over none shown", () => {
    expect(readerColumn(null, "review", shown, 744)).toBe("in_progress");
    expect(readerColumn(null, null, shown, 440)).toBe("ready");
    // In the gap between two, the one whose middle is nearer.
    expect(readerColumn(null, null, shown, 293)).toBe("concept");
    expect(readerColumn(null, null, shown, 296)).toBe("ready");
    expect(readerColumn(null, null, shown, -500)).toBe("concept");
  });

  it("is none when the scroller shows no column", () => {
    expect(readerColumn("ready", "ready", [], 400)).toBeNull();
  });
});

/**
 * A board scrolled to show 0 to 900 sideways and 200 to 700 down, its columns at `columns`
 * (status, left, the tops of its cards, each card its row `row-STATUS-N`), the focus on `active`,
 * the pointer over `hovered`, a mouse's (`mouse`) or a touch screen's last tap.
 */
function board(
  columns: [string, number, number[]][],
  hovered: string | null = null,
  active: Element | null = null,
  mouse = true,
) {
  const made = columns.map(([status, left, tops]) => {
    // A column is the closest column to itself.
    const closest: Record<string, Element> = {};
    const attributes = { "data-status": status };
    const column = element({ attributes, box: { left, right: left + 288 }, closest });
    closest[COLUMN] = column;
    const cards = tops.map((top, n) =>
      element({
        id: `row-${status}-${n}`,
        attributes: { "data-slug": `${status}-${n}` },
        box: { top, left, right: left + 288 },
        closest: { [ITS_COLUMN]: column },
      }),
    );
    return { status, column, cards };
  });
  const all: Record<string, Element[]> = {
    ".column[data-status]": made.map(({ column }) => column),
    ".column[data-status]:hover": made
      .filter(({ status }) => status === hovered)
      .map(({ column }) => column),
    [ROW_IDS]: made.flatMap(({ cards }) => cards),
  };
  for (const { status, cards } of made) {
    all[rows(status)] = cards;
    all[rowsOf(status)] = cards;
  }
  const scroller = element({ box: { top: 200, bottom: 700, left: 0, right: 900 }, all });
  vi.stubGlobal("document", {
    activeElement: active,
    querySelector: (selector: string) => (selector === ".board-scroll" ? scroller : null),
  });
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query === "(hover: hover)" && mouse,
  }));
  vi.stubGlobal("innerHeight", 1000);
  return scroller;
}

describe("the card a live update holds", () => {
  it("is every card without a page or a board's scroller", () => {
    expect(readerRows()).toBe(ROWS);
    vi.stubGlobal("document", { activeElement: null, querySelector: () => null });
    expect(readerRows()).toBe(ROWS);
  });

  it("is the first on the screen of the column nearest the scroller's middle, named alone", () => {
    board([
      ["concept", 0, [250]],
      ["ready", 300, [180, 260, 300]],
      ["in_progress", 600, [300]],
    ]);
    expect(readerRows()).toBe(cardOf("ready", 1));
    expect(cardOf("ready", 1)).toBe(
      '.board-scroll .column[data-status="ready"] [data-slug="ready-1"]',
    );
  });

  it("is the first on the screen of the column under the pointer, or holding the focus", () => {
    board(
      [
        ["concept", 0, [250]],
        ["ready", 300, [260]],
      ],
      "concept",
    );
    expect(readerRows()).toBe(cardOf("concept", 0));
    const column = element({ attributes: { "data-status": "in_progress" } });
    board(
      [
        ["concept", 0, [250]],
        ["ready", 300, [260]],
        ["in_progress", 600, [300]],
      ],
      "concept",
      element({ closest: { [COLUMN]: column } }),
    );
    expect(readerRows()).toBe(cardOf("in_progress", 0));
  });

  it("leaves out the column of a touch screen's last tap, wherever the reader swiped since", () => {
    const columns: [string, number, number[]][] = [
      ["concept", 0, [250]],
      ["ready", 300, [260]],
    ];
    board(columns, "concept", null, false);
    expect(readerRows()).toBe(cardOf("ready", 0));
    // A browser that cannot say what its pointer is counts none.
    board(columns, "concept");
    vi.stubGlobal("matchMedia", undefined);
    expect(readerRows()).toBe(cardOf("ready", 0));
  });

  it("leaves out a column scrolled away sideways", () => {
    // Only the first shows: the one past the scroller's right, nearer its middle, is not the reader's.
    board([
      ["concept", -200, [250]],
      ["ready", 950, [260]],
    ]);
    expect(readerRows()).toBe(cardOf("concept", 0));
  });

  it("is one in the window as well, where the shell's hold looks for it", () => {
    board([["ready", 300, [180, 650]]]);
    expect(readerRows()).toBe(cardOf("ready", 1));
    // The window ends above the scroller's bottom: the card under it is not on the screen.
    vi.stubGlobal("innerHeight", 600);
    expect(readerRows()).toBe(ROWS);
  });

  it("is every card when no card of the reader's column has its top on the scroller", () => {
    board([
      ["concept", 0, [250]],
      ["ready", 300, [100, 700, 900]],
    ]);
    expect(readerRows()).toBe(ROWS);
    board([["ready", 300, []]]);
    expect(readerRows()).toBe(ROWS);
  });
});

describe("the row the reader's place is kept by", () => {
  it("is the reader's column's first row on the screen, and how far below its top", () => {
    const scroller = board([
      ["concept", 0, [250]],
      ["ready", 300, [150, 260.4, 400]],
      ["in_progress", 600, [300]],
    ]);
    expect(anchorOf(scroller)).toEqual({ row: "ready-1", column: "ready", at: 60 });
  });

  it("is any column's first row on the screen when the reader's has none there", () => {
    const scroller = board([
      ["concept", 0, [100, 230]],
      ["ready", 300, [100, 800]],
      ["in_progress", 600, [300]],
    ]);
    expect(anchorOf(scroller)).toEqual({ row: "concept-1", column: "concept", at: 30 });
  });

  it("is none on a board with no row on the screen, or none in a column", () => {
    expect(anchorOf(board([["ready", 300, [100, 800]]]))).toBeNull();
    expect(anchorOf(board([]))).toBeNull();
    const loose = element({ id: "row-w/a", box: { top: 300, left: 0, right: 288 } });
    const scroller = element({
      box: { top: 200, bottom: 700, left: 0, right: 900 },
      all: { [ROW_IDS]: [loose] },
    });
    vi.stubGlobal("document", { activeElement: null });
    expect(anchorOf(scroller)).toBeNull();
  });

  it("is named for the shell's hold by its id, quoted, in its column or anywhere", () => {
    expect(rowOf("w/a")).toBe('[id="row-w/a"]');
    expect(rowOf('w/"a\\')).toBe('[id="row-w/\\"a\\\\"]');
    expect(rowIn('re"ady', "w/a")).toBe(
      '.board-scroll .column[data-status="re\\"ady"] [id="row-w/a"]',
    );
  });
});

describe("a moved card's focus", () => {
  /** The board's card of `slug`, its links `links`, the focus on its link `at` (none: outside). */
  function focused(slug: string, links: number, at: number | null) {
    const all: Record<string, Element[]> = {};
    const card = element({ attributes: { "data-slug": slug }, all });
    const inCard = () => element({ closest: { [CARD]: card } });
    all[FOCUSABLE] = Array.from({ length: links }, inCard);
    const active = at === null ? element() : (all[FOCUSABLE][at] ?? inCard());
    vi.stubGlobal("document", { activeElement: active });
  }

  it("is no card's without a page", () => {
    expect(focusedSlug()).toBeNull();
    expect(focusedLink("w/a")).toBeNull();
  });

  it("names the card holding it and which of its links does", () => {
    focused("w/a", 3, 2);
    expect(focusedSlug()).toBe("w/a");
    expect(focusedLink("w/a")).toBe(2);
    expect(focusedLink("w/b")).toBeNull();
  });

  it("is in no card when the focus is outside every card", () => {
    focused("w/a", 3, null);
    expect(focusedSlug()).toBeNull();
    expect(focusedLink("w/a")).toBeNull();
  });

  it("names the card's first link when the card holds it in none of them", () => {
    focused("w/a", 2, 5);
    expect(focusedLink("w/a")).toBe(0);
  });

  /** The task `slug`'s row on the page, its card's links `links`, the focus on `active`. */
  function moved(slug: string, links: number, active: "body" | "field" | null) {
    const focusable = Array.from({ length: links }, () => element());
    const card = element({ all: { [FOCUSABLE]: focusable, "a[href]": focusable.slice(0, 1) } });
    const row = element({ all: { ".taskcard": [card] } });
    const body = element();
    vi.stubGlobal("document", {
      body,
      activeElement: active === "body" ? body : active === "field" ? element() : null,
      getElementById: (id: string) => (id === `row-${slug}` ? row : null),
    });
    return focusable;
  }

  it("is given back to the same link of the card in its new row, where it stands", () => {
    const links = moved("w/a", 3, "body");
    takeFocus("w/a", 1);
    expect(links[1]?.focus).toHaveBeenCalledWith({ preventScroll: true });
    expect(links[0]?.focus).not.toHaveBeenCalled();
    const again = moved("w/a", 3, null);
    takeFocus("w/a", 2);
    expect(again[2]?.focus).toHaveBeenCalledWith({ preventScroll: true });
  });

  it("is given to the card's title when it has fewer links than it had", () => {
    const links = moved("w/a", 1, "body");
    takeFocus("w/a", 3);
    expect(links[0]?.focus).toHaveBeenCalledWith({ preventScroll: true });
  });

  it("stays where the reader put it since, and on no row the page lacks", () => {
    const links = moved("w/a", 2, "field");
    takeFocus("w/a", 0);
    expect(links[0]?.focus).not.toHaveBeenCalled();
    const none = moved("w/a", 2, "body");
    takeFocus("w/b", 0);
    expect(none[0]?.focus).not.toHaveBeenCalled();
  });
});

describe("the board's place in its history entry", () => {
  const place: Place = { left: 300, top: 1200, page: 150 };

  it("is read from the entry's state when it holds one whole", () => {
    expect(keptPlace({ board: place, other: 1 })).toEqual(place);
    expect(keptPlace({ board: { left: 0, top: 0, page: 0 } })).toEqual({
      left: 0,
      top: 0,
      page: 0,
    });
    const kept = { ...place, row: "w/a", column: "ready", at: 40 };
    expect(keptPlace({ board: kept })).toEqual(kept);
  });

  it("is read without its row when the row, its column or its height on the screen is not whole", () => {
    for (const row of [
      { row: "", column: "ready", at: 40 },
      { row: 3, column: "ready", at: 40 },
      { row: "w/a", column: "ready" },
      { row: "w/a", column: "ready", at: -1 },
      { row: "w/a", column: "ready", at: "40" },
      // A place an earlier build kept, its row's column not named.
      { row: "w/a", at: 40 },
      { row: "w/a", column: "", at: 40 },
      { row: "w/a", column: 3, at: 40 },
    ]) {
      expect(keptPlace({ board: { ...place, ...row } })).toEqual(place);
    }
  });

  it("is none for an entry with none, or one it cannot read", () => {
    for (const state of [
      null,
      undefined,
      3,
      "board",
      {},
      { board: null },
      { board: "here" },
      { board: { left: 1, top: 2 } },
      { board: { left: -1, top: 2, page: 0 } },
      { board: { left: 1, top: Number.NaN, page: 0 } },
      { board: { left: 1, top: Number.POSITIVE_INFINITY, page: 0 } },
      { board: { left: "1", top: 2, page: 0 } },
    ]) {
      expect(keptPlace(state)).toBeNull();
    }
  });

  it("is written into the entry's state beside what else it holds", () => {
    expect(withPlace(null, place)).toEqual({ board: place });
    expect(withPlace("text", place)).toEqual({ board: place });
    expect(withPlace({ other: 1, board: { left: 0, top: 0, page: 0 } }, place)).toEqual({
      other: 1,
      board: place,
    });
  });

  it("is the same only as one at the very same place", () => {
    expect(samePlace(place, { ...place })).toBe(true);
    const kept = { ...place, row: "w/a", column: "ready", at: 40 };
    expect(samePlace(kept, { ...kept })).toBe(true);
    for (const other of [
      { left: 301 },
      { top: 1199 },
      { page: 0 },
      { row: "w/b" },
      { column: "design" },
      { at: 41 },
    ]) {
      expect(samePlace(kept, { ...kept, ...other })).toBe(false);
    }
    expect(samePlace(place, kept)).toBe(false);
    expect(samePlace(null, place)).toBe(false);
    expect(samePlace(place, null)).toBe(false);
  });
});

describe("the board's place kept", () => {
  /** The window's listeners, by kind, each with the signal that ends it. */
  let listeners: [string, () => void, AbortSignal | undefined][];
  let frames: (() => void)[];
  let entry: { state: unknown };
  let navigation: string;
  /** The board's rows on the page. */
  let onPage: Element[];
  const replaced = vi.fn();
  /** The shell's hold (`reviewHoldPlace`). */
  const held = vi.fn();

  beforeEach(() => {
    listeners = [];
    frames = [];
    entry = { state: null };
    navigation = "ready";
    replaced.mockClear();
    held.mockClear();
    vi.stubGlobal("window", { reviewHoldPlace: held });
    vi.useFakeTimers();
    vi.stubGlobal("location", { pathname: "/tasks", href: "http://here/tasks" });
    vi.stubGlobal("history", {
      get state() {
        return entry.state;
      },
      replaceState: (state: unknown) => {
        replaced(state);
        entry.state = state;
      },
    });
    vi.stubGlobal("scrollX", 0);
    vi.stubGlobal("scrollY", 0);
    vi.stubGlobal("scrollTo", (_x: number, y: number) => vi.stubGlobal("scrollY", y));
    onPage = [];
    vi.stubGlobal("document", {
      activeElement: null,
      documentElement: {
        dataset: {
          get navigation() {
            return navigation;
          },
        },
      },
      getElementById: (id: string) => onPage.find((row) => row.id === id) ?? null,
    });
    vi.stubGlobal("requestAnimationFrame", (step: () => void) => frames.push(step));
    vi.stubGlobal(
      "addEventListener",
      (kind: string, listener: () => void, options?: AddEventListenerOptions) =>
        listeners.push([kind, listener, options?.signal]),
    );
  });

  /** Run the frames asked for so far; how many there were. */
  function frame(): number {
    const now = frames;
    frames = [];
    for (const step of now) step();
    return now.length;
  }

  /** What the reader does: each listener of `kind` still listening hears it. */
  function gesture(kind: string) {
    for (const [one, listener, signal] of listeners) {
      if (one === kind && signal?.aborted !== true) listener();
    }
  }

  /** Rows of a column: each its slug and how far down the column it stands. */
  type Rows = [slug: string, offset: number][];

  /** A place kept by Ready's row `w/b`, 50 px below the scroller's top, the scroller 250 down. */
  const KEPT: Place = { left: 0, top: 250, page: 0, row: "w/b", column: "ready", at: 50 };

  /**
   * The board's scroller, its box 100 to 600 down and 0 to 900 sideways, showing one column,
   * Ready's, whose rows are `rows` (each its slug and how far down the column it stands), on the
   * screen where the scroller's top puts them, and on the page (`getElementById`); or, while
   * `waiting`, none yet, its placeholder (`[data-pending]`) standing for them.
   */
  function surface(rows: Rows = [], waiting = false): Scroller & { draw: (rows: Rows) => void } {
    const box = { top: 100, bottom: 600, left: 0, right: 900 };
    const scroller = { scrollLeft: 0, scrollTop: 0 } as Scroller;
    const inColumn: Record<string, Element[]> = {};
    const column = element({
      attributes: { "data-status": "ready" },
      box: { left: 0, right: 288 },
      all: inColumn,
    });
    const all: Record<string, Element[]> = {};
    /** Draw Ready's rows as `rows`, in place of those it had, or its placeholder while `waits`. */
    const draw = (rows: Rows, waits = false) => {
      for (const selector of Object.keys(all)) delete all[selector];
      all[".column[data-status]"] = [column];
      all['.column[data-status="ready"]'] = [column];
      const made = rows.map(([slug, offset]) => {
        const top = box.top + offset;
        const edges = () => ({ top: top - scroller.scrollTop, left: 0, right: 288 });
        const closest = (selector: string) => (selector === ITS_COLUMN ? column : null);
        const row = { id: `row-${slug}`, getBoundingClientRect: edges, closest };
        all[rowIn("ready", slug)] = [row as unknown as Element];
        return row as unknown as Element;
      });
      all[ROW_IDS] = made;
      all[rowsOf("ready")] = made;
      inColumn[ROW_IDS] = made;
      inColumn['[data-pending="true"]'] = waits ? [element()] : [];
      onPage = made;
    };
    draw(rows, waiting);
    return Object.assign(scroller, {
      draw,
      getBoundingClientRect: () => box,
      querySelectorAll: (selector: string) => all[selector] ?? [],
      querySelector: (selector: string) => all[selector]?.[0] ?? null,
    });
  }

  function keeper(scroller = surface()) {
    const controller = new AbortController();
    return { keeper: new PlaceKeeper(scroller, controller.signal), scroller, controller };
  }

  it("is written once the reader's scrolling rests, and only when it moved", async () => {
    const { keeper: kept, scroller } = keeper();
    entry.state = { other: true };
    scroller.scrollTop = 400.4;
    kept.later();
    await vi.advanceTimersByTimeAsync(RESTS_MS - 1);
    scroller.scrollTop = 800.6;
    kept.later();
    await vi.advanceTimersByTimeAsync(RESTS_MS - 1);
    expect(replaced).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(replaced).toHaveBeenCalledExactlyOnceWith({
      other: true,
      board: { left: 0, top: 801, page: 0 },
    });
    kept.keep();
    expect(replaced).toHaveBeenCalledOnce();
  });

  it("keeps the row on the screen the place is kept by, and how far down the screen it stands", () => {
    const { keeper: kept, scroller } = keeper(
      surface([
        ["w/a", 0],
        ["w/b", 300],
        ["w/c", 600],
      ]),
    );
    scroller.scrollTop = 250;
    kept.keep();
    expect(entry.state).toEqual({ board: KEPT });
  });

  it("returns to its row where it stood when the rows above it are drawn at another height", () => {
    entry.state = { board: KEPT };
    // The row above is 40 px taller drawn again (its card, where a ghost stood).
    const { keeper: kept, scroller } = keeper(
      surface([
        ["w/a", 0],
        ["w/b", 340],
      ]),
    );
    kept.back();
    expect(scroller.scrollTop).toBe(290);
    // There at once; the shell's hold keeps the row there while the rows near it draw.
    expect(frames).toEqual([]);
    expect(held).toHaveBeenCalledExactlyOnceWith(rowIn("ready", "w/b"));
  });

  it("returns to the scroller's top while its column draws its rows, then to its row", () => {
    entry.state = { board: KEPT };
    // The board's first draw: its columns' rows not drawn yet.
    const scroller = surface([], true);
    const { keeper: kept } = keeper(scroller);
    kept.back();
    expect(scroller.scrollTop).toBe(250);
    expect(frames).toHaveLength(1);
    expect(held).not.toHaveBeenCalled();
    // Its column draws its rows, the ones above taller than they were.
    scroller.draw([
      ["w/a", 0],
      ["w/b", 340],
    ]);
    expect(frame()).toBe(1);
    expect(scroller.scrollTop).toBe(290);
    expect(frames).toEqual([]);
    expect(held).toHaveBeenCalledExactlyOnceWith(rowIn("ready", "w/b"));
  });

  it("gives up at the scroller's top, holding nothing, when its column never draws its rows", () => {
    entry.state = { board: KEPT };
    const { keeper: kept, scroller } = keeper(surface([], true));
    kept.back();
    let ran = 1;
    while (frame() > 0) ran += 1;
    expect(ran).toBe(RETURN_FRAMES);
    expect(scroller.scrollTop).toBe(250);
    expect(held).not.toHaveBeenCalled();
  });

  it("returns to the scroller's top, holding nothing, when its row left its column", () => {
    entry.state = { board: KEPT };
    const scroller = surface([
      ["w/a", 0],
      ["w/c", 300],
    ]);
    // Its task moved on (claimed, or cancelled under the scroller): its row is elsewhere on the
    // page, far down, and no place of the scroller's.
    const moved = { id: "row-w/b", getBoundingClientRect: () => ({ top: 4000 }) };
    onPage = [...onPage, moved as unknown as Element];
    const { keeper: kept } = keeper(scroller);
    kept.back();
    expect(scroller.scrollTop).toBe(250);
    expect(frames).toEqual([]);
    expect(held).not.toHaveBeenCalled();
  });

  it("waits while its column draws, then returns to the scroller's top when its row is not in it", () => {
    entry.state = { board: KEPT };
    const scroller = surface([], true);
    const { keeper: kept } = keeper(scroller);
    kept.back();
    expect(frames).toHaveLength(1);
    scroller.draw([["w/a", 0]]);
    expect(frame()).toBe(1);
    expect(scroller.scrollTop).toBe(250);
    expect(frames).toEqual([]);
    expect(held).not.toHaveBeenCalled();
  });

  it("returns to the scroller's top when its column is not on the board", () => {
    entry.state = { board: { ...KEPT, column: "design" } };
    const { keeper: kept, scroller } = keeper(surface([["w/b", 300]]));
    kept.back();
    expect(scroller.scrollTop).toBe(250);
    expect(frames).toEqual([]);
    expect(held).not.toHaveBeenCalled();
  });

  it("goes sideways at the columns' tops, where each snaps, and only when not there", () => {
    entry.state = { board: { left: 300, top: 900, page: 0 } };
    const scroller = surface();
    // Where the scroller stood down its columns each time it went sideways.
    const across: number[] = [];
    let left = 0;
    Object.defineProperty(scroller, "scrollLeft", {
      get: () => left,
      set: (to: number) => {
        across.push(scroller.scrollTop);
        left = to;
      },
    });
    scroller.scrollTop = 600;
    navigation = "loading";
    const { keeper: kept } = keeper(scroller);
    kept.back();
    expect([left, scroller.scrollTop]).toEqual([300, 900]);
    // Already there sideways: the frames after go only down.
    scroller.scrollTop = 0;
    navigation = "ready";
    frame();
    expect(across).toEqual([0]);
    expect([left, scroller.scrollTop]).toEqual([300, 900]);
    expect(frames).toEqual([]);
  });

  it("goes on returning while its row cannot reach its place yet", () => {
    entry.state = { board: KEPT };
    const scroller = surface([["w/b", 340]]);
    // The scroller holds only 200 px of its rows yet, then all of them.
    let most = 200;
    let top = 0;
    Object.defineProperty(scroller, "scrollTop", {
      get: () => top,
      set: (to: number) => {
        top = Math.min(to, most);
      },
    });
    const { keeper: kept } = keeper(scroller);
    kept.back();
    expect(top).toBe(200);
    expect(frame()).toBe(1);
    expect(frames).toHaveLength(1);
    most = 1000;
    expect(held).not.toHaveBeenCalled();
    frame();
    expect(top).toBe(290);
    expect(frames).toEqual([]);
    expect(held).toHaveBeenCalledExactlyOnceWith(rowIn("ready", "w/b"));
  });

  it("is written at once before a link is followed, the rest's wait dropped", async () => {
    const { keeper: kept, scroller } = keeper();
    scroller.scrollLeft = 300;
    vi.stubGlobal("scrollY", 150);
    kept.later();
    kept.keep();
    expect(entry.state).toEqual({ board: { left: 300, top: 0, page: 150 } });
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(RESTS_MS);
    expect(replaced).toHaveBeenCalledOnce();
  });

  it("is never written into another page's entry, nor once the board is gone", async () => {
    const { keeper: kept, scroller, controller } = keeper();
    scroller.scrollTop = 10;
    vi.stubGlobal("location", { pathname: "/task/w/a", href: "http://here/task/w/a" });
    kept.keep();
    expect(replaced).not.toHaveBeenCalled();
    vi.stubGlobal("location", { pathname: "/tasks", href: "http://here/tasks" });
    controller.abort();
    kept.keep();
    kept.later();
    await vi.advanceTimersByTimeAsync(RESTS_MS);
    expect(replaced).not.toHaveBeenCalled();
  });

  it("is returned to from the entry: the scroller at once, the window once navigation is ready", () => {
    entry.state = { board: { left: 300, top: 1200, page: 150 } };
    navigation = "loading";
    const { keeper: kept, scroller } = keeper();
    kept.back();
    expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([300, 1200]);
    expect(scrollY).toBe(0);
    // The shell puts the window at its top as its navigation ends.
    scroller.scrollTop = 0;
    expect(frame()).toBe(1);
    expect(scroller.scrollTop).toBe(1200);
    expect(scrollY).toBe(0);
    navigation = "ready";
    frame();
    expect(scrollY).toBe(150);
    // There: no frame more, and nothing written meanwhile.
    expect(frame()).toBe(0);
    expect(replaced).not.toHaveBeenCalled();
  });

  it("keeps nothing while it returns, and keeps what the reader took over at", () => {
    entry.state = { board: { left: 0, top: 1200, page: 0 } };
    const scroller = surface();
    // The scroller holds only 500 px of its rows yet.
    Object.defineProperty(scroller, "scrollTop", {
      get: () => 500,
      set: () => {},
    });
    const { keeper: kept } = keeper(scroller);
    kept.back();
    kept.keep();
    expect(replaced).not.toHaveBeenCalled();
    frame();
    gesture("wheel");
    frame();
    expect(frames).toEqual([]);
    kept.keep();
    expect(entry.state).toEqual({ board: { left: 0, top: 500, page: 0 } });
  });

  it("gives up after its frames when the board cannot reach the place", () => {
    entry.state = { board: { left: 0, top: 0, page: 4000 } };
    vi.stubGlobal("scrollTo", () => {});
    const { keeper: kept } = keeper();
    kept.back();
    let ran = 1;
    while (frame() > 0) ran += 1;
    expect(ran).toBe(RETURN_FRAMES);
  });

  it("returns nowhere for an entry that keeps no place, or once the board is gone", () => {
    const { keeper: kept } = keeper();
    kept.back();
    expect(frames).toEqual([]);
    expect(listeners).toEqual([]);
    entry.state = { board: { left: 5, top: 5, page: 0 } };
    const { keeper: gone, scroller, controller } = keeper();
    controller.abort();
    gone.back();
    expect(scroller.scrollTop).toBe(0);
    expect(frames).toEqual([]);
  });

  it("stops returning once the board is gone", () => {
    entry.state = { board: { left: 5, top: 5, page: 0 } };
    navigation = "loading";
    const { keeper: kept, scroller, controller } = keeper();
    kept.back();
    controller.abort();
    scroller.scrollTop = 0;
    frame();
    expect(scroller.scrollTop).toBe(0);
    expect(frames).toEqual([]);
  });

  it("drops the scrolling not yet kept on Back, and returns to the board entry's own place", async () => {
    const { keeper: kept, scroller } = keeper();
    scroller.scrollTop = 600;
    kept.later();
    // Back to another entry of the board, which kept its own place.
    entry.state = { board: { left: 0, top: 90, page: 0 } };
    kept.moved();
    expect(scroller.scrollTop).toBe(90);
    await vi.advanceTimersByTimeAsync(RESTS_MS);
    expect(replaced).not.toHaveBeenCalled();
    // Back to another page's entry: nothing to return to there.
    vi.stubGlobal("location", { pathname: "/task/w/a", href: "http://here/task/w/a" });
    entry.state = { board: { left: 0, top: 30, page: 0 } };
    kept.moved();
    expect(scroller.scrollTop).toBe(90);
  });

  it("starts a new return in place of one under way", () => {
    entry.state = { board: { left: 0, top: 100, page: 0 } };
    navigation = "loading";
    const { keeper: kept, scroller } = keeper();
    kept.back();
    entry.state = { board: { left: 0, top: 200, page: 0 } };
    kept.back();
    navigation = "ready";
    scroller.scrollTop = 0;
    // The first's frame does nothing; the second's goes to its place.
    frame();
    expect(scroller.scrollTop).toBe(200);
    expect(frame()).toBe(0);
  });
});

it("holds the reading column in the waiting board without a horizontal scroller", () => {
  const column = element({
    attributes: { "data-status": "review" },
    box: { left: 900, right: 1100 },
  });
  const row = element({
    attributes: { "data-slug": "review/one" },
    box: { top: 250, left: 900, right: 1100 },
  });
  const root = element({
    box: { top: 100, bottom: 1000, left: 0, right: 1400 },
    all: {
      ".column[data-status]": [column],
      '.waiting-columns .column[data-status="review"] [data-slug]': [row],
    },
  });
  vi.stubGlobal("document", {
    activeElement: null,
    querySelector: (selector: string) => (selector === ".waiting-columns" ? root : null),
  });
  vi.stubGlobal("innerHeight", 800);
  expect(readerRows()).toBe(
    '.waiting-columns .column[data-status="review"] [data-slug="review/one"]',
  );
});

describe("the waiting board's window position", () => {
  function windowBoard(offset: number | null, ready = true) {
    let y = 0;
    const state = { board: { left: 0, top: 0, page: 900, row: "kept", column: "ready", at: 25 } };
    const column = element({
      attributes: { "data-status": "ready" },
      box: { left: 0, right: 300 },
    });
    const row = element({ id: "row-kept", closest: { ".column[data-status]": column } });
    row.getBoundingClientRect = () =>
      ({ top: (offset ?? 0) - y, left: 0, right: 300, bottom: (offset ?? 0) - y + 118 }) as DOMRect;
    const surface = {
      scrollLeft: 0,
      scrollTop: 0,
      getBoundingClientRect: () => ({ top: 200 - y, bottom: 3000 - y, left: 0, right: 300 }),
      querySelectorAll: (selector: string) =>
        selector === ".column[data-status]" ? [column] : offset === null ? [] : [row],
      querySelector: (selector: string) =>
        selector === rowIn("ready", "kept", true)
          ? offset === null
            ? null
            : row
          : selector === '.column[data-status="ready"]'
            ? column
            : null,
    } as unknown as Scroller;
    const doc = {
      activeElement: null,
      documentElement: { dataset: { navigation: ready ? "ready" : "loading" } },
    };
    const frames: FrameRequestCallback[] = [];
    const held = vi.fn();
    vi.stubGlobal("window", { reviewHoldPlace: held });
    vi.stubGlobal("document", doc);
    vi.stubGlobal("location", { pathname: "/tasks" });
    vi.stubGlobal("history", {
      state,
      replaceState: vi.fn((next) => {
        Object.assign(state, next);
      }),
    });
    vi.stubGlobal("innerHeight", 800);
    vi.stubGlobal("scrollX", 0);
    vi.stubGlobal("scrollY", 0);
    vi.stubGlobal("scrollTo", (_x: number, next: number) => {
      y = next;
      vi.stubGlobal("scrollY", y);
    });
    vi.stubGlobal("addEventListener", vi.fn());
    vi.stubGlobal("requestAnimationFrame", (frame: FrameRequestCallback) => {
      frames.push(frame);
      return frames.length;
    });
    const page = new AbortController();
    return {
      keeper: new PlaceKeeper(surface, page.signal, true),
      surface,
      page,
      held,
      doc,
      frames,
      state,
    };
  }
  it("returns to the column scrolled sideways and keeps its updated position", () => {
    const test = windowBoard(955);
    test.state.board.left = 864;
    test.keeper.back();
    expect(test.surface.scrollLeft).toBe(864);
    expect(scrollY).toBe(930);
    test.surface.scrollLeft = 1152;
    test.keeper.keep();
    expect(test.state.board.left).toBe(1152);
    test.page.abort();
  });
  it("returns to the saved row's window position even if rows above grew", () => {
    const test = windowBoard(955);
    test.keeper.back();
    expect(scrollY).toBe(930);
    expect(test.held).toHaveBeenCalledWith(
      '.waiting-columns .column[data-status="ready"] [id="row-kept"]',
    );
    test.keeper.keep();
    expect(test.state.board).toEqual({
      left: 0,
      top: 0,
      page: 930,
      row: "kept",
      column: "ready",
      at: 25,
    });
    test.page.abort();
  });
  it("uses the saved window offset when the old row is absent from its column", () => {
    const test = windowBoard(null);
    test.keeper.back();
    expect(scrollY).toBe(900);
    expect(test.held).not.toHaveBeenCalled();
    test.page.abort();
  });
  it("waits for shell navigation before restoring the window", () => {
    const test = windowBoard(925, false);
    test.keeper.back();
    expect(scrollY).toBe(0);
    expect(test.frames).toHaveLength(1);
    test.doc.documentElement.dataset.navigation = "ready";
    test.frames.shift()?.(0);
    expect(scrollY).toBe(900);
    test.page.abort();
  });
});
