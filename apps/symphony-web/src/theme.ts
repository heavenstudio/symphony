// A project's look on this device: a shadcn registry theme is data, and its `cssVars` apply to
// every page of the project. The stylesheet it makes is kept on the device (`studio.cssVars`),
// which the shell applies before any page draws (shell/shell.ts), and installed at once.

import { absentOr, isRecord, isText, mapOf } from "./blocks-read";
import { codePointOrder, trimmed } from "./panel-model";
import { save } from "./storage";

/** Where this device keeps the project's theme, through `storage.ts`. */
export const THEME_KEY = "studio.cssVars";

/** A registry theme's tokens: those of both schemes, and each scheme's own. */
export interface CssVars {
  theme: Record<string, string>;
  light: Record<string, string>;
  dark: Record<string, string>;
}

const isTokens = absentOr(mapOf(isText));

/** The tokens of the theme file `value` (its JSON), or none when it has no `cssVars` of text tokens. */
export function readTheme(value: unknown): CssVars | null {
  const vars = isRecord(value) ? value.cssVars : undefined;
  if (!isRecord(vars) || !isTokens(vars.theme) || !isTokens(vars.light) || !isTokens(vars.dark)) {
    return null;
  }
  const tokens = (scheme: unknown) => (scheme ?? {}) as Record<string, string>;
  return { theme: tokens(vars.theme), light: tokens(vars.light), dark: tokens(vars.dark) };
}

/** Older shadcn exports give a colour as its HSL channels (`0 0% 98%`), current ones in full. */
function colour(value: string): string {
  const channels =
    value.includes("%") &&
    !value.includes("(") &&
    /^[0-9 .%+-]*$/.test(value) &&
    value.split(" ").filter((part) => part !== "").length === 3;
  return channels ? `hsl(${value})` : value;
}

/** One scheme's declarations, in its tokens' order, or the token that may not be one. */
function declarations(tokens: Record<string, string>): string {
  let written = "";
  for (const name of Object.keys(tokens).sort(codePointOrder)) {
    const value = tokens[name] ?? "";
    const key = name.startsWith("--") ? name.slice(2) : name;
    if (
      !/^[A-Za-z0-9_-]+$/.test(key) ||
      trimmed(value) === "" ||
      /[;{}<>]/.test(value) ||
      value.replace(/[A-Z]/g, (letter) => letter.toLowerCase()).includes("url(") ||
      value.includes("/*")
    ) {
      throw new Error(`Invalid theme token: ${key}`);
    }
    written += `--${key}:${colour(value)};`;
  }
  return written;
}

/**
 * The theme's stylesheet: custom properties on the root alone, the dark scheme's under its media
 * query. A theme cannot bring a selector, an import or a remote asset: a token that would throws,
 * naming it, and so does a theme with none.
 */
export function stylesheet(vars: CssVars): string {
  const common = declarations(vars.theme);
  const light = declarations(vars.light);
  const dark = declarations(vars.dark);
  if (common === "" && light === "" && dark === "")
    throw new Error("The theme has no cssVars tokens.");
  return `:root[data-studio-theme]{${common}${light}}@media(prefers-color-scheme:dark){:root[data-studio-theme]{${dark}}}`;
}

/** `css` installed as the page's theme and kept on this device for the project; empty for none. */
export function install(css: string): void {
  let style = document.getElementById("studio-theme");
  if (!style) {
    style = document.createElement("style");
    style.id = "studio-theme";
    document.head.append(style);
  }
  style.textContent = css;
  document.documentElement.setAttribute("data-studio-theme", "");
  save(THEME_KEY, css);
}
