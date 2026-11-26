import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import {
  linkName,
  markTasksReady,
  Roles,
  sameCard,
  statusWords,
  TaskCard,
  type TaskCardProps,
  WorkPhases,
} from "./taskcard";
import { aMachine, anOperation, aRole, aTask, aWork } from "./testing";

const draw = (props: TaskCardProps) => renderToStaticMarkup(<TaskCard {...props} />);

describe("a task's card", () => {
  // The title is the card's one link to the task, and the slug above it plain text; the subtask's
  // parent and the linked documents keep theirs.
  it("opens its task from its title, its one link to it", () => {
    const task = aTask("tools/fix-it", { title: "Fix the thing" });
    let html = draw({ task });
    expect(html.match(/<a /g)).toHaveLength(1);
    expect(html).toContain('<a class="taskcard-title" href="/task/tools/fix-it">Fix the thing</a>');
    expect(html).toContain('<code class="taskcard-slug">tools/fix-it</code>');
    html = draw({ task: { ...task, parent: "tools/parent", links: ["docs/TASKS.md"] } });
    expect(html.match(/href="\/task\/tools\/fix-it"/g)).toHaveLength(1);
    expect(html).toContain('<div class="subtask-parent">Subtask of <a href="/task/tools/parent">');
    expect(html).toContain('<a class="docchip" href="/docs/TASKS.md">TASKS.md</a>');
  });

  it("says its priority and its status, in the card's colours", () => {
    const html = draw({ task: aTask("w/a", { priority: 1, status: "in_progress" }) });
    expect(html).toMatch(/^<div class="taskcard p1" data-slug="w\/a">/);
    expect(html).toContain('<span class="prio">p1</span>');
    expect(html).toContain('<span class="task-status st-in_progress">in progress</span>');
  });

  it("names who holds its roles, each in the health of its holder's session", () => {
    const html = renderToStaticMarkup(
      <Roles roles={[aRole("executor", "amber"), aRole("reviewer", "birch", "stale")]} />,
    );
    expect(html).toBe(
      '<span class="chips"><span class="chip health-live" title="executor is live">executor amber</span><span class="chip health-stale" title="reviewer is stale">reviewer birch</span></span>',
    );
  });

  it("shows its builds and tests, live while a fresh machine runs them", () => {
    const work = aWork([
      aMachine("alpha", [
        anOperation("alpha", "b", "build", "running"),
        anOperation("alpha", "t", "test", "queued", { reason: "waiting" }),
      ]),
    ]);
    const html = renderToStaticMarkup(<WorkPhases work={work} slug="sim/a" connected />);
    expect(html).toContain('role="group" aria-label="Build and test work"');
    expect(html).toContain(
      '<span class="chip work-phase health-live" data-work-kind="build" title="fixture · run_slot · ">Build running · alpha</span>',
    );
    expect(html).toContain(
      '<span class="chip work-phase health-stale" data-work-kind="test" title="fixture · run_slot · waiting">Test queued · alpha</span>',
    );
    // While the feed is lost, a machine's work reads as what it last said.
    expect(
      renderToStaticMarkup(<WorkPhases work={work} slug="sim/a" connected={false} />),
    ).toContain(
      '<span class="chip work-phase health-stale" data-work-kind="build" title="fixture · run_slot · ">Observed Build running · alpha · disconnected</span>',
    );
    expect(draw({ task: aTask("sim/a"), work })).toContain("Build running · alpha");
    // A task with none draws no row of them.
    expect(renderToStaticMarkup(<WorkPhases work={work} slug="sim/b" connected />)).toBe("");
    expect(draw({ task: aTask("sim/b"), work })).not.toContain("work-phases");
  });
});

describe("a card drawn again", () => {
  const task = aTask("sim/a");
  const running = aWork([aMachine("alpha", [anOperation("alpha", "b", "build", "running")])]);

  it("is drawn only for another task object, feed or set of phases", () => {
    expect(sameCard({ task }, { task })).toBe(true);
    expect(sameCard({ task }, { task, connected: true, work: running })).toBe(false);
    expect(sameCard({ task }, { task: { ...task } })).toBe(false);
    expect(sameCard({ task }, { task, connected: false })).toBe(false);
    expect(sameCard({ task, work: running }, { task, work: running })).toBe(true);
  });

  it("is not drawn for the machines' heartbeat, or another task's work", () => {
    const beat = aWork([
      aMachine("alpha", [anOperation("alpha", "b", "build", "running")], { sequence: 2 }),
    ]);
    expect(sameCard({ task, work: running }, { task, work: beat })).toBe(true);
    const other = aWork([
      aMachine("alpha", [anOperation("alpha", "b", "build", "running", { task: "sim/b" })]),
    ]);
    expect(sameCard({ task }, { task, work: other })).toBe(true);
    // Disconnected, a machine's own phase reads so: a heartbeat still draws nothing again.
    expect(
      sameCard({ task, work: running, connected: false }, { task, work: beat, connected: false }),
    ).toBe(true);
  });

  it("is memoized by `sameCard`", () => {
    const { $$typeof, compare } = TaskCard as unknown as { $$typeof: symbol; compare: unknown };
    expect([$$typeof, compare]).toEqual([Symbol.for("react.memo"), sameCard]);
  });
});

describe("a status", () => {
  it("reads as words", () => {
    expect(statusWords("in_progress")).toBe("in progress");
    expect(statusWords("a_b_c")).toBe("a b c");
    expect(statusWords("ready")).toBe("ready");
  });
});

describe("the page's mark of a drawn card", () => {
  it("is set once", () => {
    const mark = vi.spyOn(performance, "mark");
    markTasksReady();
    markTasksReady();
    expect(mark.mock.calls.filter(([name]) => name === "tasks-ready")).toHaveLength(1);
    mark.mockRestore();
  });
});

describe("a linked document's chip", () => {
  it("names it by the last part of its path", () => {
    expect(linkName("docs/analysis/x.md#part")).toBe("x.md#part");
    expect(linkName("README.md")).toBe("README.md");
    expect(linkName("docs/")).toBe("");
    expect(linkName("")).toBe("");
  });
});
