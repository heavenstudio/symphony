import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  type Asked,
  AttemptRow,
  AttemptsView,
  type AttemptsViewProps,
  askedWords,
  bound,
  control,
  controlOf,
  lineage,
  NONE,
  readAttempt,
  readAttempts,
  readControls,
  readHistory,
  short,
  stateWords,
  view,
  viewAddress,
  when,
  words,
} from "./attempts";
import { field, STALE } from "./shared";
import { asking, CASES, type Found, fire, reply, server } from "./testing";
import type { AttemptView } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
  STALE.set(false);
});

const HISTORY = readAttempts(CASES.samples.attempts) as AttemptView[];
const [STOPPED, RUNNING, ELSEWHERE] = HISTORY as [AttemptView, AttemptView, AttemptView];
const A = "a".repeat(32);
const B = "b".repeat(32);

describe("a task's attempts as read", () => {
  it("are the server's, each word and reference kept", () => {
    expect(HISTORY.map((attempt) => attempt.id)).toEqual([A, B, "c".repeat(32)]);
    expect(RUNNING).toEqual({
      id: B,
      created: 1_763_500_000_004,
      revision: 4,
      current: true,
      lane: "executor/amber",
      role: "executor",
      handle: "amber",
      configuration: "cloud",
      predecessor: A,
      relation: "resume",
      successor: null,
      dispatch: "running",
      ending: null,
      held: null,
      cancellation: "none",
      collection: "not_started",
      cleanup: "not_due",
      resource: "res-2",
      generation: "g1",
      conversation: null,
      initiator: "human local/owner",
      controls: {
        cancel: true,
        stop: ["pause", "terminate"],
        resume: [],
        view: true,
        unserved: null,
      },
    });
    expect(ELSEWHERE.handle).toBeNull();
    expect(ELSEWHERE.controls.unserved).toBe("its provider lab is not one this server holds");
  });

  it("default what serde defaults, and keep a word a newer server adds", () => {
    const sample = (CASES.samples.attempts as { attempts: Record<string, unknown>[] }).attempts;
    const bare = { ...sample[1] };
    for (const key of ["handle", "predecessor", "relation", "resource", "generation", "controls"]) {
      delete bare[key];
    }
    expect(readAttempt(bare)).toMatchObject({
      handle: null,
      predecessor: null,
      resource: null,
      controls: NONE,
    });
    expect(readAttempt({ ...bare, dispatch: "migrating" })?.dispatch).toBe("migrating");
    expect(readControls({ cancel: false })).toEqual(NONE);
    expect(readControls({ cancel: true, unserved: null })).toEqual({ ...NONE, cancel: true });
  });

  it("are none for what serde refuses", () => {
    const sample = (CASES.samples.attempts as { attempts: Record<string, unknown>[] }).attempts;
    const changed = (fields: Record<string, unknown>) => readAttempt({ ...sample[1], ...fields });
    expect(changed({ id: 7 })).toBeNull();
    expect(changed({ created: 1.5 })).toBeNull();
    expect(changed({ revision: "4" })).toBeNull();
    expect(changed({ current: "yes" })).toBeNull();
    expect(changed({ handle: 3 })).toBeNull();
    expect(changed({ dispatch: undefined })).toBeNull();
    expect(changed({ controls: null })).toBeNull();
    expect(changed({ controls: { stop: [] } })).toBeNull();
    expect(readControls({ cancel: true, stop: null })).toBeNull();
    expect(readControls({ cancel: true, resume: [1] })).toBeNull();
    expect(readControls({ cancel: true, view: null })).toBeNull();
    expect(readControls({ cancel: true, unserved: 2 })).toBeNull();
    expect(readAttempts({ attempts: [...sample, { id: "x" }] })).toBeNull();
    expect(readAttempts({ attempts: {} })).toBeNull();
    expect(readAttempts(null)).toBeNull();
    expect(readAttempts({ attempts: [] })).toEqual([]);
  });
});

