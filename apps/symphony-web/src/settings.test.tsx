import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  Entry,
  fromLine,
  openSettings,
  readSettings,
  SettingsPage,
  SettingsView,
  scopeTag,
  shownValue,
  type Told,
} from "./settings";
import { asking, CASES, reply, server } from "./testing";
import type { SettingsPage as Page, Setting, SettingsEntry } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
});

const SAMPLE = CASES.samples.settings as Page;
const entryOf = (id: string) => {
  const found = SAMPLE.sections.flatMap((section) => section.entries).find((e) => e.id === id);
  if (!found) throw new Error(`no entry ${id}`);
  return found;
};
const settingOf = (id: string, key: string) => {
  const found = (entryOf(id).settings ?? []).find((setting) => setting.key === key);
  if (!found) throw new Error(`no setting ${key}`);
  return found;
};

/** The sample with the value at `path` set to `value`, or taken out when `value` is undefined. */
function changed(path: (string | number)[], value?: unknown): unknown {
  const copy = JSON.parse(JSON.stringify(SAMPLE));
  let at = copy;
  for (const step of path.slice(0, -1)) at = at[step];
  const last = path[path.length - 1] ?? "";
  if (value === undefined) delete at[last];
  else at[last] = value;
  return copy;
}

/** What a read told, in order. */
function telling(): { told: unknown[]; check: Told } {
  const told: unknown[] = [];
  return {
    told,
    check: {
      answered: (page) => told.push(["answered", page]),
      failed: (why, missing) => told.push(["failed", why, missing]),
    },
  };
}

describe("the read", () => {
  it("takes the server's page, and an empty one", () => {
    expect(readSettings(SAMPLE)).toEqual(SAMPLE);
    expect(readSettings(CASES.samples.emptySettings)).toEqual({
      files: [],
      problems: [],
      sections: [],
    });
  });

  it("reads a list the server left out as empty, and passes a field it does not draw", () => {
    const read = readSettings(changed(["problems"]));
    expect(read?.problems).toEqual([]);
    expect(read?.sections).toEqual(SAMPLE.sections);
    expect(readSettings(changed(["later"], "a newer server's"))?.files).toEqual(SAMPLE.files);
    const bare = readSettings(changed(["sections", 0, "entries", 0, "needs"]));
    expect(bare?.sections[0]?.entries[0]?.needs).toBeUndefined();
  });

  it("refuses an answer whose drawn field is not of its type", () => {
    const first = ["sections", 0, "entries", 0];
    const broken: [(string | number)[], unknown][] = [
      [["files"], "symphony.toml"],
      [["files", 0, "found"], "yes"],
      [["files", 0, "scope"], "planet"],
      [["problems"], [3]],
      [["sections"], {}],
      [["sections", 0, "title"], undefined],
      [[...first, "name"], 3],
      [[...first, "state", "on"], "on"],
      [[...first, "state", "why"], undefined],
      [[...first, "needs", 0, "met"], 1],
      [[...first, "needs"], "hc"],
      [[...first, "settings", 0, "value"], 8],
      [[...first, "settings", 0, "scopes"], ["planet"]],
      [[...first, "settings", 0, "source", "said"], null],
      [[...first, "settings", 0, "source", "over"], 4],
      [[...first, "settings", 0, "choices"], "hc"],
      [["sections", 2, "entries", 1, "source"], "a file"],
    ];
    for (const [path, value] of broken) {
      expect(readSettings(changed(path, value)), path.join(".")).toBeNull();
    }
    for (const value of [null, "settings", [], 3]) expect(readSettings(value)).toBeNull();
  });

  it("asks once and tells the page what the server answered", async () => {
    const asked = server();
    const page = new AbortController();
    const { told, check } = telling();
    const done = openSettings(page.signal, check, async () => {
      throw new Error("an answered read waits for nothing");
    });
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/settings");
    expect(asked[0]?.signal).not.toBeNull();
    asked[0]?.answer(reply(200, SAMPLE));
    await done;
    expect(told).toEqual([["answered", SAMPLE]]);
    expect(asked).toHaveLength(1);
  });

  it("says a failed read, tries again by itself and stops once it is answered", async () => {
    const asked = server();
    const page = new AbortController();
    const { told, check } = telling();
    let waits = 0;
    const done = openSettings(page.signal, check, async () => {
      waits += 1;
    });
    await asking(asked, 1);
    asked[0]?.answer(reply(500, { error: "the settings could not be read: boom" }));
    await asking(asked, 2);
    asked[1]?.answer(reply(200, { files: "no page" }));
    await asking(asked, 3);
    asked[2]?.answer(reply(200, SAMPLE));
    await done;
    expect(waits).toBe(2);
    expect(told).toEqual([
      [
        "failed",
        "the settings could not be read: boom (the server answered 500). Trying again…",
        false,
      ],
      ["failed", "The settings' answer is not a page of settings. Trying again…", false],
      ["answered", SAMPLE],
    ]);
  });

  it("ends on a server that has no such page, and says nothing once its page left", async () => {
    const asked = server();
    const { told, check } = telling();
    const gone = openSettings(new AbortController().signal, check, async () => {
      throw new Error("a missing page is not asked for again");
    });
    await asking(asked, 1);
    asked[0]?.answer(reply(404, { error: "no such page" }));
    await gone;
    expect(told).toHaveLength(1);
    expect(told[0]).toEqual(["failed", expect.not.stringContaining("Trying again"), true]);
    const page = new AbortController();
    const left = openSettings(page.signal, check);
    await asking(asked, 2);
    page.abort();
    asked[1]?.answer(reply(200, SAMPLE));
    await left;
    expect(told).toHaveLength(1);
  });
});

