import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixture from "../../../crates/symphony-api/fixtures/task-gates.json";
import {
  drawn,
  GateRow,
  GatesView,
  gatesShown,
  kept,
  remember,
  roomFor,
  TaskGatesPanel,
  width,
} from "./gates";
import {
  DEFAULT_ROOM,
  type Gates,
  KEPT,
  READING,
  type Room,
  readGate,
  readTaskGates,
  type Waiter,
} from "./gates-model";
import { Parts, PartsContext, type Shown } from "./parts";
import { Failure } from "./request";
import { gatesPart, type TaskEvent } from "./task-live";
import { aRole, aTask, memoryStorage, reply, until } from "./testing";
import { NOT_IN_TIME } from "./trouble";
import type { Gate, GatesSeen, TaskGates } from "./wire";

/** 2026-11-11 13:10 UTC, the day of every time in the fixture. */
const NOW = 1_794_402_600;

/** The A pictures' task: certificate missing, dependency waived, nightly met, review met. */
const pictured = () => readTaskGates(structuredClone(fixture)) as TaskGates;

/** A gate as the section reads it from `value`. */
function gate(value: object): Gate {
  const read = readGate(value);
  if (read === null) throw new Error(`not a gate: ${JSON.stringify(value)}`);
  return read;
}

const answer = (gates: Gate[]): TaskGates => ({ slug: "tools/x", branch: "tools/x", gates });

const read = (gates: TaskGates): Gates => ({ kind: "read", answer: gates });

const room = (count: number, waiting: number, drawn: [number, number] | null = null): Room => ({
  seen: { count, waiting, unsigned: 0 },
  drawn,
});

/** A task in review, which waits on the gates it waits on. */
const IN_REVIEW: Waiter = aTask("tools/x", { status: "review" });

/** The section of a task in review, or of `task`, in a window `wide` px wide. */
const shown = (
  gates: Gates,
  held: Room = DEFAULT_ROOM,
  wide: number | null = null,
  task: Waiter | null = IN_REVIEW,
) =>
  renderToStaticMarkup(
    <GatesView gates={gates} task={task} now={NOW} room={held} width={wide} onRetry={() => {}} />,
  );

const holding = (held: Room, wide: number | null = null) => shown(READING, held, wide);

/** Each `<li …>…</li>` of `html`, in order. */
function rows(html: string): string[] {
  return html
    .split("<li ")
    .slice(1)
    .map((row) => row.split("</li>")[0] as string);
}

/** A gate's row as drawn: its name, state, detail and who-and-when markup. */
const row = (name: string, state: string, detail: string, by: string) =>
  `class="gate gate-${state}" data-gate="${name}" data-state="${state}"><span class="gate-name"><span class="gate-label" dir="auto">${name}</span></span><span class="pill gate-state">${state}</span>${detail}${by}`;

/** Whether each row's `gate-by` holds a shadow bar. */
const bylinesHeld = (html: string) =>
  rows(html).map((one) =>
    (one.split('<span class="gate-by">')[1] ?? "").startsWith('<span class="gate-bar"'),
  );

/** The panel as a task page draws it. */
const panel = (seen?: GatesSeen | null) =>
  renderToStaticMarkup(
    seen === undefined ? (
      <TaskGatesPanel slug="tools/x" scope="" task={IN_REVIEW} />
    ) : (
      <TaskGatesPanel slug="tools/x" scope="" task={IN_REVIEW} seen={seen} />
    ),
  );

