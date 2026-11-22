import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PHONE_STAGES, STAGES } from "./boardwait";
import {
  type Definition,
  flowOf,
  type Lanes,
  lanes,
  name,
  putStyle,
  readDefinition,
  readFlow,
  readNames,
  Strip,
  type StripProps,
  stateCounts,
} from "./flow";
import STYLE from "./flow.css?inline";
import { asking, CASES, elementOf, type Found, fire, reply, server } from "./testing";
import type { WaitingBoard } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
});

const DEFINITION = readDefinition(CASES.samples.workflow) as Definition;
const OWN: Lanes = [STAGES, PHONE_STAGES];
const WAITING = (CASES.samples.heldListing as { waiting: WaitingBoard }).waiting;

/** A listing's waiting board whose columns are `columns`, each `[status, ordinary, blocked]`. */
function listing(columns: [string, number, number][]): WaitingBoard {
  return {
    ...WAITING,
    columns: columns.map(([status, ordinary, blocked]) => ({
      status,
      ordinary,
      blocked,
      oldest: 1,
    })),
  };
}

describe("a board's active definition", () => {
  it("is its phases, states and the places a task can be at", () => {
    expect(DEFINITION.phases.map((phase) => [phase.id, phase.column])).toEqual([
      ["build", true],
      ["review", true],
      ["verify", true],
      ["parked", false],
    ]);
    expect(DEFINITION.states.map((state) => state.id)).toEqual([
      "available",
      "working",
      "blocked",
      "awaiting",
      "accepted",
    ]);
    expect(DEFINITION.pairs).toContainEqual(["review", "awaiting"]);
  });

  it("is none where no workflow is active, and none a board draws for one it cannot", () => {
    const workflow = CASES.samples.workflow as { workflow: { definition: object } };
    const changed = (definition: object) => ({
      ...workflow,
      workflow: {
        ...workflow.workflow,
        definition: { ...workflow.workflow.definition, ...definition },
      },
    });
    expect(readDefinition(CASES.samples.noWorkflow)).toBeUndefined();
    expect(readDefinition(changed({ phases: [{ id: "build", label: "Build" }] }))).toBeNull();
    expect(readDefinition(changed({ states: [{ id: "Not An ID", label: "x" }] }))).toBeNull();
    expect(readDefinition(changed({ positions: [{ at: { phase: "build" } }] }))).toBeNull();
    expect(readDefinition(changed({ phases: null }))).toBeNull();
    expect(readDefinition({})).toBeNull();
  });

  it("names each phase's column, each state's facet and each place, and forgets an older's", () => {
    const names = new Map([["phase:gone", "Gone"]]);
    name(DEFINITION, names);
    expect(names.get("phase:gone")).toBeUndefined();
    expect(names.get("phase:verify")).toBe("Verify");
    expect(names.get("state:awaiting")).toBe("Awaiting review");
    expect(names.get("review/awaiting")).toBe("Review · Awaiting review");
    // The machines' lanes, on a board whose phase may be named Build too.
    expect([names.get("build"), names.get("test")]).toEqual(["Machine build", "Machine test"]);
    expect(names.get("phase:build")).toBe("Build");
  });
});

describe("a workflow's lanes and tabs", () => {
  it("are its column phases by order on a desk, every phase by phone order on a phone", () => {
    expect(lanes(DEFINITION, WAITING, OWN)).toEqual([
      ["phase:build", "phase:review", "phase:verify", "build", "test"],
      ["phase:verify", "phase:review", "phase:build", "phase:parked"],
    ]);
  });

  it("keep a phase a newer definition names, and each status its own writers hold a task in", () => {
    const waiting = listing([
      ["phase:build", 1, 0],
      ["phase:triage", 0, 1],
      ["review", 1, 0],
      ["ready", 0, 2],
      ["done", 0, 0],
    ]);
    const [desk, phone] = lanes(DEFINITION, waiting, OWN);
    expect(desk).toEqual([
      "phase:build",
      "phase:review",
      "phase:verify",
      "phase:triage",
      "build",
      "test",
      "ready",
      "review",
    ]);
    expect(phone).toEqual([
      "phase:verify",
      "phase:review",
      "phase:build",
      "phase:parked",
      "phase:triage",
      "review",
      "ready",
    ]);
  });

  it("are the phases alone before the listing answers", () => {
    expect(lanes(DEFINITION, null, OWN)[0]).toEqual([
      "phase:build",
      "phase:review",
      "phase:verify",
      "build",
      "test",
    ]);
  });
});

describe("the open tasks of each state", () => {
  it("are the listing's counts, a pair it cannot read left out", () => {
    expect([...stateCounts(WAITING)]).toEqual([
      ["awaiting", 1],
      ["blocked", 1],
      ["working", 1],
    ]);
    const states = [["working", 2], ["blocked", "1"], "x", [3, 1]];
    expect([...stateCounts({ ...WAITING, states } as unknown as WaitingBoard)]).toEqual([
      ["working", 2],
    ]);
    expect(stateCounts(null).size).toBe(0);
  });
});

