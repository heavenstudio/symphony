import { afterEach, describe, expect, it, vi } from "vitest";
import { themeOf } from "./appearance";
import { memoryStorage } from "./testing";
import { type CssVars, install, readTheme, stylesheet, THEME_KEY } from "./theme";

afterEach(() => vi.unstubAllGlobals());

const vars = (light: Record<string, string>): CssVars => ({ theme: {}, light, dark: {} });

describe("a registry theme", () => {
  it("keeps its fonts, radius and both schemes, its keys in Rust's order", () => {
    const theme = readTheme(
      JSON.parse(
        '{"name":"Copper","type":"registry:style","cssVars":{"theme":{"radius":"1rem","font-sans":"Georgia, serif"},"light":{"primary":"oklch(0.6 0.1 45)","background":"0 0% 98%"},"dark":{"background":"#121212","like":"#00aa00"}}}',
      ),
    );
    expect(theme).not.toBeNull();
    const css = stylesheet(theme as CssVars);
    expect(css).toBe(
      ":root[data-studio-theme]{--font-sans:Georgia, serif;--radius:1rem;--background:hsl(0 0% 98%);--primary:oklch(0.6 0.1 45);}@media(prefers-color-scheme:dark){:root[data-studio-theme]{--background:#121212;--like:#00aa00;}}",
    );
    expect(css).not.toContain("--question:");
  });

  it("takes a token named with its dashes, and channels only as three plain numbers", () => {
    expect(stylesheet(vars({ "--ring": "1 2% 3%" }))).toContain("--ring:hsl(1 2% 3%);");
    expect(stylesheet(vars({ a: "1% 2%" }))).toContain("--a:1% 2%;");
    expect(stylesheet(vars({ a: "1 2 3" }))).toContain("--a:1 2 3;");
    expect(stylesheet(vars({ a: "calc(1%) 2 3" }))).toContain("--a:calc(1%) 2 3;");
    expect(stylesheet(vars({ a: "1\t2% 3%" }))).toContain("--a:1\t2% 3%;");
  });

  it("cannot replace the current theme when it is no theme", () => {
    expect(readTheme({ colors: {} })).toBeNull();
    expect(readTheme({ cssVars: { light: { radius: 4 } } })).toBeNull();
    expect(readTheme({ cssVars: { dark: null } })).toBeNull();
    expect(readTheme({ cssVars: [] })).toBeNull();
    expect(readTheme(null)).toBeNull();
    expect(readTheme({ cssVars: {} })).toEqual({ theme: {}, light: {}, dark: {} });
    expect(() => stylesheet({ theme: {}, light: {}, dark: {} })).toThrow(
      "The theme has no cssVars tokens.",
    );
  });

  it("cannot bring a selector, an import, a remote asset or an empty token", () => {
    for (const [key, value] of [
      ["", "red"],
      ["--", "red"],
      ["a}body", "red"],
      ["primary", "red;}body{color:red"],
      ["font-sans", "url(https://example.com/font)"],
      ["font-sans", "URL(https://example.com/font)"],
      ["primary", "red/*"],
      ["primary", "<b>"],
      ["radius", " "],
      ["radius", "　"],
    ] as const) {
      expect(() => stylesheet(vars({ [key]: value })), `${key}: ${value}`).toThrow(
        `Invalid theme token: ${key.replace(/^--/, "")}`,
      );
    }
  });

  it("names the first token it refuses in Rust's order", () => {
    expect(() => stylesheet(vars({ "b!": "1", "a!": "2" }))).toThrow("Invalid theme token: a!");
  });
});

describe("a theme file", () => {
  it("says why it makes no theme", () => {
    expect(() => themeOf("{")).toThrow("The theme is not valid JSON.");
    expect(() => themeOf('{"colors":{}}')).toThrow(
      "The theme needs cssVars with theme, light or dark tokens.",
    );
    expect(() => themeOf('{"cssVars":{}}')).toThrow("The theme has no cssVars tokens.");
    expect(themeOf('{"cssVars":{"light":{"radius":"2px"}}}')).toBe(
      ":root[data-studio-theme]{--radius:2px;}@media(prefers-color-scheme:dark){:root[data-studio-theme]{}}",
    );
  });
});

describe("installing a theme", () => {
  it("writes the page's theme and keeps it for the project on this device", () => {
    const head: { id: string; textContent: string }[] = [];
    const root = new Map<string, string>();
    const storage = memoryStorage();
    vi.stubGlobal("localStorage", storage);
    vi.stubGlobal("document", {
      getElementById: (id: string) =>
        id === "studio-theme" ? (head.find((style) => style.id === id) ?? null) : null,
      createElement: () => ({ id: "", textContent: "" }),
      head: { append: (style: { id: string; textContent: string }) => head.push(style) },
      documentElement: { setAttribute: (name: string, value: string) => root.set(name, value) },
    });
    install(":root{--a:1}");
    install("");
    expect(head).toEqual([{ id: "studio-theme", textContent: "" }]);
    expect(root.get("data-studio-theme")).toBe("");
    expect(storage.getItem(THEME_KEY)).toBe("");
  });
});
