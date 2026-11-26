import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import fullBoard from "../../../crates/symphony-api/fixtures/board.json";
import emptyBoard from "../../../crates/symphony-api/fixtures/board-empty.json";
import { age, BoardView, greeting, HomePage, keepTime, lead } from "./home";
import { readBoard } from "./home-board";
import { PLAIN } from "./project";
import type { Board, Card, ProjectView, Region } from "./wire";
import { parseMoment } from "./words";

/** A moment the tests name, as a register writes it. */
function at(text: string): number {
  const read = parseMoment(text);
  if (read === null) throw new Error(`${text} does not parse`);
  return read;
}

/** A fixture crates/symphony-api pins, as this client reads it. */
function board(value: unknown = fullBoard): Board {
  const read = readBoard(structuredClone(value));
  if (read === null) throw new Error("the fixture is no board");
  return read;
}

/** The board drawn at 10:12 on 2026-10-08, for `view`'s project. */
function render(shown: Board, view: ProjectView = PLAIN): string {
  return renderToStaticMarkup(<BoardView board={shown} now={at("2026-10-08 10:12")} view={view} />);
}

/** The markup from `marker` to the end of the first section after it. */
function region(html: string, marker: string): string {
  const start = html.indexOf(marker);
  if (start < 0) throw new Error(`no ${marker} in ${html}`);
  const rest = html.slice(start);
  const end = rest.indexOf("</section>");
  return end < 0 ? rest : rest.slice(0, end);
}

const count = (html: string, needle: string) => html.split(needle).length - 1;

describe("the owner's clock", () => {
  afterEach(() => vi.useRealTimers());

  it("tells the moment at each minute's tick, and nothing once the page is gone", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 8, 10, 12));
    const shown: number[] = [];
    const page = new AbortController();
    const kept = keepTime(page.signal, (now) => shown.push(now));
    await vi.advanceTimersByTimeAsync(59_999);
    expect(shown).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(shown).toEqual([at("2026-10-08 10:13")]);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(shown).toEqual([at("2026-10-08 10:13"), at("2026-10-08 10:14")]);
    page.abort();
    await kept;
    expect(vi.getTimerCount()).toBe(0);
    expect(shown).toHaveLength(2);
  });

  it("opened within a minute, tells the next as it comes, not a minute after the opening", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 8, 10, 12, 45));
    const shown: number[] = [];
    const page = new AbortController();
    const kept = keepTime(page.signal, (now) => shown.push(now));
    await vi.advanceTimersByTimeAsync(14_999);
    expect(shown).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(shown).toEqual([at("2026-10-08 10:13")]);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(shown).toEqual([at("2026-10-08 10:13"), at("2026-10-08 10:14")]);
    page.abort();
    await kept;
  });
});

describe("the greeting", () => {
  it("follows the hour", () => {
    expect(greeting(at("2026-10-08 00:00"))).toBe("Good morning");
    expect(greeting(at("2026-10-08 11:59"))).toBe("Good morning");
    expect(greeting(at("2026-10-08 12:00"))).toBe("Good afternoon");
    expect(greeting(at("2026-10-08 17:59"))).toBe("Good afternoon");
    expect(greeting(at("2026-10-08 18:00"))).toBe("Good evening");
    expect(greeting(at("1969-12-31 23:00"))).toBe("Good evening");
  });
});

describe("an age", () => {
  const now = at("2026-10-08 12:00");

  it("is new, then hours, then days", () => {
    expect(age("2026-10-08 11:30", now)).toBe("new");
    expect(age("2026-10-08 11:00", now)).toBe("1 h");
    expect(age("2026-10-08 09:00", now)).toBe("3 h");
    expect(age("2026-10-07 12:01", now)).toBe("23 h");
    expect(age("2026-10-07 12:00", now)).toBe("1 d");
    expect(age("2026-10-01", now)).toBe("7 d");
  });

  it("is new for a register ahead of the clock, and nothing without a time", () => {
    expect(age("2026-10-09 08:00", now)).toBe("new");
    expect(age("soon", now)).toBe("");
    expect(age(null, now)).toBe("");
    expect(age(undefined, now)).toBe("");
  });
});

describe("the lead", () => {
  it("counts cards and documents in the singular and the plural", () => {
    expect(lead(37, 9)).toBe("37 cards wait on you across 9 documents.");
    expect(lead(1, 1)).toBe("1 card waits on you across 1 document.");
    expect(lead(0, 0)).toBe("Your queue is empty.");
  });
});