describe("the strip of a workflow's states", () => {
  const props = (fields: Partial<StripProps> = {}): StripProps => ({
    waiting: WAITING,
    wait: "",
    choose: vi.fn(),
    open: vi.fn(),
    opened: "",
    phone: false,
    ...fields,
  });
  const strip = (fields: Partial<StripProps> = {}) =>
    Strip({ definition: DEFINITION, ...props(fields) });
  const markup = (fields: Partial<StripProps> = {}) => renderToStaticMarkup(strip(fields));
  const chip = (state: string) => (element: Found) => element.props["data-state"] === state;

  it("opens each phase that is no column on a desk, and chips each state with open tasks", () => {
    const drawn = markup();
    // Led by what it holds: the workflow's own phases and states, not the board's waits.
    expect(drawn).toMatch(/^<nav [^>]*><span class="strip-label">Workflow<\/span><button/);
    expect(drawn).toContain('data-open-stage="phase:parked"');
    expect(drawn).toContain("Parked <b>0</b>");
    expect(drawn.match(/data-state="[a-z]+"/g)).toEqual([
      'data-state="working"',
      'data-state="blocked"',
      'data-state="awaiting"',
    ]);
    expect(drawn).toContain("Awaiting review <b>1</b>");
    expect(markup({ phone: true })).not.toContain("phase-open");
  });

  it("keeps the state chosen though none is open, pressed, and chooses one by its facet", () => {
    const choose = vi.fn();
    const chosen = strip({ wait: "state:accepted", choose });
    expect(elementOf(chosen, chip("accepted"))?.props["aria-pressed"]).toBe(true);
    expect(elementOf(chosen, chip("working"))?.props["aria-pressed"]).toBe(false);
    fire(chosen, chip("working"), "onClick");
    expect(choose).toHaveBeenCalledWith("state:working");
  });

  it("opens a phase as a list, and closes it again", () => {
    const open = vi.fn();
    const phase = (element: Found) => element.props["data-open-stage"] === "phase:parked";
    fire(strip({ open }), phase, "onClick");
    fire(strip({ open, opened: "phase:parked" }), phase, "onClick");
    expect(open.mock.calls).toEqual([["phase:parked"], [""]]);
  });

  it("is none on a phone with no state open", () => {
    expect(strip({ phone: true, waiting: { ...WAITING, states: [] } })).toBeNull();
  });

  it("is the flow's, beside its lanes", () => {
    const flow = flowOf(DEFINITION, OWN);
    expect(flow.lanes(WAITING)).toEqual(lanes(DEFINITION, WAITING, OWN));
    expect(renderToStaticMarkup(flow.strip(props()))).toBe(markup());
  });
});

/** A page whose head takes the flow's style sheet: what was put in it, and before what. */
function page() {
  const put: { id: string; text: string; before: unknown }[] = [];
  const theme = { theme: true };
  vi.stubGlobal("document", {
    getElementById: (id: string) => put.find((one) => one.id === id) ?? null,
    createElement: () => ({ id: "", textContent: "" }),
    head: {
      querySelector: () => theme,
      insertBefore: (element: { id: string; textContent: string }, before: unknown) =>
        put.push({ id: element.id, text: element.textContent, before }),
    },
  });
  return { put, theme };
}

describe("reading a board's workflow", () => {
  const now = async () => {};

  it("puts its sheet in the page once, before the theme", () => {
    const { put, theme } = page();
    putStyle();
    putStyle();
    expect(put).toHaveLength(1);
    expect(put[0]?.id).toBe("flow-style");
    expect(put[0]?.text).toBe(STYLE);
    expect(put[0]?.before).toBe(theme);
  });

  it("names its IDs and gives the board its flow, read again until it answers", async () => {
    const asked = server();
    const { put } = page();
    const names = new Map<string, string>();
    const reading = readFlow("design~policy", new AbortController().signal, names, OWN, now);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/@design~policy/api/workflow");
    asked[0]?.answer(reply(503, { error: "the board is slow" }));
    await asking(asked, 2);
    asked[1]?.answer(reply(200, CASES.samples.workflow));
    const flow = await reading;
    expect(flow?.lanes(WAITING)).toEqual(lanes(DEFINITION, WAITING, OWN));
    expect(names.get("phase:build")).toBe("Build");
    expect(put).toHaveLength(1);
  });

  it("is none where none is active, the read is not one a board draws or the page went", async () => {
    const asked = server();
    page();
    const names = new Map<string, string>();
    const none = readFlow("", new AbortController().signal, names, OWN, now);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.noWorkflow));
    expect(await none).toBeNull();
    const unread = readFlow("", new AbortController().signal, names, OWN, now);
    await asking(asked, 2);
    asked[1]?.answer(reply(200, { workflow: { definition: {} } }));
    expect(await unread).toBeNull();
    expect(names.size).toBe(0);
    const gone = new AbortController();
    const left = readFlow("", gone.signal, names, OWN, now);
    await asking(asked, 3);
    gone.abort();
    asked[2]?.answer(reply(200, CASES.samples.workflow));
    expect(await left).toBeNull();
    expect(names.size).toBe(0);
  });

  it("gives a page that draws no board the names alone, and none where none is active", async () => {
    const asked = server();
    const { put } = page();
    const reading = readNames("design~policy", new AbortController().signal, now);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/@design~policy/api/workflow");
    asked[0]?.answer(reply(200, CASES.samples.workflow));
    const names = await reading;
    expect(names?.get("build/working")).toBe("Build · Working");
    expect(names?.get("state:awaiting")).toBe("Awaiting review");
    expect(put).toHaveLength(0);
    const none = readNames("", new AbortController().signal, now);
    await asking(asked, 2);
    asked[1]?.answer(reply(200, CASES.samples.noWorkflow));
    expect(await none).toBeNull();
  });
});
