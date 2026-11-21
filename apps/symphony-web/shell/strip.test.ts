import { readFileSync } from "node:fs";
import { parseSync } from "vite";
import { describe, expect, it } from "vitest";
import { BASE, devShell } from "../src/dev-server";
import { strip } from "./strip";

/** The checkout's source `name`, beside this file. */
const source = (name: string) => readFileSync(new URL(name, import.meta.url), "utf8");

/** Spaces as long as `text`, which is what stripping leaves of a type. */
const blank = (text: string) => " ".repeat(text.length);

/** A built shell's first line, as package.sh writes it, and the kinds the dev server serves. */
const BUILT = `const modules = ${JSON.stringify({ panel: { styles: ["/assets/shared.css"] } })};\n`;
const KINDS = { panel: "src/kinds/panel.tsx" };

describe("a stripped source", () => {
  it("keeps every byte but its types in its place", () => {
    const typed = [
      'import type { Store } from "./cache.ts";',
      "declare const window: Window & { store: Store };",
      "const answer = (value: unknown): string => String(value as number); // its comment",
      "window.store.read(1 satisfies number);",
      "",
    ].join("\n");
    const stripped = strip(typed);
    expect(stripped.length).toBe(typed.length);
    expect(stripped.split("\n").map((line) => line.trimEnd())).toEqual([
      "",
      "",
      `const answer = (value${blank(": unknown")})${blank(": string")} => String(value${blank(" as number")}); // its comment`,
      `window.store.read(1${blank(" satisfies number")});`,
      "",
    ]);
  });

  it("is refused where a type is no erasable syntax", () => {
    expect(() => strip("enum Kind { Board }\n")).toThrow();
  });
});

describe("the checkout's shell, stripped", () => {
  const cache = strip(source("./cache.ts"));
  const shell = strip(source("./shell.ts"));

  it("keeps each source's lines, the shell's manifest line among them", () => {
    for (const [name, stripped] of [
      ["./cache.ts", cache],
      ["./shell.ts", shell],
    ] as const) {
      expect(stripped.split("\n")).toHaveLength(source(name).split("\n").length);
    }
    expect(
      shell.split("\n").filter((line) => line === "const modules = __MODULES__;"),
    ).toHaveLength(1);
  });

  it("gives the store as a script the browser tests run alone, with nothing it imports or exports", () => {
    expect(() => new Function(cache)).not.toThrow();
    expect(cache).not.toMatch(/^\s*(import|export)\b/m);
  });

  it("gives the store in ASCII, so a browser's offsets in it are its bytes'", () => {
    expect([...cache].every((character) => character.charCodeAt(0) < 128)).toBe(true);
  });

  it("joins the store and the shell as package.sh does, one module declaring no name twice", () => {
    const joined = cache + shell.replace("const modules = __MODULES__;\n", "");
    const read = (code: string) =>
      parseSync("shell.js", code, { sourceType: "module", showSemanticErrors: true }).errors.map(
        (error) => error.message,
      );
    expect(read(joined)).toEqual([]);
    expect(read(`${joined}\nconst stored = 1;`)).toEqual([
      "Identifier `stored` has already been declared",
    ]);
  });

  it("makes the dev server's shell, as package.sh makes the built one", () => {
    const made = devShell(BUILT, KINDS, BASE, cache, shell);
    expect(made).toContain("window.symphonyCache = (");
    expect(made).toContain("window.reviewShell = { gone, alarm };");
    expect(made).not.toContain("__MODULES__");
    expect(made).not.toMatch(/^\s*import type\b/m);
  });
});
