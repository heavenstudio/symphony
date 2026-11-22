import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { readGuards, readRefusal } from "./guards";
import {
  type Action,
  type Board,
  guardsAlone,
  isStale,
  keyFor,
  missing,
  NextActionsView,
  type NextActionsViewProps,
  newKey,
  type Offer,
  placeWords,
  type Read,
  readActions,
  readBoard,
  readOffer,
  recoveryWords,
  requestOf,
  take,
  takenWords,
  takerWords,
  unavailableWords,
  unknownOutcome,
  whose,
} from "./nextactions";
import { Failure } from "./request";
import { field } from "./shared";
import { type Asked, asking, CASES, elementOf, type Found, fire, reply, server } from "./testing";
import type { WorkflowGuardRefusal, WorkflowPlace, WorkflowTaskGuards } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
});

const OFFER = readOffer(CASES.samples.nextActions) as Offer;
const BOARD = readBoard(CASES.samples.workflow) as Board;
const [PASS, REWORK] = OFFER.actions as [Action, Action];
/** The actions at Verify, where Accept takes the accepted output of what the task depends on. */
const TAKING = readOffer(CASES.samples.takingActions) as Offer;
const [ACCEPT] = TAKING.actions as [Action];
const AT: WorkflowPlace = { phase: "review", state: "awaiting", revision: 7 };
const GUARDS = readGuards(CASES.samples.taskGuards) as WorkflowTaskGuards;
const REFUSAL = readRefusal(CASES.samples.guardRefusal) as WorkflowGuardRefusal;
/** Pass review as a preview lists it for the owner once its guard is all that holds it. */
const HELD: Action = { ...PASS, unavailable: ["guard_unavailable"] };
const GUARDED: Offer = { ...OFFER, actions: [HELD, REWORK] };

describe("the next actions a read offers", () => {
  it("are each edge's label, destination, inputs and why it is unavailable", () => {
    expect(OFFER.snapshot).toEqual((CASES.samples.nextActions as { snapshot: unknown }).snapshot);
    expect(OFFER.actions).toEqual([
      {
        edge: "pass",
        operation: "approve",
        label: "Pass review",
        destination: { phase: "verify", state: "awaiting" },
        responsible: [{ kind: "task_role", role: "reviewer" }],
        inputs: [],
        unavailable: ["unauthorized"],
      },
      {
        edge: "rework",
        operation: "changes",
        label: "Send back",
        destination: { phase: "build", state: "working" },
        responsible: [{ kind: "project_capability", capability: "owner" }],
        inputs: ["reason"],
        unavailable: [],
      },
    ]);
    expect(OFFER.takes).toEqual([]);
  });

  it("include an apply's answer, which may offer none and takes no output", () => {
    const next = readOffer((CASES.samples.applied as { next: unknown }).next);
    expect(next?.actions).toEqual([]);
    expect(next?.takes).toEqual([]);
    expect(next?.snapshot.task_revision).toBe(8);
  });

  it("name the accepted output standing for each task an edge takes, or none", () => {
    expect(TAKING.takes).toEqual([{ task: "w/a", output: "w/a@3" }]);
    const sample = CASES.samples.takingActions as object;
    const none = readOffer({ ...sample, takes: [{ task: "w/c", output: null }] });
    expect(none?.takes).toEqual([{ task: "w/c", output: null }]);
    expect(readOffer({ ...sample, takes: [{ task: "w/a" }] })).toBeNull();
    expect(readOffer({ ...sample, takes: [{ task: 1, output: null }] })).toBeNull();
    expect(readOffer({ ...sample, takes: [{ task: "w/a", output: 3 }] })).toBeNull();
    expect(readOffer({ ...sample, takes: "w/a" })).toBeNull();
    expect(takenWords({ task: "w/a", output: "w/a@3" })).toBe("Takes w/a's accepted output w/a@3");
    expect(takenWords({ task: "w/c", output: null })).toBe(
      "Takes w/c's accepted output: none stands",
    );
  });

  it("are none for a read the page cannot draw, and pass what it does not read", () => {
    const sample = CASES.samples.nextActions as { snapshot: object; actions: object[] };
    const [first] = sample.actions;
    const changed = (action: object) => ({ ...sample, actions: [{ ...first, ...action }] });
    expect(readOffer(changed({ label: 3 }))).toBeNull();
    expect(readOffer(changed({ operation: null }))).toBeNull();
    expect(readOffer(changed({ destination: { phase: "verify" } }))).toBeNull();
    expect(readOffer(changed({ responsible: [{ kind: "task_role" }] }))).toBeNull();
    expect(readOffer(changed({ required_inputs: [1] }))).toBeNull();
    expect(readOffer(changed({ unavailable: "unauthorized" }))).toBeNull();
    expect(readOffer({ ...sample, snapshot: { ...sample.snapshot, at: null } })).toBeNull();
    expect(readOffer({ ...sample, actions: null })).toBeNull();
    expect(readOffer(null)).toBeNull();
    expect(
      readOffer(changed({ newer: true, unavailable: ["a_newer_code"] }))?.actions,
    ).toHaveLength(1);
  });
});

