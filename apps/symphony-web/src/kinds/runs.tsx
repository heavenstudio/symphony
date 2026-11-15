// The Runs page's module, as the shell loads a page kind: `run` mounts it in its root
// (`#review-root-runs`, which the shell made and shows), and `stop` aborts its mount's signal and
// unmounts it, its reads with it, before the shell shows another kind. The page opens no feed: it
// reads what runs now, its list and an open run again on timers of its own.

import "../runs.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Frame } from "../frame";
import { MountContext } from "../mount";
import { useRoute } from "../route";
import { RunsPage } from "../runs";

function RunsApp() {
  const route = useRoute();
  return (
    <Frame route={route}>
      <RunsPage />
    </Frame>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-runs");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <RunsApp />
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
