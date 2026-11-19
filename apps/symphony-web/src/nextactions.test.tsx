import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  type Action,
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
  unavailableWords,
  unknownOutcome,
  whose,
} from "./nextactions";
import { Failure } from "./request";
import { field } from "./shared";
import { type Asked, asking, CASES, elementOf, type Found, fire, reply, server } from "./testing";
import type { WorkflowPlace } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
});

const OFFER = readOffer(CASES.samples.nextActions) as Offer;
const BOARD = readBoard(CASES.samples.workflow);
const [PASS, REWORK] = OFFER.actions as [Action, Action];
const AT: WorkflowPlace = { phase: "review", state: "awaiting", revision: 7 };

describe("the next actions a read offers", () => {
  it("are each edge's label, destination, inputs and why it is unavailable", () => {
    expect(OFFER.snapshot).toEqual((CASES.samples.nextActions as { snapshot: unknown }).snapshot);
    expect(OFFER.actions).toEqual([
      {
        edge: "pass",
        label: "Pass review",
        destination: { phase: "verify", state: "awaiting" },
        responsible: [{ kind: "task_role", role: "reviewer" }],
        inputs: [],
        unavailable: ["unauthorized"],
      },
      {
        edge: "rework",
        label: "Send back",
        destination: { phase: "build", state: "working" },
        responsible: [{ kind: "project_capability", capability: "owner" }],
        inputs: ["reason"],
        unavailable: [],
      },
    ]);
  });

  it("include an apply's answer, which may offer none", () => {
    const next = readOffer((CASES.samples.applied as { next: unknown }).next);
    expect(next?.actions).toEqual([]);
    expect(next?.snapshot.task_revision).toBe(8);
  });

  it("are none for a read the page cannot draw, and pass what it does not read", () => {
    const sample = CASES.samples.nextActions as { snapshot: object; actions: object[] };
    const [first] = sample.actions;
    const changed = (action: object) => ({ ...sample, actions: [{ ...first, ...action }] });
    expect(readOffer(changed({ label: 3 }))).toBeNull();
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
    expect(unavailableWords(PASS)).toBe("not the owner's to take: needs the reviewer");
    expect(unavailableWords(REWORK)).toBe("");
    expect(
      whose([
        { kind: "task_role", role: "executor" },
        { kind: "system_capability", capability: "land_tasks" },
      ]),
    ).toBe("the executor or the land tasks capability");
  });

  it("says a code it does not know once, and no one where none is named", () => {
    const action = { ...PASS, responsible: [], unavailable: ["unsupported_writer", "newer"] };
    expect(unavailableWords(action)).toBe("not supported here");
    expect(unavailableWords({ ...PASS, unavailable: ["dependency", "capacity"] })).toBe(
      "waits on what it depends on; no slot is free: needs the reviewer",
    );
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
    expect(requestOf("symphony", "w/b", OFFER, REWORK, values)).toEqual({
      project: "symphony",
      task: "w/b",
      edge: "rework",
      expected: OFFER.snapshot,
      input_revisions: {},
      inputs: { reason: { kind: "text", value: "the test is red" } },
    });
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
    });
    expect(asked).toHaveLength(3);
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
    const taken = take("", "w/b", "symphony", OFFER, REWORK, values, null);
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
    const taken = take("", "w/b", "symphony", OFFER, REWORK, values, null);
    await asking(asked, 1);
    asked[0]?.answer(reply(400, { error: "stale_task: the task is at revision 8, not 7" }));
    expect(await taken).toEqual({
      done: false,
      next: null,
      said: "Send back was not taken: stale_task: the task is at revision 8, not 7",
      moved: true,
      sent: null,
    });
  });

  it("sends a request whose answer is not known again with its key", async () => {
    const asked = server();
    const first = await (async () => {
      const taken = take("", "w/b", "symphony", OFFER, REWORK, values, null);
      await asking(asked, 1);
      asked[0]?.answer(reply(502, { error: "the server restarted" }));
      return taken;
    })();
    expect(first).toMatchObject({ done: false, moved: false });
    const key = keyOf(asked, 0);
    expect(first.sent?.key).toBe(key);
    const again = take("", "w/b", "symphony", OFFER, REWORK, values, first.sent);
    await asking(asked, 2);
    expect(asked[1]?.body).toEqual(asked[0]?.body);
    asked[1]?.answer(reply(200, { ok: true, result: CASES.samples.applied }));
    expect((await again).done).toBe(true);
    const other = take(
      "",
      "w/b",
      "symphony",
      OFFER,
      REWORK,
      { rework: { reason: "x" } },
      first.sent,
    );
    await asking(asked, 3);
    expect(keyOf(asked, 2)).not.toBe(key);
    asked[2]?.answer(reply(200, { ok: true, result: {} }));
    expect(await other).toMatchObject({ done: true, next: null });
  });
});

describe("the part drawn", () => {
  const view = (props: Partial<NextActionsViewProps> = {}) =>
    NextActionsView({
      place: AT,
      read: { board: BOARD, offer: OFFER, refused: "" },
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

  it("says a resume, a recovery by hand, a refusal and an offer of none", () => {
    const place: WorkflowPlace = {
      phase: "build",
      state: "blocked",
      revision: 2,
      resume: { phase: "build", state: "available" },
      manual: { trigger: "release", holder: "amber", role: "executor" },
    };
    const none = { ...OFFER, snapshot: { ...OFFER.snapshot, task_revision: 2 }, actions: [] };
    const drawn = markup({ place, read: { board: BOARD, offer: none, refused: "" } });
    expect(drawn).toContain("At <b>Build · Blocked</b> · resumes at Build · Available");
    expect(drawn).toContain("amber left its executor");
    expect(drawn).toContain("No action is open now");
    const refused = markup({ read: { board: null, offer: null, refused: "Forbidden" } });
    expect(refused).toContain("At <b>review · awaiting</b>");
    expect(refused).toContain("No actions: Forbidden");
    expect(markup({ read: null })).toContain('aria-busy="true"');
  });
});