describe("a setting's words", () => {
  it("tags a value by the scope of the file that wrote it, or as the default", () => {
    expect(scopeTag(settingOf("herdr", "[agents] panes").source)).toEqual(["project", "project"]);
    expect(scopeTag(settingOf("models", "[agents.models] executor").source)).toEqual([
      "machine",
      "this machine",
    ]);
    expect(scopeTag(settingOf("appearance", "appearance").source)).toEqual([
      "device",
      "this device",
    ]);
    expect(scopeTag(settingOf("models", "[limits] machine_agents").source)).toEqual([
      "default",
      "default",
    ]);
  });

  it("says a switch as a word and any other value as written", () => {
    const base = settingOf("herdr", "[agents] panes");
    const on: Setting = { ...base, kind: "switch", value: "on" };
    expect(shownValue(on)).toBe("On");
    expect(shownValue({ ...on, value: "off" })).toBe("Off");
    expect(shownValue(base)).toBe("hc");
  });

  it("names the committed value a local file's holds over", () => {
    expect(fromLine(settingOf("models", "[agents.models] executor").source)).toBe(
      "symphony.local.toml [agents.models] executor, line 2 · over symphony.toml [agents.models] executor, line 16",
    );
    expect(fromLine(settingOf("landing", "[land]").source)).toBe("symphony.toml [land], line 36");
  });
});

describe("an entry", () => {
  const draw = (entry: SettingsEntry) => renderToStaticMarkup(<Entry entry={entry} />);

  it("says its state and why, what it needs and each setting with its source", () => {
    const html = draw(entryOf("herdr"));
    expect(html).toMatch(/^<article class="setting-entry" id="setting-herdr">/);
    expect(html).toContain('<span class="setting-state" data-on="true">On</span>');
    expect(html).toContain(
      '<p class="setting-about">Agents&#x27; terminals in Herdr. Agents&#x27; terminals open in Herdr.</p>',
    );
    expect(html).toContain(
      '<li data-met="true"><span class="setting-mark" aria-hidden="true">✓</span>the hc program: Found on this machine: /bin/hc.</li>',
    );
    // What the server says is text: a tag in it is drawn as its characters.
    expect(html).toContain(
      '<li data-met="false"><span class="setting-mark" aria-hidden="true">!</span>a token: Not found on this machine: &lt;nothing&gt; opens.</li>',
    );
    expect(html).toContain('<span class="setting-value">hc</span>');
    expect(html).toContain('<span class="setting-choices">one of hc, none</span>');
    expect(html).toContain(
      '<span class="setting-from">[agents] panes · symphony.toml [agents] panes, line 10</span>',
    );
    expect(html).toContain('<span class="setting-tag" data-scope="project">project</span>');
    expect(html).not.toContain("reviewed");
  });

  it("draws a reviewed setting as a sentence with its file and line, tagged reviewed", () => {
    const html = draw(entryOf("landing"));
    expect(html).toContain('data-kind="sentence" data-class="reviewed"');
    expect(html).toContain(
      '<p class="setting-sentence">No check; pushes main; no generator; merged here.</p>',
    );
    expect(html).toContain('<span class="setting-tag" data-scope="reviewed">reviewed</span>');
    expect(html).toContain("[land] · symphony.toml [land], line 36");
    expect(html).not.toContain("setting-state");
  });

  it("draws one that is a sentence alone with where it is said, and no setting", () => {
    const html = draw(entryOf("workflow"));
    expect(html).toContain('<span class="setting-state">Built in</span>');
    expect(html).toContain('<p class="setting-from">default: no [workflow] written</p>');
    expect(html).not.toContain("<dl");
    expect(html).not.toContain("setting-tag");
  });

  it("draws one a server sent with nothing but its name", () => {
    expect(draw({ id: "later", name: "A later thing", about: "What a plugin describes." })).toBe(
      '<article class="setting-entry" id="setting-later"><header class="setting-entry-head"><h3>A later thing</h3></header><p class="setting-about">What a plugin describes.</p></article>',
    );
  });

  it("offers no control: nothing on it writes", () => {
    for (const section of SAMPLE.sections) {
      for (const entry of section.entries) {
        expect(draw(entry)).not.toMatch(/<(button|input|select|textarea|form|a)\b/);
      }
    }
  });
});