describe("the board's workflow on a task's page", () => {
  it("names the project and labels the phases, states and places", () => {
    expect(BOARD?.project).toBe("symphony");
    expect(BOARD?.taking).toEqual(new Set(["accept"]));
    expect(BOARD?.names.get("phase:review")).toBe("Review");
    expect(placeWords(BOARD?.names as Map<string, string>, AT)).toBe("Review · Awaiting review");
  });

  it("is none where no workflow is active or its read is not one the page draws", () => {
    expect(readBoard(CASES.samples.noWorkflow)).toBeNull();
    expect(readBoard({ ...(CASES.samples.workflow as object), project: 1 })).toBeNull();
  });

  it("says a place it has no label for by its phase's and state's, else by its IDs", () => {
    const names = new Map([
      ["phase:review", "Review"],
      ["state:awaiting", "Awaiting"],
    ]);
    expect(placeWords(names, { phase: "review", state: "awaiting" })).toBe("Review · Awaiting");
    expect(placeWords(new Map(), { phase: "second_review", state: "in_progress" })).toBe(
      "second review · in progress",
    );
  });
});

describe("an unavailable action", () => {
  it("says why and whom it needs: a role on the task or a capability", () => {
    expect(unavailableWords(PASS, "w/b")).toBe("not the owner's to take: needs the reviewer");
    expect(unavailableWords(REWORK, "w/b")).toBe("");
    expect(
      whose([
        { kind: "task_role", role: "executor" },
        { kind: "system_capability", capability: "land_tasks" },
      ]),
    ).toBe("the executor or the land tasks capability");
  });

  it("leaves its guards to their rows, and is its guards' alone when nothing else holds it", () => {
    expect(unavailableWords(HELD, "w/b")).toBe("waits on its guard: needs the reviewer");
    expect(unavailableWords(HELD, "w/b", true)).toBe("");
    const both = { ...PASS, unavailable: ["guard_unavailable", "unauthorized"] };
    expect(unavailableWords(both, "w/b", true)).toBe("not the owner's to take: needs the reviewer");
    expect([HELD, both, REWORK].map(guardsAlone)).toEqual([true, false, false]);
  });

  it("says a code it does not know once, and no one where none is named", () => {
    const action = { ...PASS, responsible: [], unavailable: ["unsupported_writer", "newer"] };
    expect(unavailableWords(action, "w/b")).toBe("not supported here");
    expect(unavailableWords({ ...PASS, unavailable: ["dependency", "capacity"] }, "w/b")).toBe(
      "waits on what it depends on; no slot is free: needs the reviewer",
    );
  });

  it("says the land or an adapter takes an edge of theirs, in place of an explicit action", () => {
    const explicit = { ...PASS, unavailable: ["explicit_action_required"] };
    for (const operation of ["prepare_land", "confirm_land"]) {
      const land = { ...explicit, operation };
      expect(takerWords(land, "w/b")).toBe("the land takes it: just land w/b");
      expect(unavailableWords(land, "w/b")).toBe("the land takes it: just land w/b");
      expect(
        unavailableWords({ ...land, unavailable: ["capacity", ...land.unavailable] }, "w/b"),
      ).toBe("the land takes it: just land w/b; no slot is free: needs the reviewer");
      // Unavailable for another reason alone, it says that reason, as any edge's.
      expect(unavailableWords({ ...land, unavailable: ["capacity"] }, "w/b")).toBe(
        "no slot is free: needs the reviewer",
      );
      // Held by a guard as well, whose row says it: the line says who takes it, and only that.
      const guarded = { ...land, unavailable: ["guard_unavailable", ...land.unavailable] };
      expect(unavailableWords(guarded, "w/b", true)).toBe("the land takes it: just land w/b");
      expect(unavailableWords(guarded, "w/b")).toBe(
        "the land takes it: just land w/b; waits on its guard: needs the reviewer",
      );
    }
    for (const operation of ["prepare_external", "confirm_external"]) {
      expect(unavailableWords({ ...explicit, operation }, "w/b")).toBe("its adapter takes it");
    }
    expect(takerWords(PASS, "w/b")).toBeNull();
    expect(unavailableWords(explicit, "w/b")).toBe("needs an explicit action: needs the reviewer");
  });
});

