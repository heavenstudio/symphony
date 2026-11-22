import { renderToStaticMarkup as render } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  blocked,
  Ends,
  ends,
  fold,
  NAMES,
  OrchestratorBanner,
  savedStage,
  WaitCard,
  waitAge,
  waitingOrder,
  waitingStage,
  waitingToStart,
  waitKind,
  waitLabel,
  waitSince,
} from "./boardwait";
import { aMachine, anOperation, aRole, aTask, aWork } from "./testing";
import { NO_WORK } from "./work";

const now = 1800000000;
const task = (kind: string, who = "named", hours = 1) =>
  aTask("work/task", {
    title: "A clear title",
    status: "blocked",
    from: "review",
    waits_on: { kind, who, why: "A reason", since: now - hours * 3600 },
  });

/** What a drawn card says, its tags out: a cut line's two parts read as one text. */
const said = (html: string) => html.replace(/<[^>]*>/g, "");

describe("a cut line's end", () => {
  it.each([
    ["", "", ""],
    ["work/task", "work/task", ""],
    ["exactly-16-chars", "exactly-16-chars", ""],
    // The first break among the last 16 characters, so the longest end that starts at one.
    ["tools/review-app-verdicts-recovery-alpha", "tools/review-app-verdicts", "-recovery-alpha"],
    ["On docs/research-handoff-register-beta", "On docs/research-handoff", "-register-beta"],
    [
      "Saved owner answer waits for candidate gamma",
      "Saved owner answer waits for",
      " candidate gamma",
    ],
    // A break that would leave an end of fewer than ten characters is passed over.
    ["a-text-of-many-characters-ending-", "a-text-of-many-characte", "rs-ending-"],
    ["seventeen-chars-x", "sevente", "en-chars-x"],
    // No break among them: the last ten characters.
    ["anunbrokenwordofthirtycharacters", "anunbrokenwordofthirty", "characters"],
  ])("cuts %j into %j and %j", (text, head, tail) => {
    expect(ends(text)).toEqual([head, tail]);
    expect(ends(text).join("")).toBe(text);
  });
  it("never cuts inside a character", () => {
    const text = `${"x".repeat(21)}😀${"y".repeat(9)}`;
    expect(ends(text)).toEqual(["x".repeat(21), `😀${"y".repeat(9)}`]);
  });
  it("draws both parts with nothing between them, and no empty end", () => {
    expect(render(<Ends text="tools/review-app-verdicts-recovery-alpha" />)).toBe(
      '<span class="cut-head">tools/review-app-verdicts</span><span class="cut-tail">-recovery-alpha</span>',
    );
    expect(render(<Ends text="work/task" />)).toBe('<span class="cut-head">work/task</span>');
  });
});

describe("a closed card's line", () => {
  it.each([
    ["", ""],
    ["A reason", "A reason"],
    // A line break, a run of spaces, a tab and a carriage return each fold to one space.
    ["waits\nfor the register", "waits for the register"],
    ["the register   regroup", "the register regroup"],
    ["waits\r\n\t  for", "waits for"],
    // Every white space Unicode names, not only ASCII's.
    ["waits\u2028for\u00a0the\u3000register", "waits for the register"],
    // The ends are folded, not trimmed: the board trims a reason's ends before it is drawn.
    ["\n candidate alpha\t", " candidate alpha "],
  ])("folds %j to %j", (text, folded) => {
    expect(fold(text)).toBe(folded);
  });
});

