// The People page: who is on the project's board besides the person at the machine, who holds
// every power, as the one read PEOPLE_PATH answers (crates/symphony-api's `people`): each person
// with the name their profile shows, the powers they hold (their own, and their groups'), their
// groups, the services they sign in by and their git handles; the board's groups; and the log of
// every change, newest first. Everyone on the board reads it. A sign-in is said by its service
// alone: no answer carries its subject.
//
// Its controls are the read's to give. Where it says the reader administers (the person at the
// machine, or a person holding administer), each person's powers are toggles that grant or revoke
// their own, their groups are joined and left, they are taken off the board, a group is made,
// given powers and removed, and a person is added (PEOPLE_CHANGE_PATH). A profile's form is shown
// on the reader's own card, and on every card for the person at the machine, the one who gives a
// sign-in (PROFILE_PATH). Each write answers the page as it left it, which the page draws: no
// read follows it, and what the write did is said on the part that sent it, in the log's words.

import { type FormEvent, useEffect, useState } from "react";
import { Button } from "./button";
import { Input } from "./input";
import { useMount } from "./mount";
import { Failure, getJson, post, said } from "./request";
import { SelectField } from "./select";
import { field } from "./shared";
import { tryAgain } from "./state";
import type { retryWait } from "./timing";
import { failed, ready } from "./trouble";
import {
  type GroupRow,
  type PeoplePage as Page,
  PEOPLE_CHANGE_PATH,
  PEOPLE_PATH,
  type PeopleAction,
  type PeopleChange,
  type PeopleLogLine,
  type PersonRow,
  PROFILE_PATH,
  type ProfileWrite,
} from "./wire";
import { savedAt } from "./words";

const isText = (value: unknown): value is string => typeof value === "string";
const isTexts = (value: unknown): value is string[] => Array.isArray(value) && value.every(isText);
const isNumber = (value: unknown): value is number => typeof value === "number";
const absent = (value: unknown) => value === undefined || value === null;
const maybeText = (value: unknown) => absent(value) || isText(value);

const isPerson = (value: unknown): value is PersonRow =>
  isText(field(value, "name")) &&
  isText(field(value, "display")) &&
  isTexts(field(value, "powers")) &&
  isTexts(field(value, "own")) &&
  isTexts(field(value, "groups")) &&
  isTexts(field(value, "signins")) &&
  isTexts(field(value, "git")) &&
  typeof field(value, "profile") === "boolean";

const isGroup = (value: unknown): value is GroupRow =>
  isText(field(value, "name")) && isTexts(field(value, "powers"));

const isLine = (value: unknown): value is PeopleLogLine => {
  const powers = field(value, "powers");
  return (
    isNumber(field(value, "n")) &&
    isNumber(field(value, "at")) &&
    isText(field(value, "by")) &&
    isText(field(value, "did")) &&
    isText(field(value, "name")) &&
    maybeText(field(value, "group")) &&
    (absent(powers) || isTexts(powers))
  );
};

/**
 * The answer of PEOPLE_PATH, or of a write, as the page reads it: the fields it draws, each of its
 * type; none when it is no page of people.
 */
export function readPeople(value: unknown): Page | null {
  const people = field(value, "people");
  const groups = field(value, "groups");
  const log = field(value, "log");
  const you = field(value, "you");
  const problem = field(value, "problem");
  if (
    !Array.isArray(people) ||
    !people.every(isPerson) ||
    !Array.isArray(groups) ||
    !groups.every(isGroup) ||
    !Array.isArray(log) ||
    !log.every(isLine) ||
    !isNumber(field(value, "revision")) ||
    typeof field(value, "administers") !== "boolean" ||
    typeof field(value, "machine") !== "boolean" ||
    !maybeText(you) ||
    !maybeText(problem)
  ) {
    return null;
  }
  return {
    people,
    groups,
    log,
    revision: field(value, "revision") as number,
    administers: field(value, "administers") === true,
    machine: field(value, "machine") === true,
    you: isText(you) ? you : null,
    problem: isText(problem) ? problem : null,
  };
}

/** What a read of the people tells the page. */
export interface Told {
  answered(page: Page): void;
  /** It failed, said for the page; `missing` when the server has no such thing (a `404`). */
  failed(why: string, missing: boolean): void;
}

/**
 * Reads the people once, and again after each of the feed's waits while no answer came, until one
 * does, the server has no such thing (a `404`) or `signal` ends it.
 */
