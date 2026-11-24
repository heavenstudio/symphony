// Which Go to session control an agent's session is given (docs/workflow/TASKS.md, "Going to a
// session"), apart from the control that draws it (session.tsx): a task's page reads it before that
// control's chunk comes (task-page.tsx's `SessionPart`), so the place it holds is the one the
// control takes, a button's box, a link's line or none.
//
// What crates/symphony-api's agents.rs decides for it (`SessionGo::control`, `via_parent`) is
// carried here, held to the Rust by the cases it writes (wire.cases.json, session.test.tsx), as
// project.ts carries the project's routes; the address it links is a `webAddress` (links.ts).

import { webAddress } from "./links";
import type { SessionGo } from "./wire";

/** What a Go to session control does for the page that shows it. */
export type SessionControl =
  /** A button: the server's machine shows the pane in the Herdr app and brings it forward. */
  | { kind: "focus"; pane: string }
  /** An ordinary link to the session's web address. */
  | { kind: "link"; url: string }
  /** The pane's screen, read only: what a page on another machine sees. */
  | { kind: "screen"; pane: string };

/**
 * The control a page shows for `go`, `local` when it was asked from the server's own machine: a pane
 * is focused there, else its web address is linked, else its screen is shown; none with neither.
 * An address that is no `webAddress` is never linked.
 */
export function sessionControl(go: SessionGo, local: boolean): SessionControl | null {
  const url = go.url !== null && webAddress(go.url) ? go.url : null;
  if (go.pane !== null && local) return { kind: "focus", pane: go.pane };
  if (url !== null) return { kind: "link", url };
  if (go.pane !== null) return { kind: "screen", pane: go.pane };
  return null;
}

/** Whether `go` goes to another session than `handle`'s own: a subagent's parent's. */
export function viaParent(go: SessionGo, handle: string): boolean {
  return go.handle !== handle;
}
