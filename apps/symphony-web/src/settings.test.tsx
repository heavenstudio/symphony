import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  Control,
  type ControlProps,
  changeLine,
  Entry,
  fromLine,
  openingText,
  openSettings,
  readSettings,
  readWritten,
  SettingField,
  SettingsPage,
  SettingsView,
  scopeTag,
  shownValue,
  storedLine,
  type Told,
  unwritable,
  type Wrote,
  writeSetting,
  writtenValue,
} from "./settings";
import { asking, CASES, elementOf, type Found, fire, reading, reply, server } from "./testing";
import type {
  SettingsPage as Page,
  Setting,
  SettingsEntry,
  SettingWrite,
  StoredSetting,
} from "./wire";

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
      stored: [],
    });
  });

  it("reads a list the server left out as empty, and passes a field it does not draw", () => {
    const read = readSettings(changed(["problems"]));
    expect(read?.problems).toEqual([]);
    expect(readSettings(changed(["stored"]))?.stored).toEqual([]);
    const unchanged = readSettings(changed(["stored", 0, "changes"]));
    expect(unchanged?.stored?.[0]?.changes).toBeUndefined();
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
      [["stored"], "limits.low_volume"],
      [["stored", 0, "key"], undefined],
      [["stored", 0, "setting"], 3],
      [["stored", 0, "value"], 3],
      [["stored", 0, "held"], "yes"],
      [["stored", 0, "refused"], false],
      [["stored", 0, "waiting"], 4],
      [["stored", 0, "changes"], "two"],
      [["stored", 0, "changes", 0, "at"], 1],
      [["stored", 0, "changes", 1, "before"], 2],
      [[...first, "settings", 0, "source", "scope"], "planet"],
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
    expect(scopeTag(settingOf("models", "[limits] low_volume").source)).toEqual([
      "board",
      "this board",
    ]);
  });

  it("says a switch as a word and any other value as written", () => {
    const base = settingOf("herdr", "[agents] panes");
    const on: Setting = { ...base, kind: "switch", value: "on" };
    expect(shownValue(on)).toBe("On");
    expect(shownValue({ ...on, value: "off" })).toBe("Off");
    expect(shownValue(base)).toBe("hc");
  });

  it("names what a local file's or the store's value stands over", () => {
    expect(fromLine(settingOf("models", "[agents.models] executor").source)).toBe(
      'symphony.local.toml [agents.models] executor, line 2 · over "opus" (the board\'s store [agents.models] executor)',
    );
    expect(fromLine(settingOf("models", "[limits] low_volume").source)).toBe(
      "the board's store [limits] low_volume · over 4 (symphony.toml [limits] low_volume, line 12)",
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
    // Its state's why alone: what it is for, its `about`, is drawn nowhere.
    expect(html).toContain(
      '</header><p class="setting-why">Agents&#x27; terminals open in Herdr.</p><ul class="setting-needs">',
    );
    expect(html).not.toContain("Agents&#x27; terminals in Herdr.");
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
    const html = draw(entryOf("plugin-shop-catalogue"));
    expect(html).toContain('<span class="setting-state">Read</span>');
    expect(html).toContain('<p class="setting-from">catalogue.toml, line 1</p>');
    expect(html).not.toContain("<dl");
    expect(html).not.toContain("setting-tag");
  });

  it("draws the workflow as the reviewed sentence its default says", () => {
    const html = draw(entryOf("workflow"));
    expect(html).toContain('<p class="setting-sentence">The board&#x27;s own, built-in v1.</p>');
    expect(html).toContain("[workflow] · default: no [workflow] written");
    expect(html).not.toContain("setting-state");
  });

  it("draws one a server sent with nothing but its name, and none of what it is for", () => {
    const head =
      '<article class="setting-entry" id="setting-later"><header class="setting-entry-head"><h3>A later thing</h3></header>';
    expect(draw({ id: "later", name: "A later thing", about: "What a plugin describes." })).toBe(
      `${head}</article>`,
    );
    expect(draw({ id: "later", name: "A later thing", about: "" })).toBe(`${head}</article>`);
    const quiet = { word: "Read", why: "" };
    expect(draw({ id: "later", name: "A later thing", about: "", state: quiet }), "no why").toBe(
      `${head.replace("</h3>", '</h3><span class="setting-state">Read</span>')}</article>`,
    );
  });

  it("offers no control where the page gives it no writes", () => {
    for (const section of SAMPLE.sections) {
      for (const entry of section.entries) {
        expect(draw(entry)).not.toMatch(/<(button|input|select|textarea|form|a)\b/);
      }
    }
  });
});

