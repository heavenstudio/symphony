// What a document page draws beside and around its prose, as the Dioxus client's doc_components.rs
// drew it: the related tasks' panel, the note that names the task a branch's differences belong
// to, and a section's difference from main.

import type { ReactNode } from "react";
import { HistoricalBlocks } from "./blocks";
import { related } from "./document";
import { href } from "./project";
import { NearViewport } from "./viewport";
import type { DocumentComparison, DocumentTask, SectionComparison } from "./wire";

/**
 * The task panel's place beside a standalone document and a folder's introduction alike: the
 * reader supplies its content, and this places the related work beside it. Finished work folds
 * under its count, so it never crowds the open work above it.
 */
export function DocumentWithTasks({
  tasks,
  children,
}: {
  tasks: readonly DocumentTask[];
  children: ReactNode;
}) {
  const { open, finished } = related(tasks);
  return (
    <div className="document-content-layout">
      <div className="document-content">{children}</div>
      {(open.length > 0 || finished.length > 0) && (
        <aside className="doc-tasks" aria-label="Related tasks">
          <h2>Related tasks</h2>
          {open.length > 0 && (
            <ul className="doc-tasks-open">
              {open.map((task) => (
                <RelatedTask key={task.slug} task={task} />
              ))}
            </ul>
          )}
          {finished.length > 0 && (
            <details className="doc-tasks-finished">
              <summary>{`${finished.length} finished`}</summary>
              <ul>
                {finished.map((task) => (
                  <RelatedTask key={task.slug} task={task} />
                ))}
              </ul>
            </details>
          )}
        </aside>
      )}
    </div>
  );
}

/**
 * One row of the panel: the title, then the slug that tells it from a row of the same title, and
 * the status by its board column's name (`in progress`), as every other view names it.
 */
export function RelatedTask({ task }: { task: DocumentTask }) {
  return (
    <NearViewport height={40} listItem>
      <a href={href(`/task/${task.slug}`)}>{task.title}</a>
      <span className="doc-task-meta">
        <code className="doc-task-slug">{task.slug}</code>
        {" · "}
        <span className="doc-task-status">{task.status.replaceAll("_", " ")}</span>
      </span>
    </NearViewport>
  );
}

/** A branch's document: the task its differences from main belong to. */
export function ComparisonNote({ comparison }: { comparison: DocumentComparison }) {
  return (
    <aside className="document-comparison">
      {"Differences from main belong to "}
      <a href={href(`/task/${comparison.task}`)}>{comparison.task}</a>
    </aside>
  );
}

const DIFFERENCES = { added: "Added", changed: "Changed", removed: "Removed" } as const;

/**
 * A section of a branch's document that differs from main: its kind of difference, what main had
 * before it (folded), then the section. A section with no difference is drawn as it is.
 */
export function SectionDifference({
  comparison,
  children,
}: {
  comparison: SectionComparison | null | undefined;
  children: ReactNode;
}) {
  if (comparison == null) return children;
  const kind = comparison.kind;
  const before = comparison.before ?? [];
  return (
    <div className={`section-difference difference-${kind}`} data-difference={kind}>
      <p className="difference-label" title="Compared with main">
        {DIFFERENCES[kind]}
      </p>
      {before.length > 0 && (
        <details className="difference-before">
          <summary>Previously on main</summary>
          <NearViewport>
            <HistoricalBlocks blocks={before} />
          </NearViewport>
        </details>
      )}
      {children}
    </div>
  );
}