// The reader's clock is UTC unless a test says otherwise, as the Dioxus client's host was.
beforeEach(() => {
  vi.spyOn(Date.prototype, "getTimezoneOffset").mockReturnValue(0);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("the section", () => {
  it("shows what the pictures' task waits on, its counts and four gates, missing first", () => {
    const html = shown(read(pictured()));
    expect(html).toMatch(
      /^<section class="task-gates card" data-gates="read" aria-labelledby="task-gates-title">/,
    );
    expect(html).toContain(
      '<div class="gates-head"><h2 class="eyebrow" id="task-gates-title">Gates</h2><span class="pill gate-missing gates-waits">waits on <span dir="auto">certificate</span></span><span class="gates-counts">2 met · 1 waived · 1 missing</span></div>',
    );
    // Read before the task's page: the pill's outline where the room kept says one was drawn,
    // none where it says nothing waited.
    expect(shown(read(pictured()), room(4, 1), null, null)).toContain(
      '<h2 class="eyebrow" id="task-gates-title">Gates</h2><span class="pill gates-waits gate-shadow"><span class="gate-bar" style="--w:9em"></span></span><span class="gates-counts">2 met · 1 waived · 1 missing</span>',
    );
    expect(shown(read(pictured()), room(4, 0), null, null)).not.toContain("gates-waits");
    const drawnRows = rows(html);
    expect(drawnRows).toHaveLength(4);
    expect(drawnRows[0]).toBe(
      row(
        "certificate",
        "missing",
        '<span class="gate-detail"><span dir="auto">Verify on the merge tree 4be91c2 running, since main moved at 14:50</span></span>',
        '<span class="gate-by"><span dir="auto">studio</span> · <span dir="auto">12:52</span></span>',
      ),
    );
    expect(drawnRows[1], "only the slug the link names is linked").toBe(
      row(
        "dependency",
        "waived",
        '<span class="gate-detail"><span dir="auto">Waits on <a href="/task/tools/slot-pool-by-tree">tools/slot-pool-by-tree</a>, waived</span><span class="gate-why">: <span dir="auto">tools/slot-pool-by-tree shares no code with this branch</span></span></span>',
        '<span class="gate-by"><span dir="auto">cobalt-otter</span> · <span dir="auto">12:12</span></span>',
      ),
    );
    expect(drawnRows[2]).toContain(
      '<span class="gate-detail"><span dir="auto">Night of 11-10 green on main a71f0d3</span></span>',
    );
    expect(drawnRows[3], "only the round is linked").toContain(
      '<span dir="auto">Approved in <a href="/task/tools/verify-stops-when-moved#message-124">round 2 of 2</a></span>',
    );
    expect(html, "every word is drawn, none kept for a hover").not.toContain(" title=");
  });

  it("is absent for a task nothing gates, and for one the board has not", () => {
    expect(shown(read(answer([])))).toBe("");
    expect(shown({ kind: "read", answer: { slug: "", branch: "", gates: [] } })).toBe("");
  });

  it("says a failed read could not read the gates, and offers a retry", () => {
    const html = shown({ kind: "failed", why: "The server answered 500." });
    expect(html).toMatch(
      /^<section class="task-gates card" data-gates="failed" aria-labelledby="task-gates-title">/,
    );
    expect(html).toContain(
      '<p class="gates-note" role="status">The gates could not be read: The server answered 500. The rest of the page still works.</p>',
    );
    expect(html).toContain(
      '<button class="dx-button" data-style="outline" data-size="default" type="button">Retry</button>',
    );
    expect(html).not.toContain("<li");
    expect(shown({ kind: "failed", why: " " })).toContain(
      ">The gates could not be read. The rest of the page still works.</p>",
    );
  });

  it("says a read the server never answered took too long, not to check before sending", () => {
    const html = shown({ kind: "slow" });
    expect(html).toContain(
      '<p class="gates-note" role="status">The gates took too long to answer and were given up on. The rest of the page still works.</p>',
    );
    expect(html).not.toContain("sending again");
    expect(html).toContain(">Retry</button>");
  });

  it("says its retry reads while a read is out after the failed one, and asks for none more", () => {
    for (const gates of [{ kind: "failed", why: "The server answered 500." }, { kind: "slow" }]) {
      const html = renderToStaticMarkup(
        <GatesView
          gates={gates as Gates}
          task={IN_REVIEW}
          now={NOW}
          room={DEFAULT_ROOM}
          width={null}
          retrying
          onRetry={() => {}}
        />,
      );
      expect(html, gates.kind).toContain(
        '<button class="dx-button" data-style="outline" data-size="default" type="button" disabled="">Retrying…</button>',
      );
      expect(html, "the failure stays said meanwhile").toContain('class="gates-note"');
      expect(html, "and no region is held busy for it").not.toContain("aria-busy");
    }
    expect(shown({ kind: "slow" }), "none out").not.toContain("disabled");
  });

  it("retries by its button alone", () => {
    const retried: string[] = [];
    const view = GatesView({
      gates: { kind: "slow" },
      task: IN_REVIEW,
      now: NOW,
      room: DEFAULT_ROOM,
      width: null,
      onRetry: () => retried.push("retry"),
    });
    const section = view as ReactElement<{ children: ReactElement<{ onClick?: () => void }>[] }>;
    for (const child of section.props.children) child.props.onClick?.();
    expect(retried).toEqual(["retry"]);
  });
});

describe("while it reads", () => {
  it("is its own outline, a shadow row a gate it expects", () => {
    const html = holding(room(4, 1));
    expect(html).toMatch(
      /^<section class="task-gates card" data-gates="reading" aria-labelledby="task-gates-title" aria-busy="true">/,
    );
    expect(html).toContain(
      '<span class="pill gates-waits gate-shadow"><span class="gate-bar" style="--w:9em"></span></span><span class="gates-counts"><span class="gate-bar" style="--w:11em"></span></span>',
    );
    expect(html).toContain('<p class="gates-said" role="status">Reading the gates…</p>');
    expect(html).toContain('<ul class="gates" aria-hidden="true">');
    const drawnRows = rows(html);
    expect(drawnRows).toHaveLength(4);
    for (const one of drawnRows) {
      expect(one).toMatch(/^class="gate gate-shadow">/);
      expect(one, "the state's own pill, empty").toContain(
        '<span class="pill gate-state"> </span>',
      );
      for (const part of ["gate-label", "gate-detail", "gate-by"]) {
        expect(one, part).toContain(`class="${part}"`);
      }
    }
    expect(html).not.toContain("Retry");
    const calm = holding(room(2, 0));
    expect(rows(calm)).toHaveLength(2);
    expect(calm, "nothing to wait on").not.toContain("gates-waits");
    expect(holding(room(0, 0)), "none is held for a task known to have no gates").toBe("");
  });

  it("draws each shadow row's bars at the widths the Dioxus outline drew", () => {
    const html = holding(room(4, 0));
    const bars = [...html.matchAll(/--w:([^"]+)"/g)].map((match) => match[1]);
    expect(bars).toEqual([
      "11em",
      ...["6em", "55%", "7em"],
      ...["7em", "72%", "7em"],
      ...["8em", "89%", "7em"],
      ...["6em", "71%", "7em"],
    ]);
  });

  it("holds the height this browser drew in a window as wide", () => {
    const at390 = holding(room(4, 1, [390, 612]), 390);
    expect(at390, "the height itself, so the outline is no taller than its answer").toContain(
      'aria-busy="true" style="height:612px">',
    );
    for (const wide of [391, 1280, null]) {
      expect(holding(room(4, 1, [390, 612]), wide), `${wide}`).toContain('aria-busy="true">');
    }
  });

  it("draws no who and when for the gates that had none", () => {
    const withUnsigned = (unsigned: number): Room => ({
      seen: { count: 3, waiting: 1, unsigned },
      drawn: null,
    });
    expect(bylinesHeld(holding(withUnsigned(0)))).toEqual([true, true, true]);
    expect(
      bylinesHeld(holding(withUnsigned(1))),
      "the review gate before its first verdict says no one and no time",
    ).toEqual([true, true, false]);
    expect(bylinesHeld(holding(withUnsigned(3)))).toEqual([false, false, false]);
    expect(bylinesHeld(holding(withUnsigned(9))), "more unsigned than gates is all").toEqual([
      false,
      false,
      false,
    ]);
  });
});

describe("the panel", () => {
  it("is the outline the server or this browser expects until it reads", () => {
    const html = panel();
    expect(html).toMatch(
      /^<div id="gates-tools\/x" class="task-gates-slot"><section class="task-gates card" data-gates="reading"/,
    );
    expect(rows(html), "four, nothing known").toHaveLength(4);
    const two = panel({ count: 2, waiting: 0, unsigned: 1 });
    expect(rows(two)).toHaveLength(2);
    expect(two).not.toContain("gates-waits");
    expect(bylinesHeld(two)).toEqual([true, false]);
    expect(
      panel({ count: 0, waiting: 0, unsigned: 0 }),
      "a task the server last answered with no gates holds no room",
    ).toBe('<div id="gates-tools/x" class="task-gates-slot"></div>');
    expect(rows(panel(null)), "nothing said by the server").toHaveLength(4);
  });

  /** The panel in a page whose server answers its gates as `answered` does. */
  async function inPage(answered: () => Response, task: Waiter | null = IN_REVIEW) {
    vi.stubGlobal("document", { getElementById: () => null });
    const asked: string[] = [];
    vi.stubGlobal("fetch", async (url: string) => {
      asked.push(url);
      return answered();
    });
    const mount = new AbortController();
    const parts = new Parts<TaskEvent>(mount.signal);
    const part = gatesPart("", "tools/x");
    parts.open(part);
    await until(() => {
      const shown = parts.shown(part);
      expect(shown.answered || shown.error !== null).toBe(true);
    });
    const html = renderToStaticMarkup(
      <PartsContext.Provider value={parts}>
        <TaskGatesPanel slug="tools/x" scope="" task={task} />
      </PartsContext.Provider>,
    );
    mount.abort();
    return { html, asked };
  }

  it("draws the answer its page's store holds, asked once under the page's checkout", async () => {
    const { html, asked } = await inPage(() => reply(200, fixture));
    expect(asked).toEqual(["/api/task-gates?slug=tools%2Fx"]);
    expect(html).toContain('data-gates="read"');
    expect(rows(html)).toHaveLength(4);
    expect(html).toContain("gates-waits");
    // Before the task's page has come, the gates are drawn all the same, the pill its outline.
    const alone = await inPage(() => reply(200, fixture), null);
    expect(rows(alone.html)).toHaveLength(4);
    expect(alone.html).toContain('<span class="pill gates-waits gate-shadow">');
    expect(alone.html).not.toContain("waits on");
  });

  it("draws nothing for a task the server has no gates for, and says a failed read in its place", async () => {
    const none = await inPage(() => reply(404, { error: "No such task" }));
    expect(none.html).toBe('<div id="gates-tools/x" class="task-gates-slot"></div>');
    const failed = await inPage(() => reply(500, { error: "Board snapshot unavailable." }));
    expect(failed.html).toContain('data-gates="failed"');
    expect(failed.html).toContain(
      "The gates could not be read: Board snapshot unavailable (the server answered 500). The rest of the page still works.",
    );
    const unread = await inPage(() =>
      reply(200, { slug: "tools/x", branch: "tools/x", gates: [{ name: 1 }] }),
    );
    expect(unread.html).toContain("s gates could not be read. The rest of the page still works.");
  });

  it("says its retry reads while the store's read is out after a failed one", async () => {
    vi.stubGlobal("document", { getElementById: () => null });
    let held: (answer: Response) => void = () => {};
    let reads = 0;
    vi.stubGlobal("fetch", async () => {
      reads += 1;
      if (reads === 1) return reply(500, { error: "Board snapshot unavailable." });
      return new Promise<Response>((settle) => {
        held = settle;
      });
    });
    const mount = new AbortController();
    const parts = new Parts<TaskEvent>(mount.signal);
    const part = gatesPart("", "tools/x");
    const section = () =>
      renderToStaticMarkup(
        <PartsContext.Provider value={parts}>
          <TaskGatesPanel slug="tools/x" scope="" task={IN_REVIEW} />
        </PartsContext.Provider>,
      );
    parts.open(part);
    await until(() => expect(parts.shown(part).error).not.toBeNull());
    expect(section()).toContain('type="button">Retry</button>');
    // The reader's Retry, while its read is out.
    const asked = parts.read(part);
    expect(section()).toContain('type="button" disabled="">Retrying…</button>');
    held(reply(200, fixture));
    await asked;
    expect(section()).toContain('data-gates="read"');
    expect(section()).not.toContain("Retry");
    mount.abort();
  });

  it("holds the room this browser kept for the task, at the height it drew in a window as wide", () => {
    const storage = memoryStorage();
    storage.setItem(KEPT, "2 1 0 390 140 tools/x\n3 0 0 390 200 tools/y");
    vi.stubGlobal("localStorage", storage);
    vi.stubGlobal("innerWidth", 390.2);
    const html = panel();
    expect(rows(html)).toHaveLength(2);
    expect(html).toContain('aria-busy="true" style="height:140px">');
    expect(panel({ count: 2, waiting: 1, unsigned: 0 }), "the server agrees").toContain(
      "height:140px",
    );
    const other = panel({ count: 3, waiting: 1, unsigned: 0 });
    expect(rows(other), "the server says otherwise").toHaveLength(3);
    expect(other).not.toContain("height:");
  });
});

describe("a gate", () => {
  it("has its state's colour class, and unknown is what a failing plugin gives", () => {
    const html = shown(
      read(
        answer([
          gate({ name: "hold", state: "held", by: "owner", why: "the owner holds it", detail: "" }),
          gate({
            name: "review",
            state: "unknown",
            by: "shelf-gates",
            detail: "shelf-gates did not answer in 10 s",
          }),
          gate({ name: "later", state: "expired", detail: "a word this build does not know" }),
        ]),
      ),
    );
    const drawnRows = rows(html);
    expect(drawnRows[0]).toMatch(/^class="gate gate-held" data-gate="hold" data-state="held">/);
    expect(drawnRows[0], "a why with no detail before it takes no colon").toContain(
      '<span class="gate-detail"><span class="gate-why"><span dir="auto">the owner holds it</span></span></span>',
    );
    expect(drawnRows[1]).toContain(
      '<span class="pill gate-state">unknown</span><span class="gate-detail"><span dir="auto">shelf-gates did not answer in 10 s</span></span>',
    );
    expect(drawnRows[2]).toContain('data-state="unknown"');
    expect(html).toContain(
      '<span class="pill gate-held gates-waits">waits on <span dir="auto">hold</span></span><span class="gates-counts">2 unknown · 1 held</span>',
    );
  });

  it("draws its detail's backticks as code, escapes markup and links only its evidence", () => {
    const html = shown(
      read(
        answer([
          gate({
            name: "check",
            state: "missing",
            detail: "`just verify` red at <b>9ac5149</b>: exit 1",
            commit: "9ac5149e0",
          }),
          gate({
            name: "verify",
            state: "met",
            detail: "`just verify` green at `4be91c2`",
            commit: "4be91c2",
            link: "https://ci.example/runs/77",
          }),
        ]),
      ),
    );
    const drawnRows = rows(html);
    expect(drawnRows[0], "the commit the words name is not said again").toContain(
      '<span dir="auto"><code>just verify</code> red at &lt;b&gt;9ac5149&lt;/b&gt;: exit 1</span></span><span class="gate-by"></span>',
    );
    expect(drawnRows[1], "the commit's words carry the link, still code").toContain(
      '<span dir="auto"><code>just verify</code> green at <a href="https://ci.example/runs/77"><code>4be91c2</code></a></span>',
    );
  });

  it("puts a link with no words to stand on on its name", () => {
    const html = shown(
      read(answer([gate({ name: "owner", state: "held", link: "/docs/DECISIONS.md#card" })])),
    );
    expect(rows(html)[0]).toContain(
      '<span class="gate-label" dir="auto"><a href="/docs/DECISIONS.md#card">owner</a></span>',
    );
  });

  it("draws a link that is no path of the app or web address as its words", () => {
    const unsafe = [
      "javascript:void(document.title='pwned')",
      "JaVaScRiPt:alert(1)",
      " javascript:alert(1)",
      "\u{1}javascript:alert(1)",
      "data:text/html,<script>alert(1)</script>",
      "vbscript:msgbox(1)",
      "//evil.example/x",
      "/\\evil.example/x",
      "\tjavascript:alert(1)",
      "",
    ];
    for (const link of unsafe) {
      for (const detail of ["Open the evidence", ""]) {
        const html = shown(read(answer([gate({ name: "plugin", state: "met", detail, link })])));
        expect(html, `${JSON.stringify(link)} ${detail}`).not.toContain("<a ");
        expect(html, `${JSON.stringify(link)} ${detail}`).not.toContain("href");
        expect(html).toContain(">plugin<");
      }
    }
  });

  it("keeps the direction of every part", () => {
    const html = shown(
      read(
        answer([
          gate({
            name: "بوابة",
            state: "missing",
            by: "لوسيد-هيرون",
            at: 1_794_398_700,
            detail: "تحقق من الشجرة",
            why: "السبب",
          }),
        ]),
      ),
    );
    for (const part of [
      '<span class="gate-label" dir="auto">بوابة</span>',
      '<span dir="auto">تحقق من الشجرة</span>',
      '<span class="gate-why">: <span dir="auto">السبب</span></span>',
      '<span dir="auto">لوسيد-هيرون</span> · <span dir="auto">12:05</span>',
      'waits on <span dir="auto">بوابة</span>',
    ]) {
      expect(html, part).toContain(part);
    }
  });

  it("names every gate the task waits on in the pill, the last after and", () => {
    const html = shown(
      read(
        answer(["certificate", "check", "owner"].map((name) => gate({ name, state: "missing" }))),
      ),
    );
    expect(html).toContain(
      'waits on <span dir="auto">certificate</span>, <span dir="auto">check</span> and <span dir="auto">owner</span></span>',
    );
    const two = shown(
      read(answer(["certificate", "owner"].map((name) => gate({ name, state: "held" })))),
    );
    expect(two).toContain(
      '<span class="pill gate-held gates-waits">waits on <span dir="auto">certificate</span> and <span dir="auto">owner</span></span>',
    );
  });

  it("draws an empty why as none, and an unsafe link on a named gate as its name alone", () => {
    const html = renderToStaticMarkup(
      <GateRow
        gate={gate({ name: "n", state: "met", why: "", link: "javascript:x" })}
        byline={[]}
      />,
    );
    expect(html).toBe(
      '<li class="gate gate-met" data-gate="n" data-state="met"><span class="gate-name"><span class="gate-label" dir="auto">n</span></span><span class="pill gate-state">met</span><span class="gate-detail"></span><span class="gate-by"></span></li>',
    );
  });

  it("writes its link under the project's base, so a copy opens what a click does", async () => {
    vi.resetModules();
    vi.stubGlobal("document", {
      getElementById: () => ({ textContent: '{"name":"","register":"","base":"/alpha"}' }),
    });
    const { GateRow: Row } = await import("./gates");
    const html = renderToStaticMarkup(
      <Row
        gate={gate({ name: "dep", state: "waived", detail: "Waits on ab-1", link: "/task/ab-1" })}
        byline={[]}
      />,
    );
    expect(html).toContain('<span dir="auto">Waits on <a href="/alpha/task/ab-1">ab-1</a></span>');
    const web = renderToStaticMarkup(
      <Row
        gate={gate({ name: "ci", state: "met", link: "https://ci.example/runs/1" })}
        byline={[]}
      />,
    );
    expect(web, "a web address stays as it is").toContain(
      '<a href="https://ci.example/runs/1">ci</a>',
    );
  });
});

describe("the pill", () => {
  /** An unassessed task's gates: its review stamp missing, the rest waived, its check unknown. */
  const stamped = read(
    answer([
      gate({ name: "review", state: "missing", detail: "Not reviewed yet" }),
      gate({ name: "check", state: "unknown", detail: "`just verify` has not run at 4be91c2" }),
      gate({ name: "tests", state: "waived", detail: "Not needed by default" }),
    ]),
  );
  /** The head of the section `stamped` draws for a task in `status`, its other fields `fields`. */
  const head = (status: string, fields: Partial<Waiter> = {}) =>
    shown(stamped, DEFAULT_ROOM, null, aTask("tools/x", { status, ...fields })).split(
      '<ul class="gates">',
    )[0] ?? "";

  it("in review, says the gates the task waits on", () => {
    expect(head("review", { roles: [aRole("reviewer", "rapid-badger")] })).toContain(
      '<span class="pill gate-missing gates-waits">waits on <span dir="auto">review</span></span>',
    );
  });

  it("in progress, says its executor in the status's colour", () => {
    const html = head("in_progress", {
      roles: [aRole("orchestrator", "silver-sparrow"), aRole("executor", "dusky-plover")],
    });
    expect(html).toContain(
      '<span class="pill st-in_progress gates-waits">waits on <span dir="auto">executor dusky-plover</span></span>',
    );
    expect(html, "not the review it has not reached").not.toContain(">review<");
  });

  it("blocked, says the tasks it depends on", () => {
    expect(
      head("blocked", { depends: ["tools/board-postgresql", "tools/machine-runs-the-release"] }),
    ).toContain(
      '<span class="pill st-blocked gates-waits">waits on <span dir="auto">tools/board-postgresql</span> and <span dir="auto">tools/machine-runs-the-release</span></span>',
    );
  });

  it("done, cancelled or not started, is not drawn, the gates still are", () => {
    for (const status of ["done", "cancelled", "backlog", "ready"]) {
      const html = shown(stamped, DEFAULT_ROOM, null, aTask("tools/x", { status }));
      expect(html, status).not.toContain("gates-waits");
      expect(html, status).toContain(
        '<span class="gates-counts">1 waived · 1 unknown · 1 missing</span>',
      );
      expect(rows(html), status).toHaveLength(3);
    }
  });

  it("is its outline for a task the section is not told of yet, and says nothing", () => {
    const untold = shown(stamped, DEFAULT_ROOM, null, null);
    expect(untold).toContain('<span class="pill gates-waits gate-shadow">');
    expect(untold).not.toContain("waits on");
  });
});

describe("the room held until the gates answer", () => {
  it("is four gates where nothing is known", () => {
    expect(roomFor("tools/x", null)).toEqual(DEFAULT_ROOM);
  });

  it("is the room kept for its task, else the last drawn as wide, else four gates", () => {
    const storage = memoryStorage();
    storage.setItem(KEPT, "1 0 0 1280 90 tools/y\n2 1 0 390 140 tools/x");
    vi.stubGlobal("localStorage", storage);
    vi.stubGlobal("innerWidth", 390);
    expect(roomFor("tools/x", null)).toEqual(room(2, 1, [390, 140]));
    expect(roomFor("tools/z", null), "the last task drawn at 390").toEqual(room(2, 1, [390, 140]));
    vi.stubGlobal("innerWidth", 800);
    expect(roomFor("tools/z", null)).toEqual(DEFAULT_ROOM);
    vi.stubGlobal("localStorage", memoryStorage(true));
    expect(roomFor("tools/x", null), "storage refused").toEqual(DEFAULT_ROOM);
  });

  it("is what the server last answered, at the height kept for the task alone", () => {
    const storage = memoryStorage();
    storage.setItem(KEPT, "1 0 0 1280 90 tools/y\n2 1 0 390 140 tools/x");
    vi.stubGlobal("localStorage", storage);
    vi.stubGlobal("innerWidth", 390);
    const seen = { count: 2, waiting: 1, unsigned: 0 };
    expect(roomFor("tools/x", seen)).toEqual(room(2, 1, [390, 140]));
    // Another task's height is never held for what the server said of this one.
    expect(roomFor("tools/z", seen)).toEqual(room(2, 1));
    expect(holding(roomFor("tools/x", { count: 0, waiting: 0, unsigned: 0 }), 390)).toBe("");
  });
});

describe("the window's width", () => {
  it("is none without a window, and whole px with one", () => {
    expect(width()).toBeNull();
    vi.stubGlobal("innerWidth", 390.6);
    expect(width()).toBe(391);
  });
});

describe("what the section has of its part", () => {
  const NOTHING: Shown<TaskGates> = {
    value: undefined,
    stale: false,
    error: null,
    retrying: false,
    missing: false,
    answered: false,
  };

  it("is reading before any answer, and the answer once there is one", () => {
    expect(gatesShown(NOTHING)).toBe(READING);
    expect(gatesShown({ ...NOTHING, value: pictured(), answered: true })).toEqual(read(pictured()));
  });

  it("reads the server's 404 as a task it has no gates for", () => {
    expect(gatesShown({ ...NOTHING, missing: true, answered: true })).toEqual({
      kind: "read",
      answer: { slug: "", branch: "", gates: [] },
    });
  });

  it("says why a read failed, or that it was too slow, only while no answer is shown", () => {
    const refused = new Failure("known", "Board snapshot unavailable (the server answered 500)");
    expect(gatesShown({ ...NOTHING, error: refused })).toEqual({
      kind: "failed",
      why: "Board snapshot unavailable (the server answered 500)",
    });
    expect(gatesShown({ ...NOTHING, error: new Error(NOT_IN_TIME) })).toEqual({ kind: "slow" });
    // The last answer stays shown while a later read fails.
    expect(gatesShown({ ...NOTHING, value: pictured(), answered: true, error: refused })).toEqual(
      read(pictured()),
    );
  });
});

/** A page whose `#gates-tools/x` holds a section `height` px tall, in a window `wide` px wide. */
function page(height: number | null, wide: number) {
  const section = { getBoundingClientRect: () => ({ height }) };
  vi.stubGlobal("document", {
    getElementById: (id: string) =>
      id === "gates-tools/x"
        ? {
            querySelector: (css: string) =>
              css === "section.task-gates" && height !== null ? section : null,
          }
        : null,
  });
  vi.stubGlobal("innerWidth", wide);
  const storage = memoryStorage();
  vi.stubGlobal("localStorage", storage);
  return storage;
}

describe("an answer drawn", () => {
  it("keeps the room it took in this window", () => {
    const storage = page(611.6, 390);
    drawn("tools/x", pictured());
    expect(storage.getItem(KEPT)).toBe("4 1 0 390 612 tools/x");
    expect(kept("tools/x")).toEqual(room(4, 1, [390, 612]));
  });

  it("keeps a task with no gates as no room", () => {
    const storage = page(null, 1280);
    drawn("tools/x", { slug: "tools/x", branch: "", gates: [] });
    expect(storage.getItem(KEPT), "nothing drawn, no height").toBe("0 0 0 0 0 tools/x");
    drawn("tools/x", answer([gate({ name: "review", state: "missing" })]));
    expect(storage.getItem(KEPT), "one line a task, the newest").toBe("1 1 1 0 0 tools/x");
  });

  it("keeps nothing where the browser refuses its storage, and the page goes on", () => {
    page(100, 390);
    vi.stubGlobal("localStorage", memoryStorage(true));
    expect(() => remember("tools/x", { count: 1, waiting: 0, unsigned: 0 })).not.toThrow();
    expect(kept("tools/x")).toBeNull();
  });

  it("keeps the count alone where there is no page to measure", () => {
    const storage = memoryStorage();
    vi.stubGlobal("localStorage", storage);
    remember("tools/x", { count: 2, waiting: 1, unsigned: 0 });
    expect(storage.getItem(KEPT)).toBe("2 1 0 0 0 tools/x");
    expect(kept("tools/y")).toBeNull();
  });
});
