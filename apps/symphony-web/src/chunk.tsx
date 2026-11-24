// A part of a task's page that a chunk of its own draws, imported once the part is first drawn: a
// workflow's next actions (nextactions.tsx), whose the task is (owner-part.tsx), a holder's Go to
// session control (session-part.tsx), a task's attempts (attempts.tsx) and the files of its changes
// (task-diff-files.tsx). One that does not come is said with its retry, once the shell has said the
// page's build is not gone (the page then loads again). Such a chunk imports nothing of the kind's
// own, so the kind's entry stays one file (the client's guide).

import { useEffect, useState } from "react";
import { Button } from "./button";
import { snapshot } from "./shared";

/**
 * The chunk `load` imports, asked as the part is first drawn: none until it has come, or `loaded`'s
 * at once when it has come before; whether it did not come (said only once the build is there); and
 * the retry, which asks for it again. `load` is a function of the module's, the same at each draw.
 */
export function useChunk<T>(
  load: () => Promise<T>,
  loaded: () => T | null = () => null,
): [T | null, boolean, () => void] {
  const [chunk, setChunk] = useState<T | null>(loaded);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let on = chunk === null && !failed;
    if (on)
      load().then(
        (come) => on && setChunk(() => come),
        async () => {
          const shell = snapshot("reviewShell") as { gone(): Promise<boolean> } | undefined;
          if (on && !(await shell?.gone())) setFailed(true);
        },
      );
    return () => {
      on = false;
    };
  }, [chunk, failed, load]);
  return [chunk, failed, () => setFailed(false)];
}

/**
 * That `what` could not be loaded, with its retry: drawn by the part itself, in its place, a
 * paragraph of its own, or `inline` in a line of other words (a role's chip).
 */
export function chunkFailed(what: string, retry: () => void, inline = false) {
  const said = (
    <>
      {`${what} could not be loaded. `}
      <Button onClick={retry}>Retry</Button>
    </>
  );
  return inline ? <span role="alert">{said}</span> : <p role="alert">{said}</p>;
}
