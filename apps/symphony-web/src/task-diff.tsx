// The task kind's changed files. The summary is its own live part; a file's patch is read only
// after its details opens, bound to that summary's commits, and cancelled as the comparison leaves.
import { useContext, useEffect, useMemo, useState } from "react";
import { Button } from "./button";
import { type Part, PartsContext, usePart } from "./parts";
import { scoped } from "./project";
import { getJson, said } from "./request";
import { field } from "./shared";
import { changesTask, type TaskEvent } from "./task-live";
import { failed } from "./trouble";
import {
  TASK_DIFF_FILE_PATH,
  TASK_DIFF_PATH,
  type TaskDiff,
  type TaskDiffFile,
  type TaskDiffPatch,
} from "./wire";
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

export function readPatch(
  value: unknown,
  base: string,
  head: string,
  path: string,
): TaskDiffPatch | null {
  return field(value, "base") === base &&
    field(value, "head") === head &&
    field(value, "path") === path &&
    typeof field(value, "patch") === "string" &&
    typeof field(value, "truncated") === "boolean"
    ? (value as TaskDiffPatch)
    : null;
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

export function PatchText({ patch, truncated }: { patch: string; truncated: boolean }) {
  return (
    <>
      {/* biome-ignore lint/a11y/noNoninteractiveTabindex: this code region scrolls in both directions and needs keyboard access */}
      <section className="task-diff-patch" tabIndex={0} aria-label="File patch">
        <pre>{patch}</pre>
      </section>
      {truncated && <p role="status">Patch stops at 256 KiB. View the full comparison in Git.</p>}
    </>
  );
}

function Patch({
  scope,
  slug,
  base,
  head,
  path,
}: {
  scope: string;
  slug: string;
  base: string;
  head: string;
  path: string;
}) {
  const [answer, setAnswer] = useState<TaskDiffPatch | null>(null);
  const [error, setError] = useState("");
  const [attempt, retry] = useState(0);
  useEffect(() => {
    void attempt;
    const end = new AbortController();
    setError("");
    const query = new URLSearchParams({ slug, base, head, path });
    void getJson(scoped(scope, `${TASK_DIFF_FILE_PATH}?${query}`), end.signal)
      .then((value) => {
        if (end.signal.aborted) return;
        const patch = readPatch(value, base, head, path);
        if (patch === null) throw new Error("The file patch is not readable");
        setAnswer(patch);
      })
      .catch((why: unknown) => {
        if (!end.signal.aborted) setError(said(why));
      });
    return () => end.abort();
  }, [scope, slug, base, head, path, attempt]);
  if (answer !== null) return <PatchText patch={answer.patch} truncated={answer.truncated} />;
  return error === "" ? (
    <p role="status" aria-busy="true">
      Reading this file's patch…
    </p>
  ) : (
    <p role="status">
      {failed(error)} <Button onClick={() => retry((n) => n + 1)}>Retry patch</Button>
    </p>
  );
}

function File({
  file,
  scope,
  slug,
  base,
  head,
}: {
  file: TaskDiffFile;
  scope: string;
  slug: string;
  base: string;
  head: string;
}) {
  const [opened, setOpened] = useState(false);
  return (
    <details
      onToggle={(event) => {
        if (event.currentTarget.open) setOpened(true);
      }}
    >
      <summary>
        <span className="task-diff-path">{file.path}</span>
        <span className="task-diff-counts">
          {file.added === null ? (
            "Binary file"
          ) : (
            <>
              <span data-change="added">+{file.added}</span>{" "}
              <span data-change="removed">−{file.removed}</span>
            </>
          )}
        </span>
      </summary>
      {opened && <Patch scope={scope} slug={slug} base={base} head={head} path={file.path} />}
    </details>
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
      {diff.base !== null &&
        diff.head !== null &&
        diff.files.map((file) => (
          <File
            key={`${diff.base}:${diff.head}:${file.path}`}
            file={file}
            scope={scope}
            slug={slug}
            base={diff.base as string}
            head={diff.head as string}
          />
        ))}
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
        <Button size="sm" onClick={() => void parts?.read(part)}>
          Refresh files
        </Button>
      </div>
      {answer.value !== undefined ? (
        <DiffFiles diff={answer.value} scope={scope} slug={slug} />
      ) : answer.error !== null ? (
        <p role="status">{failed(said(answer.error), true)}</p>
      ) : answer.missing ? (
        <p>No comparison is available.</p>
      ) : (
        <p role="status" aria-busy="true">
          Reading changed files…
        </p>
      )}
    </section>
  );
}
