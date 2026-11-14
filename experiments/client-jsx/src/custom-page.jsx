// A project's block drawn in the page itself (custom.jsx, `?blocks=page`): the project's module
// imported into the page and registered by its kind, its `draw(element, data)` called with an
// element of the page. Evidence for the comparison, not a proposal: it is the form that costs least
// and reaches most, as the module runs as the page does, on the origin every project of the
// machine's server shares. It can read and change the whole page, replace any of its functions,
// read the API and the browser's storage, and post as the owner, as the probe module shows
// (README, "A project's own block").
import { useEffect, useRef } from "react";

const kinds = new Map();

/** The `draw` registered for `kind`: the first module named for it, imported once. */
export function register(kind, module) {
  if (!kinds.has(kind)) kinds.set(kind, import(/* @vite-ignore */ module).then((loaded) => loaded.default));
  return kinds.get(kind);
}

export default function PageBlock({ kind, module, data, onDrawn, onFailed }) {
  const element = useRef(null);
  useEffect(() => {
    let live = true;
    register(kind, module)
      .then((draw) => draw(element.current, data))
      .then(
        (result) => live && onDrawn({ height: element.current.getBoundingClientRect().height, result: result ?? null }),
        (error) => live && onFailed(String(error?.message ?? error)),
      );
    return () => {
      live = false;
    };
  }, [kind, module, data, onDrawn, onFailed]);
  return <div ref={element} />;
}
