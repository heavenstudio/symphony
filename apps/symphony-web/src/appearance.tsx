// What the header's Appearance button opens (`#appearance-panel`): this project's look on this
// device, a shadcn theme imported or the default back. Opened, it takes the focus; Escape or ×
// closes it and gives the focus back to the button.

import { type CSSProperties, useEffect, useState } from "react";
import { Button } from "./button";
import { Input } from "./input";
import { useMount } from "./mount";
import { said } from "./request";
import { publish, remember, snapshot } from "./shared";
import { install, readTheme, stylesheet } from "./theme";

/** The CSS a theme file's text makes, or why it makes none. */
export function themeOf(text: string): string {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("The theme is not valid JSON.");
  }
  const vars = readTheme(value);
  if (vars === null) throw new Error("The theme needs cssVars with theme, light or dark tokens.");
  return stylesheet(vars);
}

/** The project's look on this device made by a theme file's text (`install`), and what the status
 * line says of it: applied, or why not. */
export function applyTheme(text: string): { applied: boolean; status: string } {
  try {
    install(themeOf(text));
    return { applied: true, status: "Theme applied." };
  } catch (failure) {
    return { applied: false, status: said(failure) };
  }
}

/** The project's default look back on this device, and what the status line says of it. */
export function restoreDefault(): string {
  try {
    install("");
    return "Default theme restored.";
  } catch (failure) {
    return said(failure);
  }
}

/** The two things a device can do to the project's look: import a shadcn theme, or go back to the
 * default. */
export function ThemeMenu() {
  const mount = useMount();
  const [importing, setImporting] = useState(false);
  const [status, setStatus] = useState("");
  return (
    <div className="theme-menu">
      <div className="theme-actions">
        <Button type="button" data-theme-action="import" onClick={() => setImporting(true)}>
          Import shadcn theme…
        </Button>
        <Button
          type="button"
          data-theme-action="default"
          onClick={() => setStatus(restoreDefault())}
        >
          Default theme
        </Button>
      </div>
      {importing && (
        // The label draws the picker, as the composer's Attach does; the native input inside it
        // takes its clicks.
        <label className="attach-pick theme-pick" htmlFor="theme-file">
          Choose a theme file…
          <Input
            id="theme-file"
            type="file"
            accept=".json,application/json"
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (!file) return;
              void (async () => {
                let text: string;
                try {
                  text = await file.text();
                } catch (failure) {
                  if (!mount.aborted) setStatus(said(failure));
                  return;
                }
                if (mount.aborted) return;
                const outcome = applyTheme(text);
                setStatus(outcome.status);
                if (outcome.applied) setImporting(false);
              })();
            }}
          />
        </label>
      )}
      <p role="status">{status}</p>
    </div>
  );
}

/** The Appearance panel closed, said to the frame, and the focus given back to its button. */
export function closeAppearance(): void {
  remember("reviewAppearanceOpen", false);
  publish("review:appearance");
  document.querySelector<HTMLElement>("#appearance")?.focus();
}

export function AppearancePanel() {
  const [heard, setHeard] = useState(0);
  useEffect(() => {
    const opened = () => setHeard((count) => count + 1);
    addEventListener("review:appearance", opened);
    return () => removeEventListener("review:appearance", opened);
  }, []);
  const open = snapshot("reviewAppearanceOpen") === true;
  useEffect(() => {
    void heard;
    if (snapshot("reviewAppearanceOpen") === true) {
      document.querySelector<HTMLElement>("#appearance-panel .theme-menu button")?.focus();
    }
  }, [heard]);
  if (!open) return null;
  // Under its button wherever the header sits (a notice can push it down): frame.css anchors the
  // panel to the button where the browser can, and this measured place stands in where it cannot.
  const button = document.getElementById("appearance");
  const below = button ? button.getBoundingClientRect().bottom + 8 : 60;
  return (
    <aside
      id="appearance-panel"
      role="dialog"
      aria-label="Appearance"
      style={{ "--appearance-top": `${Math.round(below)}px` } as CSSProperties}
      onKeyDown={(event) => {
        if (event.key === "Escape") closeAppearance();
      }}
    >
      <div className="panel-heading">
        <h2>Appearance</h2>
        <Button type="button" aria-label="Close appearance" onClick={closeAppearance}>
          ×
        </Button>
      </div>
      <ThemeMenu />
    </aside>
  );
}