export async function openPeople(
  signal: AbortSignal,
  told: Told,
  wait?: typeof retryWait,
): Promise<void> {
  let settled = false;
  const read = async () => {
    try {
      const page = readPeople(await getJson(PEOPLE_PATH, signal));
      if (signal.aborted) return;
      if (page === null) throw new Error("The people's answer is not a page of people");
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

/** The powers a board gives, in the order they are said, each by its word and as it is said. */
export const POWERS: [string, string][] = [
  ["read", "read"],
  ["speak", "speak"],
  ["decide", "decide"],
  ["direct_agents", "direct agents"],
  ["administer", "administer"],
];

/** A power as a sentence says it: `direct agents` for its word `direct_agents`. */
export function powerName(word: string): string {
  return POWERS.find(([power]) => power === word)?.[1] ?? word;
}

/** Powers said, `no power` for none. */
export function powersSaid(words: readonly string[]): string {
  return words.length === 0 ? "no power" : words.map(powerName).join(", ");
}

/** One change of the log, said: who did what to whom. A did the page does not know is said as written. */
export function logLine(line: PeopleLogLine): string {
  const powers = powersSaid(line.powers ?? []);
  // A group's own grant or revoke names the group as the change's name and its group.
  const whom = line.group === line.name ? `the group ${line.name}` : line.name;
  switch (line.did) {
    case "add":
      return `${line.by} put ${line.name} on the board, holding ${powers}`;
    case "remove":
      return `${line.by} took ${line.name} off the board`;
    case "join":
      return `${line.by} put ${line.name} in ${line.group ?? "a group"}`;
    case "leave":
      return `${line.by} took ${line.name} out of ${line.group ?? "a group"}`;
    case "grant":
      return `${line.by} gave ${whom} ${powers}`;
    case "revoke":
      return `${line.by} took ${powers} from ${whom}`;
    case "group-add":
      return `${line.by} made the group ${line.name}, holding ${powers}`;
    case "group-remove":
      return `${line.by} removed the group ${line.name}`;
    default:
      return `${line.by}: ${line.did} ${line.name}`;
  }
}

/**
 * What a write that answered `after` did, said: the log's newest line where the write added one
 * since `before`, else that nothing needed changing; a profile's write says it was written.
 */
export function doneLine(before: number, after: Page, profile: boolean): string {
  if (profile) return "The profile is written.";
  const newest = after.log.at(-1);
  if (after.revision === before || newest === undefined) {
    return "It holds that already: nothing was written.";
  }
  return `${logLine(newest)}.`;
}

/** `text` as the list a write sends: its comma-separated items, trimmed, empty ones dropped. */
export function listed(text: string): string[] {
  return text
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item !== "");
}

/** What a form holds as it is sent: each field's text. */
export interface Fields {
  name: string;
  display: string;
  signin: string;
  git: string;
  group: string;
  powers: string[];
}

/** The add a person's form sends: their name, powers and group, and what their profile is given. */
export function addition(fields: Fields): PeopleChange {
  const body: PeopleChange = {
    action: "add",
    name: fields.name.trim(),
    powers: fields.powers,
    groups: fields.group === "" ? [] : [fields.group],
    signins: listed(fields.signin),
    git: listed(fields.git),
  };
  if (fields.display.trim() !== "") body.display = fields.display.trim();
  return body;
}

/** The profile's write `name`'s form sends: a display name where one is written, the rest as lists. */
export function profileWrite(
  name: string,
  fields: Omit<Fields, "name" | "group" | "powers">,
): ProfileWrite {
  const body: ProfileWrite = { name, git: listed(fields.git), signins: listed(fields.signin) };
  if (fields.display.trim() !== "") body.display = fields.display.trim();
  return body;
}

/** What a write tells the part that sent it. */
export interface Said {
  /** The part, by its key and its element's id (`person-ana`, `group-staff`, `people-add`). */
  part: string;
  text: string;
  refused: boolean;
}

/** A write of the page: `body` posted to `route` by the part `part`. */
export type Send = (part: string, route: string, body: PeopleChange | ProfileWrite) => void;

/**
 * Posts `body` to `route` and tells `told` the page it answered, and what came of it for `part`.
 * A post takes no signal, as the server may have acted on it; `signal`, the page's mount, ends
 * what it tells once the page is gone.
 */
export async function writePeople(
  route: string,
  body: PeopleChange | ProfileWrite,
  before: number,
  signal: AbortSignal,
  told: { answered(page: Page): void; said(said: Omit<Said, "part">): void },
): Promise<void> {
  try {
    const page = readPeople(await post(route, { ...body }));
    if (signal.aborted) return;
    if (page === null) throw new Error("The server's answer is not a page of people");
    told.answered(page);
    told.said({ text: doneLine(before, page, route === PROFILE_PATH), refused: false });
  } catch (error) {
    if (signal.aborted) return;
    told.said({ text: said(error), refused: true });
  }
}

/** What the page's parts share: whether a write is out, what the last one said, and its send. */
export interface Writes {
  busy: boolean;
  said: Said | null;
  send: Send;
}

/** What a write said on the part `part`, where it was this part's. */
function Spoken({ part, writes }: { part: string; writes: Writes }) {
  const spoken = writes.said?.part === part ? writes.said : null;
  return (
    <p className="people-said" data-refused={spoken?.refused} aria-live="polite">
      {spoken?.text}
    </p>
  );
}

/**
 * The powers a person or a group holds, each a tag: one given them, one held by a group (`held`),
 * none. Where `toggle` is given, each is a button that grants a power not given them and revokes
 * one that is.
 */
/**
 * How `word` is held by one whose own (given) powers are `own` and whose powers in all are `held`:
 * their own where given, and read their own as well where another power of theirs is given, as
 * every power includes it (`implied`: taking it would change nothing, so a press does nothing, and
 * it keeps the look of a power held rather than a control's that is off); else
 * by a group, or not at all.
 */
export function powerState(
  word: string,
  held: readonly string[],
  own: readonly string[],
): { state: "own" | "group" | "none"; given: boolean; implied: boolean } {
  const given = own.includes(word);
  const implied = word === "read" && own.some((other) => other !== "read");
  const state = given || implied ? "own" : held.includes(word) ? "group" : "none";
  return { state, given, implied };
}

export function PowerTags({
  held,
  own,
  label,
  busy,
  toggle,
}: {
  held: readonly string[];
  own: readonly string[];
  label: string;
  busy: boolean;
  toggle?: ((word: string, given: boolean) => void) | undefined;
}) {
  if (toggle === undefined) {
    return (
      <ul className="people-powers" aria-label={`${label}'s powers`}>
        {held.length === 0 && <li className="people-none">no power</li>}
        {POWERS.map(([word, name]) => [word, name, powerState(word, held, own).state] as const)
          .filter(([, , state]) => state !== "none")
          .map(([word, name, state]) => (
            <li key={word} className="people-power" data-held={state}>
              {name}
            </li>
          ))}
      </ul>
    );
  }
  return (
    <ul className="people-powers" aria-label={`${label}'s powers`}>
      {POWERS.map(([word, name]) => {
        const { state, given, implied } = powerState(word, held, own);
        return (
          <li key={word}>
            <Button
              type="button"
              size="xs"
              className="people-power"
              data-held={state}
              aria-pressed={given || implied}
              aria-label={`${name} for ${label}`}
              title={
                implied
                  ? `Held with every other power of ${label}'s own`
                  : given
                    ? `Take ${name} from ${label}`
                    : state === "group"
                      ? `Held by a group; give ${label} ${name} of their own`
                      : `Give ${label} ${name}`
              }
              disabled={busy}
              aria-disabled={implied || undefined}
              onClick={() => {
                if (!implied) toggle(word, given);
              }}
            >
              {name}
            </Button>
          </li>
        );
      })}
    </ul>
  );
}

/** A two-step removal: pressed once it asks, pressed again it sends. */
function Remove({ label, busy, onRemove }: { label: string; busy: boolean; onRemove: () => void }) {
  const [asking, setAsking] = useState(false);
  if (!asking) {
    return (
      <Button
        type="button"
        size="xs"
        variant="ghost"
        disabled={busy}
        onClick={() => setAsking(true)}
      >
        {label}
      </Button>
    );
  }
  return (
    <span className="people-confirm">
      <Button
        type="button"
        size="xs"
        variant="destructive"
        disabled={busy}
        onClick={() => {
          setAsking(false);
          onRemove();
        }}
      >
        {`${label}: yes`}
      </Button>
      <Button type="button" size="xs" variant="ghost" onClick={() => setAsking(false)}>
        Keep
      </Button>
    </span>
  );
}

/** A person's profile, as the person at the machine or the person themself writes it. */
function ProfileForm({
  person,
  machine,
  writes,
}: {
  person: PersonRow;
  machine: boolean;
  writes: Writes;
}) {
  const [display, setDisplay] = useState("");
  const [git, setGit] = useState("");
  const [signin, setSignin] = useState("");
  const submit = (event: FormEvent) => {
    event.preventDefault();
    writes.send(
      `person-${person.name}`,
      PROFILE_PATH,
      profileWrite(person.name, { display, git, signin }),
    );
    setDisplay("");
    setGit("");
    setSignin("");
  };
  return (
    <details className="people-profile">
      <summary>{person.profile ? "Profile" : "Make a profile"}</summary>
      <form onSubmit={submit}>
        <Input
          aria-label={`Display name of ${person.name}`}
          placeholder={person.display}
          value={display}
          onChange={(event) => setDisplay(event.currentTarget.value)}
        />
        <Input
          aria-label={`Git handles of ${person.name}`}
          placeholder="git handle, more by commas"
          value={git}
          onChange={(event) => setGit(event.currentTarget.value)}
        />
        {machine && (
          <Input
            aria-label={`Sign-in of ${person.name}`}
            placeholder="tailscale:login@example.com"
            value={signin}
            onChange={(event) => setSignin(event.currentTarget.value)}
          />
        )}
        <Button type="submit" size="sm" disabled={writes.busy}>
          Keep
        </Button>
      </form>
    </details>
  );
}

/** One person on the board, with what the reader may change of them. */
export function Person({
  person,
  page,
  writes,
}: {
  person: PersonRow;
  page: Page;
  writes: Writes | undefined;
}) {
  const part = `person-${person.name}`;
  const change = (action: PeopleAction, fields: Partial<PeopleChange> = {}) =>
    writes?.send(part, PEOPLE_CHANGE_PATH, { action, name: person.name, ...fields });
  const admin = page.administers && writes !== undefined;
  const joinable = page.groups.filter((group) => !person.groups.includes(group.name));
  const profiled = writes !== undefined && (page.machine || page.you === person.name);
  const reach =
    person.signins.length === 0
      ? "No sign-in yet: reaches no board"
      : `Signs in by ${person.signins.join(", ")}`;
  return (
    <article className="people-person" id={part}>
      <header className="people-person-head">
        <h3>{person.display}</h3>
        {person.display !== person.name && <span className="people-name">{person.name}</span>}
        {page.you === person.name && <span className="people-you">you</span>}
      </header>
      <p className="people-reach">
        {person.git.length > 0 ? `${reach} · git ${person.git.join(", ")}` : reach}
      </p>
      <PowerTags
        held={person.powers}
        own={person.own}
        label={person.name}
        busy={writes?.busy ?? false}
        toggle={
          admin
            ? (word, given) => change(given ? "revoke" : "grant", { powers: [word] })
            : undefined
        }
      />
      {(person.groups.length > 0 || (admin && joinable.length > 0)) && (
        <div className="people-groups">
          {person.groups.map((group) => (
            <span key={group} className="people-group-tag">
              {group}
              {admin && (
                <button
                  type="button"
                  className="people-leave"
                  aria-label={`Take ${person.name} out of ${group}`}
                  disabled={writes.busy}
                  onClick={() => change("leave", { group })}
                >
                  ×
                </button>
              )}
            </span>
          ))}
          {admin && joinable.length > 0 && (
            <SelectField
              label={`Put ${person.name} in a group`}
              value=""
              options={[
                ["", "Put in a group…"],
                ...joinable.map((g): [string, string] => [g.name, g.name]),
              ]}
              disabled={writes.busy}
              onChange={(group) => {
                if (group !== "") change("join", { group });
              }}
            />
          )}
        </div>
      )}
      {(admin || profiled) && (
        <div className="people-actions">
          {profiled && <ProfileForm person={person} machine={page.machine} writes={writes} />}
          {admin && (
            <Remove
              label="Take off the board"
              busy={writes.busy}
              onRemove={() => change("remove")}
            />
          )}
        </div>
      )}
      {writes && <Spoken part={part} writes={writes} />}
    </article>
  );
}

/** One group of the board, with its powers and, for an admin, its toggles and its removal. */
function Group({
  group,
  page,
  writes,
}: {
  group: GroupRow;
  page: Page;
  writes: Writes | undefined;
}) {
  const part = `group-${group.name}`;
  const admin = page.administers && writes !== undefined;
  const members = page.people.filter((person) => person.groups.includes(group.name));
  return (
    <article className="people-person people-group" id={part}>
      <header className="people-person-head">
        <h3>{group.name}</h3>
        <span className="people-name">
          {members.length === 0 ? "no member" : members.map((person) => person.name).join(", ")}
        </span>
      </header>
      <PowerTags
        held={group.powers}
        own={group.powers}
        label={`the group ${group.name}`}
        busy={writes?.busy ?? false}
        toggle={
          admin
            ? (word, given) =>
                writes.send(part, PEOPLE_CHANGE_PATH, {
                  action: given ? "revoke" : "grant",
                  name: group.name,
                  group: group.name,
                  powers: [word],
                })
            : undefined
        }
      />
      {admin && (
        <div className="people-actions">
          <Remove
            label="Remove the group"
            busy={writes.busy}
            onRemove={() =>
              writes.send(part, PEOPLE_CHANGE_PATH, { action: "group-remove", name: group.name })
            }
          />
        </div>
      )}
      {writes && <Spoken part={part} writes={writes} />}
    </article>
  );
}

/** Powers chosen in a form before it is sent, as toggles. */
function Choose({
  chosen,
  label,
  onChoose,
}: {
  chosen: string[];
  label: string;
  onChoose: (chosen: string[]) => void;
}) {
  return (
    <PowerTags
      held={chosen}
      own={chosen}
      label={label}
      busy={false}
      toggle={(word, given) =>
        onChoose(given ? chosen.filter((power) => power !== word) : [...chosen, word])
      }
    />
  );
}

/** The form that puts a person on the board, with their profile where they have none. */
function AddPerson({ page, writes }: { page: Page; writes: Writes }) {
  const [name, setName] = useState("");
  const [display, setDisplay] = useState("");
  const [signin, setSignin] = useState("");
  const [git, setGit] = useState("");
  const [group, setGroup] = useState("");
  const [powers, setPowers] = useState<string[]>(["read"]);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    writes.send(
      "people-add",
      PEOPLE_CHANGE_PATH,
      addition({ name, display, signin, git, group, powers }),
    );
  };
  return (
    <form className="people-form" id="people-add" onSubmit={submit} aria-label="Add a person">
      <div className="people-fields">
        <Input
          aria-label="Short name"
          placeholder="short name, as ana"
          value={name}
          onChange={(event) => setName(event.currentTarget.value)}
        />
        <Input
          aria-label="Display name"
          placeholder="display name"
          value={display}
          onChange={(event) => setDisplay(event.currentTarget.value)}
        />
        <Input
          aria-label="Sign-in"
          placeholder="tailscale:login@example.com"
          value={signin}
          onChange={(event) => setSignin(event.currentTarget.value)}
        />
        <Input
          aria-label="Git handles"
          placeholder="git handle"
          value={git}
          onChange={(event) => setGit(event.currentTarget.value)}
        />
        {page.groups.length > 0 && (
          <SelectField
            label="Group"
            value={group}
            options={[
              ["", "No group"],
              ...page.groups.map((g): [string, string] => [g.name, g.name]),
            ]}
            onChange={setGroup}
          />
        )}
      </div>
      <Choose chosen={powers} label="the new person" onChoose={setPowers} />
      <p className="people-hint">
        {page.machine
          ? "A profile is made for them where the machine holds none, or given what you write."
          : "A profile is made for them where the machine holds none; one it holds is kept as it is."}
      </p>
      <Button
        type="submit"
        size="sm"
        variant="primary"
        disabled={writes.busy || name.trim() === ""}
      >
        Add
      </Button>
      <Spoken part="people-add" writes={writes} />
    </form>
  );
}

/** The form that makes a group. */
function AddGroup({ writes }: { writes: Writes }) {
  const [name, setName] = useState("");
  const [powers, setPowers] = useState<string[]>([]);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    writes.send("people-add-group", PEOPLE_CHANGE_PATH, {
      action: "group-add",
      name: name.trim(),
      powers,
    });
  };
  return (
    <form className="people-form" id="people-add-group" onSubmit={submit} aria-label="Make a group">
      <div className="people-fields">
        <Input
          aria-label="Group name"
          placeholder="group name, as staff"
          value={name}
          onChange={(event) => setName(event.currentTarget.value)}
        />
      </div>
      <Choose chosen={powers} label="the new group" onChoose={setPowers} />
      <Button type="submit" size="sm" disabled={writes.busy || name.trim() === ""}>
        Make the group
      </Button>
      <Spoken part="people-add-group" writes={writes} />
    </form>
  );
}

