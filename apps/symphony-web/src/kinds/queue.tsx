// The review queue's page module, as the shell loads a page kind: `run` mounts it in its root
// (`#review-root-queue`, which the shell made and shows), and `stop` aborts its mount's signal and
// unmounts it, its reads and its feed with it, before the shell shows another kind. A move within
// the kind is the shell's `review:navigate`, which the page follows without mounting again.

import "../queue.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { BoardPage } from "../board-page";
import { Frame } from "../frame";
import { MountContext } from "../mount";
import { changed, QueuePage } from "../queue";
import { useRoute } from "../route";

function QueueApp() {
  const route = useRoute();
  return (
    <Frame route={route}>
      {/* Each scope's board is a page of its own: its kept copy, its reads and its feed. */}
      <BoardPage key={route.scope} scope={route.scope} kept moves={changed}>
        {(refreshed) => <QueuePage scope={route.scope} refreshed={refreshed} />}
      </BoardPage>
    </Frame>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-queue");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <QueueApp />
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
