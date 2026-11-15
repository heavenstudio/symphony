import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ComparisonNote, DocumentWithTasks, RelatedTask, SectionDifference } from "./related";
import type { DocumentTask } from "./wire";

const task = (slug: string, title: string, status: string): DocumentTask => ({
  slug,
  title,
  status,
});

const panel = (tasks: DocumentTask[]) =>
  renderToStaticMarkup(
    <DocumentWithTasks tasks={tasks}>
      <article>Document prose</article>
    </DocumentWithTasks>,
  );

/** A row as it draws once near the screen: what its latch holds. */
const row = (one: DocumentTask) =>
  renderToStaticMarkup(
    (RelatedTask({ task: one }) as ReactElement<{ children: ReactNode }>).props.children,
  );

type Node = ReactElement<{ children?: ReactNode; className?: string; task?: DocumentTask }>;

/** Every element of `node`'s tree, as its components wrote them, none of them run. */
const elements = (node: ReactNode): Node[] => {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement(node)) return [];
  const element = node as Node;
  return [element, ...elements(element.props.children)];
};

/**
 * The slugs of the rows under the panel's element of class `className`, in their order: the
 * rows themselves draw only once near the screen, so the markup cannot name them.
 */
const rowsUnder = (tasks: DocumentTask[], className: string): string[] => {
  const tree = DocumentWithTasks({ tasks, children: null });
  const shelf = elements(tree).find((element) => element.props.className === className);
  return elements(shelf?.props.children)
    .filter((element) => element.type === RelatedTask)
    .map((element) => element.props.task?.slug ?? "");
};

describe("a document's related tasks", () => {
  it("are a panel beside the prose, each title as text", () => {
    const tasks = [
      task("ui/layout", "Layout <review> & spacing", "in_progress"),
      task("ui/navigation", "Navigation", "ready"),
    ];
    const html = panel(tasks);
    expect(html).toContain('<div class="document-content"><article>Document prose</article></div>');
    expect(html).toContain('<aside class="doc-tasks" aria-label="Related tasks">');
    expect(html.match(/<aside/g)).toHaveLength(1);
    expect(html.match(/<li /g)).toHaveLength(2);
    const [layout, navigation] = tasks.map(row);
    expect(layout).toContain('href="/task/ui/layout"');
    expect(layout).toContain("Layout &lt;review&gt; &amp; spacing");
    // A status is its column's name, never the raw value.
    expect(layout).toContain('<span class="doc-task-status">in progress</span>');
    expect(layout).not.toContain("in_progress");
    expect(navigation).toContain('<span class="doc-task-status">ready</span>');
  });

  it("are no empty panel when there are none, or only the verdicts' tasks", () => {
    for (const tasks of [
      [],
      [
        task("verdicts/2026-11-11-1244", "Answer the owner on lamp-a-pic", "done"),
        task("verdicts/2026-11-11-1742", "Answer the owner on lamp-a-pic", "ready"),
      ],
    ]) {
      const html = panel(tasks);
      expect(html).toContain("<article>Document prose</article>");
      expect(html).not.toContain("<aside");
      expect(html).not.toContain("Related tasks");
    }
  });

  it("name each row's slug beside its title, then its status", () => {
    for (const [slug, status, shown] of [
      ["perf/client-cache", "ready", "ready"],
      ["perf/client-cache/design", "in_progress", "in progress"],
    ] as const) {
      expect(row(task(slug, "Measure return visits", status))).toBe(
        `<a href="/task/${slug}">Measure return visits</a><span class="doc-task-meta"><code class="doc-task-slug">${slug}</code> · <span class="doc-task-status">${shown}</span></span>`,
      );
    }
  });

  it("fold the finished work under its count, below the open work", () => {
    const tasks = [
      task("perf/one", "One", "done"),
      task("perf/open", "Open", "ready"),
      task("perf/two", "Two", "cancelled"),
      task("verdicts/2026-11-11-1244", "Answer the owner on lamp-a-pic", "done"),
    ];
    const html = panel(tasks);
    const open = html.indexOf('class="doc-tasks-open"');
    const folded = html.indexOf(
      '<details class="doc-tasks-finished"><summary>2 finished</summary>',
    );
    expect(open).toBeGreaterThan(0);
    expect(folded).toBeGreaterThan(open);
    expect(html).not.toContain('<details class="doc-tasks-finished" open');
    expect(html.slice(open, folded).match(/<li /g)).toHaveLength(1);
    expect(html.slice(folded).match(/<li /g)).toHaveLength(2);
    // Which rows: the open one above, the finished ones folded in the board's order, and the
    // verdicts' task on neither side.
    expect(rowsUnder(tasks, "doc-tasks-open")).toEqual(["perf/open"]);
    expect(rowsUnder(tasks, "doc-tasks-finished")).toEqual(["perf/one", "perf/two"]);
  });

  it("are only the count when all of them are finished", () => {
    const html = panel([task("perf/one", "One", "done")]);
    expect(html).not.toContain("doc-tasks-open");
    expect(html).toContain("<summary>1 finished</summary>");
  });
});

describe("a branch's differences from main", () => {
  it("name the task they belong to", () => {
    expect(renderToStaticMarkup(<ComparisonNote comparison={{ task: "design/policy" }} />)).toBe(
      '<aside class="document-comparison">Differences from main belong to <a href="/task/design/policy">design/policy</a></aside>',
    );
  });

  it("label each kind of a section's difference, its prose under the label", () => {
    for (const [kind, label] of [
      ["added", "Added"],
      ["changed", "Changed"],
      ["removed", "Removed"],
    ] as const) {
      const html = renderToStaticMarkup(
        <SectionDifference comparison={{ kind, before: [] }}>
          <p>Current prose</p>
        </SectionDifference>,
      );
      expect(html).toBe(
        `<div class="section-difference difference-${kind}" data-difference="${kind}"><p class="difference-label" title="Compared with main">${label}</p><p>Current prose</p></div>`,
      );
    }
  });

  it("fold what main had before, and leave a section with none as it is", () => {
    const before = renderToStaticMarkup(
      <SectionDifference
        comparison={{
          kind: "changed",
          before: [{ type: "paragraph", spans: [{ type: "text", text: "Old" }] }],
        }}
      >
        <p>New</p>
      </SectionDifference>,
    );
    expect(before).toContain(
      '<details class="difference-before"><summary>Previously on main</summary>',
    );
    expect(before.indexOf("difference-before")).toBeLessThan(before.indexOf("<p>New</p>"));
    for (const comparison of [null, undefined]) {
      const html = renderToStaticMarkup(
        <SectionDifference comparison={comparison}>
          <p>Unchanged prose</p>
        </SectionDifference>,
      );
      expect(html).toBe("<p>Unchanged prose</p>");
    }
  });
});
