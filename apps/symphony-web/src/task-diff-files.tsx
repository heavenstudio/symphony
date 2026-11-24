// The files of a task's changes (task-diff.tsx), a chunk of its own: imported once a comparison has
// files to list, so a task's first visit, whose changes are below its first screen, carries none of
// it for a task with none. Each file is a summary of its path and counts; its patch is read only
// after its details opens, bound to the comparison's commits, and given up as the comparison leaves.
// It imports nothing of the task kind's own, and puts its style (task-diff-files.css) in the page.

import { useEffect, useLayoutEffect, useState } from "react";
import { Button } from "./button";
import { scoped } from "./project";
import { getJson, said } from "./request";
import { field } from "./shared";
import STYLE from "./task-diff-files.css?inline";
import { failed } from "./trouble";
import { TASK_DIFF_FILE_PATH, type TaskDiffFile, type TaskDiffPatch } from "./wire";

/** Put the list's style in the page once, before the project's theme as the shell links a kind's. */
function putStyle(): void {
  if (document.getElementById("task-diff-files-style")) return;
  const element = document.createElement("style");
  element.id = "task-diff-files-style";
  element.textContent = STYLE;
  document.head.insertBefore(element, document.head.querySelector("[data-symphony-theme]"));
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

export function PatchText({ patch, truncated }: { patch: string; truncated: boolean }) {
  return (
    <>
      {/* biome-ignore lint/a11y/noNoninteractiveTabindex: this code region scrolls in both directions and needs keyboard access */}
      <section className="task-diff-patch" tabIndex={0} aria-label="File patch">
        <pre>{patch}</pre>
      </section>
      {truncated && <p role="status">Patch stops at 256 KiB.</p>}
    </>
  );
}

export function Patch({
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

export function File({
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

export interface DiffFileListProps {
  files: TaskDiffFile[];
  scope: string;
  slug: string;
  /** The comparison's commits, which each patch is read at. */
  base: string;
  head: string;
}

/** The comparison's files, a summary each, whose patch is read once it opens. */
export function DiffFileList({ files, scope, slug, base, head }: DiffFileListProps) {
  useLayoutEffect(putStyle, []);
  return (
    <>
      {files.map((file) => (
        <File
          key={`${base}:${head}:${file.path}`}
          file={file}
          scope={scope}
          slug={slug}
          base={base}
          head={head}
        />
      ))}
    </>
  );
}
