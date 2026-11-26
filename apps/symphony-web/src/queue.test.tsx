import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AskLine,
  active,
  askLate,
  askLine,
  CategoryChips,
  changed,
  checkQueue,
  chooseTopic,
  chosenTopic,
  counts,
  HeldGroup,
  keepRows,
  minutes,
  QueueGroup,
  QueuePage,
  queueCategory,
  readQueue,
  recallRows,
  visible,
  WaitingQueue,
} from "./queue";
import { TaskTag } from "./task-tag";
import { asking, CASES, memoryStorage, reply, server } from "./testing";
import { AtOnce } from "./viewport";
import type { Card, QueueHeld, QueueItem } from "./wire";

/** A card of `doc`, its slug `slug`. */
function card(doc: string, slug = "s"): Card {
  return { doc, slug, title: `Decision ${slug}`, doc_title: "D", why: "design call" };
}

/** Thirty picture cards and three engineering ones. */
function cards(): Card[] {
  return [
    ...Array.from({ length: 30 }, (_, n) => card("docs/pictures/CARDS.md", `pictures-${n}`)),
    ...Array.from({ length: 3 }, (_, n) => card("docs/engineering/CARDS.md", `engineering-${n}`)),
  ];
}

const draw = (selected: string, items = cards()) =>
  renderToStaticMarkup(<WaitingQueue items={items} selected={selected} onSelect={() => {}} />);

describe("a card's topic", () => {
  it.each(CASES.queueCategory)("of %j is %j, as crates/symphony-api says", (path, topic) => {
    expect(queueCategory(path)).toBe(topic);
  });
});

describe("the topics", () => {
  it("count each topic's cards, in the topics' order, summing to the group", () => {
    const found = counts(cards().map((one) => one.doc));
    expect([...found]).toEqual([
      ["engineering", 3],
      ["pictures", 30],
    ]);
    expect([...found.values()].reduce((sum, n) => sum + n, 0)).toBe(33);
  });

  it("show all for a topic chosen before that the queue no longer has", () => {
    const found = new Map([["lamps", 1]]);
    expect(active("lamps", found)).toBe("lamps");
    expect(active("gone", found)).toBe("");
    expect(active("", found)).toBe("");
  });
});

describe("what waits on the owner", () => {
  it("shows the chosen topic while it has cards, else all of them", () => {
    const items = [card("docs/lamps/a.md"), card("docs/DECISIONS.md"), card("docs/lamps/b.md")];
    expect(visible(items, "")).toHaveLength(3);
    const lamps = visible(items, "lamps");
    expect(lamps).toHaveLength(2);
    expect(lamps.every((one) => one.doc.startsWith("docs/lamps/"))).toBe(true);
    expect(visible(items, "gone")).toHaveLength(3);
  });

  it("draws every card, and its chips count each topic and all", () => {
    const all = draw("");
    expect(all.match(/data-card=/g)).toHaveLength(33);
    expect(all).toContain("All (33)");
    expect(all).toContain("pictures (30)");
    expect(all).toContain("engineering (3)");
    expect(all).toContain("Waiting on you (33)");
    expect(all).not.toContain("Showing");
  });

  it("keeps only the chosen topic's rows, and says how many of all it shows", () => {
    const pictures = draw("pictures");
    expect(pictures.match(/data-card=/g)).toHaveLength(30);
    expect(pictures).not.toContain('data-card="docs/engineering/');
    expect(pictures).toContain("Showing 30 of 33");
    const engineering = draw("engineering");
    expect(engineering.match(/data-card=/g)).toHaveLength(3);
    expect(engineering).not.toContain('data-card="docs/pictures/');
  });

  it("falls back to all for a topic chosen before that is gone", () => {
    expect(draw("removed-folder")).toBe(draw(""));
  });

  it("says when nothing waits", () => {
    const empty = draw("pictures", []);
    expect(empty).toContain("All (0)");
    expect(empty).toContain("nothing to decide");
    expect(empty).not.toContain("data-card=");
  });
});

