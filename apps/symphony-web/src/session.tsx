// An agent's Go to session control (docs/TASKS.md, "Going to a session"), which a task's role chips
// and the agents page's rows draw: an ordinary link to its session's web address; for a page on the
// server's machine, a button that has the Herdr app show its pane; for a page anywhere else, a
// button that shows the pane's screen, read only. A subagent's goes to its parent's session and says
// so; with nothing known there is none.
//
// `sessionControl` and `viaParent` carry crates/symphony-api's own (`SessionGo::control`,
// `SessionGo::via_parent`), held to the Rust by the cases it writes (wire.cases.json,
// session.test.tsx); the address it links is a `webAddress` (links.ts).

import { useState } from "react";
import { Button } from "./button";
import { webAddress } from "./links";
import { useMount } from "./mount";
import { scoped } from "./project";
import { getJson, post, said } from "./request";
import { currentRoute } from "./route";
import { field } from "./shared";
import { readScreen } from "./task-read";
import type { AgentScreen, SessionGo } from "./wire";
import { AGENT_FOCUS_PATH, AGENT_SCREEN_PATH } from "./wire";

/** The control a page shows for a session, `local` when it was asked from the server's own
 * machine: a pane is focused there, else its web address is linked, else its screen is shown. */
export type SessionControl =
  | { kind: "focus"; pane: string }
  | { kind: "link"; url: string }
  | { kind: "screen"; pane: string };

/** The control `go` gives, none when nothing is known: an address that is no `webAddress` is never
 * linked, the session gone to by its pane or not at all. */
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

/** What a focus post answered: the server's words, else why they could not be read. */
export function focusSaid(answer: unknown): string {
  const words = field(answer, "said");
  return typeof words === "string" ? words : "The server's answer has no words";
}

export interface GoToSessionProps {
  handle: string;
  go: SessionGo | null | undefined;
  local: boolean;
}

/** `handle`'s Go to session, as `go` says it is reached; nothing when nothing is known. */
export function GoToSession({ handle, go, local }: GoToSessionProps) {
  const mount = useMount();
  const [words, setWords] = useState("");
  const [busy, setBusy] = useState(false);
  const [screen, setScreen] = useState<AgentScreen | null>(null);
  if (!go) return null;
  const control = sessionControl(go, local);
  if (control === null) return null;
  const parent = viaParent(go, handle) ? go.handle : null;
  const focus = async () => {
    if (busy) return;
    setBusy(true);
    setWords("");
    let told: string;
    try {
      told = focusSaid(await post(scoped(currentRoute().scope, AGENT_FOCUS_PATH), { handle }));
    } catch (error) {
      told = said(error);
    }
    if (mount.aborted) return;
    setWords(told);
    setBusy(false);
  };
  const look = async () => {
    if (busy) return;
    setBusy(true);
    setWords("");
    try {
      const path = `${AGENT_SCREEN_PATH}?handle=${encodeURIComponent(handle)}`;
      const value = await getJson(scoped(currentRoute().scope, path), mount);
      if (mount.aborted) return;
      const read = readScreen(value);
      if (read === null) setWords("The session's screen is not readable");
      else setScreen(read);
    } catch (error) {
      if (mount.aborted) return;
      setWords(said(error));
    }
    setBusy(false);
  };
  return (
    <span className="go-session" data-go={control.kind}>
      {control.kind === "link" ? (
        <a className="go-session-link" href={control.url} target="_blank" rel="noopener noreferrer">
          Go to session
        </a>
      ) : control.kind === "focus" ? (
        <Button
          type="button"
          size="xs"
          disabled={busy}
          title={`Shows ${control.pane} in the Herdr app on this machine`}
          onClick={() => void focus()}
        >
          Go to session
        </Button>
      ) : (
        <Button
          type="button"
          size="xs"
          disabled={busy}
          title={`Shows ${control.pane}'s screen, read only`}
          onClick={() => void look()}
        >
          {screen !== null ? "Read the screen again" : "Go to session"}
        </Button>
      )}
      {parent !== null && <span className="go-session-via">{`in ${parent}'s session`}</span>}
      {words !== "" && <span role="status">{words}</span>}
      {screen !== null && (
        // biome-ignore lint/a11y/useSemanticElements: a span inside the chip's line, as the Dioxus page drew it, not a section of the page
        <span
          className="session-screen"
          role="region"
          aria-label={`${screen.pane} in ${screen.handle}'s session, read only`}
        >
          <pre>{screen.text}</pre>
          <Button type="button" size="xs" onClick={() => setScreen(null)}>
            Close
          </Button>
        </span>
      )}
    </span>
  );
}
