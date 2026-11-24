/* biome-ignore-all lint/correctness/useHookAtTopLevel: the useState mocked here answers for a
 * component called outside a render while a test turns it on, and calls React's own otherwise. */

import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, type Mock, vi } from "vitest";
import {
  AddGroup,
  AddPerson,
  addition,
  addressesSaid,
  Choose,
  doneLine,
  givable,
  kindName,
  listed,
  logLine,
  Naming,
  type NoticeFields,
  NoticesForm,
  naming,
  noticeFields,
  noticesWrite,
  openPeople,
  PeoplePage,
  PeopleView,
  Person,
  POWERS,
  PowerTags,
  powerName,
  powerState,
  powersSaid,
  profileWrite,
  Remove,
  readPeople,
  renamed,
  type Said,
  type Told,
  takesSignins,
  unreached,
  type Writes,
  writePeople,
  You,
  Yours,
} from "./people";
import { asking, CASES, elementOf, type Found, fire, reply, server } from "./testing";
import type {
  PeoplePage as Page,
  PeopleChange,
  PeopleLogLine,
  PersonRow,
  ProfileWrite,
} from "./wire";

/**
 * React's `useState` while `on` is false; while it is true, a stand-in a component is drawn by as
 * a function: each call gives the next of `states` (its initial past them) and a spy setter, kept
 * in `setters`, as task-page-controls.test.tsx draws its controls. Its `useEffect` runs nothing
 * while `on` is true, as no render is there to run it after.
 */
const hooks = vi.hoisted(() => ({ on: false, states: [] as unknown[], setters: [] as Mock[] }));

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useState: (initial: unknown) => {
      if (!hooks.on) return actual.useState(initial);
      const first = typeof initial === "function" ? initial() : initial;
      const value = hooks.states.length > 0 ? hooks.states.shift() : first;
      const set = vi.fn();
      hooks.setters.push(set);
      return [value, set];
    },
    useEffect: (...effect: Parameters<typeof actual.useEffect>) => {
      if (!hooks.on) actual.useEffect(...effect);
    },
  };
});

afterEach(() => {
  vi.unstubAllGlobals();
  hooks.on = false;
  hooks.states = [];
  hooks.setters = [];
});

const SAMPLE = CASES.samples.people as Page;
const EMPTY = CASES.samples.emptyPeople as Page;
const NAMED = CASES.samples.namedPeople as Page;
const ana = () => {
  const found = SAMPLE.people.find((person) => person.name === "ana");
  if (!found) throw new Error("no ana");
  return found;
};
/** The sample as the server answers a reader not at the machine: no profile of the machine's. */
const { mine: _mine, ...ELSEWHERE } = SAMPLE;
/** The sample as a reader without administer reads it: Cy, at another machine. */
const READER: Page = { ...ELSEWHERE, administers: false, machine: false, you: "cy" };

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

/** Writes that keep what is sent, by the part that sent it. */
function writing(said: Said | null = null): { sent: unknown[]; writes: Writes } {
  const sent: unknown[] = [];
  return {
    sent,
    writes: { busy: false, said, send: (part, route, body) => sent.push([part, route, body]) },
  };
}

