// The Settings page: everything in force for the project and where each value comes from, as the
// one read SETTINGS_PATH answers (crates/symphony-api's `settings`), and a control for each
// preference the board's store keeps, which writes it there (SETTING_WRITE_PATH): in force at
// once, the store named as its source with the committed value it stands over.
//
// The page knows no entry by name. The server describes each thing in one shape (its name, what
// it is for, its state and why, what it needs and whether this machine has it, and its settings,
// each with its key, kind, value, source, scopes and class), and `Entry` draws every one of them.
// What it writes is the read's `stored` list, by each setting's key: a setting it names gets a
// control chosen by its kind (a switch, a choice, a number, a text), never by its name, and a value
// the store holds gets "Make it the project's" and "Remove" beside it. Every other setting is
// drawn as its value alone, a reviewed one with the task that changes it.

import { type FormEvent, useEffect, useState } from "react";
import { Button } from "./button";
import { Input } from "./input";
import { useMount } from "./mount";
import { Failure, getJson, post, said } from "./request";
import { SelectField } from "./select";
import { field } from "./shared";
import { tryAgain } from "./state";
import { load, save } from "./storage";
import type { retryWait } from "./timing";
import { failed, ready } from "./trouble";
import {
  type EntryNeed,
  type EntryState,
  type SettingsPage as Page,
  SETTING_WRITE_PATH,
  SETTINGS_PATH,
  type Setting,
  type SettingScope,
  type SettingSource,
  type SettingsEntry,
  type SettingsFile,
  type SettingsSection,
  type SettingValue,
  type SettingWrite,
  type SettingWritten,
  type StoredChange,
  type StoredSetting,
} from "./wire";

const isText = (value: unknown): value is string => typeof value === "string";
const isTexts = (value: unknown): value is string[] => Array.isArray(value) && value.every(isText);
/** A field a server leaves out when it has none. */
const absent = (value: unknown) => value === undefined || value === null;
const SCOPES = ["project", "board", "machine", "device"];
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

/** Text a server leaves out when it has none. */
const maybeText = (value: unknown) => absent(value) || isText(value);

const isChange = (value: unknown): value is StoredChange =>
  maybeText(field(value, "before")) &&
  maybeText(field(value, "after")) &&
  isText(field(value, "at")) &&
  isText(field(value, "by"));

const isStored = (value: unknown): value is StoredSetting =>
  isText(field(value, "setting")) &&
  isText(field(value, "key")) &&
  maybeText(field(value, "value")) &&
  typeof field(value, "held") === "boolean" &&
  maybeText(field(value, "refused")) &&
  listOf(field(value, "changes"), isChange) &&
  maybeText(field(value, "waiting"));

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
  const stored = field(value, "stored");
  if (
    !Array.isArray(files) ||
    !files.every(isFile) ||
    !(absent(problems) || isTexts(problems)) ||
    !Array.isArray(sections) ||
    !sections.every(isSection) ||
    !listOf(stored, isStored)
  ) {
    return null;
  }
  return {
    files,
    problems: isTexts(problems) ? problems : [],
    sections,
    stored: Array.isArray(stored) ? (stored as StoredSetting[]) : [],
  };
}

/**
 * The answer of SETTING_WRITE_PATH as the page reads it: what it did, in a sentence; none when it
 * is no such answer.
 */
export function readWritten(value: unknown): SettingWritten | null {
  const words = field(value, "said");
  const commit = field(value, "commit");
  const waiting = field(value, "waiting");
  if (!isText(words) || !maybeText(commit) || !(absent(waiting) || typeof waiting === "boolean")) {
    return null;
  }
  return { said: words, commit: isText(commit) ? commit : null, waiting: waiting === true };
}

/** What a write tells the row that sent it. */
export interface Wrote {
  /** What it did, or why it was refused (`refused`), in a sentence. */
  said(text: string, refused: boolean): void;
  /** It is over, whatever it said: `wrote` when the server did what it asked. */
  done(wrote: boolean): void;
}

/**
 * Sends `write` and tells the row what came of it. A post takes no signal, as the server may have
 * acted on it; `signal`, the page's mount, ends what it tells once the page is gone.
 */