describe("waiting cards", () => {
  it("draws a reason written on several lines on one, keeping its end, and titles it as written", () => {
    const why = "Saved owner answer recovery waits\nfor the register   regroup of candidate alpha";
    const t = aTask("tools/review-app-verdicts-saved-owner-answer-recovery-alpha", {
      status: "blocked",
      from: "ready",
      waits_on: { kind: "task", who: "docs/research-handoff\nregister-alpha", why, since: now },
    });
    const html = render(<WaitCard task={t} now={now} />);
    const reason = html.match(/<span class="why" title="([^"]*)">(.*?)<\/span><\/div>/s);
    expect(reason?.[1]).toBe(why);
    expect(reason?.[2]).toBe(
      '<span class="cut-head">Saved owner answer recovery waits for the register regroup of</span>' +
        '<span class="cut-tail"> candidate alpha</span>',
    );
    // The wait folds as well, its title as written.
    expect(html).toContain(
      '<span class="on" title="On docs/research-handoff\nregister-alpha">' +
        '<span class="cut-head">On docs/research-handoff</span>' +
        '<span class="cut-tail"> register-alpha</span></span>',
    );
  });
  it("tells apart cards whose title, slug, wait and reason differ only at the end", () => {
    const cards = ["alpha", "beta", "gamma"].map((end) =>
      render(
        <WaitCard
          task={aTask(`tools/review-app-verdicts-saved-owner-answer-recovery-${end}`, {
            title: `Symphony could not commit an owner answer for candidate ${end}`,
            status: "blocked",
            from: "ready",
            waits_on: {
              kind: "task",
              who: `docs/research-handoff-owner-answer-register-${end}`,
              why: `Saved owner answer recovery waits for candidate ${end}`,
              since: now,
            },
          })}
          now={now}
        />,
      ),
    );
    const tails = cards.map((html) =>
      [...html.matchAll(/class="cut-tail">([^<]*)</g)].map((found) => found[1]),
    );
    expect(tails).toEqual([
      ["-recovery-alpha", "-register-alpha", " candidate alpha"],
      ["-recovery-beta", "-register-beta", " candidate beta"],
      ["-recovery-gamma", "-register-gamma", " candidate gamma"],
    ]);
    expect(said(cards[0] ?? "")).toContain(
      "tools/review-app-verdicts-saved-owner-answer-recovery-alpha",
    );
  });
  it("is closed until its button opens it, which names the task and says its state", () => {
    const html = render(<WaitCard task={task("check")} now={now} />);
    expect(html).not.toContain("data-open");
    expect(html).toContain(
      '<button type="button" class="card-more" aria-expanded="false" aria-label="Show all of A clear title">▾</button>',
    );
    expect(html.indexOf("card-more")).toBeGreaterThan(html.indexOf('class="prio"'));
    expect(html.indexOf("card-more")).toBeLessThan(html.indexOf('class="task-wait"'));
  });
  it.each([undefined, null, 0, 1, 3])("draws only known positive review counts (%s)", (rounds) => {
    for (const status of ["review", "done", "cancelled"]) {
      const t = aTask("work/task", {
        status,
        ...(rounds === undefined ? {} : { review_rounds: rounds }),
      });
      const html = render(<WaitCard task={t} now={now} />);
      if (rounds === 1) expect(html).toContain('class="review-mark">1 review</span>');
      else if (rounds === 3) expect(html).toContain('class="review-mark">3 reviews</span>');
      else expect(html).not.toContain("review-mark");
    }
  });
  it.each([
    ["decision", "On your decision named"],
    ["check", "On named"],
    ["task", "On named"],
    ["nobody", "On nobody"],
    ["working", "Working"],
    ["queued", "Queued"],
    ["parked", "Parked"],
  ])("names %s and its reason", (kind, line) => {
    const t = task(kind);
    if (["working", "queued"].includes(kind)) t.status = "ready";
    expect(waitLabel(t)).toBe(line);
    const html = render(<WaitCard task={t} now={now} />);
    expect(said(html)).toContain(line);
    expect(html).toContain("A reason");
    expect(html).toContain("1 h");
    expect(html.indexOf("A clear title")).toBeLessThan(
      html.indexOf('<code><span class="cut-head">work/task'),
    );
  });
  it("shows no document or role chips and preserves the priority edge", () => {
    const t = {
      ...task("decision"),
      priority: 0,
      links: ["docs/A.md"],
      roles: [aRole("executor", "gone", "left")],
    };
    const html = render(<WaitCard task={t} now={now} />);
    expect(html).toContain("p0");
    expect(html).not.toContain("docchip");
    expect(html).not.toContain('class="chips"');
    expect(html).not.toContain("gone");
  });
  it("says stale on a working holder's reason", () => {
    const t = {
      ...task("working"),
      status: "in_progress",
      roles: [aRole("executor", "named", "stale")],
    };
    expect(render(<WaitCard task={t} now={now} />)).toContain("(stale)");
    t.waits_on = { kind: "working", who: "named", why: "named (stale)", since: now };
    expect(render(<WaitCard task={t} now={now} />).match(/\(stale\)/g)).toHaveLength(2); // title and visible reason
  });
  it("uses hours below 48, whole days after, and only highlights overdue active work", () => {
    expect(waitAge(now - 47 * 3600, now)).toBe("47 h");
    expect(waitAge(now - 49 * 3600, now)).toBe("2 d");
    expect(waitAge(now + 3600, now)).toBe("0 h");
    expect(render(<WaitCard task={task("check", "named", 168)} now={now} />)).toContain(
      'class="age old"',
    );
    for (const kind of ["queued", "parked"]) {
      const t = task(kind, "named", 312);
      if (kind === "queued") t.status = "ready";
      expect(render(<WaitCard task={t} now={now} />)).not.toContain('class="age old"');
    }
    for (const status of ["done", "cancelled"]) {
      const t = { ...task("nobody"), status, updated: now - 3600 };
      const html = render(<WaitCard task={t} now={now} />);
      expect(html).toContain(status === "done" ? ">Done<" : ">Cancelled<");
      expect(html).not.toContain('class="age old"');
    }
  });
  it("does not draw unnamed legacy blocks as blocked", () => {
    for (const t of [
      task("nobody", ""),
      task("decision", ""),
      { ...task("decision"), waits_on: null },
      task("agent"),
    ]) {
      expect(blocked(t)).toBe(false);
      expect(waitingStage(NO_WORK, t, true)).toBe("review");
      expect(render(<WaitCard task={t} now={now} />)).not.toContain("blocked-mark");
    }
    expect(blocked(task("parked", ""))).toBe(true);
    expect(blocked(task("nobody", "gone/dependency"))).toBe(true);
    expect(
      blocked({
        ...task("decision"),
        waits_on: { kind: "decision", who: "named", why: "no reason recorded", since: 0 },
      }),
    ).toBe(false);
    expect(waitingStage(NO_WORK, { ...task("decision"), from: "invalid" }, true)).toBe("ready");
    expect(waitingStage(NO_WORK, aTask("a"), true)).toBe("ready");
  });
  it("handles absent and newer wait kinds and missing or nonfinite timestamps", () => {
    expect(waitKind(aTask("a"))).toBe("nobody");
    expect(waitKind(task("new-kind"))).toBe("nobody");
    expect(waitSince(aTask("a", { updated: 7 }))).toBe(7);
    expect(
      waitSince({
        ...task("check"),
        updated: 8,
        waits_on: { kind: "check", who: "x", why: "y", since: NaN },
      }),
    ).toBe(8);
  });
  it("orders blocks by kind then age and slug, ordinary next and parked last", () => {
    const rows = [
      task("parked"),
      aTask("ordinary", { updated: 0 }),
      task("check"),
      task("decision"),
      task("nobody"),
      task("task"),
    ];
    expect(rows.sort(waitingOrder).map((t) => t.waits_on?.kind ?? "ordinary")).toEqual([
      "decision",
      "nobody",
      "check",
      "task",
      "ordinary",
      "parked",
    ]);
    const older = { ...task("check"), slug: "a2" };
    const newer = { ...task("check"), slug: "a10" };
    expect(waitingOrder(older, newer)).toBeLessThan(0);
    expect(waitingOrder(task("check", "named", 2), task("check", "named", 1))).toBeLessThan(0);
  });
});