describe("a task waiting on a recovery by hand", () => {
  it("says whose exit left it and of which role", () => {
    expect(recoveryWords({ trigger: "lost_holder", holder: "sable-finch", role: "reviewer" })).toBe(
      "sable-finch, its reviewer, was lost: it waits on you to move it on",
    );
    expect(recoveryWords({ trigger: "release", holder: "amber", role: "executor" })).toBe(
      "amber left its executor: it waits on you to move it on",
    );
  });
});

describe("the request an action posts", () => {
  it("names its edge, the snapshot it was offered under and each input as text, trimmed", () => {
    const values = { rework: { reason: "  the test is red  " } };
    expect(requestOf(BOARD, "w/b", OFFER, REWORK, values)).toEqual({
      project: "symphony",
      task: "w/b",
      edge: "rework",
      expected: OFFER.snapshot,
      input_revisions: {},
      inputs: { reason: { kind: "text", value: "the test is red" } },
    });
  });

  it("names the accepted output standing for each task an edge depending on them takes", () => {
    expect(requestOf(BOARD, "w/d", TAKING, ACCEPT, {}).input_revisions).toEqual({
      "w/a": "w/a@3",
    });
    // One none stands for is not named: the server says the edge waits on it.
    const some = { ...TAKING, takes: [...TAKING.takes, { task: "w/c", output: null }] };
    expect(requestOf(BOARD, "w/d", some, ACCEPT, {}).input_revisions).toEqual({
      "w/a": "w/a@3",
    });
    // Another edge leaving the same place names none, which the server refuses as stale.
    const other = { ...ACCEPT, edge: "block" };
    expect(requestOf(BOARD, "w/d", TAKING, other, {}).input_revisions).toEqual({});
  });

  it("waits for each input it needs", () => {
    expect(missing(REWORK, {})).toBe(true);
    expect(missing(REWORK, { rework: { reason: "   " } })).toBe(true);
    expect(missing(REWORK, { pass: { reason: "why" } })).toBe(true);
    expect(missing(REWORK, { rework: { reason: "why" } })).toBe(false);
    expect(missing(PASS, {})).toBe(false);
  });

  it("keeps its key while its answer is not known, and takes a new one for another", () => {
    const key = newKey();
    expect(key).toMatch(/^[0-9a-f]{32}$/);
    expect(newKey()).not.toBe(key);
    expect(keyFor("a", { body: "a", key })).toBe(key);
    expect(keyFor("b", { body: "a", key })).not.toBe(key);
    expect(keyFor("a", null)).toMatch(/^[0-9a-f]{32}$/);
  });
});

