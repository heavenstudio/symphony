// The overlay module, as the shell loads its persistent kind `overlay` (on the first select or
// picture any page shows, or ahead of it when the page is idle): `run` mounts the overlay's host in
// its root (`#overlay-root`, which the shell made beside every page's), and `stop` aborts its
// mount's signal and unmounts it. The host is drawn in the page frame's class, so its buttons and
// popups look as they do on the page that asked for them.

import "../overlay.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { MountContext } from "../mount";
import { OverlayHost } from "../overlay";

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the host in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("overlay-root");
  if (!element || mounted) return;
  const host = { root: createRoot(element), mount: new AbortController() };
  mounted = host;
  flushSync(() =>
    host.root.render(
      <MountContext.Provider value={host.mount.signal}>
        <div className="app-frame">
          <OverlayHost />
        </div>
      </MountContext.Provider>,
    ),
  );
}

/** Unmount the host: what it drew goes with it, and its effects' cleanups run. */
export async function stop(): Promise<void> {
  mounted?.mount.abort();
  mounted?.root.unmount();
  mounted = null;
}