/** The sample's store item for the setting the page names `setting`. */
const storedOf = (setting: string): StoredSetting => {
  const found = (SAMPLE.stored ?? []).find((stored) => stored.setting === setting);
  if (!found) throw new Error(`no stored ${setting}`);
  return found;
};

/** What a write told its row, in order. */
function wrote(): { told: unknown[]; check: Wrote } {
  const told: unknown[] = [];
  return {
    told,
    check: {
      said: (text, refused) => told.push(["said", text, refused]),
      done: (written) => told.push(["done", written]),
    },
  };
}

describe("a write", () => {
  it("takes a field's text as the value its kind writes, and refuses one it cannot", () => {
    const cases: [string, string, unknown][] = [
      ["number", "3", 3],
      ["number", " 12 ", 12],
      ["number", "-1", -1],
      ["number", "1.5", null],
      ["number", "", null],
      ["number", "three", null],
      ["number", "9007199254740993", null],
      ["switch", "on", true],
      ["switch", "off", false],
      ["switch", "yes", null],
      ["text", " opus ", "opus"],
      ["text", "  ", null],
      ["choice", "hc", "hc"],
    ];
    for (const [kind, text, value] of cases) {
      expect(writtenValue(kind, text), `${kind} ${JSON.stringify(text)}`).toBe(value);
    }
    expect(unwritable("number")).toBe("Write a whole number.");
    expect(unwritable("text")).toBe("Write a value.");
  });

  it("opens a field on the value a file or the store writes, and on none for a default", () => {
    expect(openingText(settingOf("models", "[limits] low_volume"))).toBe("3");
    expect(openingText(settingOf("models", "[agents.models] executor"))).toBe("fable");
    expect(openingText(settingOf("models", "[limits] machine_agents"))).toBe("");
    const written = settingOf("models", "[limits] low_volume");
    expect(openingText({ ...written, value: "no cap" })).toBe("");
  });

  it("says each change of the record, and the store's entry beside its setting", () => {
    const [first, second] = storedOf("[limits] low_volume").changes ?? [];
    expect(first && changeLine(first)).toBe("2026-11-22 20:10 · owner: none → 2");
    expect(second && changeLine(second)).toBe("2026-11-22 20:15 · owner: 2 → 3");
    expect(changeLine({ before: "3", at: "2026-11-22 20:20", by: "owner" })).toBe(
      "2026-11-22 20:20 · owner: 3 → removed",
    );
    expect(storedLine(storedOf("[limits] low_volume"))).toBe(
      "The board's store holds 3, in force.",
    );
    expect(storedLine(storedOf("[agents.models] executor"))).toBe(
      "The board's store holds opus; the local settings file writes over it.",
    );
    expect(storedLine({ ...storedOf("[agents.models] executor"), refused: "not a model" })).toBe(
      "The board's store holds opus, not read.",
    );
    expect(storedLine(storedOf("[limits] machine_agents"))).toBe("");
  });

  it("reads the server's answer, and refuses one that is not a write's", () => {
    expect(readWritten({ said: "kept" })).toEqual({ said: "kept", commit: null, waiting: false });
    expect(readWritten({ said: "committed", commit: "abc", waiting: false, later: 1 })).toEqual({
      said: "committed",
      commit: "abc",
      waiting: false,
    });
    for (const broken of [
      null,
      {},
      { said: 3 },
      { said: "x", commit: 4 },
      { said: "x", waiting: "y" },
    ]) {
      expect(readWritten(broken)).toBeNull();
    }
  });

  const write: SettingWrite = { key: "limits.low_volume", action: "store", value: 4 };

  it("posts the write, and tells its row what the server did", async () => {
    const asked = server();
    const { told, check } = wrote();
    const done = writeSetting(write, new AbortController().signal, check);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/setting");
    expect(asked[0]?.body).toEqual(write);
    // A post takes no signal: the server may have acted on it.
    expect(asked[0]?.signal).not.toBeNull();
    asked[0]?.answer(reply(200, { said: "[limits] low_volume is 4 in the board's store" }));
    await done;
    expect(told).toEqual([
      ["said", "[limits] low_volume is 4 in the board's store", false],
      ["done", true],
    ]);
  });

  it("tells its row a refusal in the server's words, and an answer that is no write's", async () => {
    const asked = server();
    const { told, check } = wrote();
    const refused = writeSetting(write, new AbortController().signal, check);
    await asking(asked, 1);
    asked[0]?.answer(reply(422, { error: "not committed: the settings check refuses it" }));
    await refused;
    const odd = writeSetting(write, new AbortController().signal, check);
    await asking(asked, 2);
    asked[1]?.answer(reply(200, { done: true }));
    await odd;
    expect(told).toEqual([
      ["said", "not committed: the settings check refuses it", true],
      ["done", false],
      ["said", "The server's answer is not a setting's write", true],
      ["done", false],
    ]);
  });

  it("tells nothing once its page is gone", async () => {
    const asked = server();
    const { told, check } = wrote();
    const page = new AbortController();
    const done = writeSetting(write, page.signal, check);
    await asking(asked, 1);
    page.abort();
    asked[0]?.answer(reply(200, { said: "kept" }));
    await done;
    const failed = writeSetting(write, page.signal, check);
    await asking(asked, 2);
    asked[1]?.answer(reply(500, { error: "boom" }));
    await failed;
    expect(told).toEqual([]);
  });
});

