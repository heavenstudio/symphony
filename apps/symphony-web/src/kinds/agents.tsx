// The agents page's module (`/agents`), as the shell loads a page kind: `run` mounts it in its root
// (`#review-root-agents`, which the shell made and shows), and `stop` aborts its mount's signal and
// unmounts it, its reads and its feed with it, before the shell shows another kind.

import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { AgentsPage } from "../agents";
import { Frame } from "../frame";
import { MountContext } from "../mount";
import { useRoute } from "../route";

function AgentsApp() {
  const route = useRoute();
  return (
    <Frame route={route}>
      {/* Each scope's agents are a page of their own: their reads and their feed. */}
      {route.path === "agents" ? (
        <AgentsPage key={route.scope} scope={route.scope} />
      ) : (
        <p>Loading page…</p>
      )}
    </Frame>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-agents");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <AgentsApp />
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