describe("an attempt as said", () => {
  it("names its state, its lineage and what was bound to it, never an address", () => {
    expect(stateWords(STOPPED)).toBe("stopped");
    expect(stateWords(RUNNING)).toBe("running");
    expect(stateWords({ ...STOPPED, ending: null })).toBe("ended");
    expect(stateWords({ ...RUNNING, cancellation: "requested" })).toBe("running, cancel requested");
    expect(lineage(STOPPED)).toBe("followed by bbbbbbbb");
    expect(lineage(RUNNING)).toBe("resume of aaaaaaaa");
    expect(lineage({ ...RUNNING, relation: null, successor: "c".repeat(32) })).toBe(
      "follows of aaaaaaaa · followed by cccccccc",
    );
    expect(lineage(ELSEWHERE)).toBe("");
    expect(bound(STOPPED)).toBe("resource res-1 (g1) · conversation conversation-1");
    expect(bound(ELSEWHERE)).toBe("resource lab-7 · conversation conversation-7");
    expect(bound({ ...ELSEWHERE, resource: null, conversation: null })).toBe("");
    expect(words("reuse_with_new_generation")).toBe("reuse with new generation");
    expect(short(A)).toBe("aaaaaaaa");
    expect(when(1_763_500_000_004)).toBe("2025-11-18 21:06 UTC");
    expect(when(Number.NaN)).toBe("");
    expect(askedWords({ act: "stop", mode: "hard_stop" })).toBe("Stop: hard stop");
    expect(askedWords({ act: "cancel", mode: null })).toBe("Cancel");
  });
});

describe("a control", () => {
  const stop: Asked = { act: "stop", mode: "pause" };
  const resume: Asked = { act: "resume", mode: "new_resource" };

  it("names the attempt's revision as read, and a key for a resume alone, never who asks", () => {
    expect(controlOf("w/a", RUNNING, stop, "k")).toEqual({
      slug: "w/a",
      attempt: B,
      revision: 4,
      act: "stop",
      mode: "pause",
      key: null,
    });
    expect(controlOf("w/a", STOPPED, resume, "k")).toMatchObject({ act: "resume", key: "k" });
  });

  it("posts and draws the history it answers", async () => {
    const asked = server();
    const keys = new Map<string, string>();
    const done = control("wt", "w/a", RUNNING, stop, keys);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/@wt/api/attempt-control");
    expect(Object.keys(asked[0]?.body as object).sort()).toEqual(
      ["act", "attempt", "key", "mode", "revision", "slug"].sort(),
    );
    asked[0]?.answer(reply(200, CASES.samples.attempts));
    expect(await done).toEqual({ attempts: HISTORY, said: "Stop: pause of bbbbbbbb: asked" });
  });

  it("sends a resume again with its key, and a resume of another read with another", async () => {
    const asked = server();
    const keys = new Map<string, string>();
    const first = control("", "w/a", STOPPED, resume, keys);
    await asking(asked, 1);
    asked[0]?.answer(reply(502, { error: "no answer from the provider" }));
    expect(await first).toEqual({
      attempts: null,
      said: "Resume: new resource of aaaaaaaa was not taken: no answer from the provider",
    });
    const key = field(asked[0]?.body, "key");
    expect(key).toMatch(/^[0-9a-f]{32}$/);
    const again = control("", "w/a", STOPPED, resume, keys);
    await asking(asked, 2);
    expect(asked[1]?.body).toEqual(asked[0]?.body);
    asked[1]?.answer(reply(409, { error: "attempt aaaa moved since it was read" }));
    expect((await again).said).toContain("moved since it was read");
    const moved = control("", "w/a", { ...STOPPED, revision: 7 }, resume, keys);
    await asking(asked, 3);
    expect(field(asked[2]?.body, "key")).not.toBe(key);
    asked[2]?.answer(reply(200, { attempts: "unread" }));
    expect(await moved).toEqual({
      attempts: null,
      said: "Resume: new resource of aaaaaaaa: asked",
    });
  });
});

