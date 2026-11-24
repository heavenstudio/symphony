// The People page's module, as the shell loads a page kind: `run` mounts it in its root
// (`#review-root-people`, which the shell made and shows), and `stop` aborts its mount's signal and
// unmounts it, its reads with it, before the shell shows another kind. The page opens no feed and
// keeps nothing on the device: it reads the people once, again while no answer came, and draws
// each write's answer.

import "../people.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Frame } from "../frame";
import { MountContext } from "../mount";
import { PeoplePage } from "../people";
import { useRoute } from "../route";

function PeopleApp() {
  const route = useRoute();
  return (
    <Frame route={route}>
      <PeoplePage />
    </Frame>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-people");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <PeopleApp />
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
