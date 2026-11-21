// The Settings page: everything in force for the project and where each value comes from, as the
// one read SETTINGS_PATH answers (crates/symphony-api's `settings`). It reads and never writes:
// no control changes a value, and a preference is drawn as its value alone.
//
// The page knows no entry by name. The server describes each thing in one shape (its name, what
// it is for, its state and why, what it needs and whether this machine has it, and its settings,
// each with its key, kind, value, source, scopes and class), and `Entry` draws every one of them.

import { useEffect, useState } from "react";
import { Failure, getJson, said } from "./request";
import { field } from "./shared";
import { tryAgain } from "./state";
import type { retryWait } from "./timing";
import { failed, ready } from "./trouble";
import {
  type EntryNeed,
  type EntryState,
  type SettingsPage as Page,
  SETTINGS_PATH,
  type Setting,
  type SettingScope,
  type SettingSource,
  type SettingsEntry,
  type SettingsFile,
  type SettingsSection,
} from "./wire";

const isText = (value: unknown): value is string => typeof value === "string";
const isTexts = (value: unknown): value is string[] => Array.isArray(value) && value.every(isText);
/** A field a server leaves out when it has none. */
const absent = (value: unknown) => value === undefined || value === null;
const SCOPES = ["project", "machine", "device"];
const isScope = (value: unknown): value is SettingScope => isText(value) && SCOPES.includes(value);

function isSource(value: unknown): value is SettingSource {
  const scope = field(value, "scope");
  const over = field(value, "over");
  return (
    isText(field(value, "said")) &&
    (absent(scope) || isScope(scope)) &&
    (absent(over) || isText(over))
  );
}

function isSetting(value: unknown): value is Setting {
  const choices = field(value, "choices");
  const scopes = field(value, "scopes");
  return (
    isText(field(value, "key")) &&
    isText(field(value, "name")) &&
    isText(field(value, "kind")) &&
    isText(field(value, "value")) &&
    (absent(choices) || isTexts(choices)) &&
    isSource(field(value, "source")) &&
    Array.isArray(scopes) &&
    scopes.every(isScope) &&
    isText(field(value, "class"))
  );
}

function isState(value: unknown): value is EntryState {
  const on = field(value, "on");
  return (
    isText(field(value, "word")) &&
    isText(field(value, "why")) &&
    (absent(on) || typeof on === "boolean")
  );
}

const isNeed = (value: unknown): value is EntryNeed =>
  isText(field(value, "what")) &&
  typeof field(value, "met") === "boolean" &&
  isText(field(value, "said"));

/** A list a server leaves out when it is empty, each of its items read by `one`. */
const listOf = (value: unknown, one: (item: unknown) => boolean) =>
  absent(value) || (Array.isArray(value) && value.every(one));

function isEntry(value: unknown): value is SettingsEntry {
  const state = field(value, "state");
  const source = field(value, "source");
  return (
    isText(field(value, "id")) &&
    isText(field(value, "name")) &&
    isText(field(value, "about")) &&
    (absent(state) || isState(state)) &&
    listOf(field(value, "needs"), isNeed) &&
    listOf(field(value, "settings"), isSetting) &&
    (absent(source) || isSource(source))
  );
}

function isSection(value: unknown): value is SettingsSection {
  const entries = field(value, "entries");
  return (
    isText(field(value, "id")) &&
    isText(field(value, "title")) &&
    isText(field(value, "about")) &&
    Array.isArray(entries) &&
    entries.every(isEntry)
  );
}

const isFile = (value: unknown): value is SettingsFile =>
  isText(field(value, "name")) &&
  isScope(field(value, "scope")) &&
  isText(field(value, "path")) &&
  typeof field(value, "found") === "boolean";

/**
 * The answer of SETTINGS_PATH as the page reads it: the fields it draws, each of its type, a list
 * the server left out as empty; none when it is no page of settings.
 */