describe("the read", () => {
  it("takes the server's page, and an empty one", () => {
    const unnamed = { name: null, display: "", git: [] };
    expect(readPeople(SAMPLE)).toEqual({ ...SAMPLE, you: null, problem: null, mine: unnamed });
    expect(readPeople(EMPTY)).toEqual({
      people: [],
      groups: [],
      log: [],
      revision: 0,
      administers: true,
      machine: true,
      withheld: SAMPLE.withheld,
      you: null,
      problem: null,
      mine: unnamed,
    });
    expect(readPeople(NAMED)?.mine).toEqual({
      name: "stefano",
      display: "Stefano",
      git: ["stefano-b"],
    });
    // A reader not at the machine is sent no profile of theirs, and a server off a tailnet no
    // sign-in service.
    const { mine: _, signins: __, ...theirs } = SAMPLE;
    expect(readPeople({ ...theirs, signins: [] })).toEqual({ ...theirs, you: null, problem: null });
    expect(SAMPLE.withheld.map((power) => power.power)).toEqual(["direct_agents"]);
    expect(readPeople({ ...SAMPLE, you: "cy", problem: "down" })).toMatchObject({
      you: "cy",
      problem: "down",
    });
  });

  it("refuses an answer missing or mistyping a field it draws, and a log line's powers left out are none", () => {
    const broken: [(string | number)[], unknown][] = [
      [["people"], undefined],
      [["people", 0, "display"], 3],
      [["people", 0, "powers"], "speak"],
      [["people", 0, "own"], undefined],
      [["people", 0, "groups"], [1]],
      [["people", 0, "signins"], undefined],
      [["people", 0, "git"], undefined],
      [["people", 0, "profile"], "yes"],
      [["groups"], undefined],
      [["groups", 0, "powers"], undefined],
      [["log"], {}],
      [["log", 0, "n"], "1"],
      [["log", 0, "at"], undefined],
      [["log", 0, "by"], undefined],
      [["log", 0, "did"], 2],
      [["log", 0, "group"], 2],
      [["log", 0, "powers"], "decide"],
      [["withheld"], undefined],
      [["withheld", 0, "power"], 1],
      [["withheld", 0, "why"], undefined],
      [["revision"], undefined],
      [["administers"], "yes"],
      [["machine"], undefined],
      [["you"], 3],
      [["problem"], []],
      [["people", 0, "addresses"], "ana@example.com"],
      [["people", 0, "addresses", "email"], 3],
      [["people", 0, "addresses", "slack"], ["U024BE7LH"]],
      [["people", 0, "addresses", "whatsapp"], 1],
      [["people", 0, "notices"], { card: "slack" }],
      [["people", 0, "notices", 0, "kind"], 1],
      [["people", 0, "notices", 0, "way"], 2],
    ];
    for (const [path, value] of broken) {
      expect(readPeople(changed(path, value)), path.join(".")).toBeNull();
    }
    for (const value of [null, "people", [], 3]) expect(readPeople(value)).toBeNull();
    expect(readPeople(changed(["log", 0, "powers"]))).not.toBeNull();
    // A reader who may not see a person's addresses or ways is sent neither field.
    expect(readPeople(changed(["people", 0, "addresses"]))).not.toBeNull();
    expect(readPeople(changed(["people", 0, "notices"]))).not.toBeNull();
    expect(readPeople(changed(["people", 0, "notices", 1, "way"], null))).not.toBeNull();
  });

  it("refuses a profile of the person at the machine, or sign-in services, of another shape", () => {
    for (const mine of ["stefano", { name: 1 }, { display: 2 }, { git: "x" }]) {
      expect(readPeople({ ...SAMPLE, mine })).toBeNull();
    }
    expect(readPeople({ ...SAMPLE, signins: [1] })).toBeNull();
    expect(readPeople({ ...SAMPLE, signins: "tailscale" })).toBeNull();
  });

  it("asks once and tells the page what the server answered", async () => {
    const asked = server();
    const { told, check } = telling();
    const done = openPeople(new AbortController().signal, check, async () => {
      throw new Error("an answered read waits for nothing");
    });
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/people");
    asked[0]?.answer(reply(200, SAMPLE));
    await done;
    expect(told).toEqual([["answered", readPeople(SAMPLE)]]);
  });

  it("says a failed read, tries again by itself and stops once it is answered", async () => {
    const asked = server();
    const { told, check } = telling();
    let waits = 0;
    const done = openPeople(new AbortController().signal, check, async () => {
      waits += 1;
    });
    await asking(asked, 1);
    asked[0]?.answer(reply(503, { error: "the task board cannot be reached" }));
    await asking(asked, 2);
    asked[1]?.answer(reply(200, { people: "nobody" }));
    await asking(asked, 3);
    asked[2]?.answer(reply(200, SAMPLE));
    await done;
    expect(waits).toBe(2);
    expect(told.map((each) => (each as unknown[])[0])).toEqual(["failed", "failed", "answered"]);
    expect(told[1]).toEqual([
      "failed",
      "The people's answer is not a page of people. Trying again…",
      false,
    ]);
  });

  it("ends on a server that has no such page, and says nothing once its page left", async () => {
    const asked = server();
    const { told, check } = telling();
    const gone = openPeople(new AbortController().signal, check, async () => {
      throw new Error("a missing page is not asked for again");
    });
    await asking(asked, 1);
    asked[0]?.answer(reply(404, { error: "no such page" }));
    await gone;
    expect(told).toEqual([["failed", expect.not.stringContaining("Trying again"), true]]);
    const page = new AbortController();
    const left = openPeople(page.signal, check);
    await asking(asked, 2);
    page.abort();
    asked[1]?.answer(reply(200, SAMPLE));
    await left;
    const failing = openPeople(page.signal, check);
    await asking(asked, 3);
    asked[2]?.answer(reply(500, { error: "boom" }));
    await failing;
    expect(told).toHaveLength(1);
  });
});

describe("the page's words", () => {
  const line = (did: string, more: Partial<PeopleLogLine> = {}): PeopleLogLine => ({
    n: 7,
    at: 0,
    by: "owner",
    did,
    name: "ana",
    powers: ["speak", "direct_agents"],
    ...more,
  });

  it("says a power as a sentence does, and powers as a list", () => {
    expect(powerName("direct_agents")).toBe("direct agents");
    expect(powerName("decide")).toBe("decide");
    expect(powerName("fly")).toBe("fly");
    expect(powersSaid([])).toBe("no power");
    expect(powersSaid(["speak", "direct_agents"])).toBe("speak, direct agents");
  });

  it("says each change of the log, a group's own grant as the group's", () => {
    expect(
      [
        line("add"),
        line("remove"),
        line("join", { group: "staff" }),
        line("leave", { group: "staff" }),
        line("join", { group: null }),
        line("grant"),
        line("revoke", { powers: [] }),
        line("grant", { name: "staff", group: "staff", powers: ["decide"] }),
        line("revoke", { name: "staff", group: "staff" }),
        // A line whose powers the server left out, as it does for none.
        { n: 7, at: 0, by: "owner", did: "group-add", name: "staff" },
        line("group-remove", { name: "staff" }),
        line("rename"),
      ].map(logLine),
    ).toEqual([
      "owner put ana on the board, holding speak, direct agents",
      "owner took ana off the board",
      "owner put ana in staff",
      "owner took ana out of staff",
      "owner put ana in a group",
      "owner gave ana speak, direct agents",
      "owner took no power from ana",
      "owner gave the group staff decide",
      "owner took speak, direct agents from the group staff",
      "owner made the group staff, holding no power",
      "owner removed the group staff",
      "owner: rename ana",
    ]);
  });

  it("says what a write did: its log line, nothing needed, or a profile written", () => {
    expect(doneLine(4, SAMPLE, false)).toBe("owner put cy on the board, holding read.");
    expect(doneLine(5, SAMPLE, false)).toBe("It holds that already: nothing was written.");
    expect(doneLine(0, EMPTY, false)).toBe("It holds that already: nothing was written.");
    expect(doneLine(4, SAMPLE, true)).toBe("The profile is written.");
  });

  it("reads a field's list by its commas, and sends a form's fields as a change and a profile", () => {
    expect(listed(" ana-l, ,ana ")).toEqual(["ana-l", "ana"]);
    expect(listed("")).toEqual([]);
    const fields = {
      name: " bo ",
      display: " Bo Reis ",
      signin: "tailscale:bo@example.com",
      git: "bo, bo-r",
      group: "staff",
      powers: ["read", "speak"],
    };
    expect(addition(fields)).toEqual({
      action: "add",
      name: "bo",
      display: "Bo Reis",
      powers: ["read", "speak"],
      groups: ["staff"],
      signins: ["tailscale:bo@example.com"],
      git: ["bo", "bo-r"],
    } satisfies PeopleChange);
    expect(addition({ ...fields, display: " ", group: "" })).not.toHaveProperty("display");
    expect(addition({ ...fields, group: "" }).groups).toEqual([]);
    expect(profileWrite("ana", { display: "", git: "ana-l", signin: "" })).toEqual({
      name: "ana",
      git: ["ana-l"],
      signins: [],
    } satisfies ProfileWrite);
    expect(profileWrite("ana", { display: " Ana ", git: "", signin: "github:ana" })).toEqual({
      name: "ana",
      display: "Ana",
      git: [],
      signins: ["github:ana"],
    });
  });
});