describe("the topic chips", () => {
  const chips = (selected: string) =>
    renderToStaticMarkup(
      <CategoryChips
        topics={
          new Map([
            ["engineering", 3],
            ["pictures", 30],
          ])
        }
        selected={selected}
        onSelect={() => {}}
      />,
    );
  const pressed = (markup: string) =>
    [...markup.matchAll(/aria-pressed="true" data-category="([^"]*)"/g)].map((match) => match[1]);

  it("press the chosen topic's, or all's", () => {
    expect(pressed(chips("pictures"))).toEqual(["pictures"]);
    expect(pressed(chips(""))).toEqual([""]);
    expect(pressed(chips("gone"))).toEqual([""]);
  });

  it("are a labelled group of small outline buttons", () => {
    const markup = chips("");
    expect(markup).toContain('role="group" aria-label="Review categories"');
    expect(markup.match(/class="dx-button" data-style="outline" data-size="sm"/g)).toHaveLength(3);
  });
});

describe("this device's topic", () => {
  beforeEach(() => vi.stubGlobal("localStorage", memoryStorage()));
  afterEach(() => vi.unstubAllGlobals());

  it("is kept for the next visit, and all clears it", () => {
    expect(chosenTopic()).toBe("");
    chooseTopic("pictures");
    expect(chosenTopic()).toBe("pictures");
    chooseTopic("");
    expect(chosenTopic()).toBe("");
  });

  it("is all where the browser keeps nothing", () => {
    vi.stubGlobal("localStorage", memoryStorage(true));
    chooseTopic("pictures");
    expect(chosenTopic()).toBe("");
  });
});

describe("the rows each group showed", () => {
  it("are kept in the queue's order and read back as kept", () => {
    const kept = keepRows([1, 0, 3]);
    expect(kept).toBe("1 0 3");
    expect(recallRows(kept)).toEqual([1, 0, 3]);
  });

  it("are none for a group the kept line does not say, or says unreadably", () => {
    expect(recallRows("")).toEqual([null, null, null]);
    expect(recallRows("2 x")).toEqual([2, null, null]);
    expect(recallRows("  4\t+5 -1 9")).toEqual([4, 5, null]);
  });
});

