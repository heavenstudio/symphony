// RULES.md `code/pages-explain-nothing`: a page draws no text that explains itself. What can be
// told from the sources is held here: no class names a subtitle, a lead or an about, and nothing
// reads the `about` a settings entry still carries for the plugins that write it. The shared chart
// frame and a settings section have no note or lead to give at all, which the type checker holds.
// What no test tells is the sentence itself: a new one in a class of another name, a heading that
// explains, a `title` that does, or a control drawn where it cannot act. The rule and review hold
// those.

import { describe, expect, it } from "vitest";

// Every module and stylesheet the pages are built from, as text: not the tests, which name what
// they refuse, nor the wire types, which are written from the Rust.
const sources = import.meta.glob<string>(
  ["./**/*.{ts,tsx,css}", "!./**/*.test.{ts,tsx}", "!./wire.ts", "!./testing.ts"],
  { query: "?raw", import: "default", eager: true },
);

/** A class whose name says it holds what explains a page, a section or a control. */
const EXPLAINING =
  /^(?:lead|subtitle|intro|blurb)$|-(?:sub|lead|about|intro|subtitle|blurb|explain|note)$/;

/** The kept notes: a value's unit or a state, each drawn by a class of that name. */
const KEPT = new Set([
  // A chart's readout of a marked change: what changed in the period, as the server words it.
  "chart-mark-note",
  // Under the charts of tokens: what every dollar is, the unit, and the models no price names.
  "reports-note",
  // A schema update's failures and its task.
  "run-note",
  // A timeline's state: what its history lacks, what is no longer kept, a failed read.
  "tl-note",
  // A task's gates that could not be read, with Retry.
  "gates-note",
]);

/**
 * Every class name `text` gives: a module's in a `className`, its words or a template's outside
 * what it fills in (`stylesheet` false), a stylesheet's in its selectors.
 */
function classNames(text: string, stylesheet = false): string[] {
  const names = new Set<string>();
  const add = (words: string) => {
    for (const name of words.split(/\s+/)) if (/^[\w-]+$/.test(name)) names.add(name);
  };
  if (stylesheet) {
    for (const [, name] of text.matchAll(/(?:^|[\s,>+~(])\.([a-zA-Z][\w-]*)/gm)) add(name ?? "");
  } else {
    for (const [, given] of text.matchAll(/className="([^"]*)"/g)) add(given ?? "");
    for (const [, given] of text.matchAll(/className=\{`([^`]*)`\}/g)) {
      add((given ?? "").replace(/\$\{[^}]*\}/g, " "));
    }
  }
  return [...names];
}

/** A read of `about`: a property, a field named in a string, or one destructured. */
const READS_ABOUT = /\.about\b|["'`]about["'`]|[{,]\s*about\s*[,}:]/;

/** `text` without its comments, which may say the word. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
}

describe("a page explains nothing of itself", () => {
  it("reads every page's module and stylesheet", () => {
    expect(Object.keys(sources)).toEqual(
      expect.arrayContaining(["./settings.tsx", "./people.tsx", "./chart.tsx", "./runs.css"]),
    );
    expect(Object.keys(sources).filter((file) => file.includes(".test."))).toEqual([]);
  });

  it("names no class for a subtitle, a lead, an about or a note but the kept ones", () => {
    const found = Object.entries(sources).flatMap(([file, text]) =>
      classNames(text, file.endsWith(".css"))
        .filter((name) => EXPLAINING.test(name) && !KEPT.has(name))
        .map((name) => `${file}: ${name}`),
    );
    expect(found).toEqual([]);
  });

  it("reads no settings entry's `about`", () => {
    const read = Object.entries(sources)
      .filter(([file, text]) => !file.endsWith(".css") && READS_ABOUT.test(code(text)))
      .map(([file]) => file);
    expect(read).toEqual([]);
    for (const text of ["entry.about", 'field(value, "about")', "const { id, about } = entry;"]) {
      expect(READS_ABOUT.test(code(text)), text).toBe(true);
    }
    expect(READS_ABOUT.test(code("// a type, about, in pixels\n/* about, */ const a = 1;"))).toBe(
      false,
    );
  });

  it("tells an explaining class from a kept one and from a word that only ends alike", () => {
    for (const name of ["lead", "settings-sub", "people-sub", "setting-about", "chart-note"]) {
      expect(EXPLAINING.test(name), name).toBe(true);
    }
    for (const name of ["head-state", "runs-unit", "reports-spend", "subtle", "sub", "notes"]) {
      expect(EXPLAINING.test(name), name).toBe(false);
    }
    expect(classNames('<p className="a b-sub">')).toEqual(["a", "b-sub"]);
    const filled = ["<p className={`chart-col", "{on ? ' on' : ''} x-lead`}>"].join("$");
    expect(classNames(filled)).toEqual(["chart-col", "x-lead"]);
    expect(
      classNames(".runs-unit,\n.runs-label {\n  margin: 0;\n}\n.a > .b-about {}", true),
    ).toEqual(["runs-unit", "runs-label", "a", "b-about"]);
  });
});