describe("a person's notices", () => {
  const labelled = (label: string) => (element: Found) => element.props["aria-label"] === label;
  const CY = SAMPLE.people[2];
  if (!CY) throw new Error("no cy");
  /** Ana's fields with `change` made. */
  const anas = (change: (fields: NoticeFields) => void) => {
    const fields = noticeFields(ana());
    change(fields);
    return fields;
  };

  it("says each kind as the form names it, and a person's addresses in a line", () => {
    expect(["card", "question", "verdict", "poll"].map(kindName)).toEqual([
      "Cards",
      "Questions",
      "Verdicts",
      "poll",
    ]);
    expect(addressesSaid(ana().addresses ?? {})).toBe(
      "Notices reach them by email ana@example.com · Slack U024BE7LH",
    );
    expect(addressesSaid({ whatsapp: "+15551234567", email: null })).toBe(
      "Notices reach them by WhatsApp +15551234567",
    );
    expect(addressesSaid({})).toBe("No address for notices");
  });

  it("draws a form with what the person holds, and sends nothing where nothing changed", () => {
    expect(noticeFields(ana())).toEqual({
      addresses: { email: "ana@example.com", slack: "U024BE7LH", whatsapp: "" },
      ways: { card: "slack", question: "", verdict: "email" },
    });
    expect(noticeFields(CY)).toEqual({
      addresses: { email: "", slack: "", whatsapp: "" },
      ways: { card: "", question: "", verdict: "" },
    });
    expect(noticeFields({ ...CY, addresses: null, notices: null })).toEqual({
      addresses: { email: "", slack: "", whatsapp: "" },
      ways: {},
    });
    expect(noticesWrite(ana(), noticeFields(ana()))).toBeNull();
    // Spaces at either end are no change.
    const spaced = anas((fields) => {
      fields.addresses.email = " ana@example.com ";
    });
    expect(noticesWrite(ana(), spaced)).toBeNull();
  });

  it("sends each address and way that differs, an emptied one taking it away", () => {
    const fields = anas((fields) => {
      fields.addresses.slack = "";
      fields.addresses.whatsapp = " +1 (555) 123-4567 ";
      fields.ways.card = "";
      fields.ways.question = "whatsapp";
    });
    expect(noticesWrite(ana(), fields)).toEqual({
      name: "ana",
      slack: "",
      whatsapp: "+1 (555) 123-4567",
      notices: [{ kind: "card" }, { kind: "question", way: "whatsapp" }],
    } satisfies ProfileWrite);
    expect(
      noticesWrite(
        CY,
        anas(() => undefined),
      ),
    ).toEqual({
      name: "cy",
      email: "ana@example.com",
      slack: "U024BE7LH",
      notices: [
        { kind: "card", way: "slack" },
        { kind: "verdict", way: "email" },
      ],
    });
  });

  it("names the kinds sent by a way the form gives no address", () => {
    expect(unreached(noticeFields(ana()))).toEqual([]);
    const left = anas((fields) => {
      fields.addresses.email = " ";
      fields.ways.question = "whatsapp";
    });
    expect(unreached(left)).toEqual(["question", "verdict"]);
  });

  it("keeps what each field and choice is given, and sends the form's write by its own part", () => {
    hooks.on = true;
    const { sent, writes } = writing();
    const notices = ana().notices ?? [];
    const unchanged = NoticesForm({ person: ana(), notices, writes });
    const keep = elementOf(unchanged, (element) => element.props.type === "submit");
    expect(keep).toBeUndefined();
    fire(unchanged, (element) => element.type === "form", "onSubmit", { preventDefault() {} });
    expect(sent).toEqual([]);
    const typed = (label: string, value: string) =>
      fire(unchanged, labelled(label), "onChange", { currentTarget: { value } });
    typed("Slack member id of ana", "W1");
    const choose = elementOf(
      unchanged,
      (element) => element.props.label === "Questions for ana by",
    );
    expect(choose?.props.options).toEqual([
      ["", "the page alone"],
      ["email", "email"],
      ["slack", "Slack"],
      ["whatsapp", "WhatsApp"],
    ]);
    (choose?.props.onChange as ((way: string) => void) | undefined)?.("email");
    const set = hooks.setters[0];
    expect(set?.mock.calls).toEqual([
      [
        anas((fields) => {
          fields.addresses.slack = "W1";
        }),
      ],
      [
        anas((fields) => {
          fields.ways.question = "email";
        }),
      ],
    ]);
    hooks.states = [
      anas((fields) => {
        fields.addresses.whatsapp = "+15551234567";
        fields.ways.verdict = "slack";
        fields.ways.question = "whatsapp";
        fields.addresses.slack = "";
      }),
    ];
    const edited = NoticesForm({ person: ana(), notices, writes });
    expect(elementOf(edited, (element) => element.props.type === "submit")?.props.disabled).toBe(
      false,
    );
    fire(edited, (element) => element.type === "form", "onSubmit", { preventDefault() {} });
    expect(sent).toEqual([
      [
        "notices-ana",
        "/api/people-profile",
        {
          name: "ana",
          slack: "",
          whatsapp: "+15551234567",
          notices: [
            { kind: "question", way: "whatsapp" },
            { kind: "verdict", way: "slack" },
          ],
        },
      ],
    ]);
    hooks.states = [
      anas((fields) => {
        fields.addresses.slack = "";
      }),
    ];
    const warned = renderToStaticMarkup(NoticesForm({ person: ana(), notices, writes }));
    expect(warned).toContain('<p class="people-warning">Cards: no address for that way.</p>');
    expect(warned).not.toContain("people-hint");
  });
});

