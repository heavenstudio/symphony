import { renderToStaticMarkup as render } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  blocked,
  OrchestratorBanner,
  savedStage,
  WaitCard,
  waitAge,
  waitingOrder,
  waitingStage,
  waitKind,
  waitLabel,
  waitSince,
} from "./boardwait";
import { aRole, aTask } from "./testing";
import { NO_WORK } from "./work";

const now = 1800000000;
const task = (kind: string, who = "named", hours = 1) =>
  aTask("work/task", {
    title: "A clear title",
    status: "blocked",
    from: "review",
    waits_on: { kind, who, why: "A reason", since: now - hours * 3600 },
  });

describe("waiting cards", () => {
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
    expect(html).toContain(line);
    expect(html).toContain("A reason");
    expect(html).toContain("1 h");
    expect(html.indexOf("A clear title")).toBeLessThan(html.indexOf("<code>work/task"));
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