describe("a failed request", () => {
  it("is stale when the server's own words begin with a stale code", () => {
    expect(isStale(new Failure("known", "x", "stale_task: the task moved"))).toBe(true);
    expect(isStale(new Failure("known", "x", "stale_activation: another"))).toBe(true);
    expect(isStale(new Failure("known", "x", "unauthorized: stale_task"))).toBe(false);
    expect(isStale(new Error("stale_task"))).toBe(false);
  });

  it("may have been taken unless the server answered it", () => {
    expect(unknownOutcome(new Failure("uncertain", "no answer"))).toBe(true);
    expect(unknownOutcome(new TypeError("failed to fetch"))).toBe(true);
    expect(unknownOutcome(new Failure("known", "refused"))).toBe(false);
    expect(unknownOutcome(new Failure("missing", "no task"))).toBe(false);
  });
});

describe("reading the actions", () => {
  const now = async () => {};

  it("reads the board's workflow and the task's actions under the route's checkout", async () => {
    const asked = server();
    const read = readActions("design~policy", "w/b", new AbortController().signal, now);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/@design~policy/api/workflow");
    asked[0]?.answer(reply(200, CASES.samples.workflow));
    await asking(asked, 2);
    expect(asked[1]?.url).toBe("/@design~policy/api/workflow-actions?slug=w%2Fb");
    asked[1]?.answer(reply(200, CASES.samples.nextActions));
    const answer = (await read) as Read;
    expect(answer.board?.project).toBe("symphony");
    expect(answer.offer).toEqual(OFFER);
    expect(answer.refused).toBe("");
  });

  it("says a refusal of the actions, and reads again only a read with no answer", async () => {
    const asked = server();
    const read = readActions("", "w/b", new AbortController().signal, now);
    await asking(asked, 1);
    asked[0]?.answer(reply(503, { error: "the board is slow" }));
    await asking(asked, 2);
    asked[1]?.answer(reply(403, { error: "Forbidden: next actions are the local owner's" }));
    await asking(asked, 3);
    expect(asked[2]?.url).toBe("/api/workflow");
    asked[2]?.answer(reply(200, CASES.samples.noWorkflow));
    expect(await read).toEqual({
      board: null,
      offer: null,
      refused: "Forbidden: next actions are the local owner's (the server answered 403)",
      guards: null,
    });
    expect(asked).toHaveLength(3);
  });

  it("reads the task's guards where the workflow declares one or an action says one holds it", async () => {
    const guarded = structuredClone(CASES.samples.workflow) as {
      workflow: { definition: { guards: unknown[] } };
    };
    guarded.workflow.definition.guards = [{ id: "release_signoff", adapter: "release" }];
    expect(readBoard(guarded)?.guarded).toBe(true);
    expect(BOARD?.guarded).toBe(false);
    const asked = server();
    const read = readActions("design~policy", "w/b", new AbortController().signal, now);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, guarded));
    await asking(asked, 2);
    asked[1]?.answer(reply(200, CASES.samples.nextActions));
    await asking(asked, 3);
    expect(asked[2]?.url).toBe("/@design~policy/api/workflow-guards?slug=w%2Fb");
    // A read with no answer is asked again, and it alone.
    asked[2]?.answer(reply(503, { error: "the board is slow" }));
    await asking(asked, 4);
    expect(asked[3]?.url).toBe(asked[2]?.url);
    asked[3]?.answer(reply(200, CASES.samples.taskGuards));
    expect((await read)?.guards).toEqual(GUARDS);

    // The board's read failed for good: an action a guard holds still asks for the guards, whose
    // refusal leaves the actions drawn without them.
    const held = { ...(CASES.samples.nextActions as object), actions: [] as unknown[] };
    held.actions = [
      {
        ...(field(CASES.samples.nextActions, "actions") as object[])[0],
        unavailable: ["guard_unavailable"],
      },
    ];
    const other = readActions("", "w/b", new AbortController().signal, now);
    await asking(asked, 5);
    asked[4]?.answer(reply(400, { error: "no" }));
    await asking(asked, 6);
    asked[5]?.answer(reply(200, held));
    await asking(asked, 7);
    expect(asked[6]?.url).toBe("/api/workflow-guards?slug=w%2Fb");
    asked[6]?.answer(reply(403, { error: "Forbidden" }));
    expect(await other).toMatchObject({ board: null, guards: null, refused: "" });
    expect(asked).toHaveLength(7);
  });

  it("says an answer it cannot read, and is none once the page has gone", async () => {
    const asked = server();
    const read = readActions("", "w/b", new AbortController().signal, now);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.workflow));
    await asking(asked, 2);
    asked[1]?.answer(reply(200, { snapshot: null }));
    expect((await read)?.refused).toBe("the server's answer could not be read");
    const gone = new AbortController();
    const left = readActions("", "w/b", gone.signal, now);
    await asking(asked, 3);
    gone.abort();
    asked[2]?.answer(reply(200, CASES.samples.workflow));
    expect(await left).toBeNull();
    expect(asked).toHaveLength(3);
  });
});