describe("a write", () => {
  function told() {
    const said: unknown[] = [];
    return {
      said,
      check: {
        answered: (page: Page) => said.push(["answered", page.revision]),
        said: (spoken: Omit<Said, "part">) => said.push(["said", spoken.text, spoken.refused]),
      },
    };
  }
  const change: PeopleChange = { action: "add", name: "cy", powers: ["read"] };

  it("posts the change, draws the page it answers and says what it did", async () => {
    const asked = server();
    const { said, check } = told();
    const done = writePeople("/api/people-change", change, 4, new AbortController().signal, check);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/people-change");
    expect(asked[0]?.body).toEqual(change);
    asked[0]?.answer(reply(200, SAMPLE));
    await done;
    expect(said).toEqual([
      ["answered", 5],
      ["said", "owner put cy on the board, holding read.", false],
    ]);
  });

  it("says a refusal in the server's words, and an answer that is no page", async () => {
    const asked = server();
    const { said, check } = told();
    const refused = writePeople(
      "/api/people-change",
      change,
      4,
      new AbortController().signal,
      check,
    );
    await asking(asked, 1);
    asked[0]?.answer(reply(403, { error: "Forbidden: cy may not administer on this board" }));
    await refused;
    const odd = writePeople(
      "/api/people-profile",
      { name: "cy" },
      4,
      new AbortController().signal,
      check,
    );
    await asking(asked, 2);
    asked[1]?.answer(reply(200, { done: true }));
    await odd;
    expect(said).toEqual([
      ["said", "Forbidden: cy may not administer on this board", true],
      ["said", "The server's answer is not a page of people", true],
    ]);
  });

  it("tells nothing once its page is gone", async () => {
    const asked = server();
    const { said, check } = told();
    const page = new AbortController();
    const done = writePeople("/api/people-change", change, 4, page.signal, check);
    await asking(asked, 1);
    page.abort();
    asked[0]?.answer(reply(200, SAMPLE));
    await done;
    const failed = writePeople("/api/people-change", change, 4, page.signal, check);
    await asking(asked, 2);
    asked[1]?.answer(reply(500, { error: "boom" }));
    await failed;
    expect(said).toEqual([]);
  });
});

