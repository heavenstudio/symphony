// The Settings page's module, as the shell loads a page kind: `run` mounts it in its root
// (`#review-root-settings`, which the shell made and shows), and `stop` aborts its mount's signal and
// unmounts it, its reads with it, before the shell shows another kind. The page opens no feed and
// keeps nothing on the device but a field's draft: it reads the settings once, again while no
// answer came, and again after each write.

import "../settings.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Frame } from "../frame";
import { MountContext } from "../mount";
import { useRoute } from "../route";
import { SettingsPage } from "../settings";

function SettingsApp() {
  const route = useRoute();
  return (
    <Frame route={route}>
      <SettingsPage />
    </Frame>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-settings");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <SettingsApp />
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