describe("the history read", () => {
  it("is the task's under the route's checkout, or why it is not", async () => {
    const asked = server();
    const reading = new AbortController();
    const read = readHistory("", "w/a b", reading.signal);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/task-attempts?slug=w%2Fa%20b");
    asked[0]?.answer(reply(200, CASES.samples.attempts));
    expect(await read).toEqual({ attempts: HISTORY, refused: "" });
    const refused = readHistory("", "w/a", reading.signal);
    await asking(asked, 2);
    asked[1]?.answer(reply(403, { error: "Forbidden: not this task" }));
    expect((await refused)?.refused).toContain("Forbidden: not this task");
    const unread = readHistory("", "w/a", reading.signal);
    await asking(asked, 3);
    asked[2]?.answer(reply(200, { attempts: [{}] }));
    expect(await unread).toEqual({
      attempts: null,
      refused: "the server's answer could not be read",
    });
  });

  it("is none once the page that asked has stopped", async () => {
    const asked = server();
    const reading = new AbortController();
    const read = readHistory("", "w/a", reading.signal);
    await asking(asked, 1);
    reading.abort();
    asked[0]?.answer(reply(200, CASES.samples.attempts));
    expect(await read).toBeNull();
  });
});

describe("a view", () => {
  const opened = () => ({ location: { replace: vi.fn() }, close: vi.fn() });

  it("is an http or https address alone", () => {
    expect(viewAddress(CASES.samples.attemptLink)).toBe("https://provider.invalid/res-2");
    expect(viewAddress({ url: "http://10.0.0.2/x", valid_for_ms: 1 })).toBe("http://10.0.0.2/x");
    for (const url of ["javascript:alert(1)", "data:text/html,x", "/relative", "not a url"]) {
      expect(viewAddress({ url, valid_for_ms: 1 })).toBeNull();
    }
    expect(viewAddress({ url: "https://p.invalid/" })).toBeNull();
  });

  it("is resolved on demand and sent to the window the click opened alone", async () => {
    const asked = server();
    const window = opened();
    const said = view("", "w/a", RUNNING, window as unknown as Window);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/attempt-view");
    expect(asked[0]?.body).toEqual({ slug: "w/a", attempt: B });
    asked[0]?.answer(reply(200, CASES.samples.attemptLink));
    expect(await said).toBe("");
    expect(window.location.replace).toHaveBeenCalledWith("https://provider.invalid/res-2");
    expect(window.close).not.toHaveBeenCalled();
  });

  it("closes the window when the server refuses it or answers no address", async () => {
    const asked = server();
    for (const [index, answer, why] of [
      [0, reply(409, { error: "its provider is not one this server holds" }), "not one this"],
      [1, reply(200, { url: "javascript:alert(1)", valid_for_ms: 1 }), "no address to open"],
    ] as const) {
      const window = opened();
      const said = view("", "w/a", RUNNING, window as unknown as Window);
      await asking(asked, index + 1);
      asked[index]?.answer(answer);
      expect(await said).toContain(why);
      expect(window.location.replace).not.toHaveBeenCalled();
      expect(window.close).toHaveBeenCalled();
    }
  });

  it("says a window the browser did not open", async () => {
    const asked = server();
    const said = view("", "w/a", RUNNING, null);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.attemptLink));
    expect(await said).toContain("allow this page's pop-ups");
  });
});

