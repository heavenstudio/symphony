// A project's own block, the narrow form of front-end extension the comparison prices
// (docs/analysis/client-framework-comparison.md, "Plugins and front-end extension"). The block
// vocabulary has no such kind and this experiment adds none, so it takes a fenced code block whose
// language is `custom` and whose text is the JSON a server would send as the block:
// `{"kind": "chart", "module": "/docs/blocks/chart.js", "data": {…}}`, plain data and the address
// of the project's module that draws it. The module's default export is `draw(element, data)`, the
// one contract both forms call; what it returns (JSON) is kept for the measurement.
//
// Two forms, picked by the page's address: by default the module draws in a sandboxed frame
// (custom-frame.jsx); with `?blocks=page` it is imported into the page and registered by its kind
// (custom-page.jsx), the form that reaches the whole origin. Each is a chunk of its own, loaded once
// a custom block comes near the screen, so a page without one carries this file and no more. Only a
// project file (a site path, as `media.rs` allows a viewer's script) is ever loaded; any other
// address draws the block as the code it is.
//
// For the measurement, each block's opening and drawing go in `window.customBlocks` (its kind, its
// form, both times from the page's start, its height and what the module returned), and the block
// is marked `data-drawn` once drawn, or `data-failed` with why.
import { useEffect, useMemo, useRef, useState } from "react";
import { useVisible } from "./near.jsx";
import { href } from "./project.js";

export const FORM = new URLSearchParams(location.search).get("blocks") === "page" ? "page" : "frame";
const FORMS = { frame: () => import("./custom-frame.jsx"), page: () => import("./custom-page.jsx") };
let form = null;
const loadForm = () => (form ??= FORMS[FORM]().then((module) => module.default));

const log = (window.customBlocks ??= []);

/** `media::local_asset`: a site path, never another origin or a scheme. */
function localAsset(src) {
  return typeof src === "string" && src.startsWith("/") && !src.startsWith("//") && !/[\u0000-\u001f\u007f-\u009f\\]/.test(src);
}

function parse(text) {
  try {
    const block = JSON.parse(text);
    if (typeof block?.kind === "string" && localAsset(block.module)) return block;
  } catch {}
  return null;
}

export function CustomBlock({ text, fallback }) {
  const block = useMemo(() => parse(text), [text]);
  const [shown, ref] = useVisible();
  const [Form, setForm] = useState(null);
  const [state, setState] = useState({});
  const entry = useRef(null);
  // The form's callbacks, the same functions for the block's life.
  const told = useMemo(
    () => ({
      onDrawn: ({ height, result }) => {
        Object.assign(entry.current, { drawn: performance.now(), height, result });
        setState({ drawn: true });
      },
      onFailed: (why) => {
        Object.assign(entry.current, { failed: String(why) });
        setState({ failed: String(why) });
      },
    }),
    [],
  );
  useEffect(() => {
    if (!shown || !block) return;
    entry.current = { kind: block.kind, form: FORM, opened: performance.now() };
    log.push(entry.current);
    let live = true;
    loadForm().then(
      (loaded) => live && setForm(() => loaded),
      (error) => live && told.onFailed(error?.message ?? error),
    );
    return () => {
      live = false;
    };
  }, [shown, block, told]);
  if (!block) return fallback;
  return (
    <div
      className="custom-block"
      ref={ref}
      data-kind={block.kind}
      data-form={FORM}
      data-drawn={state.drawn ? "true" : undefined}
      data-failed={state.failed}
      style={state.drawn ? undefined : RESERVE}
    >
      {Form ? <Form kind={block.kind} module={href(block.module)} data={block.data ?? null} {...told} /> : null}
      {state.failed ? <p role="alert">This block's module could not draw it: {state.failed}</p> : null}
    </div>
  );
}

const RESERVE = { minHeight: "120px" };
