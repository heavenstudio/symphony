// Home's page module (`/`, the kind the shell names `review`), as the shell loads a page kind: `run`
// mounts it in its root (`#review-root-review`, which the shell made and shows), and `stop` aborts
// its mount's signal and unmounts it, its reads, its feed and its media layer with it, before the
// shell shows another kind. A move within the kind is the shell's `review:navigate`, which the page
// follows without mounting again. Its pictures' and frames' styles are the Dioxus client's, which
// its media layer brings.

import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { BoardPage } from "../board-page";
import { Frame } from "../frame";
import { HomePage } from "../home";
import { heard, readBoard } from "../home-board";
import { MediaLayer } from "../media-layer";
import { MountContext } from "../mount";
import { useRoute } from "../route";

function HomeApp() {
  const route = useRoute();
  return (
    <>
      {/* Before the page, so it hears what the page's first pictures say. */}
      <MediaLayer owner="review-pictures" />
      <Frame route={route}>
        {/* Each scope's board is a page of its own: its kept copy, its reads and its feed. */}
        <BoardPage key={route.scope} scope={route.scope} kept read={readBoard} apply={heard}>
          {(_, board) => <HomePage board={board} />}
        </BoardPage>
      </Frame>
    </>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-review");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <HomeApp />
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