describe("the page", () => {
  const draw = (page: Page | null, error: string | null = null, missing = false) =>
    renderToStaticMarkup(<SettingsView page={page} error={error} missing={missing} />);

  it("opens busy on its outline while its read is out", () => {
    vi.stubGlobal("document", { getElementById: () => null });
    const html = renderToStaticMarkup(<SettingsPage />);
    expect(html).toMatch(
      /^<main class="settings-page"><div class="settings" data-ready="loading" aria-busy="true">/,
    );
    expect(html).toContain("<h1>Settings</h1>");
    expect(html).toContain('class="settings-ghost" aria-hidden="true"');
  });

  it("draws every section and entry the server describes, in its order", () => {
    const html = draw(SAMPLE);
    expect(html).toMatch(/^<div class="settings" data-ready="settings" aria-busy="false">/);
    const titles = [...html.matchAll(/<h2 id="settings-([a-z]+)">([^<]+)<\/h2>/g)].map(
      (found) => `${found[1]} ${found[2]}`,
    );
    expect(titles).toEqual([
      "integrations Integrations and tools",
      "models Models and limits",
      "reviewed Shown here, changed by a reviewed task",
      "device This device",
    ]);
    expect([...html.matchAll(/id="setting-([a-z]+)"/g)].map((found) => found[1])).toEqual([
      "herdr",
      "models",
      "landing",
      "workflow",
      "appearance",
    ]);
    expect(html).not.toMatch(/<(button|input|select|textarea|form)\b/);
  });

  it("says what does not load, the files read and one not written", () => {
    const html = draw(SAMPLE);
    expect(html).toContain(
      '<section class="settings-problems" role="alert"><h2>A settings file does not load</h2><ul><li>symphony.local.toml: [land] check (line 5) is not read</li></ul></section>',
    );
    expect(html).toContain(
      '<li data-found="true"><span class="setting-tag" data-scope="project">project</span>/repo/symphony.toml</li>',
    );
    expect(html).toContain("/repo/symphony.local.toml (not written)");
    expect(draw({ ...SAMPLE, problems: [] })).not.toContain("settings-problems");
  });

  it("says a failed read in its place, and a missing page as missing", () => {
    const failed = draw(null, "The server did not answer. Trying again…");
    expect(failed).toContain('data-ready="failed" aria-busy="false"');
    expect(failed).toContain(
      '<p class="settings-error" role="alert">The settings could not be read. The server did not answer. Trying again…</p>',
    );
    expect(failed).not.toContain("settings-ghost");
    expect(draw(null, "No such page.", true)).toContain('data-ready="missing"');
    // A page read before a later read failed stays drawn under what the failure says.
    const kept = draw(SAMPLE, "The server did not answer.");
    expect(kept).toContain('data-ready="settings"');
    expect(kept).toContain("settings-error");
    expect(kept).toContain('id="setting-herdr"');
  });
});
