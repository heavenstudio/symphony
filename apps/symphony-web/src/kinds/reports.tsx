// The Reports page's module, as the shell loads a page kind: `run` mounts it in its root
// (`#review-root-reports`, which the shell made and shows), and `stop` aborts its mount's signal
// and unmounts it, its reads with it, before the shell shows another kind. The page opens no feed:
// it reads its window again on a timer of its own.

import "../reports.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Frame } from "../frame";
import { MountContext } from "../mount";
import { ReportsPage } from "../reports";
import { useRoute } from "../route";

function ReportsApp() {
  const route = useRoute();
  return (
    <Frame route={route}>
      {/* Each scope's reports are a page of its own: its window, its read and what it hid. */}
      <ReportsPage key={route.scope} scope={route.scope} />
    </Frame>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-reports");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <ReportsApp />
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