describe("the part drawn", () => {
  const props = (fields: Partial<AttemptsViewProps> = {}): AttemptsViewProps => ({
    attempts: HISTORY,
    refused: "",
    busy: false,
    checking: false,
    result: "",
    confirming: null,
    onAsk: vi.fn(),
    onConfirm: vi.fn(),
    onKeep: vi.fn(),
    onView: vi.fn(),
    onRefresh: vi.fn(),
    ...fields,
  });
  const row = (attempt: AttemptView, fields: Partial<AttemptsViewProps> = {}) =>
    AttemptRow({ ...props(fields), attempt });
  const markup = (attempt: AttemptView, fields: Partial<AttemptsViewProps> = {}) =>
    renderToStaticMarkup(row(attempt, fields));
  const button = (label: string) => (element: Found) =>
    element.props.type === "button" && element.props.children === label;

  it("lists the attempts newest first, with their count", () => {
    const drawn = renderToStaticMarkup(AttemptsView(props()));
    expect(drawn).toContain('<span class="count">3</span>');
    const order = [...drawn.matchAll(/data-attempt="(\w)/g)].map((found) => found[1]);
    expect(order).toEqual(["c", "b", "a"]);
    expect(drawn).not.toContain("Reading…");
  });

  it("says a read on its way, a refusal and what a control came to", () => {
    expect(renderToStaticMarkup(AttemptsView(props({ attempts: null })))).toContain("Reading…");
    const refused = renderToStaticMarkup(AttemptsView(props({ attempts: null, refused: "403" })));
    expect(refused).toContain("The attempts could not be read: 403");
    expect(refused).not.toContain("Reading…");
    const done = renderToStaticMarkup(AttemptsView(props({ result: "Cancel of bbbbbbbb: asked" })));
    expect(done).toContain('role="status">Cancel of bbbbbbbb: asked');
  });

  it("draws an attempt's state, lineage, references and the controls it offers", () => {
    const drawn = markup(RUNNING);
    expect(drawn).toContain('data-current="true"');
    expect(drawn).toContain("holds its lane");
    expect(drawn).toContain("executor amber on cloud");
    expect(drawn).toContain("asked by human local/owner");
    expect(drawn).toContain("resume of aaaaaaaa · resource res-2 (g1)");
    for (const label of ["Cancel", "Stop: pause", "Stop: terminate", "View"]) {
      expect(drawn).toContain(`>${label}</button>`);
    }
    expect(drawn).not.toContain("Resume");
    expect(drawn).not.toContain("provider.invalid");
  });

  it("offers a stopped attempt's view alone, and says a provider not held here", () => {
    const stopped = markup(STOPPED);
    expect(stopped).toContain(">View</button>");
    expect(stopped).not.toContain(">Cancel</button>");
    expect(stopped).toContain('data-current="false"');
    const elsewhere = markup(ELSEWHERE);
    expect(elsewhere).toContain(">Cancel</button>");
    expect(elsewhere).not.toContain(">View</button>");
    expect(elsewhere).toContain(
      "No resume or view here: its provider lab is not one this server holds",
    );
    expect(markup({ ...ELSEWHERE, controls: NONE })).not.toContain("attempt-controls");
    expect(markup({ ...STOPPED, held: "the provider asks a person" })).toContain(
      "Waits on a person: the provider asks a person",
    );
  });

  it("asks a control's confirmation first, and views at once", () => {
    const onAsk = vi.fn();
    const onView = vi.fn();
    fire(row(RUNNING, { onAsk, onView }), button("Stop: terminate"), "onClick");
    expect(onAsk).toHaveBeenCalledWith(RUNNING, { act: "stop", mode: "terminate" });
    fire(row(RUNNING, { onAsk, onView }), button("View"), "onClick");
    expect(onView).toHaveBeenCalledWith(RUNNING);
    const asked: Asked = { act: "cancel", mode: null };
    const onConfirm = vi.fn();
    const onKeep = vi.fn();
    const confirming = row(RUNNING, { confirming: { id: B, asked }, onConfirm, onKeep });
    expect(renderToStaticMarkup(confirming)).toContain("Cancel attempt bbbbbbbb?");
    fire(confirming, button("Confirm"), "onClick");
    expect(onConfirm).toHaveBeenCalledWith(RUNNING, asked);
    fire(confirming, button("Keep it"), "onClick");
    expect(onKeep).toHaveBeenCalled();
    // Another attempt's confirmation leaves this one's controls as they were.
    expect(markup(STOPPED, { confirming: { id: B, asked } })).toContain(">View</button>");
  });

  it("holds the controls while a post is out or the page checks", () => {
    const sending = markup(RUNNING, {
      busy: true,
      confirming: { id: B, asked: { act: "cancel", mode: null } },
    });
    expect(sending).toContain("Sending…");
    expect(sending).toMatch(/<button[^>]*disabled=""[^>]*>Sending…/);
    const checking = markup(RUNNING, { checking: true });
    expect(checking).toMatch(/<button[^>]*disabled=""[^>]*>Checking/);
  });
});
