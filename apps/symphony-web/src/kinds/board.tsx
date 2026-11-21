// The task board's page module (`/tasks`, and `/task` with no task), as the shell loads a page kind:
// `run` mounts it in its root (`#review-root-board`, which the shell made and shows), and `stop`
// aborts its mount's signal and unmounts it, its reads and its feed with it, before the shell shows
// another kind. A move within the kind is the shell's `review:navigate`, which the page follows
// without mounting again.

import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { TasksPage } from "../boardlive";
import { CardCostsOf } from "../card-costs";
import { Frame } from "../frame";
import { MountContext } from "../mount";
import { useRoute } from "../route";

function BoardApp() {
  const route = useRoute();
  return (
    <Frame route={route}>
      {/* Each scope's board is a page of its own: its kept copy, its reads and its feed. */}
      {route.path === "tasks" ? (
        <CardCostsOf key={route.scope} scope={route.scope}>
          <TasksPage scope={route.scope} />
        </CardCostsOf>
      ) : (
        <p>Loading page…</p>
      )}
    </Frame>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-board");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <BoardApp />
      </MountContext.Provider>,
    ),
  );
}

/** Unmount the page: what it started ends with its mount, and its effects' cleanups run. */
export async function stop(): Promise<void> {
  mounted?.mount.abort();
  mounted?.root.unmount();
  mounted = null;
}
