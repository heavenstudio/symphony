// The review queue's page module, as the shell loads a page kind: `run` mounts it in its root
// (`#review-root-queue`, which the shell made and shows), and `stop` unmounts it, its reads and its
// feed with it, before the shell shows another kind. A move within the kind is the shell's
// `review:navigate`, which the page follows without mounting again.

import "../queue.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Frame } from "../frame";
import { BoardPage } from "../live";
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

let root: Root | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const mount = document.getElementById("review-root-queue");
  if (!mount || root) return;
  const mounted = createRoot(mount);
  root = mounted;
  flushSync(() => mounted.render(<QueueApp />));
}

/** Unmount the page: its effects' cleanups close its feed and abandon its reads. */
export async function stop(): Promise<void> {
  root?.unmount();
  root = null;
}
