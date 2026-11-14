import { describe, expect, it } from "vitest";
import {
  advance,
  chip,
  chipRows,
  GAP,
  LINK,
  Lines,
  layoutAt,
  linkName,
  PHASE,
  ROLE,
  rowHeight,
  textWidth,
} from "./cardheight";
import type { Task } from "./wire";

type Card = Pick<Task, "slug" | "title" | "priority" | "parent" | "links" | "roles">;

const card = (
  slug: string,
  priority: number,
  title: string,
  parent: string | null,
  links: string[],
  roles: [string, string][],
): Card => ({
  slug,
  priority,
  title,
  parent,
  links,
  roles: roles.map(([role, handle]) => ({ handle, role, since: 0, expires: null, health: "live" })),
});

const task = (title: string, parent: string | null, links: string[], roles: [string, string][]) =>
  card("bulk/ready-1", 3, title, parent, links, roles);

const desk = (one: Card) => rowHeight(one, [], "desk");

describe("a card's lines", () => {
  // Measured in a 1280 px window: 238 px of "A task whose title is long enough to" fits a line,
  // 256 px of "Ready task number 1 in a long column" does not, and a word longer than a line breaks
  // inside itself.
  it("wrap a title where a card wraps it", () => {
    const lines = (text: string) => new Lines(2440).set(text, "bold", 140).count();
    expect(lines("")).toBe(0);
    expect(lines("   ")).toBe(0);
    expect(lines("Polish the product")).toBe(1);
    expect(lines("A task whose title is long enough to")).toBe(1);
    expect(lines("Ready task number 1 in a long column")).toBe(2);
    expect(lines("Merge the lamp catalogue's near-duplicates down to about ninety models")).toBe(3);
    expect(
      lines(
        "A task whose title is long enough to wrap over three lines on a card in a narrow column of the board, so the card's layout is tested",
      ),
    ).toBe(4);
    // 60 m's are 747 px, about three lines and a bit.
    expect(lines("m".repeat(60))).toBe(4);
  });

  it("break between words at any of Unicode's spaces, as Rust's", () => {
    const lines = (text: string) => new Lines(2440).set(text, "bold", 140).count();
    const words = "A task whose title is long enough to wrap";
    for (const space of [" ", "\t", "\n", " ", " ", " ", "　", "  "]) {
      expect(lines(words.replaceAll(" ", space))).toBe(2);
    }
    // A zero-width space is no space to Rust: the words are one, broken inside itself as a word
    // of as wide a letter (an x, in the bold face) in its place is.
    expect(lines(words.replaceAll(" ", "​"))).toBe(lines(words.replaceAll(" ", "x")));
  });

  // The slug breaks after its hyphens and slashes, 35 of its characters to a line, and its
  // priority follows it after a space, in a smaller face than the slug's.
  it("hold a slug and its priority on one while they fit", () => {
    const slug = (text: string, priority: string) =>
      new Lines(2410)
        .set(text, "mono", 115)
        .space("regular", 140)
        .set(priority, "bold", 101)
        .count();
    expect(slug("bulk/ready-1", "p3")).toBe(1);
    // 32 characters of slug are 221 px, and the priority 15 px more: 236 px in a p1's 241 px.
    expect(slug("inbox/agents-page-needs-redesign", "p1")).toBe(1);
    expect(slug("inbox/agents-page-needs-redesign-now", "p1")).toBe(2);
    expect(
      slug("app/a-very-long-task-slug-that-goes-on-and-on-to-test-wrapping-in-cards", "p1"),
    ).toBe(3);
    // Hyphens and slashes in a row, or leading, are each a piece of their own.
    expect(slug("a--b//c-", "p1")).toBe(1);
    expect(slug("-", "p1")).toBe(1);
  });

  it("measure each face as Helvetica draws it", () => {
    expect(advance("x", "mono")).toBe(600);
    expect(advance("i", "regular")).toBe(222);
    expect(advance("i", "bold")).toBe(278);
    expect(advance("'", "bold")).toBe(238);
    expect(advance("W", "regular")).toBe(944);
    expect(advance("M", "bold")).toBe(833);
    expect(advance("é", "regular")).toBe(556);
  });
});