describe("the queue read", () => {
  it("is the server's answer as crates/symphony-api writes it", () => {
    const queue = readQueue(CASES.samples.queue);
    expect(queue?.waiting.map((one) => one.slug)).toEqual(["lamp-colour", "s"]);
    expect(queue?.agent[0]?.branch).toBe("app/lamps");
    expect(queue?.agent.map((one) => one.ask?.holder ?? null)).toEqual([null, null, "umber-stoat"]);
    expect(queue?.agent[1]?.ask).toEqual({ id: 3, asked: 1000 });
    expect(queue?.held).toEqual([
      {
        id: 4,
        href: "/@proposal~lamps/docs/lamps/LAMPS.md#held-4",
        doc_title: "Lamps",
        author: "ana",
      },
    ]);
    expect(readQueue(CASES.samples.emptyQueue)).toEqual(CASES.samples.emptyQueue);
    expect(readQueue(CASES.samples.emptyQueue)?.held).toBeUndefined();
  });

  it("takes a field it does not draw as the server sends it, or without it", () => {
    expect(readQueue({ waiting: [], spot_check: [], agent: [], newer: 1 })).not.toBeNull();
  });

  it.each([
    ["nothing", null],
    ["no agent group", { waiting: [], spot_check: [] }],
    [
      "a card without its title",
      { waiting: [{ slug: "s", doc: "d", doc_title: "t", why: "w" }], spot_check: [], agent: [] },
    ],
    [
      "a card postponed by a word",
      { waiting: [{ ...card("d"), postponed: "yes" }], spot_check: [], agent: [] },
    ],
    [
      "a card's task of another type",
      { waiting: [{ ...card("d"), task: 1 }], spot_check: [], agent: [] },
    ],
    [
      "an item's branch of another type",
      {
        waiting: [],
        spot_check: [],
        agent: [{ href: "", path: "", slug: "", title: "", kind: "", text: "", branch: 2 }],
      },
    ],
    ...[
      ["an item's ask without when it was said", { id: 3 }],
      ["an item's ask numbered by a word", { id: "3", asked: 1 }],
      ["an item's ask held by a number", { id: 3, asked: 1, holder: 7 }],
      ["an item's ask held since a word", { id: 3, asked: 1, holder: "a", held: "now" }],
      ["an item's ask that is a word", "ask 3"],
    ].map(([name, ask]): [unknown, unknown] => [
      name,
      {
        waiting: [],
        spot_check: [],
        agent: [{ href: "", path: "", slug: "", title: "", kind: "", text: "", ask }],
      },
    ]),
    [
      "an item without its text",
      {
        waiting: [],
        spot_check: [{ href: "", path: "", slug: "", title: "", kind: "" }],
        agent: [],
      },
    ],
    ...[
      ["held comments that are no list", { id: 1 }],
      ["held comments that are null", null],
      ["a held comment numbered by a word", [{ id: "1", href: "", doc_title: "", author: "" }]],
      ["a held comment numbered by a fraction", [{ id: 1.5, href: "", doc_title: "", author: "" }]],
      ["a held comment without its author", [{ id: 1, href: "", doc_title: "" }]],
      ["a held comment whose link is no text", [{ id: 1, href: 2, doc_title: "", author: "" }]],
    ].map(([name, held]): [unknown, unknown] => [
      name,
      { waiting: [], spot_check: [], agent: [], held },
    ]),
  ])("is none for %s", (_, value) => {
    expect(readQueue(value)).toBeNull();
  });
});

describe("a card's ask", () => {
  it.each([
    [0, "0 min"],
    [59, "0 min"],
    [-5, "0 min"],
    [14 * 60 + 30, "14 min"],
    [3600, "1 h"],
    [72 * 60, "1 h 12 min"],
    [74 * 3600 + 31 * 60, "74 h 31 min"],
  ])("ages %i s as %j, as just tasks asks does", (seconds, said) => {
    expect(minutes(seconds)).toBe(said);
  });

  it("is late from ten minutes, and not a second before", () => {
    expect(askLate(1000, 1599)).toBe(false);
    expect(askLate(1000, 1600)).toBe(true);
  });

  it("says its age and that nobody holds it, late once it is", () => {
    expect(askLine({ id: 3, asked: 1000 }, 1000 + 240)).toBe("4 min · held by nobody");
    expect(askLine({ id: 3, asked: 1000, holder: null, held: null }, 1000 + 74 * 3600)).toBe(
      "74 h, late · held by nobody",
    );
  });

  it("names its holder, and for how long once the board says since when", () => {
    const ask = { id: 3, asked: 1000, holder: "umber-stoat", held: 1120 };
    expect(askLine(ask, 1000 + 14 * 60)).toBe("14 min, late · held by umber-stoat for 12 min");
    expect(askLine({ ...ask, held: null }, 1060)).toBe("1 min · held by umber-stoat");
  });

  it("draws as a line of its row that says whether it is late", () => {
    expect(renderToStaticMarkup(<AskLine ask={{ id: 3, asked: 1000 }} now={1060} />)).toBe(
      '<p class="queue-ask muted" data-ask="3" data-late="false">1 min · held by nobody</p>',
    );
    expect(renderToStaticMarkup(<AskLine ask={{ id: 3, asked: 1000 }} now={1600} />)).toContain(
      'data-late="true">10 min, late · held by nobody',
    );
  });

  it("is on the row of the card it is about, and on no other", () => {
    const item = (slug: string, ask?: QueueItem["ask"]): QueueItem => ({
      href: `/docs/A.md#${slug}`,
      path: "docs/A.md",
      slug,
      title: slug,
      kind: "approved",
      text: "B",
      ...(ask === undefined ? {} : { ask }),
    });
    const markup = renderToStaticMarkup(
      <AtOnce.Provider value={true}>
        <QueueGroup
          id="agent"
          title="Waiting on the agent"
          items={[item("held", { id: 4, asked: 1000 }), item("unasked"), item("null", null)]}
          empty="none"
          now={1000 + 3600}
        />
      </AtOnce.Provider>,
    );
    expect(markup.match(/class="queue-ask muted"/g)).toHaveLength(1);
    expect(markup).toMatch(
      /data-card="docs\/A\.md#held".*?<p class="muted">B<\/p><p class="queue-ask muted" data-ask="4" data-late="true">1 h, late · held by nobody<\/p>/,
    );
  });
});