/** The page's outline while its read is out. */
function Ghost() {
  return (
    <div className="people-ghost" aria-hidden="true">
      {["one", "two"].map((part) => (
        <div key={part} className="people-panel people-ghost-panel" />
      ))}
    </div>
  );
}

export interface PeopleViewProps {
  page: Page | null;
  /** Why the read did not bring the page, said whole. */
  error: string | null;
  missing: boolean;
  /** The page's writes; none draws no control. */
  writes?: Writes | undefined;
  /** When it is drawn, which the log's times are said from. */
  now?: Date;
}

/** The page as drawn from what was read. */
export function PeopleView({ page, error, missing, writes, now = new Date() }: PeopleViewProps) {
  const admin = page?.administers === true && writes !== undefined;
  return (
    <div
      className="people"
      data-ready={page !== null ? "people" : ready(missing, error !== null)}
      aria-busy={page === null && error === null}
    >
      <header className="people-head">
        <h1>People</h1>
        <p className="people-sub">
          Who is on this board besides the person at the machine, who holds every power: what each
          may do, their own and by their groups, and every change of them.
          {page !== null &&
            !page.administers &&
            " Changing who is on the board needs administer, which you do not hold."}
        </p>
      </header>
      {error !== null && (
        <p className="people-error" role="alert">
          {`The people could not be read. ${error}`}
        </p>
      )}
      {page === null && error === null && <Ghost />}
      {page !== null && (
        <>
          {page.problem && (
            <p className="people-error" role="alert">
              {page.problem}
            </p>
          )}
          <section className="people-section" aria-labelledby="people-people">
            <h2 id="people-people">People</h2>
            <div className="people-panel">
              {page.people.length === 0 && (
                <p className="people-empty">
                  Nobody is on this board but the person at the machine, who holds every power.
                </p>
              )}
              {page.people.map((person) => (
                <Person key={person.name} person={person} page={page} writes={writes} />
              ))}
            </div>
            {admin && (
              <div className="people-panel people-add">
                <h3>Add a person</h3>
                <AddPerson page={page} writes={writes} />
              </div>
            )}
          </section>
          <section className="people-section" aria-labelledby="people-groups">
            <h2 id="people-groups">Groups</h2>
            <p className="people-sub">
              Each member holds what their groups hold, beside their own.
            </p>
            <div className="people-panel">
              {page.groups.length === 0 && <p className="people-empty">No group yet.</p>}
              {page.groups.map((group) => (
                <Group key={group.name} group={group} page={page} writes={writes} />
              ))}
              {admin && (
                <div className="people-add">
                  <AddGroup writes={writes} />
                </div>
              )}
            </div>
          </section>
          <section className="people-section" aria-labelledby="people-log">
            <h2 id="people-log">Log</h2>
            <div className="people-panel">
              {page.log.length === 0 ? (
                <p className="people-empty">No change yet.</p>
              ) : (
                <ol className="people-log" reversed>
                  {[...page.log].reverse().map((line) => (
                    <li key={line.n} value={line.n}>
                      <time dateTime={new Date(line.at * 1000).toISOString()}>
                        {savedAt(new Date(line.at * 1000), now)}
                      </time>
                      {logLine(line)}
                    </li>
                  ))}
                </ol>
              )}
            </div>
          </section>
        </>
      )}
    </div>
  );
}

/** The page at `/people`: read once, and drawn again from each write's answer. */
export function PeoplePage() {
  const mount = useMount();
  const [page, setPage] = useState<Page | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [spoken, setSpoken] = useState<Said | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    void openPeople(controller.signal, {
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
  const send: Send = (part, route, body) => {
    setBusy(true);
    setSpoken(null);
    void writePeople(route, body, page?.revision ?? 0, mount, {
      answered: setPage,
      said: (said) => setSpoken({ part, ...said }),
    }).finally(() => {
      if (!mount.aborted) setBusy(false);
    });
  };
  return (
    <main className="people-page">
      <PeopleView
        page={page}
        error={error}
        missing={missing}
        writes={{ busy, said: spoken, send }}
      />
    </main>
  );
}