describe("a control", () => {
  /** The props of a control for `setting`, its handlers kept as what they were called with. */
  function propsOf(setting: Setting, stored: StoredSetting, more: Partial<ControlProps> = {}) {
    const called: unknown[] = [];
    const props: ControlProps = {
      setting,
      stored,
      draft: null,
      state: { busy: false, said: null },
      onDraft: (text) => called.push(["draft", text]),
      onWrite: (written) => called.push(["write", written]),
      onRefuse: (why) => called.push(["refuse", why]),
      ...more,
    };
    return { props, called };
  }
  const draw = (props: ControlProps) => renderToStaticMarkup(<Control {...props} />);
  const lowVolume = () =>
    propsOf(settingOf("models", "[limits] low_volume"), storedOf("[limits] low_volume"));
  const named = (text: string) => (element: Found) => reading(text)(element);
  const switchOf = (value: string) => {
    const setting: Setting = { ...settingOf("herdr", "[agents] panes"), kind: "switch", value };
    return propsOf(setting, { setting: setting.key, key: "agents.switch", held: false });
  };
  const choiceOf = () => {
    const setting: Setting = {
      ...settingOf("herdr", "[agents] panes"),
      scopes: ["project", "board"],
    };
    return propsOf(setting, { setting: setting.key, key: "agents.panes", held: false });
  };

  it("draws a number as a number field on the value in force, kept only once changed", () => {
    const html = draw(lowVolume().props);
    expect(html).toMatch(/^<div class="setting-control"><form class="setting-field">/);
    expect(html).toContain(
      '<input class="dx-input" type="number" inputMode="numeric" step="1" aria-label="Open slots below which an orchestrator hears low volume" placeholder="3" value="3"/>',
    );
    expect(html).toMatch(
      /<button class="dx-button" data-style="outline" data-size="sm" type="submit" disabled="">Keep<\/button>/,
    );
    const drafted = draw({ ...lowVolume().props, draft: "5" });
    expect(drafted).toContain('value="5"');
    expect(drafted).toMatch(/type="submit">Keep<\/button>/);
  });

  it("draws a text as a text field, and a default as an empty field that names it", () => {
    const executor = propsOf(
      settingOf("models", "[agents.models] executor"),
      storedOf("[agents.models] executor"),
    );
    expect(draw(executor.props)).toContain(
      'type="text" aria-label="The executor&#x27;s model" placeholder="fable" value="fable"/>',
    );
    const unset = propsOf(
      settingOf("models", "[limits] machine_agents"),
      storedOf("[limits] machine_agents"),
    );
    const html = draw(unset.props);
    expect(html).toContain('placeholder="no limit" value=""/>');
    // No entry in the store: nothing to make the project's or remove, and no record.
    expect(html).not.toContain("setting-store");
    expect(html).not.toContain("setting-record");
  });

  it("draws a switch as a switch, which writes the other side", () => {
    const { props, called } = switchOf("on");
    expect(draw(props)).toContain(
      '<button class="dx-button setting-switch" data-style="outline" data-size="sm" type="button" role="switch" aria-checked="true" aria-label="What agents&#x27; terminals run in">On</button>',
    );
    expect(draw(props)).toContain(">On</button>");
    fire(SettingField(props), named("On"), "onClick");
    const off = switchOf("off");
    expect(draw(off.props)).toContain('aria-checked="false"');
    fire(SettingField(off.props), named("Off"), "onClick");
    expect([...called, ...off.called]).toEqual([
      ["write", { key: "agents.switch", action: "store", value: false }],
      ["write", { key: "agents.switch", action: "store", value: true }],
    ]);
  });

  it("draws a choice as a select of its choices, which writes the one chosen", () => {
    const { props, called } = choiceOf();
    expect(draw(props)).toContain('role="combobox" class="dx-select-trigger"');
    expect(draw(props)).toContain("<span>hc</span>");
    const select = SettingField(props);
    expect(select.props.options).toEqual([
      ["hc", "hc"],
      ["none", "none"],
    ]);
    select.props.onChange("hc");
    select.props.onChange("none");
    expect(called).toEqual([["write", { key: "agents.panes", action: "store", value: "none" }]]);
  });

  it("keeps a field's text on each input, writes it by its kind, and refuses what it cannot", () => {
    const { props, called } = lowVolume();
    const prevented: string[] = [];
    const submit = { preventDefault: () => prevented.push("submit") };
    fire(SettingField(props), (element) => element.props.type === "number", "onChange", {
      currentTarget: { value: "7" },
    });
    fire(
      SettingField({ ...props, draft: "7" }),
      (element) => element.type === "form",
      "onSubmit",
      submit,
    );
    fire(
      SettingField({ ...props, draft: "seven" }),
      (element) => element.type === "form",
      "onSubmit",
      submit,
    );
    expect(prevented).toEqual(["submit", "submit"]);
    expect(called).toEqual([
      ["draft", "7"],
      ["write", { key: "limits.low_volume", action: "store", value: 7 }],
      ["refuse", unwritable("number")],
    ]);
  });

  it("offers to make a stored value the project's or remove it, and says each write", () => {
    const executor = propsOf(
      settingOf("models", "[agents.models] executor"),
      storedOf("[agents.models] executor"),
    );
    const html = draw(executor.props);
    expect(html).toContain(
      '<span class="setting-stored">The board&#x27;s store holds opus; the local settings file writes over it.</span>',
    );
    // What it does, which its words leave out, is its title.
    expect(html).toMatch(
      /data-size="sm" type="button" title="Commits it to symphony.toml, delivered by a pull request">Make it the project&#x27;s<\/button>/,
    );
    expect(html).toMatch(/data-style="ghost" data-size="sm" type="button">Remove<\/button>/);
    const control = Control(executor.props);
    fire(control, named("Make it the project's"), "onClick");
    fire(control, named("Remove"), "onClick");
    expect(executor.called).toEqual([
      ["write", { key: "agents.models.executor", action: "commit" }],
      ["write", { key: "agents.models.executor", action: "remove" }],
    ]);
    const done = draw({
      ...executor.props,
      state: { busy: false, said: { text: "committed to main", refused: false } },
    });
    expect(done).toContain(
      '<p class="setting-said" data-refused="false" role="status">committed to main</p>',
    );
    const refused = draw({
      ...executor.props,
      state: { busy: false, said: { text: "not committed: red", refused: true } },
    });
    expect(refused).toContain(
      '<p class="setting-said" data-refused="true" role="alert">not committed: red</p>',
    );
  });

  it("holds every write while one is out", () => {
    const html = draw({ ...lowVolume().props, draft: "5", state: { busy: true, said: null } });
    expect(html).not.toMatch(/<(button|input)(?![^>]*disabled="")[^>]*>/);
  });

  it("says a commit waiting for its delivery, and offers it no second time", () => {
    const html = draw(lowVolume().props);
    expect(html).toContain(
      '<p class="setting-waiting">Waiting for its delivery by pull request (docs/owner-save-4: waiting for its pull request).</p>',
    );
    expect(html).toMatch(
      /type="button" disabled="" title="[^"]+">Make it the project&#x27;s<\/button>/,
    );
    expect(html).toMatch(/type="button">Remove<\/button>/);
  });

  it("says why the store does not read its entry, and commits none", () => {
    const executor = propsOf(settingOf("models", "[agents.models] executor"), {
      ...storedOf("[agents.models] executor"),
      refused: "the board's store [agents.models] executor is not a model",
    });
    const html = draw(executor.props);
    expect(html).toContain(
      '<p class="setting-refused">The board&#x27;s store does not read its entry: the board&#x27;s store [agents.models] executor is not a model</p>',
    );
    expect(html).toMatch(
      /type="button" disabled="" title="[^"]+">Make it the project&#x27;s<\/button>/,
    );
  });

  it("keeps its record closed until it is opened", () => {
    const html = draw(lowVolume().props);
    expect(html).toContain(
      '<details class="setting-record"><summary>Its record in the board&#x27;s store: 2 changes</summary></details>',
    );
    const record = elementOf(
      Control(lowVolume().props),
      (element) => element.props.changes !== undefined,
    );
    expect(record?.props.changes).toHaveLength(2);
    const one = propsOf(
      settingOf("models", "[agents.models] executor"),
      storedOf("[agents.models] executor"),
    );
    expect(draw(one.props)).toContain(
      "<summary>Its record in the board&#x27;s store: 1 change</summary>",
    );
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
      "reviewed Reviewed settings",
      "device This device",
    ]);
    expect([...html.matchAll(/id="setting-([a-z-]+)"/g)].map((found) => found[1])).toEqual([
      "herdr",
      "plugin-shop-catalogue",
      "models",
      "landing",
      "workflow",
      "appearance",
    ]);
    expect(html).not.toMatch(/<(button|input|select|textarea|form)\b/);
  });

  it("draws a control for each setting the store keeps, and none for any other", () => {
    const html = renderToStaticMarkup(
      <SettingsView page={SAMPLE} error={null} missing={false} reread={() => undefined} />,
    );
    expect(html.match(/class="setting-control"/g)).toHaveLength(3);
    // The head is its title alone, and no legend tells what a tag says.
    expect(html).toContain('<header class="settings-head"><h1>Settings</h1></header>');
    expect(html).not.toContain("settings-legend");
    expect(html).not.toContain("settings-sub");
    for (const id of ["herdr", "landing", "workflow", "appearance"]) {
      const entry = html.slice(html.indexOf(`id="setting-${id}"`)).split("</article>")[0] ?? "";
      expect(entry, id).not.toContain("setting-control");
    }
  });

  it("draws a plugin's description of a key the store keeps as its describer's, with no control", () => {
    // A describer that gives one of the Symphony's own stored keys, as the server leaves it: none
    // of the board's scope ([`judged`] in crates/symphony/src/settings/described.rs).
    const page = JSON.parse(JSON.stringify(SAMPLE)) as Page;
    const own = settingOf("models", "[limits] low_volume");
    const described: Setting = {
      ...own,
      name: "The shop's low volume",
      value: "9",
      source: { scope: "project", said: "catalogue.toml low_volume, line 4" },
      scopes: own.scopes.filter((scope) => scope !== "board"),
    };
    const plugin = page.sections
      .flatMap((section) => section.entries)
      .find((entry) => entry.id === "plugin-shop-catalogue");
    if (!plugin) throw new Error("no plugin entry");
    plugin.settings = [described];
    const html = renderToStaticMarkup(
      <SettingsView page={page} error={null} missing={false} reread={() => undefined} />,
    );
    const entry = (id: string) =>
      html.slice(html.indexOf(`id="setting-${id}"`)).split("</article>")[0] ?? "";
    const theirs = entry("plugin-shop-catalogue");
    expect(theirs).toContain('<span class="setting-value">9</span>');
    expect(theirs).toContain(
      '<span class="setting-from">[limits] low_volume · catalogue.toml low_volume, line 4</span>',
    );
    expect(theirs).not.toContain("setting-control");
    expect(theirs).not.toContain("setting-stored");
    expect(theirs).not.toMatch(/<(button|input|select|textarea|form)\b/);
    // The Symphony's own rows keep their controls, that key's among them, and the page draws no
    // other.
    expect(entry("models").match(/class="setting-control"/g)).toHaveLength(3);
    expect(html.match(/class="setting-control"/g)).toHaveLength(3);
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