it("keeps only a known lifecycle phone stage", () => {
  expect(savedStage(null)).toBe("review");
  expect(savedStage("blocked")).toBe("review");
  expect(savedStage("ready")).toBe("ready");
});
it("keeps a phone stage only while a workflow's tabs have it, else its first", () => {
  const tabs = ["phase:verify", "phase:review", "phase:build"];
  expect(savedStage("phase:review", tabs)).toBe("phase:review");
  expect(savedStage("review", tabs)).toBe("phase:verify");
  expect(savedStage(null, tabs)).toBe("phase:verify");
  expect(savedStage("phase:review")).toBe("review");
});
describe("a task a custom workflow holds", () => {
  const place = { phase: "build", state: "blocked", revision: 2 };
  const held = aTask("w/c", { status: "blocked", from: "review", workflow: place });
  it("goes in its phase's column, blocked or not, unless a machine builds or tests it", () => {
    expect(waitingStage(NO_WORK, held, true)).toBe("phase:build");
    const review = { ...held, status: "review", workflow: { ...place, phase: "review" } };
    expect(waitingStage(NO_WORK, review, true)).toBe("phase:review");
    const working = { ...held, status: "in_progress", workflow: { ...place, state: "working" } };
    const building = aWork([
      aMachine("m", [anOperation("m", "b", "build", "running", { task: "w/c" })]),
    ]);
    expect(waitingStage(building, working, true)).toBe("build");
    expect(waitingStage(building, working, false)).toBe("phase:build");
  });
  it("names its place on its card: its phase's and state's labels, else its state's ID", () => {
    NAMES.set("build/blocked", "Build · Blocked");
    expect(render(<WaitCard task={held} now={now} />)).toContain(
      '<span class="state-mark">Build · Blocked</span>',
    );
    NAMES.clear();
    expect(render(<WaitCard task={held} now={now} />)).toContain(
      '<span class="state-mark">blocked</span>',
    );
    expect(render(<WaitCard task={aTask("w/a")} now={now} />)).not.toContain("state-mark");
  });
});
it("shows one amber banner for stale orchestrators, red for none, and none for live or no queued work", () => {
  const banner = (health: string, unstarted = 2) =>
    render(
      <OrchestratorBanner
        orchestrators={health ? [{ handle: "orch", health }] : []}
        unstarted={unstarted}
        ready={5}
      />,
    );
  expect(banner("stale")).toContain('class="orchestrator-banner warn"');
  expect(banner("stale")).toContain("orch is stale");
  expect(banner("left")).toContain("orch left");
  expect(banner("")).toContain('class="orchestrator-banner"');
  expect(banner("")).toContain("2 of the 5 ready tasks wait to start");
  expect(banner("live")).toBe("");
  expect(banner("stale", 0)).toBe("");
});