describe("the hero", () => {
  it("is the next card with a link straight to it", () => {
    const hero = region(render(board()), 'class="hero card"');
    expect(hero).toContain("Next up · 1 of 37");
    expect(hero).toContain("North · pendant picture");
    expect(hero).toContain("Three candidates for the Heron pendant");
    expect(hero).toContain('<a class="docchip" href="/docs/lamps/north.md">North lamps</a>');
    expect(hero).toContain("picture · gates 4 lamps");
    expect(hero).toContain('<span class="muted">waiting 2 h</span>');
    expect(hero).toContain(
      '<a class="go" href="/docs/lamps/north.md#north-pendant-picture">Open this card <span aria-hidden="true">→</span></a>',
    );
    expect(count(hero, "<figure")).toBe(3);
    for (const [letter, src] of [
      ["A", "pendant-a"],
      ["B", "pendant-b"],
      ["C", "pendant-c"],
    ]) {
      expect(hero).toContain(`<figcaption>${letter}</figcaption>`);
      expect(hero).toContain(`class="picture-shell"`);
      expect(src).toMatch(/^pendant-/);
    }
  });

  it("shows a lamp not drawn yet as a picture coming, and one waiting on review marked", () => {
    const shown = board();
    shown.next = [...shown.next.slice(4), ...shown.next.slice(0, 4)];
    const hero = region(render(shown), 'class="hero card"');
    expect(count(hero, "<figure")).toBe(3);
    expect(count(hero, "picture coming")).toBe(1);
    expect(count(hero, '<figure class="waiting">')).toBe(1);
    expect(hero).not.toContain("hero-summary");
  });

  it("shows six pictures at most", () => {
    const shown = board();
    const pictures = Array.from({ length: 8 }, (_, at) => ({
      src: `/p${at}.png`,
      caption: `${at}`,
    }));
    shown.next[0] = { ...(shown.next[0] as Card), pictures };
    expect(count(region(render(shown), 'class="hero card"'), "<figure")).toBe(6);
  });

  it("is the ordinary one for a branch's decision, its task linked", () => {
    const shown = board();
    shown.next[0] = {
      ...(shown.next[0] as Card),
      doc: "@design~policy/docs/POLICY.md",
      task: "design/policy",
    };
    const hero = region(render(shown), 'class="hero card"');
    expect(hero).toContain('<a class="chip" href="/task/design/policy">design/policy</a> ');
    expect(hero).toContain('href="/@design~policy/docs/POLICY.md#north-pendant-picture"');
    expect(hero).toContain("Next up · 1 of 37");
  });

  it("says nothing of a wait without a time", () => {
    const shown = board();
    shown.next[0] = { ...(shown.next[0] as Card), posted: null };
    const hero = region(render(shown), 'class="hero card"');
    expect(hero).toContain('<span class="why">picture · gates 4 lamps</span></p>');
  });
});

