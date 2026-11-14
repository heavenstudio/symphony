// The probe: whether three libraries a whole JSX client would likely reach for run on Preact
// through the compat alias as they run on React, and what each adds. Not part of the three pages:
// a page of its own (`probe.html`), built by `npm run build:probe` into `dist/probe-react` and
// `dist/probe-preact`, so the pages' own build and figures stay as measured.
//
// `probe.html?NAME` draws one library's probe, its chunk imported by hand so that the bytes it
// adds are its chunk's alone; with no name the page draws only the runtime, the baseline.
// `primitives` is Radix (a select, a menu, a dialog, tabs, a tooltip, a checkbox), `query`
// TanStack Query (reads, paging, a feed event's re-read), `virtual` TanStack Virtual (a 2,000-card
// column), and `slider` a control that is known not to work on Preact, to show the checks can
// tell. Each marks its root `data-ready` once drawn; the checks are driven from outside by a
// headless browser (README, "The probe").
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

const probes = {
  primitives: () => import("./primitives.jsx"),
  query: () => import("./query.jsx"),
  virtual: () => import("./virtual.jsx"),
  slider: () => import("./slider.jsx"),
};

function Probe({ name }) {
  const [Page, setPage] = useState(null);
  const [failed, setFailed] = useState("");
  useEffect(() => {
    if (!probes[name]) return;
    probes[name]().then(
      (module) => setPage(() => module.default),
      (error) => setFailed(String(error)),
    );
  }, [name]);
  if (!name) return <p id="baseline" data-ready="true">{__CANDIDATE__}: the runtime alone.</p>;
  if (!probes[name]) return <p id="unknown">No probe named {name}.</p>;
  if (failed) return <p id="failed">{failed}</p>;
  return Page ? <Page /> : <p aria-busy="true">Loading {name}…</p>;
}

window.__candidate = __CANDIDATE__;
createRoot(document.getElementById("probe")).render(<Probe name={location.search.slice(1)} />);