/** The key the `index`th request posted. */
function keyOf(asked: Asked[], index: number): unknown {
  return field(field(asked[index]?.body, "request"), "idempotency_key");
}

describe("taking an action", () => {
  const values = { rework: { reason: "red" } };

  it("posts its edge with the snapshot and a key, and draws the actions it offers after", async () => {
    const asked = server();
    const taken = take("", "w/b", BOARD, OFFER, REWORK, values, null);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/workflow-apply");
    const posted = asked[0]?.body as { slug: string; request: Record<string, unknown> };
    expect(posted.slug).toBe("w/b");
    expect(posted.request).toMatchObject({ edge: "rework", task: "w/b", expected: OFFER.snapshot });
    expect(posted.request.idempotency_key).toMatch(/^[0-9a-f]{32}$/);
    asked[0]?.answer(reply(200, { ok: true, result: CASES.samples.applied }));
    expect(await taken).toMatchObject({ done: true, said: "Send back: done", sent: null });
    expect((await taken).next?.snapshot.task_revision).toBe(8);
  });

  it("prompts a refresh when the task moved, and forgets a request the server refused", async () => {
    const asked = server();
    const taken = take("", "w/b", BOARD, OFFER, REWORK, values, null);
    await asking(asked, 1);
    asked[0]?.answer(reply(400, { error: "stale_task: the task is at revision 8, not 7" }));
    expect(await taken).toEqual({
      done: false,
      next: null,
      said: "Send back was not taken: stale_task: the task is at revision 8, not 7",
      moved: true,
      sent: null,
      guard: null,
    });
  });

  it("keeps the guard a refusal names beside its words, and says it short", async () => {
    const asked = server();
    const taken = take("", "w/b", "symphony", GUARDED, HELD, {}, null);
    await asking(asked, 1);
    const error = "guard_unavailable: release_signoff refused it";
    asked[0]?.answer(reply(400, { error, guard: CASES.samples.guardRefusal }));
    expect(await taken).toEqual({
      done: false,
      next: null,
      said: "Pass review was not taken: release signoff holds it",
      moved: false,
      sent: null,
      guard: { refusal: REFUSAL, said: error },
    });
    // A guard the page cannot read is no guard: the refusal's own words are said.
    const unread = take("", "w/b", "symphony", GUARDED, HELD, {}, null);
    await asking(asked, 2);
    asked[1]?.answer(reply(400, { error, guard: { guard: 7 } }));
    expect(await unread).toMatchObject({
      said: `Pass review was not taken: ${error}`,
      guard: null,
    });
  });

  it("sends a request whose answer is not known again with its key", async () => {
    const asked = server();
    const first = await (async () => {
      const taken = take("", "w/b", BOARD, OFFER, REWORK, values, null);
      await asking(asked, 1);
      asked[0]?.answer(reply(502, { error: "the server restarted" }));
      return taken;
    })();
    expect(first).toMatchObject({ done: false, moved: false });
    const key = keyOf(asked, 0);
    expect(first.sent?.key).toBe(key);
    const again = take("", "w/b", BOARD, OFFER, REWORK, values, first.sent);
    await asking(asked, 2);
    expect(asked[1]?.body).toEqual(asked[0]?.body);
    asked[1]?.answer(reply(200, { ok: true, result: CASES.samples.applied }));
    expect((await again).done).toBe(true);
    const other = take("", "w/b", BOARD, OFFER, REWORK, { rework: { reason: "x" } }, first.sent);
    await asking(asked, 3);
    expect(keyOf(asked, 2)).not.toBe(key);
    asked[2]?.answer(reply(200, { ok: true, result: {} }));
    expect(await other).toMatchObject({ done: true, next: null });
  });
});

