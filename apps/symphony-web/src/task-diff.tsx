// The task kind's changed files. The summary is its own live part, the card's line its comparison;
// the files themselves are task-diff-files.tsx, a chunk imported once a comparison has some to list
// (chunk.tsx), which reads a file's patch only after its details opens, bound to that summary's
// commits, and cancels it as the comparison leaves.
import { type ComponentType, useContext, useMemo } from "react";
import { Button } from "./button";
import { chunkFailed, useChunk } from "./chunk";
import { type Part, PartsContext, usePart } from "./parts";
import { scoped } from "./project";
import { said } from "./request";
import { field } from "./shared";
import type { DiffFileListProps } from "./task-diff-files";
import { changesTask, type TaskEvent } from "./task-live";
import { failed } from "./trouble";
import { TASK_DIFF_PATH, type TaskDiff } from "./wire";
import "./task-diff.css";

const nullableText = (value: unknown) => value === null || typeof value === "string";
const count = (value: unknown) =>
  value === null || (typeof value === "number" && Number.isSafeInteger(value) && value >= 0);

export function readDiff(value: unknown): TaskDiff | null {
  const source = field(value, "source");
  const files = field(value, "files");
  if (
    !["branch", "landing", "unavailable"].includes(source as string) ||
    !nullableText(field(value, "base")) ||
    !nullableText(field(value, "head")) ||
    !nullableText(field(value, "reason")) ||
    typeof field(value, "truncated") !== "boolean" ||
    !Array.isArray(files) ||
    !files.every(
      (file) =>
        typeof field(file, "path") === "string" &&
        count(field(file, "added")) &&
        count(field(file, "removed")),
    )
  )
    return null;
  if (
    source !== "unavailable" &&
    (typeof field(value, "base") !== "string" || typeof field(value, "head") !== "string")
  )
    return null;
  return value as TaskDiff;
}

export function diffPart(scope: string, slug: string): Part<TaskDiff, TaskEvent> {
  return {
    key: `diff ${slug}`,
    url: scoped(scope, `${TASK_DIFF_PATH}?slug=${encodeURIComponent(slug)}`),
    read: readDiff,
    unreadable: "The task's changed files are not readable",
    hear: (value, event) =>
      changesTask(slug, event.changes) ||
      event.changes.some(
        (change) =>
          change.region === "branches" ||
          (change.region === "message" &&
            change.entry.message.slug === slug &&
            change.entry.message.kind === "verdict"),
      )
        ? { value, again: true }
        : null,
  };
}

/** The files' chunk once it has come: a comparison drawn after draws them at once. */
let fileList: ComponentType<DiffFileListProps> | null = null;

/** Import the files' chunk, kept once it has come. */
export const loadFileList = () =>
  import("./task-diff-files").then((chunk) => {
    fileList = chunk.DiffFileList;
    return fileList;
  });

/** The comparison's files, drawn by their chunk once it has come; said as read until then. */
function FileList(props: DiffFileListProps) {
  const [List, failed, retry] = useChunk(loadFileList, () => fileList);
  if (List) return <List {...props} />;
  return failed ? (
    chunkFailed("The changed files", retry)
  ) : (
    <p role="status" aria-busy="true" data-pending="true">
      Reading changed files…
    </p>
  );
}

export function DiffFiles({ diff, scope, slug }: { diff: TaskDiff; scope: string; slug: string }) {
  return (
    <>
      <p className="muted">
        {diff.source === "landing"
          ? "Landing merge"
          : diff.source === "branch"
            ? "Branch from its merge base"
            : "Comparison unavailable"}
        {diff.base !== null && diff.head !== null && (
          <>
            {" "}
            ·{" "}
            <code>
              {diff.base.slice(0, 8)} → {diff.head.slice(0, 8)}
            </code>
          </>
        )}
      </p>
      {diff.reason !== null && <p role="status">{diff.reason}</p>}
      {diff.source !== "unavailable" && diff.files.length === 0 && !diff.truncated && (
        <p>No changed files.</p>
      )}
      {diff.base !== null && diff.head !== null && diff.files.length > 0 && (
        <FileList files={diff.files} scope={scope} slug={slug} base={diff.base} head={diff.head} />
      )}
    </>
  );
}

export function TaskDiffPanel({ scope, slug }: { scope: string; slug: string }) {
  const part = useMemo(() => diffPart(scope, slug), [scope, slug]);
  const answer = usePart(part);
  const parts = useContext(PartsContext);
  return (
    <section
      className="task-diff card"
      aria-labelledby="task-diff-title"
      data-pending={`${!answer.answered && answer.error === null}`}
    >
      <div className="row">
        <h2 id="task-diff-title">Changes</h2>
        <Button size="sm" disabled={answer.retrying} onClick={() => void parts?.read(part)}>
          {answer.retrying ? "Reading files…" : "Refresh files"}
        </Button>
      </div>
      {answer.error !== null && (
        <p role="status">
          {failed(said(answer.error), true)}
          {answer.value !== undefined && " Showing files from the last successful read."}
        </p>
      )}
      {answer.value !== undefined ? (
        <DiffFiles diff={answer.value} scope={scope} slug={slug} />
      ) : answer.error !== null ? null : answer.missing ? (
        <p>No comparison is available.</p>
      ) : (
        <p role="status" aria-busy="true">
          Reading changed files…
        </p>
      )}
    </section>
  );
}