describe("then", () => {
  it("come the four after the next card, in the queue's order", () => {
    const next = region(render(board()), 'class="next card"');
    expect(count(next, 'class="nextcard"')).toBe(4);
    const order = [
      "mining-yield-curve",
      "west-chandelier-picture",
      "dock-menu-order",
      "east-lamp-silhouettes",
    ].map((slug) => next.indexOf(`#${slug}"`));
    expect(order.every((at) => at >= 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(next).toContain("Mining · design · gates 2 cards");
    expect(next).toContain('class="picture-shell thumb"');
    expect(next).toContain('<a class="more" href="/queue">show all 37 →</a>');
  });

  it("show their place in the queue when they have no picture", () => {
    const next = region(render(board()), 'class="next card"');
    const glyphs = next
      .split('<span class="thumb glyph">')
      .slice(1)
      .map((rest) => rest.slice(0, rest.indexOf("</span>")));
    expect(glyphs).toEqual(["2", "4"]);
  });

  it("show a picture to come as no thumbnail, and a branch's task as a chip", () => {
    const shown = board();
    shown.next[1] = {
      ...(shown.next[1] as Card),
      pictures: [{ src: null, caption: "A" }],
      task: "app/mining",
    };
    const next = region(render(shown), 'class="next card"');
    expect(next).toContain('<span class="thumb glyph">2</span>');
    expect(next).toContain('<small class="chip">app/mining</small>');
  });

  it("are not drawn after a single card", () => {
    const shown = board();
    shown.next = shown.next.slice(0, 1);
    expect(render(shown)).not.toContain('class="next card"');
  });
});

describe("the counts", () => {
  it("link to where their detail lives", () => {
    const html = render(board());
    for (const [to, value, label] of [
      ["/queue#you", "37", "waiting on you"],
      ["/queue#spot", "4", "auto-approved to spot-check"],
      ["/tasks", "12 · 5", "tasks in progress · in review"],
      ["/agents", "14", "agents live"],
    ]) {
      expect(html).toContain(`href="${to}"><b>${value}</b><span>${label}</span>`);
    }
    // A hint is a value of its own, never what the tile counts said again.
    expect(html).toContain("<span>waiting on you</span><small>across 9 documents</small>");
    for (const label of [
      "auto-approved to spot-check",
      "tasks in progress · in review",
      "agents live",
    ]) {
      expect(html, label).toContain(`<span>${label}</span></a>`);
    }
    expect(html).toContain("<span>answered by agents today</span></button>");
    expect(html).toContain(
      '<button type="button" class="tile card tone-answered" aria-controls="activity-panel"><b>6</b><span>answered by agents today</span>',
    );
    expect(html).not.toContain('href="#activity"');
    expect(html).toContain("across 9 documents");
  });

  it("are dashes without a task board, and all clear with nothing waiting", () => {
    const shown = board();
    shown.counts = { ...shown.counts, tasks: null, waiting: 0 };
    const html = render(shown);
    expect(html).toContain('href="/tasks"><b>–</b>');
    expect(html).toContain('href="/agents"><b>–</b>');
    expect(html).toContain("<small>all clear</small>");
  });
});

/** A plugin's region, a bar for each total, each half done. */
function regionOf(id: string, totals: number[]): Region {
  return {
    id,
    title: `${id} title`,
    note: `${id} note`,
    bars: totals.map((total, index) => ({
      key: `${id}-${index}`,
      name: `bar ${index}`,
      page: `docs/${id}.md`,
      title: "",
      done: Math.floor(total / 2),
      waiting: 0,
      total,
    })),
    legend: ["done", "waiting", "left"],
  };
}

describe("the progress", () => {
  it("shows the lamp pictures per maker and the design documents", () => {
    const progress = region(render(board()), 'class="progress panel card"');
    expect(progress).toContain("43%<small>69 of 161 lamps approved</small>");
    expect(count(progress, 'class="bar"')).toBe(6);
    expect(progress).toContain('href="/docs/lamps/south.md"');
    expect(progress).toContain("Far south");
    expect(progress).toContain("width:33.3%");
    expect(progress).toContain("width:11.1%");
    expect(progress).toContain('<a class="subtle" href="/docs/">browse all 64 →</a>');
    expect(progress).toContain("<p>41 decisions waiting</p>");
    expect(progress).toContain(
      '<p class="legend"><span class="dot ok"></span>approved <span class="dot wait"></span>waiting on you <span class="dot empty"></span>not drawn</p>',
    );
  });

  it("draws each plugin's region in its order, leaving out one with nothing to count", () => {
    const shown = board();
    shown.progress.regions = [
      regionOf("pictures", [4, 2]),
      regionOf("empty", [0, 0]),
      regionOf("bare", []),
      regionOf("audio", [10]),
    ];
    const progress = region(render(shown), 'class="progress panel card"');
    expect(count(progress, "data-region=")).toBe(2);
    expect(progress.indexOf('data-region="pictures"')).toBeLessThan(
      progress.indexOf('data-region="audio"'),
    );
    expect(progress).not.toContain("empty title");
    expect(progress).not.toContain("bare title");
    expect(progress).toContain("<h2>pictures title</h2>");
    expect(progress).toContain("50%<small>audio note</small>");
    expect(progress).toContain('href="/docs/pictures.md"');
    shown.progress.regions = [];
    const none = render(shown);
    expect(count(none, "data-region=")).toBe(0);
    expect(none).toContain("browse all 64");
  });

  it("says one decision waiting in the singular", () => {
    const shown = board();
    shown.progress.decisions_waiting = 1;
    expect(render(shown)).toContain("<p>1 decision waiting</p>");
  });
});

describe("the head", () => {
  it("greets by the hour and says how much waits", () => {
    const html = render(board());
    expect(html).toContain("<h1>Good morning</h1>");
    expect(html).toContain('<p class="head-state">37 cards wait on you across 9 documents.</p>');
  });
});

describe("an empty queue", () => {
  it("says nothing waits and points at the spot checks", () => {
    const html = render(board(emptyBoard));
    expect(html).toContain("Your queue is empty.");
    expect(html).toContain('<h2 class="hero-title">Nothing waits on you. ✦</h2>');
    expect(html).toContain(
      '<p class="hero-summary"><a href="/queue#spot">2 auto-approved cards</a> to spot-check.</p>',
    );
    expect(html).not.toContain('class="next card"');
    expect(html).not.toContain('class="pics"');
  });

  it("names one spot check in the singular, and none when there is none", () => {
    const shown = board(emptyBoard);
    shown.counts = { ...shown.counts, spot_check: 1 };
    expect(render(shown)).toContain("1 auto-approved card</a>");
    shown.counts = { ...shown.counts, spot_check: 0 };
    expect(render(shown)).not.toContain("hero-summary");
  });
});

describe("the held comments", () => {
  /** `shown` with `held` comments waiting on the reader to pass them on. */
  const holding = (shown: Board, held: number): Board => ({
    ...shown,
    counts: { ...shown.counts, held },
  });

  it("are counted under the greeting after the cards, leading to their group of the queue", () => {
    expect(render(holding(board(), 2))).toContain(
      '<p class="head-state">37 cards wait on you across 9 documents. ' +
        '<a href="/queue#held">2 held comments</a> to pass on.</p>',
    );
  });

  it("are all that is said where no card waits, the hero saying no card does", () => {
    const html = render(holding(board(emptyBoard), 1));
    expect(html).toContain(
      '<p class="head-state"><a href="/queue#held">1 held comment</a> to pass on.</p>',
    );
    expect(html).not.toContain("Your queue is empty.");
    expect(html).toContain('<h2 class="hero-title">No card waits on you.</h2>');
    expect(html).toContain("2 auto-approved cards</a> to spot-check.");
  });

  it("change nothing where none waits, as a board that names nobody sends none", () => {
    expect(render(holding(board(), 0))).toBe(render(board()));
    expect(render(holding(board(emptyBoard), 0))).toBe(render(board(emptyBoard)));
  });

  it("are read as the server counts them, and refused as anything but a count", () => {
    const counted = structuredClone(emptyBoard) as { counts: Record<string, unknown> };
    counted.counts.held = 3;
    expect(readBoard(counted)?.counts.held).toBe(3);
    counted.counts.held = "3";
    expect(readBoard(counted)).toBeNull();
    counted.counts.held = null;
    expect(readBoard(counted)).toBeNull();
  });
});

/** The home's row of page links. */
function links(html: string): string {
  const start = html.indexOf('class="dash-links"');
  const rest = html.slice(start);
  return rest.slice(0, rest.indexOf("</p>"));
}

describe("the home's links", () => {
  it("are the project's own pages and its register and guide, in order", () => {
    const plain = links(render(board()));
    expect(plain).not.toContain("/lamps");
    expect(plain).not.toContain("Decision register");
    expect(plain).not.toContain("How review works");
    const registered = links(render(board(), { ...PLAIN, register: "docs/DECISIONS.md" }));
    expect(registered).toContain('<a class="pill" href="/docs/DECISIONS.md">Decision register</a>');
    const view: ProjectView = {
      name: "Orbit",
      register: "notes/register.md",
      guide: "notes/REVIEW.md",
      nav: [
        { label: "⬡ lamps", title: "Lamp models", href: "/lamps", document: "docs/lamps/LAMPS.md" },
        { label: "♪ cues", title: "Audio cues", href: "/cues", document: "docs/audio/CUES.md" },
      ],
      entrypoints: ["notes"],
      base: "",
    };
    const shown = links(render(board(), view));
    const order = [
      '<a class="pill" href="/notes/">All docs</a>',
      '<a class="pill" href="/lamps">Lamp models</a>',
      '<a class="pill" href="/cues">Audio cues</a>',
      '<a class="pill" href="/notes/register.md">Decision register</a>',
      '<a class="pill" href="/notes/REVIEW.md">How review works</a>',
    ].map((link) => shown.indexOf(link));
    expect(order.every((at) => at >= 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(shown).toContain('<a class="pill" href="/queue">Full queue</a>');
  });
});

describe("home under its board", () => {
  it("draws the board at the minute it is", () => {
    const html = renderToStaticMarkup(<HomePage board={board()} />);
    expect(html).toContain('<div class="dash">');
    expect(html).toMatch(/<h1>Good (morning|afternoon|evening)<\/h1>/);
  });
});