describe("the part drawn", () => {
  const view = (props: Partial<NextActionsViewProps> = {}) =>
    NextActionsView({
      slug: "w/b",
      place: AT,
      read: { board: BOARD, offer: OFFER, refused: "", guards: null },
      values: {},
      busy: "",
      checking: false,
      result: "",
      moved: false,
      onWrite: vi.fn(),
      onTake: vi.fn(),
      onRefresh: vi.fn(),
      ...props,
    });
  const button = (label: string) => (element: Found) =>
    element.props.type === "button" && element.props.children === label;
  const markup = (props: Partial<NextActionsViewProps> = {}) => renderToStaticMarkup(view(props));

  it("says where the task stands and each action, an unavailable one with why", () => {
    const drawn = markup();
    expect(drawn).toContain("At <b>Review · Awaiting review</b>");
    expect(drawn).toContain('data-edge="pass"');
    expect(drawn).toContain("to Verify · Awaiting review");
    expect(drawn).toContain("not the owner&#x27;s to take: needs the reviewer");
    expect(elementOf(view(), button("Pass review"))?.props.disabled).toBe(true);
    expect(drawn).not.toContain("Refresh actions");
  });

  it("waits for an action's input, then takes it with what was written", () => {
    expect(elementOf(view(), button("Send back"))?.props.disabled).toBe(true);
    const onWrite = vi.fn();
    const input = (element: Found) => element.type === "input";
    fire(view({ onWrite }), input, "onInput", { currentTarget: { value: "red" } });
    expect(onWrite).toHaveBeenCalledWith("rework", "reason", "red");
    const onTake = vi.fn();
    const ready = view({ values: { rework: { reason: "red" } }, onTake });
    expect(elementOf(ready, button("Send back"))?.props.disabled).toBe(false);
    fire(ready, button("Send back"), "onClick");
    expect(onTake).toHaveBeenCalledWith(REWORK);
  });

  it("holds the actions while the page checks, a post is out or the task moved past them", () => {
    const values = { rework: { reason: "red" } };
    expect(markup({ values, checking: true })).toContain("Checking…");
    const sending = view({ values, busy: "rework" });
    expect(elementOf(sending, button("Sending…"))?.props.disabled).toBe(true);
    const behind = view({ values, place: { ...AT, revision: 8 } });
    expect(elementOf(behind, button("Send back"))?.props.disabled).toBe(true);
    expect(markup({ place: { ...AT, revision: 8 } })).toContain('aria-busy="true"');
  });

  it("prompts a refresh once a refusal says the task moved, and says what an action came to", () => {
    const onRefresh = vi.fn();
    const moved = view({ moved: true, result: "Send back was not taken: stale_task", onRefresh });
    expect(renderToStaticMarkup(moved)).toContain("Send back was not taken: stale_task");
    fire(moved, button("Refresh actions"), "onClick");
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  it("names the accepted outputs an edge takes, and posts them with it", () => {
    const at: WorkflowPlace = { phase: "verify", state: "awaiting", revision: 12 };
    const read = { board: BOARD, offer: TAKING, refused: "", guards: null };
    expect(markup({ place: at, read })).toContain(
      '<ul class="flow-takes" aria-label="Accepted outputs taken"><li>Takes w/a&#x27;s accepted output w/a@3</li></ul>',
    );
    expect(markup()).not.toContain("flow-takes");
    const onTake = vi.fn();
    fire(view({ place: at, read, onTake }), button("Accept"), "onClick");
    expect(onTake).toHaveBeenCalledWith(ACCEPT);
  });

  describe("of a task with guards", () => {
    const read: Read = { board: BOARD, offer: GUARDED, refused: "", guards: GUARDS };
    const refused = { edge: "pass", revision: 7, tried: { refusal: REFUSAL, said: "", at: 0 } };

    it("draws the guards that let it in over the actions, and an action's under it", () => {
      const drawn = markup({ read });
      expect(drawn.indexOf('data-guards="entry"')).toBeLessThan(drawn.indexOf("flow-actions"));
      expect(drawn).toContain('Let in by <a href="#timeline">Submit · revision 7 · ');
      expect(drawn).toContain('aria-label="Pass review needs"');
      expect(drawn).toContain('data-guard="release_signoff" data-state="no word yet"');
      // Its guard is all that holds it and this host asks its authority: the owner's to take.
      expect(elementOf(view({ read }), button("Pass review"))?.props.disabled).toBe(false);
      expect(drawn).not.toContain("waits on its guard");
      // Send back carries no guard, and draws no box.
      expect(drawn.match(/class="guard-box"/g)).toHaveLength(2);
      expect(drawn).toContain("nothing on this page approves");
    });

    it("holds an action whose authority this app cannot ask, and gives no command", () => {
      const authorities = GUARDS.authorities.map((one) => ({ ...one, asked_here: false }));
      const apart: Read = { ...read, guards: { ...GUARDS, authorities } };
      expect(elementOf(view({ read: apart }), button("Pass review"))?.props.disabled).toBe(true);
      const drawn = markup({ read: apart });
      expect(drawn).toContain('data-state="not from here"');
      expect(drawn).toContain("no plugin of this project answers for its adapter, release");
      expect(drawn).not.toContain("<code>");
      // Something else holds it too: its line says that, the guard's row the guard.
      const both = { ...HELD, unavailable: ["guard_unavailable", "unauthorized"] };
      const held = markup({ read: { ...read, offer: { ...GUARDED, actions: [both] } } });
      expect(held).toContain("not the owner&#x27;s to take: needs the reviewer");
      expect(held).toContain('data-state="no word yet"');
    });

    it("says the last refusal in its guard's row until the task moves", () => {
      const drawn = markup({ read, refused });
      expect(drawn).toContain('data-guard="release_signoff" data-state="refused"');
      expect(drawn).toContain('<a href="/task/w/a">task w/a</a>');
      const moved = markup({ read, refused: { ...refused, revision: 6 } });
      expect(moved).toContain('data-state="no word yet"');
      const other = markup({ read, refused: { ...refused, edge: "rework" } });
      expect(other).toContain('data-state="no word yet"');
    });

    it("draws the actions as they were where the guards could not be read", () => {
      const drawn = markup({ read: { ...read, guards: null } });
      expect(drawn).not.toContain("guard-box");
      expect(drawn).toContain("waits on its guard: needs the reviewer");
      const held = view({ read: { ...read, guards: null } });
      expect(elementOf(held, button("Pass review"))?.props.disabled).toBe(true);
    });
  });

  it("says a resume, a recovery by hand, a refusal and an offer of none", () => {
    const place: WorkflowPlace = {
      phase: "build",
      state: "blocked",
      revision: 2,
      resume: { phase: "build", state: "available" },
      manual: { trigger: "release", holder: "amber", role: "executor" },
    };
    const none = { ...OFFER, snapshot: { ...OFFER.snapshot, task_revision: 2 }, actions: [] };
    const drawn = markup({ place, read: { board: BOARD, offer: none, refused: "", guards: null } });
    expect(drawn).toContain("At <b>Build · Blocked</b> · resumes at Build · Available");
    expect(drawn).toContain("amber left its executor");
    expect(drawn).toContain("No action is open now");
    const refused = markup({
      read: { board: null, offer: null, refused: "Forbidden", guards: null },
    });
    expect(refused).toContain("At <b>review · awaiting</b>");
    expect(refused).toContain("No actions: Forbidden");
    expect(markup({ read: null })).toContain('aria-busy="true"');
  });
});