describe("a card's height", () => {
  // A card with no chips draws at 101 px with a title of two lines; each row of chips adds its own
  // height, a parent its line, and chips too wide for one row wrap to more.
  it("reserves its lines and its rows of chips", () => {
    const two = "Ready task number 1 in a long column";
    expect(desk(task(two, null, [], []))).toBe(101);
    expect(desk(task("Short", null, [], []))).toBe(81);
    expect(desk(task(two, null, [], [["orchestrator", "quiet-plover"]]))).toBe(122);
    expect(desk(task(two, null, ["docs/DECISIONS.md"], [["orchestrator", "quiet-plover"]]))).toBe(
      139,
    );
    expect(desk(task(two, "app/parent", [], []))).toBe(121);
    // Two role chips of 160 px and more do not share a 244 px row.
    expect(
      desk(
        task(
          two,
          null,
          [],
          [
            ["orchestrator", "quiet-plover"],
            ["executor", "misty-osprey"],
          ],
        ),
      ),
    ).toBe(147);
    // Three short link chips share one row.
    expect(desk(task(two, null, ["docs/A.md", "docs/B.md", "docs/C.md"], []))).toBe(118);
  });

  // A card with build and test phases has their row above its links, 6 px below the title.
  it("gives its phases a row of their own", () => {
    const short = task("Short", null, [], []);
    const phases = (labels: string[]) => rowHeight(short, labels, "desk");
    expect(phases([])).toBe(81);
    expect(phases(["Build running · alpha"])).toBe(81 + 6 + 21);
    // Phases of 147 and 144 px do not share a 244 px row.
    expect(phases(["Build running · alpha", "Test queued · alpha"])).toBe(81 + 6 + 21 + 4 + 21);
  });

  // Cards from this repository's own board, drawn by the client in headless Chrome at 1280 and
  // 390 px, with the heights they drew at. (The machine is renamed.)
  it("reserves what the live board drew", () => {
    const observed = "Observed Build running · build-host-mini.local · partial inventory";
    const cards: [Card, string[], [number, number]][] = [
      [
        card("inbox/agents-page-needs-redesign", 1, "agents page needs redesign", null, [], []),
        [],
        [81, 101],
      ],
      [
        card(
          "tools/runs-page",
          1,
          "Each project gets a Runs page: what runs and waits now, every run with its steps and log, and trends, on a phone and wide",
          null,
          ["docs/analysis/reports-and-monitoring.md#What the owner opens"],
          [],
        ),
        [],
        [174, 174],
      ],
      [
        card(
          "tools/agent-cost-estimate",
          2,
          "Agents' cost is estimated in money at API list prices, from a dated price table and the four kinds of token",
          null,
          ["docs/analysis/reports-and-monitoring.md#Agents"],
          [],
        ),
        [],
        [138, 159],
      ],
      [
        card(
          "tests/signal-tests-inherit-ignored",
          0,
          "Main is red in a nightly started in the background: three board signal tests assume SIGINT and SIGQUIT are inherited at their default",
          null,
          [],
          [
            ["orchestrator", "silver-sparrow"],
            ["executor", "slate-osprey"],
          ],
        ),
        [],
        [208, 229],
      ],
      [
        card(
          "app/build-test-columns",
          1,
          "The board's build and test columns are always empty: nothing in this project reports a build or a test run",
          null,
          ["crates/symphony-api/src/task_stage.rs"],
          [
            ["orchestrator", "quiet-plover"],
            ["executor", "sable-crane"],
            ["reviewer", "dusky-owl"],
          ],
        ),
        [],
        [210, 230],
      ],
      [
        card(
          "app/timeline-clicks-reach-the-page",
          0,
          "A click in the task timeline runs only the timeline's control: Show all no longer also opens the + chat sheet",
          null,
          [],
          [["executor", "fleet-otter"]],
        ),
        [observed],
        [229, 229],
      ],
      [
        card(
          "perf/client-distribution-keeps-every-build",
          0,
          "The embedded client distribution keeps every build ever published: 266 MB in the server binary and in every consumer's build",
          null,
          [],
          [["executor", "glacial-ibis"]],
        ),
        [observed, observed],
        [274, 274],
      ],
    ];
    for (const [one, phases, drew] of cards) {
      expect([rowHeight(one, phases, "desk"), rowHeight(one, phases, "phone")], one.slug).toEqual(
        drew,
      );
    }
  });
});

describe("a card's chips", () => {
  it("fill a row before they wrap, and none take no height", () => {
    const role: [number, number] = [1000, 212];
    expect(chipRows([], 2440)).toBe(0);
    expect(chipRows([role, role], 2440)).toBe(212);
    expect(chipRows([role, role, role], 2440)).toBe(2 * 212 + GAP);
    // A chip wider than the card takes one row.
    expect(chipRows([[9000, 172]], 2440)).toBe(172);
    // A row is as tall as its tallest chip.
    expect(
      chipRows(
        [
          [1000, 172],
          [1000, 324],
          [1000, 172],
        ],
        2440,
      ),
    ).toBe(324 + GAP + 172);
  });

  // A chip as wide as its text keeps to one line; a link or phase chip wider than the card wraps
  // its words inside the card's width, a role chip never does.
  it("wrap inside themselves when too wide for the card, unless a role's", () => {
    expect(chip("TASKS.md", LINK, 2440)[1]).toBe(172);
    expect(chip("reports-and-monitoring.md#What the owner opens", LINK, 2440)).toEqual([
      2440,
      20 + 2 * 152,
    ]);
    expect(chip("executor misty-osprey", ROLE, 2440)[1]).toBe(212);
    expect(chip("executor ".repeat(6), ROLE, 2440)).toEqual([2440, 212]);
    const observed = "Observed Build running · Stefanos-MacBook-Pro.local · partial inventory";
    expect(chip(observed, PHASE, 2440)[1]).toBe(20 + 2 * 192);
    expect(chip("Build running · alpha", PHASE, 2440)[1]).toBe(212);
  });

  it("name a link by the last part of its path", () => {
    expect(linkName("docs/analysis/x.md#part")).toBe("x.md#part");
    expect(linkName("README.md")).toBe("README.md");
    expect(linkName("")).toBe("");
  });
});

describe("a card's layout", () => {
  // A window of 760 px or less draws the phone's narrower columns, and a p0 or p1 card's text is
  // 3 px narrower than another's, behind its wider left border.
  it("and its priority set how wide its text runs", () => {
    expect(layoutAt(390)).toBe("phone");
    expect(layoutAt(760)).toBe("phone");
    expect(layoutAt(761)).toBe("desk");
    expect(layoutAt(1280)).toBe("desk");
    expect(textWidth("desk", 2)).toBe(2440);
    expect(textWidth("desk", 1)).toBe(2410);
    expect(textWidth("desk", 0)).toBe(2410);
    expect(textWidth("phone", 3)).toBe(2360);
    expect(textWidth("phone", 0)).toBe(2330);
  });
});
