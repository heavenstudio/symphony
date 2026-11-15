// The panel's module, as the shell loads its persistent kind `panel` (on a click of `+`, the bell
// or Appearance, a toast's arrival or a pending step, and ahead of it once the page is idle): `run`
// mounts the panel in its root (`#quick-root`, which the shell made beside every page's), and
// `stop` aborts its mount's signal and unmounts it.

import "../panel.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { MountContext } from "../mount";
import { PanelApp } from "../panel";

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the panel in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("quick-root");
  if (!element || mounted) return;
  const panel = { root: createRoot(element), mount: new AbortController() };
  mounted = panel;
  flushSync(() =>
    panel.root.render(
      <MountContext.Provider value={panel.mount.signal}>
        <PanelApp />
      </MountContext.Provider>,
    ),
  );
}

/** Unmount the panel: what it drew goes with it, and its effects' cleanups run. */
export async function stop(): Promise<void> {
  mounted?.mount.abort();
  mounted?.root.unmount();
  mounted = null;
}
