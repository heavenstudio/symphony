// A task's page module (`/task/SLUG`), as the shell loads a page kind: `run` mounts it in its root
// (`#review-root-task`, which the shell made and shows), and `stop` aborts its mount's signal and
// unmounts it, its reads, its feed and its media layer with it, before the shell shows another
// kind. A move within the kind, from task to task, is the shell's `review:navigate`, which the page
// follows without mounting again. Its gates' and its timeline's box's styles are its own; the
// timeline itself is a chunk the box imports once it nears the screen and the page has settled,
// with its own style; its pictures' and frames' styles are the Dioxus client's, which its media
// layer brings.

import "../gates.css";
import "../timeline-slot.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Frame } from "../frame";
import { MediaLayer } from "../media-layer";
import { MountContext } from "../mount";
import { useRoute } from "../route";
import { TaskPage } from "../task-page";

function TaskApp() {
  const route = useRoute();
  const slug = route.path.startsWith("task/") ? route.path.slice("task/".length) : "";
  return (
    <>
      {/* Before the page, so it hears what the page's first pictures say. */}
      <MediaLayer owner="task-pictures" />
      <Frame route={route}>
        {/* Each scope's tasks are a page of their own: their kept copies, their reads and their
            feed; within it the page follows the route from task to task. */}
        {slug !== "" ? (
          <TaskPage key={route.scope} scope={route.scope} slug={slug} />
        ) : (
          <p>Loading page…</p>
        )}
      </Frame>
    </>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-task");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <TaskApp />
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
