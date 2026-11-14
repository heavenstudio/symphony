// A project's block drawn in a sandbox (custom.jsx). The frame's `sandbox` allows scripts and
// nothing else: no `allow-same-origin`, so its origin is opaque (no cookie, storage or same-origin
// request is its, and the page around it is another origin's), no popups, forms or navigation of
// the page. Its own page, written here as its `srcdoc`, holds a policy that lets nothing load
// (`default-src 'none'`, scripts only inline or from the blob it makes): no request leaves it, to
// the Symphony's API or anywhere else.
//
// The page fetches the module's text (a project file, cached as any is) and sends it in by message
// with the block's data and the page's theme tokens, which the frame cannot read from the page; the
// frame imports the text as a module from a blob, calls its `draw(element, data)` and sends its
// height out, and again whenever it changes. The page hears only its own frame, sends to it once, at
// its first load (a second load is the frame gone to another address, which gets nothing), and
// holds its height between 40 and 4,000 px.
import { useEffect, useRef, useState } from "react";

/**
 * The theme tokens a module may use, read from the page and set on the frame's root, and the page's
 * colour scheme: a frame whose scheme is not its page's is drawn on an opaque white backdrop.
 */
const TOKENS = ["color-scheme", "--foreground", "--background", "--muted-foreground", "--border", "--primary", "--card", "--font-sans", "--font-mono"];

const POLICY = "default-src 'none'; script-src 'unsafe-inline' blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:";

// The frame's script: the first message from the page, and no other, starts it.
const SHIM = `
const root = document.getElementById("block");
const tell = (message) => parent.postMessage(message, "*");
const height = () => Math.ceil(root.getBoundingClientRect().height);
addEventListener("message", async ({ source, data: sent }) => {
  if (source !== parent || root.dataset.started) return;
  root.dataset.started = "true";
  for (const [name, value] of Object.entries(sent.theme)) document.documentElement.style.setProperty(name, value);
  try {
    const url = URL.createObjectURL(new Blob([sent.code], { type: "text/javascript" }));
    const result = await (await import(url)).default(root, sent.data);
    URL.revokeObjectURL(url);
    let last = height();
    tell({ drawn: true, height: last, result: JSON.parse(JSON.stringify(result ?? null)) });
    new ResizeObserver(() => {
      const now = height();
      if (now !== last) tell({ height: (last = now) });
    }).observe(root);
  } catch (error) {
    tell({ failed: String(error && error.message ? error.message : error) });
  }
});`;

const PAGE =
  `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${POLICY}"><meta name="color-scheme" content="light dark">` +
  "<style>html,body{margin:0;overflow:hidden;background:transparent;color:var(--foreground);font-family:var(--font-sans,system-ui,sans-serif)}</style>" +
  `</head><body><div id="block"></div><script type="module">${SHIM}</script></body></html>`;

const sources = new Map();
/** The module's text, fetched once a page. */
function source(module) {
  if (!sources.has(module)) {
    sources.set(
      module,
      fetch(module).then((response) => {
        if (!response.ok) throw new Error(`${module}: ${response.status}`);
        return response.text();
      }),
    );
  }
  return sources.get(module);
}

function theme(element) {
  const style = getComputedStyle(element);
  return Object.fromEntries(TOKENS.map((name) => [name, style.getPropertyValue(name).trim()]).filter(([, value]) => value));
}

export default function FramedBlock({ kind, module, data, onDrawn, onFailed }) {
  const frame = useRef(null);
  const loads = useRef(0);
  const [height, setHeight] = useState(120);
  useEffect(() => {
    // Fetched while the frame's page is made, not after it.
    source(module).catch(() => {});
    const heard = (event) => {
      if (!frame.current || event.source !== frame.current.contentWindow) return;
      const said = event.data ?? {};
      if (typeof said.height === "number") setHeight(Math.min(4000, Math.max(40, Math.ceil(said.height))));
      if (said.drawn === true) onDrawn({ height: said.height, result: said.result });
      if (typeof said.failed === "string") onFailed(said.failed);
    };
    window.addEventListener("message", heard);
    return () => window.removeEventListener("message", heard);
  }, [module, onDrawn, onFailed]);
  const loaded = () => {
    if (loads.current++ > 0) return;
    source(module).then((code) => frame.current?.contentWindow?.postMessage({ code, data, theme: theme(frame.current) }, "*"), (error) => onFailed(error.message));
  };
  return <iframe ref={frame} sandbox="allow-scripts" srcDoc={PAGE} title={`${kind} block`} onLoad={loaded} style={{ display: "block", width: "100%", height: `${height}px`, border: 0 }} />;
}