describe("the feed's events", () => {
  it.each([
    "queue-item",
    "queue-item-order",
    "spot-check",
    "spot-check-order",
    "agent-queue",
    "agent-queue-order",
    "held",
  ])("move the queue with %s", (region) => {
    expect(changed({ changes: [{ region: "docs" }, { region }] })).toBe(true);
  });

  it.each([
    ["another region", { changes: [{ region: "docs" }] }],
    ["no changes", { changes: [] }],
    ["changes of another type", { changes: "queue-item" }],
    ["nothing", undefined],
  ])("leave it with %s", (_, delta) => {
    expect(changed(delta)).toBe(false);
  });
});

/** The comment `id` held on `doc` by `author`, as the queue lists it. */
function held(id: number, doc = "docs/A.md", author = "ana"): QueueHeld {
  return { id, href: `/${doc}#held-${id}`, doc_title: "A", author };
}

describe("the held comments", () => {
  it("lead each to its box on its document's page, under its project, saying who wrote it", () => {
    const markup = renderToStaticMarkup(
      <AtOnce.Provider value={true}>
        <HeldGroup items={[held(4), held(9, "@x~y/docs/B.md", "cy")]} />
      </AtOnce.Provider>,
    );
    expect(markup).toContain('<section id="held" class="card queue-group">');
    expect(markup).toContain("<h2>Held comments (2)</h2>");
    expect(markup).toMatch(
      /<li data-held="4">.*?<div class="queue-row"><div><a href="\/docs\/A\.md#held-4">A<\/a><p class="muted">ana<\/p><\/div><\/div>/,
    );
    expect(markup).toContain('<a href="/@x~y/docs/B.md#held-9">A</a><p class="muted">cy</p>');
    expect(markup).not.toContain("button");
  });
});

describe("a card's task", () => {
  it("is a chip leading to its page, and none without one", () => {
    expect(renderToStaticMarkup(<TaskTag task="app/lamps" />)).toBe(
      '<a class="chip" href="/task/app/lamps">app/lamps</a>',
    );
    expect(renderToStaticMarkup(<TaskTag task={null} />)).toBe("");
    expect(renderToStaticMarkup(<TaskTag />)).toBe("");
  });
});

