// An agent's Go to session control (docs/workflow/TASKS.md, "Going to a session"), which the agents page's
// rows and a task's role chips draw: an ordinary link to its session's web address; for a page on
// the server's machine, a button that has the Herdr app show its pane; for a page anywhere else, a
// button that shows the pane's screen, read only. A subagent's goes to its parent's session and says
// so; with nothing known there is none.
//
// What crates/symphony-api's agents.rs decides for it (`SessionGo::control`, `via_parent`) is
// carried here, held to the Rust by the cases it writes (wire.cases.json, session.test.tsx), as
// project.ts carries the project's routes; the address it links is a `webAddress` (links.ts). What a
// press does, a focus or a screen read, is session-press.ts, a chunk imported as one is first pressed.

import { useState } from "react";
import { Button } from "./button";
import { webAddress } from "./links";
import { useMount } from "./mount";
import type { press } from "./session-press";
import { snapshot } from "./shared";
import type { AgentScreen, SessionGo } from "./wire";

/** What a Go to session control does for the page that shows it. */
export type SessionControl =
  /** A button: the server's machine shows the pane in the Herdr app and brings it forward. */
  | { kind: "focus"; pane: string }
  /** An ordinary link to the session's web address. */
  | { kind: "link"; url: string }
  /** The pane's screen, read only: what a page on another machine sees. */
  | { kind: "screen"; pane: string };

/** Unicode's White_Space but its one range (U+2000 to U+200A), by their code points. */
const SPACES = [0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0x85, 0xa0, 0x1680, 0x2028, 0x2029, 0x202f];
const MORE_SPACES = [0x205f, 0x3000];

/**
 * Whether `code`, a UTF-16 unit, is a space as Rust's `char::is_whitespace` names one: Unicode's
 * White_Space. No surrogate is one, and U+FEFF, a space to JavaScript's `\s` and `trim`, is not;
 * U+0085 is.
 */
export function whiteSpace(code: number): boolean {
  return (code >= 0x2000 && code <= 0x200a) || SPACES.includes(code) || MORE_SPACES.includes(code);
}

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

/** What a press of Go to session shows: whether it is on its way, what it said, and a screen read. */
export interface Going {
  busy(busy: boolean): void;
  said(text: string): void;
  screen(read: AgentScreen): void;
}

/** What a press does (session-press.ts), imported as a control is first pressed. */
const pressing = () => import("./session-press");

/**
 * What a control says where what a press does did not come, once the shell said the page's build is
 * there (where it is gone, the page loads again).
 */
export const UNPRESSED = "This page could not load the session's control. Try again.";

/**
 * A press of `control`'s button for `handle`'s session (session-press.ts `press`), once what a press
 * does has come: nothing while one is on its way (`busy`) or for a link, which the browser follows;
 * on its way while it comes. Where it did not come, UNPRESSED is said and the press may be made
 * again; nothing more once `mount` ended meanwhile.
 */
export async function pressed(
  control: SessionControl,
  handle: string,
  scope: string,
  busy: boolean,
  mount: AbortSignal,
  going: Going,
  load: () => Promise<{ press: typeof press }> = pressing,
): Promise<void> {
  if (busy || control.kind === "link") return;
  going.busy(true);
  let chunk: { press: typeof press };
  try {
    chunk = await load();
  } catch {
    const shell = snapshot("reviewShell") as { gone(): Promise<boolean> } | undefined;
    if (mount.aborted || (await shell?.gone())) return;
    going.busy(false);
    going.said(UNPRESSED);
    return;
  }
  if (!mount.aborted) await chunk.press(control, handle, scope, false, mount, going);
}

export interface GoToSessionProps {
  /** The agent the control is for. */
  handle: string;
  /** Where its control goes, as the server resolved it: none when nothing is known. */
  go: SessionGo | null;
  /** The page was asked from the server's own machine. */
  local: boolean;
  /** The checkout whose board the page shows (`""`, the main one's). */
  scope: string;
}

/** An agent's Go to session control, or nothing when its session has no way in. */
export function GoToSession({ handle, go, local, scope }: GoToSessionProps) {
  const [told, setTold] = useState("");
  const [busy, setBusy] = useState(false);
  const [shownScreen, setScreen] = useState<AgentScreen | null>(null);
  const mount = useMount();
  if (go === null) return null;
  const control = sessionControl(go, local);
  if (control === null) return null;
  return (
    <SessionView
      control={control}
      parent={viaParent(go, handle) ? go.handle : null}
      busy={busy}
      told={told}
      screen={shownScreen}
      onPress={() =>
        void pressed(control, handle, scope, busy, mount, {
          busy: setBusy,
          said: setTold,
          screen: setScreen,
        })
      }
      onClose={() => setScreen(null)}
    />
  );
}

export interface SessionViewProps {
  control: SessionControl;
  /** The parent whose session it goes to, for a subagent's. */
  parent: string | null;
  busy: boolean;
  /** What the server said the last press did, or what failed. */
  told: string;
  /** The screen read last, until it is closed. */
  screen: AgentScreen | null;
  onPress(): void;
  onClose(): void;
}

/** A Go to session control as it stands. */
export function SessionView({
  control,
  parent,
  busy,
  told,
  screen: shownScreen,
  onPress,
  onClose,
}: SessionViewProps) {
  return (
    <span className="go-session" data-go={control.kind}>
      {control.kind === "link" ? (
        <a className="go-session-link" href={control.url} target="_blank" rel="noopener noreferrer">
          Go to session
        </a>
      ) : (
        <Button
          type="button"
          size="xs"
          disabled={busy}
          title={
            control.kind === "focus"
              ? `Shows ${control.pane} in the Herdr app on this machine`
              : `Shows ${control.pane}'s screen, read only`
          }
          onClick={onPress}
        >
          {control.kind === "screen" && shownScreen !== null
            ? "Read the screen again"
            : "Go to session"}
        </Button>
      )}
      {parent !== null && <span className="go-session-via">{`in ${parent}'s session`}</span>}
      {told !== "" && <span role="status">{told}</span>}
      {shownScreen !== null && (
        // biome-ignore lint/a11y/useSemanticElements: inline in the agent's row and a task's role chip, as the Dioxus page draws it
        <span
          className="session-screen"
          role="region"
          aria-label={`${shownScreen.pane} in ${shownScreen.handle}'s session, read only`}
        >
          <pre>{shownScreen.text}</pre>
          <Button type="button" size="xs" onClick={onClose}>
            Close
          </Button>
        </span>
      )}
    </span>
  );
}