export function readSettings(value: unknown): Page | null {
  const files = field(value, "files");
  const problems = field(value, "problems");
  const sections = field(value, "sections");
  if (
    !Array.isArray(files) ||
    !files.every(isFile) ||
    !(absent(problems) || isTexts(problems)) ||
    !Array.isArray(sections) ||
    !sections.every(isSection)
  ) {
    return null;
  }
  return { files, problems: isTexts(problems) ? problems : [], sections };
}

/** What a read of the settings tells the page. */
export interface Told {
  answered(page: Page): void;
  /** It failed, said for the page; `missing` when the server has no such thing (a `404`). */
  failed(why: string, missing: boolean): void;
}

/**
 * Reads the settings once, and again after each of the feed's waits while no answer came, until
 * one does, the server has no such thing (a `404`) or `signal` ends it.
 */
export async function openSettings(
  signal: AbortSignal,
  told: Told,
  wait?: typeof retryWait,
): Promise<void> {
  let settled = false;
  const read = async () => {
    try {
      const page = readSettings(await getJson(SETTINGS_PATH, signal));
      if (signal.aborted) return;
      if (page === null) throw new Error("The settings' answer is not a page of settings");
      settled = true;
      told.answered(page);
    } catch (error) {
      if (signal.aborted) return;
      const missing = error instanceof Failure && error.kind === "missing";
      settled = missing;
      told.failed(failed(said(error), !missing), missing);
    }
  };
  await read();
  await tryAgain(() => !settled, read, signal, wait);
}

/** What a scope is called on the page. */
const SCOPE_NAMES: Record<string, string> = {
  project: "project",
  machine: "this machine",
  device: "this device",
};

/** Where a value comes from, as its tag says it: its scope's name, or that no file writes it. */
export function scopeTag(source: SettingSource): [string, string] {
  const scope = source.scope ?? "default";
  return [scope, SCOPE_NAMES[scope] ?? "default"];
}

/** A setting's value as shown: a switch as a word, any other as written. */
export function shownValue(setting: Setting): string {
  if (setting.kind !== "switch") return setting.value;
  return setting.value === "on" ? "On" : "Off";
}

/** The line under a value: where it was written, and the committed one it holds over. */
export function fromLine(source: SettingSource): string {
  return source.over ? `${source.said} · over ${source.over}` : source.said;
}

function Row({ setting }: { setting: Setting }) {
  const [scope, name] = scopeTag(setting.source);
  const sentence = setting.kind === "sentence";
  return (
    <div className="setting-row" data-kind={setting.kind} data-class={setting.class}>
      <dt>
        <span className="setting-name">{setting.name}</span>
        <span className="setting-tag" data-scope={scope}>
          {name}
        </span>
        {setting.class === "reviewed" && (
          <span className="setting-tag" data-scope="reviewed">
            reviewed
          </span>
        )}
      </dt>
      <dd>
        {sentence ? (
          <p className="setting-sentence">{setting.value}</p>
        ) : (
          <span className="setting-value">{shownValue(setting)}</span>
        )}
        {(setting.choices ?? []).length > 0 && (
          <span className="setting-choices">{`one of ${(setting.choices ?? []).join(", ")}`}</span>
        )}
        <span className="setting-from">{`${setting.key} · ${fromLine(setting.source)}`}</span>
      </dd>
    </div>
  );
}

/** One entry, whatever it describes: the page's one component for every thing it shows. */
export function Entry({ entry }: { entry: SettingsEntry }) {
  const { state } = entry;
  const needs = entry.needs ?? [];
  const settings = entry.settings ?? [];
  return (
    <article className="setting-entry" id={`setting-${entry.id}`}>
      <header className="setting-entry-head">
        <h3>{entry.name}</h3>
        {state && (
          <span className="setting-state" data-on={state.on === undefined ? undefined : state.on}>
            {state.word}
          </span>
        )}
      </header>
      <p className="setting-about">{state ? `${entry.about} ${state.why}` : entry.about}</p>
      {needs.length > 0 && (
        <ul className="setting-needs">
          {needs.map((need) => (
            <li key={need.what} data-met={need.met}>
              <span className="setting-mark" aria-hidden="true">
                {need.met ? "✓" : "!"}
              </span>
              {`${need.what}: ${need.said}`}
            </li>
          ))}
        </ul>
      )}
      {entry.source && <p className="setting-from">{fromLine(entry.source)}</p>}
      {settings.length > 0 && (
        <dl className="setting-list">
          {settings.map((setting) => (
            <Row key={setting.key} setting={setting} />
          ))}
        </dl>
      )}
    </article>
  );
}