it("does not compare the global queued count with a filtered ready total", () => {
  const html = render(<OrchestratorBanner orchestrators={[]} unstarted={8} />);
  expect(html).toContain("8 ready tasks wait to start");
  expect(html).not.toContain("of the");
});

it("says how many wait to start of the ready ones, never of fewer ready than wait", () => {
  expect(waitingToStart(2, 5)).toBe("2 of the 5");
  expect(waitingToStart(5, 5)).toBe("5 of the 5");
  expect(waitingToStart(0, 0)).toBe("0 of the 0");
  expect(waitingToStart(8, undefined)).toBe("8");
  // A total read before a live change moved more tasks to start.
  expect(waitingToStart(3, 2)).toBe("3");
  const html = render(<OrchestratorBanner orchestrators={[]} unstarted={3} ready={2} />);
  expect(html).toContain("· 3 ready tasks wait to start");
});

it("does not attach a stale author to a decision's own wait reason", () => {
  const t = { ...task("decision"), roles: [aRole("orchestrator", "named", "stale")] };
  expect(render(<WaitCard task={t} now={now} />)).not.toContain("(stale)");
});

it("names the working holder and their role from the authoritative wait line", () => {
  const t = aTask("a", {
    status: "in_progress",
    waits_on: { kind: "working", who: "amber (stale)", why: "executor", since: now },
  });
  const html = render(<WaitCard task={t} now={now} />);
  expect(html).toContain('title="executor amber (stale)"');
  expect(html).toContain(
    '<span class="cut-head">executor</span><span class="cut-tail"> amber (stale)</span>',
  );
});