describe("a person's and a group's powers", () => {
  const labelled = (label: string) => (element: Found) => element.props["aria-label"] === label;

  it("holds read as one's own where another power of one's own includes it", () => {
    const state = (word: string, held: string[], own: string[]) => powerState(word, held, own);
    // Given alone, read is given and may be taken.
    expect(state("read", ["read"], ["read"])).toEqual({
      state: "own",
      given: true,
      implied: false,
    });
    // With another power of one's own, read is theirs and taking it would change nothing.
    expect(state("read", ["read", "administer"], ["administer"])).toEqual({
      state: "own",
      given: false,
      implied: true,
    });
    expect(state("read", ["read", "speak"], ["read", "speak"])).toEqual({
      state: "own",
      given: true,
      implied: true,
    });
    // Held only by a group, read is the group's, and may be given of their own.
    expect(state("read", ["read", "decide"], [])).toEqual({
      state: "group",
      given: false,
      implied: false,
    });
    expect(state("decide", ["read", "decide"], ["speak"])).toEqual({
      state: "group",
      given: false,
      implied: false,
    });
    expect(state("administer", [], [])).toEqual({ state: "none", given: false, implied: false });
  });

  it("draws the powers held as tags, a group's dashed, when nothing may change them", () => {
    const html = renderToStaticMarkup(
      <PowerTags held={ana().powers} own={ana().own} label="ana" busy={false} />,
    );
    expect(html).toBe(
      '<ul class="people-powers" aria-label="ana&#x27;s powers">' +
        '<li class="people-power" data-held="own">read</li>' +
        '<li class="people-power" data-held="own">speak</li>' +
        '<li class="people-power" data-held="group">decide</li></ul>',
    );
    expect(
      renderToStaticMarkup(<PowerTags held={[]} own={[]} label="bo" busy={false} />),
    ).toContain('<li class="people-none">no power</li>');
    // A group given decide alone holds read with it.
    expect(
      renderToStaticMarkup(
        <PowerTags held={["decide"]} own={["decide"]} label="staff" busy={false} />,
      ),
    ).toBe(
      '<ul class="people-powers" aria-label="staff&#x27;s powers">' +
        '<li class="people-power" data-held="own">read</li>' +
        '<li class="people-power" data-held="own">decide</li></ul>',
    );
  });

  it("draws every power as a toggle that gives one not given and takes one that is", () => {
    const toggled: unknown[] = [];
    const tags = PowerTags({
      held: ana().powers,
      own: ana().own,
      label: "ana",
      busy: false,
      toggle: (word, given) => toggled.push([word, given]),
    });
    const html = renderToStaticMarkup(tags);
    expect(html.match(/<button/g)).toHaveLength(5);
    expect(html).toContain(
      'data-held="own" aria-pressed="true" aria-label="speak for ana" title="Take speak from ana"',
    );
    expect(html).toContain(
      'data-held="group" aria-pressed="false" aria-label="decide for ana" title="Held by a group; give ana decide of their own"',
    );
    expect(html).toContain(
      'data-held="none" aria-pressed="false" aria-label="administer for ana" title="Give ana administer"',
    );
    // Read, which her own speak includes: hers, and not offered, as taking it changes nothing.
    expect(html).toContain(
      'data-held="own" aria-pressed="true" aria-label="read for ana" title="Held with every other power of ana&#x27;s own" aria-disabled="true"',
    );
    fire(tags, labelled("speak for ana"), "onClick");
    fire(tags, labelled("read for ana"), "onClick");
    fire(tags, labelled("direct agents for ana"), "onClick");
    expect(toggled).toEqual([
      ["speak", true],
      ["direct_agents", false],
    ]);
  });

  it("offers no control for a power the board gives no one yet, and draws one held as held", () => {
    const toggled: unknown[] = [];
    const withheld = SAMPLE.withheld;
    const toggle = (word: string, given: boolean) => toggled.push([word, given]);
    const tags = PowerTags({
      held: ana().powers,
      own: ana().own,
      label: "ana",
      busy: false,
      toggle,
      withheld,
    });
    const html = renderToStaticMarkup(tags);
    expect(html).not.toContain("direct agents");
    expect(html.match(/<button /g)).toHaveLength(POWERS.length - 1);
    fire(tags, labelled("administer for ana"), "onClick");
    expect(toggled).toEqual([["administer", false]]);
    // Held by a group or given already (a board from before the rule), it is drawn held, with no
    // control to give or take it.
    for (const [own, state] of [
      [[], "group"],
      [["direct_agents"], "own"],
    ] as const) {
      const held = renderToStaticMarkup(
        <PowerTags
          held={["read", "direct_agents"]}
          own={own}
          label="bo"
          busy={false}
          toggle={toggle}
          withheld={withheld}
        />,
      );
      expect(held).toContain(`<li class="people-power" data-held="${state}">direct agents</li>`);
      expect(held).not.toContain('aria-label="direct agents for bo"');
    }
  });

  it("toggles a form's choice of every power the board gives, and sends none it gives no one", () => {
    const chosen: string[][] = [];
    const choice = (
      <Choose
        chosen={["read"]}
        label="the new person"
        withheld={SAMPLE.withheld}
        onChoose={(powers) => chosen.push(powers)}
      />
    );
    const html = renderToStaticMarkup(choice);
    expect(html).not.toContain("direct agents");
    expect(html).not.toContain("people-hint");
    const tags = elementOf(choice.type(choice.props), (element) => element.type === PowerTags);
    const toggle = tags?.props.toggle as (word: string, given: boolean) => void;
    toggle("speak", false);
    toggle("read", true);
    expect(chosen).toEqual([["read", "speak"], []]);
    expect(givable(["read", "direct_agents", "speak"], SAMPLE.withheld)).toEqual(["read", "speak"]);
    expect(givable(["direct_agents"], [])).toEqual(["direct_agents"]);
  });

  it("asks before a removal as the task page's Cancel does: an outlined control, then a named confirmation", () => {
    const removed: string[] = [];
    const props = {
      label: "Take off the board",
      question: "Take cy off the board?",
      keep: "Keep cy",
      busy: false,
      onRemove: () => removed.push("cy"),
    };
    const pressed = (text: string) => (element: Found) => element.props.children === text;
    hooks.on = true;
    // Its outlined control, pressed: it asks, and sends nothing.
    const control = Remove(props);
    expect(renderToStaticMarkup(control)).toBe(
      '<button class="dx-button" data-style="outline" data-size="xs" type="button">Take off the board</button>',
    );
    fire(control, pressed("Take off the board"), "onClick");
    const [asks] = hooks.setters;
    expect(asks?.mock.calls).toEqual([[true]]);
    expect(removed).toEqual([]);
    // Drawn asking, as that press left it: the named confirmation, kept, then confirmed.
    hooks.states = [true];
    const confirmation = Remove(props);
    expect(renderToStaticMarkup(confirmation)).toBe(
      '<div class="people-confirm" role="group" aria-label="Take off the board: confirmation">' +
        "<span>Take cy off the board?</span>" +
        '<button class="dx-button" data-style="destructive" data-size="xs" type="button">Confirm removal</button>' +
        '<button class="dx-button" data-style="outline" data-size="xs" type="button">Keep cy</button></div>',
    );
    const set = hooks.setters.at(-1);
    fire(confirmation, pressed("Keep cy"), "onClick");
    expect([set?.mock.calls, removed]).toEqual([[[false]], []]);
    fire(confirmation, pressed("Confirm removal"), "onClick");
    expect([set?.mock.calls, removed]).toEqual([[[false], [false]], ["cy"]]);
  });

  it("sends a person's grant, revoke, join, leave and removal for an admin, by the person's part", () => {
    const { sent, writes } = writing();
    const drawn = Person({ person: ana(), page: SAMPLE, writes });
    const tags = elementOf(drawn, (element) => element.type === PowerTags);
    expect(tags?.props.withheld).toBe(SAMPLE.withheld);
    const removal = elementOf(drawn, (element) => element.type === Remove);
    expect([removal?.props.question, removal?.props.keep]).toEqual([
      "Take ana off the board?",
      "Keep ana",
    ]);
    const toggle = tags?.props.toggle as (word: string, given: boolean) => void;
    toggle("speak", true);
    toggle("administer", false);
    fire(drawn, labelled("Take ana out of staff"), "onClick");
    fire(drawn, (element) => typeof element.props.onRemove === "function", "onRemove");
    expect(sent).toEqual([
      ["person-ana", "/api/people-change", { action: "revoke", name: "ana", powers: ["speak"] }],
      [
        "person-ana",
        "/api/people-change",
        { action: "grant", name: "ana", powers: ["administer"] },
      ],
      ["person-ana", "/api/people-change", { action: "leave", name: "ana", group: "staff" }],
      ["person-ana", "/api/people-change", { action: "remove", name: "ana" }],
    ]);
    const bo = SAMPLE.people[1];
    if (!bo) throw new Error("no bo");
    const select = elementOf(
      Person({ person: bo, page: SAMPLE, writes }),
      (element) => element.props.label === "Put bo in a group",
    );
    const onChange = select?.props.onChange as (group: string) => void;
    onChange("");
    onChange("staff");
    expect(sent.at(-1)).toEqual([
      "person-bo",
      "/api/people-change",
      { action: "join", name: "bo", group: "staff" },
    ]);
    expect(sent).toHaveLength(5);
  });
});