export async function writeSetting(
  write: SettingWrite,
  signal: AbortSignal,
  told: Wrote,
): Promise<void> {
  let wrote = false;
  try {
    const answer = readWritten(await post(SETTING_WRITE_PATH, { ...write }));
    if (signal.aborted) return;
    if (answer === null) throw new Error("The server's answer is not a setting's write");
    wrote = true;
    told.said(answer.said, false);
  } catch (error) {
    if (signal.aborted) return;
    told.said(said(error), true);
  }
  told.done(wrote);
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
  board: "this board",
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

/** A whole number, as a number control writes one. */
const WHOLE = /^-?[0-9]+$/;

/**
 * The value a control's text writes, by the setting's kind: a whole number for a number, on or off
 * for a switch, the text as written for the rest; none for a number that is not a whole one, a
 * switch that is neither, or no text at all.
 */
export function writtenValue(kind: string, text: string): SettingValue | null {
  const written = text.trim();
  if (kind === "number") {
    const number = Number(written);
    return WHOLE.test(written) && Number.isSafeInteger(number) ? number : null;
  }
  if (kind === "switch") {
    if (written === "on") return true;
    return written === "off" ? false : null;
  }
  return written === "" ? null : written;
}

/** Why a control's text writes nothing (`writtenValue`), said on its row. */
export function unwritable(kind: string): string {
  return kind === "number"
    ? "Write a whole number; Remove takes the board's entry out."
    : "Write a value; Remove takes the board's entry out.";
}

/**
 * The text a field for `setting` opens with: its value where a file or the store writes it, a
 * number's only where it is one; else none, its default said as the field's placeholder.
 */
export function openingText(setting: Setting): string {
  if (absent(setting.source.scope)) return "";
  if (setting.kind === "number" && !WHOLE.test(setting.value)) return "";
  return setting.value;
}

/** A change of the store's entry, as its record says it: when, by whom, from what to what. */
export function changeLine(change: StoredChange): string {
  return `${change.at} · ${change.by}: ${change.before ?? "none"} → ${change.after ?? "removed"}`;
}

/** What the store's entry for a setting says beside it: its value, and whether it is in force. */
export function storedLine(stored: StoredSetting): string {
  if (absent(stored.value)) return "";
  if (stored.held) return `The board's store holds ${stored.value}, in force.`;
  if (!absent(stored.refused)) return `The board's store holds ${stored.value}, not read.`;
  return `The board's store holds ${stored.value}; the local settings file writes over it.`;
}

/** A row's write: whether one is out, and what the last one said. */
export interface WriteState {
  busy: boolean;
  said: { text: string; refused: boolean } | null;
}

export interface ControlProps {
  setting: Setting;
  stored: StoredSetting;
  /** The text a field holds that is not written yet; none while it holds the value in force. */
  draft: string | null;
  state: WriteState;
  onDraft: (text: string) => void;
  onWrite: (write: SettingWrite) => void;
  /** A write refused on the page before it is sent, in its words. */
  onRefuse: (why: string) => void;
}

/** The field that changes a setting, chosen by its kind. */
export function SettingField({
  setting,
  stored,
  draft,
  state,
  onDraft,
  onWrite,
  onRefuse,
}: ControlProps) {
  const keep = (value: SettingValue) => onWrite({ key: stored.key, action: "store", value });
  if (setting.kind === "switch") {
    const on = setting.value === "on";
    return (
      <Button
        type="button"
        size="sm"
        role="switch"
        aria-checked={on}
        aria-label={setting.name}
        className="setting-switch"
        disabled={state.busy}
        onClick={() => keep(!on)}
      >
        {on ? "On" : "Off"}
      </Button>
    );
  }
  if (setting.kind === "choice") {
    const choices = setting.choices ?? [];
    return (
      <SelectField
        label={setting.name}
        value={setting.value}
        options={choices.map((choice) => [choice, choice])}
        disabled={state.busy}
        onChange={(chosen) => {
          if (chosen !== setting.value) keep(chosen);
        }}
      />
    );
  }
  const text = draft ?? openingText(setting);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const value = writtenValue(setting.kind, text);
    if (value === null) onRefuse(unwritable(setting.kind));
    else keep(value);
  };
  return (
    <form className="setting-field" onSubmit={submit}>
      <Input
        type={setting.kind === "number" ? "number" : "text"}
        inputMode={setting.kind === "number" ? "numeric" : undefined}
        step={setting.kind === "number" ? 1 : undefined}
        aria-label={setting.name}
        placeholder={setting.value}
        value={text}
        disabled={state.busy}
        onChange={(event) => onDraft(event.currentTarget.value)}
      />
      <Button type="submit" size="sm" disabled={state.busy || draft === null}>
        Keep
      </Button>
    </form>
  );
}

/** The record of the store's entry, drawn once opened. */
function Record({ changes }: { changes: StoredChange[] }) {
  const [open, setOpen] = useState(false);
  const numbered = changes.map((change, at) => ({ change, n: at + 1 })).reverse();
  return (
    <details className="setting-record" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>{`Its record in the board's store: ${changes.length} ${changes.length === 1 ? "change" : "changes"}`}</summary>
      {open && (
        <ol reversed>
          {numbered.map(({ change, n }) => (
            <li key={n}>{changeLine(change)}</li>
          ))}
        </ol>
      )}
    </details>
  );
}

/**
 * What a setting the store keeps draws under its value: its field, the store's entry with
 * "Make it the project's" and "Remove" beside a value it holds, a commit still waiting for its
 * delivery, why the store does not read its entry, what the last write said and its record.
 */
