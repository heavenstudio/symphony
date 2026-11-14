// The documents' page module (an entrypoint's documents and folders, the kind the shell names
// `docs`), as the shell loads a page kind: `run` mounts it in its root (`#review-root-docs`, which
// the shell made and shows), and `stop` aborts its mount's signal and unmounts it, its reads, its
// feeds and its media layer with it, before the shell shows another kind. A move within the kind is
// the shell's `review:navigate`, which the page follows, each document or folder a page of its own.
// Its pictures' and frames' styles are the Dioxus client's, which its media layer brings; its own
// are `docs.css`.

import "../docs.css";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { DocumentPage, FolderPage, HtmlPage } from "../document-page";
import { Frame } from "../frame";
import { MediaLayer } from "../media-layer";
import { MountContext } from "../mount";
import { isHtml } from "../project";
import { useRoute } from "../route";

function DocsApp() {
  const route = useRoute();
  const key = `${route.scope}/${route.path}`;
  let page = <p>Loading page…</p>;
  // A document's route named `.html` or `.htm` is an HTML page: decided here, in the documents'
  // kind, so no other kind carries the check.
  if (route.document && route.folder) {
    page = <FolderPage key={key} path={route.path} scope={route.scope} />;
  } else if (route.document && isHtml(route.path)) {
    page = <HtmlPage key={key} path={route.path} scope={route.scope} />;
  } else if (route.document) {
    page = <DocumentPage key={key} path={route.path} scope={route.scope} />;
  }
  return (
    <>
      {/* Before the page, so it hears what the page's first pictures say; started at once, as a
          document's opened folds are busy, which its feed reads. */}
      <MediaLayer owner="docs-pictures" immediate />
      <Frame route={route}>
        <main>{page}</main>
      </Frame>
    </>
  );
}

let mounted: { root: Root; mount: AbortController } | null = null;

/** Mount the page in its root, drawn before the promise settles. */
export async function run(): Promise<void> {
  const element = document.getElementById("review-root-docs");
  if (!element || mounted) return;
  const page = { root: createRoot(element), mount: new AbortController() };
  mounted = page;
  flushSync(() =>
    page.root.render(
      <MountContext.Provider value={page.mount.signal}>
        <DocsApp />
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