describe("the page", () => {
  const draw = (page: Page | null, writes?: Writes, error: string | null = null, missing = false) =>
    renderToStaticMarkup(
      <PeopleView
        page={page}
        error={error}
        missing={missing}
        writes={writes}
        now={new Date(1_763_000_000_000 + 6 * 3_600_000)}
      />,
    );

  it("opens busy on its outline while its read is out", () => {
    vi.stubGlobal("document", { getElementById: () => null });
    const html = renderToStaticMarkup(<PeoplePage />);
    expect(html).toMatch(
      /^<main class="people-page"><div class="people" data-ready="loading" aria-busy="true">/,
    );
    expect(html).toContain('class="people-ghost" aria-hidden="true"');
  });

  it("draws each person, group and change, the log newest first, for the person at the machine", () => {
    const html = draw(SAMPLE, writing().writes);
    expect(html).toMatch(/^<div class="people" data-ready="people" aria-busy="false">/);
    expect(
      [...html.matchAll(/<article class="people-person[^"]*" id="([a-z-]+)"/g)].map((f) => f[1]),
    ).toEqual(["person-ana", "person-bo", "person-cy", "group-staff"]);
    expect(html).toContain('<h3>Ana Lima</h3><span class="people-name">ana</span>');
    expect(html).toContain("Signs in by tailscale, github · git ana-lima");
    expect(html).toContain("No sign-in yet: reaches no board");
    expect(html).toContain('<span class="people-name">ana</span></header>');
    expect([...html.matchAll(/<li value="(\d)">/g)].map((f) => f[1])).toEqual([
      "5",
      "4",
      "3",
      "2",
      "1",
    ]);
    expect(html).toContain("owner put cy on the board, holding read</li>");
    // The person at the machine adds someone else, makes a group, and writes every profile, a
    // sign-in with it, each field labelled.
    expect(html).toContain("<h3>Add someone else</h3>");
    expect(html).toContain('aria-label="Add someone else"');
    expect(html).toContain('aria-label="Make a group"');
    expect(html.match(/<details class="people-profile">/g)).toHaveLength(3);
    expect(html).toContain('aria-label="Sign-in of bo"');
    expect(html).toContain('aria-label="Sign-in"');
    expect(html).toContain(
      '<label for="people-add-signin">Sign-in, to open boards from their own device<span class="people-optional"> (optional)</span></label>',
    );
    expect(html).toContain('<label for="people-add-name">Short name</label>');
    expect(html).toContain('<label for="people-group-name">Group name</label>');
    expect(html).toContain("<summary>Make a profile</summary>");
    // No word explains the page's controls, nor a power the board gives no one; the group's
    // removal asks.
    expect(html).not.toContain("people-hint");
    expect(html).not.toContain("Direct agents is given to no one yet");
    expect(html).not.toContain("direct agents for");
    // The person at the machine sees every address and writes every person's notices it is sent.
    expect(html).toContain(
      '<p class="people-reach people-addresses">Notices reach them by email ana@example.com · Slack U024BE7LH</p>',
    );
    expect(html).toContain('<p class="people-reach people-addresses">No address for notices</p>');
    expect(html.match(/<details class="people-profile people-notices">/g)).toHaveLength(2);
    expect(html).toContain('aria-label="Notices of ana"');
    expect(html).toContain('aria-label="Notices of cy"');
    expect(html).not.toContain('aria-label="Notices of bo"');
    expect(html).toContain(">Remove the group</button>");
  });

  it("offers an admin who is not the person at the machine no sign-in to give in the add", () => {
    const html = draw({ ...ELSEWHERE, machine: false, you: "bo" }, writing().writes);
    expect(html).toContain('aria-label="Add someone else"');
    expect(html).not.toContain('aria-label="Sign-in"');
    expect(html).not.toContain("people-hint");
    // The form explains nothing of itself: no hint of what Add makes.
    expect(html).not.toContain("A profile is made");
    // Sent others' addresses and not their ways, as the server sends an admin, it draws the
    // addresses and no one's notices to write.
    const people = SAMPLE.people.map(({ notices: _, ...person }) => person);
    const admin = draw({ ...SAMPLE, machine: false, you: "bo", people }, writing().writes);
    expect(admin).toContain("Notices reach them by email ana@example.com · Slack U024BE7LH");
    expect(admin).not.toContain("people-notices");
  });

  it("offers no sign-in to give where the server knows no sign-in's service, as off a tailnet", () => {
    const { signins: _, ...offline } = SAMPLE;
    const html = draw(offline, writing().writes);
    expect(html).toContain('aria-label="Add someone else"');
    expect(html).not.toContain("Sign-in");
    expect(takesSignins(SAMPLE)).toBe(true);
    expect(takesSignins(offline)).toBe(false);
    expect(takesSignins({ ...SAMPLE, machine: false })).toBe(false);
  });

  it("says a change of today by its time, and one of another day with its day", () => {
    const at = (...date: [number, number, number, number, number]) =>
      new Date(...date).getTime() / 1000;
    const log = [
      [1, at(2025, 10, 13, 9, 30)],
      [2, at(2026, 2, 2, 9, 30)],
      [3, at(2026, 10, 23, 9, 30)],
      [4, at(2026, 10, 24, 9, 30)],
    ].map(([n, when]) => ({ n, at: when, by: "owner", did: "remove", name: "cy", powers: [] }));
    const html = renderToStaticMarkup(
      <PeopleView
        page={{ ...EMPTY, log: log as PeopleLogLine[] }}
        error={null}
        missing={false}
        now={new Date(2026, 10, 24, 18, 0)}
      />,
    );
    expect([...html.matchAll(/<time [^>]*>([^<]*)<\/time>/g)].map((found) => found[1])).toEqual([
      "09:30",
      "yesterday 09:30",
      "2 Mar 09:30",
      "13 Nov 2025 09:30",
    ]);
  });

  it("draws a reader without administer no control but their own profile, and no sign-in to give", () => {
    // As the server sends one who is neither an admin nor the person at the machine: their own
    // addresses and ways, and no one else's.
    const people = SAMPLE.people.map((person): PersonRow => {
      if (person.name === "cy") return person;
      const { addresses: _addresses, notices: _notices, ...theirs } = person;
      return theirs;
    });
    const own = draw({ ...READER, people }, writing().writes);
    expect(own.match(/people-addresses/g)).toHaveLength(1);
    expect(own).not.toContain("ana@example.com");
    expect(own).not.toContain("U024BE7LH");
    expect(own.match(/<details class="people-profile people-notices">/g)).toHaveLength(1);
    expect(own).toContain('aria-label="Notices of cy"');
    const html = draw(READER, writing().writes);
    expect(html).toContain(
      '<header class="people-head"><h1>People</h1><p class="people-refused">Changing who is on the board needs administer.</p></header>',
    );
    expect(html).not.toContain('aria-label="Add someone else"');
    expect(html).not.toContain('id="you"');
    expect(html).not.toContain("Take off the board");
    expect(html).not.toContain("aria-pressed");
    expect(html.match(/<details class="people-profile">/g)).toHaveLength(1);
    expect(html).toContain('aria-label="Display name of cy"');
    expect(html).not.toContain('aria-label="Sign-in of cy"');
    expect(html).toContain('<span class="people-you">you</span>');
    // Drawn with no writes at all, nothing is a control.
    expect(draw(SAMPLE)).not.toMatch(/<(button|input|form|details)\b/);
  });

  it("says an empty board, a problem, a failed read, and a write's words on the part that sent it", () => {
    const empty = draw(EMPTY, writing().writes);
    // The head is its title alone, and an empty part says so in a line.
    expect(empty).toContain('<header class="people-head"><h1>People</h1></header>');
    expect(empty).toContain('<p class="people-empty">Nobody yet.</p>');
    expect(empty).not.toContain("people-sub");
    expect(empty).toContain("No group yet.");
    expect(empty).toContain("No change yet.");
    expect(empty).not.toContain("needs administer");
    expect(draw({ ...EMPTY, problem: "the board's people could not be read: down" })).toContain(
      '<p class="people-error" role="alert">the board&#x27;s people could not be read: down</p>',
    );
    const failed = draw(null, undefined, "The server could not be reached");
    expect(failed).toContain('data-ready="failed" aria-busy="false"');
    expect(failed).toContain("The people could not be read. The server could not be reached");
    const spoken = draw(SAMPLE, writing({ part: "person-bo", text: "no", refused: true }).writes);
    const bo = spoken.slice(spoken.indexOf('id="person-bo"')).split("</article>")[0] ?? "";
    expect(bo).toContain('<p class="people-said" data-refused="true" aria-live="polite">no</p>');
    const anas = spoken.slice(spoken.indexOf('id="person-ana"')).split("</article>")[0] ?? "";
    expect(anas).toContain('<p class="people-said" aria-live="polite"></p>');
  });
});

describe("the person at the machine's own profile", () => {
  const draw = (page: Page, writes?: Writes) =>
    renderToStaticMarkup(<PeopleView page={page} error={null} missing={false} writes={writes} />);
  const submitted = { preventDefault() {} };
  const named = { name: "stefano", display: "Stefano", git: ["stefano-b"] };
  const read = (element: Found) => element.props.children === "Confirm the name";

  it("gives their short name once and for good, with a display name and git handles", () => {
    expect(naming(" stefano ", { display: " Stefano ", git: "stefano-b, sb" })).toEqual({
      name: "stefano",
      display: "Stefano",
      git: ["stefano-b", "sb"],
      machine: true,
    });
    expect(naming("stefano", { display: " ", git: "" })).toEqual({
      name: "stefano",
      git: [],
      machine: true,
    });
  });

  it("is first on the page, theirs alone: the form that names them, then their name", () => {
    const { writes } = writing();
    const unnamed = draw(SAMPLE, writes);
    expect(unnamed.indexOf('id="you"')).toBeLessThan(unnamed.indexOf('id="people-people"'));
    expect(unnamed).toContain('<h2 id="people-you-head">You</h2>');
    expect(unnamed).toContain('<p class="people-state">You write as owner.</p>');
    expect(unnamed).toContain('<label for="you-name">Short name</label>');
    expect(unnamed).toContain(
      '<label for="you-display">Display name<span class="people-optional"> (optional)</span></label>',
    );
    expect(unnamed).toContain(
      '<label for="you-git">Git handles<span class="people-optional"> (optional)</span></label>',
    );
    expect(unnamed).not.toContain('for="you-signin"');
    expect(unnamed).toContain('type="submit" disabled="">Keep</button>');
    const theirs = draw(NAMED, writes);
    expect(theirs).toContain(
      '<h3>Stefano</h3><span class="people-name">stefano</span></header><p class="people-reach">git stefano-b</p>',
    );
    expect(theirs).not.toContain("You write as owner.");
    expect(theirs).not.toContain('aria-label="Your short name"');
    // Another reader, or a page drawn with no writes, has no You section.
    expect(draw(READER, writes)).not.toContain('id="you"');
    expect(draw(SAMPLE)).not.toContain('id="you"');
  });

  it("asks once more before it gives the name, saying it is fixed, and sends it confirmed", () => {
    const { sent, writes } = writing();
    hooks.on = true;
    hooks.states = ["stefano", "Stefano", "stefano-b", false];
    const form = Naming({ writes });
    fire(form, (element) => typeof element.props.onSubmit === "function", "onSubmit", submitted);
    expect(hooks.setters[3]?.mock.calls).toEqual([[true]]);
    expect(sent).toEqual([]);
    hooks.states = ["stefano", "Stefano", "stefano-b", true];
    const asking = Naming({ writes });
    const html = renderToStaticMarkup(asking);
    expect(html).toContain(
      '<div class="people-confirm" role="group" aria-label="Your short name: confirmation">' +
        "<span>Write as stefano on every board? A short name is never changed once given.</span>",
    );
    expect(html).not.toContain(">Keep</button>");
    fire(asking, (element) => element.props.children === "Change it", "onClick");
    fire(asking, read, "onClick");
    expect(sent).toEqual([
      [
        "you",
        "/api/people-profile",
        { name: "stefano", display: "Stefano", git: ["stefano-b"], machine: true },
      ],
    ]);
    // Typing another name asks again.
    fire(asking, (element) => element.props["aria-label"] === "Your short name", "onChange", {
      currentTarget: { value: "teo" },
    });
    const [setName, , , setAsking] = hooks.setters.slice(4);
    expect(setName?.mock.calls).toEqual([["teo"]]);
    // Kept, confirmed, then typed again: each leaves it asking no more.
    expect(setAsking?.mock.calls).toEqual([[false], [false], [false]]);
  });

  it("asks nothing for a name left empty", () => {
    const { writes } = writing();
    hooks.on = true;
    hooks.states = ["  ", "", "", false];
    const form = Naming({ writes });
    fire(form, (element) => typeof element.props.onSubmit === "function", "onSubmit", submitted);
    expect(hooks.setters[3]?.mock.calls).toEqual([]);
  });

  it("writes their display name and git handles under the name they gave, never another", () => {
    const { sent, writes } = writing();
    hooks.on = true;
    hooks.states = ["Stefano B", "sb"];
    const form = Yours({ mine: named, name: "stefano", writes });
    fire(form, (element) => typeof element.props.onSubmit === "function", "onSubmit", submitted);
    expect(sent).toEqual([
      [
        "you",
        "/api/people-profile",
        { name: "stefano", display: "Stefano B", git: ["sb"], signins: [] },
      ],
    ]);
    const plain = renderToStaticMarkup(
      <You mine={{ name: "stefano", display: "", git: [] }} writes={writes} />,
    );
    expect(plain).toContain("<h3>stefano</h3></header>");
    expect(plain).not.toContain("people-reach");
  });

  it("names them in the header as a write's answer names them, and no one before", async () => {
    vi.stubGlobal("document", { getElementById: () => null });
    const heard: string[] = [];
    vi.stubGlobal("dispatchEvent", (event: Event) => heard.push(event.type));
    const { project } = await import("./project");
    renamed(SAMPLE);
    renamed(READER);
    expect([heard, project().you]).toEqual([[], undefined]);
    renamed(NAMED);
    expect([heard, project().you, project().machine]).toEqual([
      ["review:frame"],
      { name: "stefano", shown: "Stefano" },
      { name: "stefano", shown: "Stefano" },
    ]);
    renamed({ ...NAMED, mine: { name: "stefano", display: "", git: [] } });
    expect(project().you).toEqual({ name: "stefano", shown: "stefano" });
  });

  it("marks their row on a board that holds them as theirs", () => {
    const { writes } = writing();
    const html = renderToStaticMarkup(
      <Person
        person={ana()}
        page={{ ...NAMED, mine: { ...named, name: "ana" } }}
        writes={writes}
      />,
    );
    expect(html).toContain('<span class="people-you">you</span>');
  });
});

describe("a form's powers", () => {
  const submitted = { preventDefault() {} };
  const submit = (element: Found) => typeof element.props.onSubmit === "function";

  it("adds someone else with no power the board gives no one, a sign-in only where one is taken", () => {
    const { sent, writes } = writing();
    hooks.on = true;
    hooks.states = ["ana", "", "tailscale:ana@example.com", "", "", ["read", "direct_agents"]];
    fire(AddPerson({ page: SAMPLE, writes }), submit, "onSubmit", submitted);
    hooks.states = ["ana", "", "tailscale:ana@example.com", "", "", ["read"]];
    const { signins: _, ...offline } = SAMPLE;
    fire(AddPerson({ page: offline, writes }), submit, "onSubmit", submitted);
    expect(sent.map((write) => (write as [string, string, PeopleChange])[2])).toEqual([
      {
        action: "add",
        name: "ana",
        powers: ["read"],
        groups: [],
        signins: ["tailscale:ana@example.com"],
        git: [],
      },
      { action: "add", name: "ana", powers: ["read"], groups: [], signins: [], git: [] },
    ]);
  });

  it("makes a group with no power the board gives no one", () => {
    const { sent, writes } = writing();
    hooks.on = true;
    hooks.states = ["design", ["decide", "direct_agents"]];
    fire(AddGroup({ page: SAMPLE, writes }), submit, "onSubmit", submitted);
    expect(sent).toEqual([
      [
        "people-add-group",
        "/api/people-change",
        { action: "group-add", name: "design", powers: ["decide"] },
      ],
    ]);
  });
});
