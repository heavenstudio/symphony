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

/** The two things a device can do to the project's look: import a shadcn theme, or go back to the
 * default. */
export function ThemeMenu() {
  const mount = useMount();
  const [importing, setImporting] = useState(false);
  const [status, setStatus] = useState("");
  return (
    <div className="theme-menu">
      <p className="muted">This project's look, on this device.</p>
      <div className="theme-actions">
        <Button type="button" data-theme-action="import" onClick={() => setImporting(true)}>
          Import shadcn theme…
        </Button>
        <Button
          type="button"
          data-theme-action="default"
          onClick={() => {
            try {
              install("");
              setStatus("Default theme restored.");
            } catch (failure) {
              setStatus(said(failure));
            }
          }}
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
                try {
                  const text = await file.text();
                  if (mount.aborted) return;
                  install(themeOf(text));
                  setStatus("Theme applied.");
                  setImporting(false);
                } catch (failure) {
                  if (!mount.aborted) setStatus(said(failure));
                }
              })();
            }}
          />
        </label>
      )}
      <p role="status">{status}</p>
    </div>
  );
}

function closeAppearance(): void {
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