describe("the queue's check", () => {
  afterEach(() => vi.unstubAllGlobals());

  /** A check of the queue whose waits are only recorded, never waited, and what it was told. */
  const check = (asked: ReturnType<typeof server>, signal = new AbortController().signal) => {
    const told: unknown[] = [];
    const waits: number[] = [];
    const done = checkQueue(
      "/api/queue",
      () => "kept",
      signal,
      {
        answered: (read, etag) => told.push(["answered", read, etag]),
        failed: (why) => told.push(["failed", why]),
      },
      async (backoff, ended) => {
        waits.push(backoff.wait(ended));
      },
    );
    return { told, waits, done, asked };
  };

  it("names the kept copy's tag, and a 304 is the copy shown", async () => {
    const asked = server();
    const { told, waits, done } = check(asked);
    await asking(asked, 1);
    asked[0]?.answer(new Response(null, { status: 304 }));
    await done;
    expect(told).toEqual([["answered", null, ""]]);
    expect(waits).toEqual([]);
  });

  it("asks again, waiting longer each time, after a refused answer and a failed read", async () => {
    const asked = server();
    const { told, waits, done } = check(asked);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, { waiting: "not a list" }, "x"));
    await asking(asked, 2);
    asked[1]?.answer(reply(503, {}));
    await asking(asked, 3);
    asked[2]?.answer(reply(200, CASES.samples.queue, "q1"));
    await done;
    expect(told).toEqual([
      ["failed", "The queue's answer is not a queue"],
      ["failed", "The server answered 503"],
      ["answered", CASES.samples.queue, "q1"],
    ]);
    expect(waits).toEqual([1_000, 2_000]);
    expect(asked).toHaveLength(3);
  });

  it("stops, telling nothing and its read given up, once the page that asked is gone", async () => {
    const asked = server();
    const page = new AbortController();
    const { told, waits, done } = check(asked, page.signal);
    await asking(asked, 1);
    expect(asked[0]?.signal?.aborted).toBe(false);
    page.abort();
    expect(asked[0]?.signal?.aborted).toBe(true);
    asked[0]?.answer(reply(200, CASES.samples.queue, "q1"));
    await done;
    expect(told).toEqual([]);
    expect(waits).toEqual([]);
  });
});

describe("the queue's page", () => {
  beforeEach(() => vi.stubGlobal("localStorage", memoryStorage()));
  afterEach(() => vi.unstubAllGlobals());

  it("opens on its heading and the ghost of each group while it loads", () => {
    const markup = renderToStaticMarkup(<QueuePage scope="" refreshed={0} />);
    expect(markup).toContain('data-ready="loading" aria-busy="true"');
    expect(markup).toContain("<h1>Review queue</h1>");
    expect(markup).toContain('<a href="/">← Home</a>');
    expect(markup).toContain("Loading the queue…");
    expect(markup.match(/class="card ghost-group"/g)).toHaveLength(3);
    // The rows each group is guessed to hold, the first with its rank and its chips.
    expect(markup.match(/class="ghost-row"/g)).toHaveLength(4 + 2 + 2);
    expect(markup.match(/class="ghost-rank"/g)).toHaveLength(4);
    expect(markup.match(/class="ghost-pill"/g)).toHaveLength(2);
  });

  it("draws as many ghost rows as each group showed last, twelve at most, and says a group was empty", () => {
    localStorage.setItem("queue-rows", "20 0 1");
    const markup = renderToStaticMarkup(<QueuePage scope="" refreshed={0} />);
    expect(markup.match(/class="ghost-rank"/g)).toHaveLength(12);
    expect(markup.match(/class="ghost-row"/g)).toHaveLength(12 + 1);
    expect(markup.match(/class="ghost-none"/g)).toHaveLength(1);
  });

  it("reserves the held comments' group after what waits on the owner where it listed some", () => {
    localStorage.setItem("queue-rows", "1 0 0");
    localStorage.setItem("queue-held-rows", "2");
    const markup = renderToStaticMarkup(<QueuePage scope="" refreshed={0} />);
    const groups = [...markup.matchAll(/class="card ghost-group" data-for="(\w+)"/g)];
    expect(groups.map((group) => group[1])).toEqual(["you", "held", "spot", "agent"]);
    expect(markup.match(/class="ghost-row"/g)).toHaveLength(1 + 2);
    localStorage.setItem("queue-held-rows", "0");
    const none = renderToStaticMarkup(<QueuePage scope="" refreshed={0} />);
    expect(none).not.toContain('data-for="held"');
  });

  it("leads home under its checkout", () => {
    const markup = renderToStaticMarkup(<QueuePage scope="proposal~x" refreshed={0} />);
    expect(markup).toContain('<a href="/@proposal~x/">← Home</a>');
  });
});