/** The page's outline while its read is out. */
function Ghost() {
  return (
    <div className="settings-ghost" aria-hidden="true">
      {["one", "two", "three"].map((part) => (
        <div key={part} className="settings-panel settings-ghost-panel" />
      ))}
    </div>
  );
}

const LEGEND: [string, string, string][] = [
  ["project", "project", "committed in the project file, for everyone"],
  ["machine", "this machine", "the local settings file, never committed"],
  ["device", "this device", "kept in this browser"],
  ["default", "default", "no file writes it"],
  ["reviewed", "reviewed", "runs a command or decides what lands: changed by a task"],
];

export interface SettingsViewProps {
  page: Page | null;
  /** Why the read did not bring the page, said whole. */
  error: string | null;
  missing: boolean;
}

/** The page as drawn from what was read. */
export function SettingsView({ page, error, missing }: SettingsViewProps) {
  return (
    <div
      className="settings"
      data-ready={page !== null ? "settings" : ready(missing, error !== null)}
      aria-busy={page === null && error === null}
    >
      <header className="settings-head">
        <h1>Settings</h1>
        <p className="settings-sub">
          What is in force for this project and where each value comes from. This page shows the
          settings and changes none.
        </p>
      </header>
      {error !== null && (
        <p className="settings-error" role="alert">
          {`The settings could not be read. ${error}`}
        </p>
      )}
      {page === null && error === null && <Ghost />}
      {page !== null && (
        <>
          {(page.problems ?? []).length > 0 && (
            <section className="settings-problems" role="alert">
              <h2>A settings file does not load</h2>
              <ul>
                {(page.problems ?? []).map((problem) => (
                  <li key={problem}>{problem}</li>
                ))}
              </ul>
            </section>
          )}
          <ul className="settings-legend" aria-label="What each tag means">
            {LEGEND.map(([scope, name, what]) => (
              <li key={scope}>
                <span className="setting-tag" data-scope={scope}>
                  {name}
                </span>
                {what}
              </li>
            ))}
          </ul>
          <ul className="settings-files" aria-label="The files read">
            {page.files.map((file) => (
              <li key={file.name} data-found={file.found}>
                <span className="setting-tag" data-scope={file.scope}>
                  {SCOPE_NAMES[file.scope]}
                </span>
                {file.found ? file.path : `${file.path} (not written)`}
              </li>
            ))}
          </ul>
          {page.sections.map((section) => (
            <section
              key={section.id}
              className="settings-section"
              aria-labelledby={`settings-${section.id}`}
            >
              <h2 id={`settings-${section.id}`}>{section.title}</h2>
              <p className="settings-sub">{section.about}</p>
              <div className="settings-panel">
                {section.entries.map((entry) => (
                  <Entry key={entry.id} entry={entry} />
                ))}
              </div>
            </section>
          ))}
        </>
      )}
    </div>
  );
}

/** The page at `/settings`. */
export function SettingsPage() {
  const [page, setPage] = useState<Page | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    void openSettings(controller.signal, {
      answered(read) {
        setPage(read);
        setError(null);
      },
      failed(why, gone) {
        setError(why);
        setMissing(gone);
      },
    });
    return () => controller.abort();
  }, []);
  return (
    <main className="settings-page">
      <SettingsView page={page} error={error} missing={missing} />
    </main>
  );
}