export function Control(props: ControlProps) {
  const { stored, state, onWrite } = props;
  const write = (action: SettingWrite["action"]) => onWrite({ key: stored.key, action });
  const changes = stored.changes ?? [];
  const waiting = !absent(stored.waiting);
  return (
    <div className="setting-control">
      <SettingField {...props} />
      {!absent(stored.value) && (
        <div className="setting-store">
          <span className="setting-stored">{storedLine(stored)}</span>
          <Button
            type="button"
            size="sm"
            disabled={state.busy || waiting || !absent(stored.refused)}
            onClick={() => write("commit")}
          >
            Make it the project&apos;s
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={state.busy}
            onClick={() => write("remove")}
          >
            Remove
          </Button>
        </div>
      )}
      {waiting && (
        <p className="setting-waiting">
          {`Waiting for its delivery by pull request (${stored.waiting}); the board's entry stays in force until it is proved.`}
        </p>
      )}
      {!absent(stored.refused) && (
        <p className="setting-refused">{`The board's store does not read its entry: ${stored.refused}`}</p>
      )}
      {state.said !== null && (
        <p
          className="setting-said"
          data-refused={state.said.refused}
          role={state.said.refused ? "alert" : "status"}
        >
          {state.said.text}
        </p>
      )}
      {changes.length > 0 && <Record changes={changes} />}
    </div>
  );
}

/** Where a field's draft is kept on this device, by the setting's key in the store. */
const draftKey = (stored: StoredSetting) => `settings-draft:${stored.key}`;

/** A setting's control with its draft and its write, which `reread` follows with a read. */
function Written({
  setting,
  stored,
  reread,
}: {
  setting: Setting;
  stored: StoredSetting;
  reread: () => void;
}) {
  const mount = useMount();
  const [draft, setDraft] = useState<string | null>(() => load(draftKey(stored)) || null);
  const [state, setState] = useState<WriteState>({ busy: false, said: null });
  return (
    <Control
      setting={setting}
      stored={stored}
      draft={draft}
      state={state}
      onDraft={(text) => {
        setDraft(text);
        save(draftKey(stored), text);
      }}
      onRefuse={(why) => setState({ busy: false, said: { text: why, refused: true } })}
      onWrite={(write) => {
        setState({ busy: true, said: null });
        void writeSetting(write, mount, {
          said: (text, refused) => setState({ busy: false, said: { text, refused } }),
          done: (wrote) => {
            if (wrote) {
              setDraft(null);
              save(draftKey(stored), "");
            }
            reread();
          },
        });
      }}
    />
  );
}

/** What the page writes, by each setting's key on the page, and the read that follows a write. */
export interface Writes {
  stored: Map<string, StoredSetting>;
  reread: () => void;
}

function Row({ setting, writes }: { setting: Setting; writes?: Writes | undefined }) {
  const [scope, name] = scopeTag(setting.source);
  const sentence = setting.kind === "sentence";
  const stored = writes?.stored.get(setting.key);
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
        {stored && writes && <Written setting={setting} stored={stored} reread={writes.reread} />}
      </dd>
    </div>
  );
}

/** One entry, whatever it describes: the page's one component for every thing it shows. */
export function Entry({ entry, writes }: { entry: SettingsEntry; writes?: Writes | undefined }) {
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
            <Row key={setting.key} setting={setting} writes={writes} />
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
  ["board", "this board", "kept in the board's store, in force at once"],
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
  /** The read that follows a write; none draws no control. */
  reread?: () => void;
}

/** The page as drawn from what was read. */
export function SettingsView({ page, error, missing, reread }: SettingsViewProps) {
  const writes: Writes | undefined =
    page !== null && reread !== undefined
      ? {
          stored: new Map((page.stored ?? []).map((stored) => [stored.setting, stored])),
          reread,
        }
      : undefined;
  return (
    <div
      className="settings"
      data-ready={page !== null ? "settings" : ready(missing, error !== null)}
      aria-busy={page === null && error === null}
    >
      <header className="settings-head">
        <h1>Settings</h1>
        <p className="settings-sub">
          What is in force for this project and where each value comes from. A preference the
          board&apos;s store keeps is changed here, in force at once; make it the project&apos;s to
          commit it. A reviewed setting is changed by a task, and this page writes no file of this
          machine&apos;s.
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
                  <Entry key={entry.id} entry={entry} writes={writes} />
                ))}
              </div>
            </section>
          ))}
        </>
      )}
    </div>
  );
}

/** The page at `/settings`: read once, and again after each write. */
export function SettingsPage() {
  const [page, setPage] = useState<Page | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);
  const [reads, setReads] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: each write's count asks a read again
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
  }, [reads]);
  return (
    <main className="settings-page">
      <SettingsView
        page={page}
        error={error}
        missing={missing}
        reread={() => setReads((count) => count + 1)}
      />
    </main>
  );
}
