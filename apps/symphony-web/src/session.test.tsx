/* biome-ignore-all lint/correctness/useHookAtTopLevel: the hooks mocked here answer for a control
 * called outside a render, and call React's own only while React renders. */
import { isValidElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, type Mock, vi } from "vitest";
import { focusSaid, GoToSession, sessionControl, viaParent } from "./session";
import { RoleChips } from "./task-page";
import { aRole, CASES, reply, server } from "./testing";
import type { AgentScreen, SessionGo } from "./wire";

/** The states the next hooks give, in the order a draw asks for them, each with its setter. */
const hooks = vi.hoisted(() => ({
  states: [] as unknown[],
  setters: [] as Mock[],
  signal: null as AbortSignal | null,
}));

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useState: (initial: unknown) => {
      if (hooks.states.length === 0) return actual.useState(initial);
      const set = vi.fn();
      hooks.setters.push(set);
      return [hooks.states.shift(), set];
    },
  };
});

vi.mock("./mount", async (original) => {
  const actual = await original<typeof import("./mount")>();
  return { ...actual, useMount: () => hooks.signal ?? actual.useMount() };
});

afterEach(() => {
  vi.unstubAllGlobals();
  hooks.states = [];
  hooks.setters = [];
  hooks.signal = null;
});

const go = (handle: string, url: string | null, pane: string | null): SessionGo => ({
  handle,
  url,
  pane,
});

/** `cobalt-otter`'s control going by `to`, on a page asked here when `local`. */
const control = (to: SessionGo | null, local: boolean) =>
  renderToStaticMarkup(<GoToSession handle="cobalt-otter" go={to} local={local} />);

describe("a session's control", () => {
  it("links a web address, from this machine and any other", () => {
    for (const local of [true, false]) {
      const html = control(go("cobalt-otter", "https://claude.ai/code/session_01abc", null), local);
      expect(html).toContain(
        '<a class="go-session-link" href="https://claude.ai/code/session_01abc" target="_blank" rel="noopener noreferrer">Go to session</a>',
      );
      expect(html).not.toContain("<button");
    }
  });

  it("focuses a pane on this machine and shows its screen on another", () => {
    const paned = go("cobalt-otter", null, "t7");
    const here = control(paned, true);
    expect(here).toContain('data-go="focus"');
    expect(here).toContain('title="Shows t7 in the Herdr app on this machine"');
    expect(here).toMatch(/>Go to session<\/button>/);
    const phone = control(paned, false);
    expect(phone).toContain('data-go="screen"');
    expect(phone).toContain("screen, read only");
    expect(phone).not.toContain("Herdr app");
  });

  it("with both, focuses the pane here and follows the address from elsewhere", () => {
    const both = go("cobalt-otter", "https://h/x", "t7");
    expect(control(both, true)).toContain('data-go="focus"');
    expect(control(both, false)).toContain('href="https://h/x"');
  });

  it("is none with nothing known", () => {
    for (const local of [true, false]) {
      expect(control(go("cobalt-otter", null, null), local)).toBe("");
      expect(control(null, local)).toBe("");
    }
    expect(renderToStaticMarkup(<GoToSession handle="x" go={undefined} local />)).toBe("");
  });

  it("never links an address that is no web address", () => {
    for (const bad of [
      "javascript:alert(1)",
      "JAVASCRIPT:alert(document.cookie)",
      "data:text/html,x",
      "/api/agent-focus",
      " https://h/x",
    ]) {
      for (const local of [true, false]) {
        expect(control(go("cobalt-otter", bad, null), local), bad).toBe("");
      }
      // With a pane beside it, the pane is the way in, here and from a phone.
      const here = control(go("cobalt-otter", bad, "t7"), true);
      expect(here).toContain('data-go="focus"');
      const phone = control(go("cobalt-otter", bad, "t7"), false);
      expect(phone).toContain('data-go="screen"');
      for (const html of [here, phone]) {
        expect(html, bad).not.toContain("<a");
        expect(html, bad).not.toContain("href");
      }
    }
  });

  it("of a subagent goes to its parent's session and says so", () => {
    const html = control(go("orch-handle", null, "t1"), true);
    expect(html).toContain('data-go="focus"');
    expect(html).toContain('<span class="go-session-via">in orch-handle&#x27;s session</span>');
    expect(control(go("cobalt-otter", null, "t1"), true)).not.toContain("go-session-via");
  });

  it("is the one the Rust gives for each way in", () => {
    const named = (said: ReturnType<typeof sessionControl>) =>
      said === null ? null : [said.kind, said.kind === "link" ? said.url : said.pane];
    for (const [to, local, expected] of CASES.sessions.control) {
      expect(named(sessionControl(to, local)), JSON.stringify([to, local])).toEqual(expected);
    }
    for (const [to, handle, expected] of CASES.sessions.via) {
      expect(viaParent(to, handle), JSON.stringify([to, handle])).toBe(expected);
    }
  });
});

describe("what a focus answered", () => {
  it("is the server's words, else that it said none", () => {
    expect(focusSaid({ said: "Shown in t7" })).toBe("Shown in t7");
    expect(focusSaid({ said: 3 })).toBe("The server's answer has no words");
    expect(focusSaid(null)).toBe("The server's answer has no words");
  });
});

describe("a task's role chips", () => {
  const sessions = {
    amber: go("amber", null, "t7"),
    sub: go("orch", "https://claude.ai/code/s1", null),
  };
  const roles = [
    aRole("executor", "amber"),
    aRole("reviewer", "sub"),
    aRole("orchestrator", "bare"),
  ];

  /** The chip of `name` in `html`, to the next chip. */
  const chip = (html: string, name: string) => {
    const at = html.indexOf(`>${name}`);
    expect(at, `${name}: ${html}`).toBeGreaterThanOrEqual(0);
    const end = html.indexOf('</span><span class="chip', at);
    return html.slice(at, end < 0 ? html.length : end);
  };

  it("each carry their holder's control", () => {
    const html = renderToStaticMarkup(<RoleChips roles={roles} sessions={sessions} local />);
    expect(chip(html, "executor amber")).toContain('data-go="focus"');
    const sub = chip(html, "reviewer sub");
    expect(sub).toContain('href="https://claude.ai/code/s1"');
    expect(sub).toContain("go-session-via");
    expect(chip(html, "orchestrator bare")).not.toContain("Go to session");
    expect(html).toContain('<span class="chip health-live" title="executor is live">');
  });

  it("show a pane's screen from another machine", () => {
    const phone = renderToStaticMarkup(
      <RoleChips roles={roles} sessions={sessions} local={false} />,
    );
    expect(phone).toContain('data-go="screen"');
    expect(phone).not.toContain('data-go="focus"');
  });
});

describe("a session's control clicked", () => {
  const paned = go("cobalt-otter", null, "t7");
  const screen: AgentScreen = {
    handle: "cobalt-otter",
    pane: "t7",
    text: "$ cargo test",
    read_at: 1,
  };

  /** The buttons `node` draws, each by its words. */
  const buttons = (node: ReactNode): Map<string, () => void> => {
    const found = new Map<string, () => void>();
    const walk = (each: ReactNode): void => {
      if (Array.isArray(each)) {
        for (const one of each) walk(one);
        return;
      }
      if (!isValidElement<{ onClick?: () => void; children?: ReactNode }>(each)) return;
      const { onClick, children } = each.props;
      if (typeof onClick === "function" && typeof children === "string") {
        found.set(children, onClick);
      }
      walk(children);
    };
    walk(node);
    return found;
  };

  /**
   * `cobalt-otter`'s control going by `to`, drawn with its words, busy and screen as given, under
   * `mount`: the setter of each, in that order, and its buttons.
   */
  const drawn = (
    to: SessionGo,
    local: boolean,
    mount: AbortController,
    state: { words?: string; busy?: boolean; screen?: AgentScreen | null } = {},
  ) => {
    hooks.signal = mount.signal;
    hooks.setters = [];
    hooks.states = [state.words ?? "", state.busy ?? false, state.screen ?? null];
    const element = GoToSession({ handle: "cobalt-otter", go: to, local });
    const [words, busy, shown] = hooks.setters as [Mock, Mock, Mock];
    return { words, busy, shown, buttons: buttons(element) };
  };

  /** Until `response`'s body is read, then the turn after it: what awaited it has gone on. */
  const read = async (response: Response) => {
    await vi.waitFor(() => expect(response.bodyUsed).toBe(true));
    await new Promise((done) => setTimeout(done, 0));
  };

  /** The server, its answers held, under a task's page. */
  const page = () => {
    vi.stubGlobal("location", { pathname: "/task/visual/in-progress" });
    return server();
  };

  it("here posts the focus for its handle and says what the server answered", async () => {
    const asked = page();
    const control = drawn(paned, true, new AbortController());
    control.buttons.get("Go to session")?.();
    expect(control.busy.mock.calls).toEqual([[true]]);
    expect(control.words.mock.calls).toEqual([[""]]);
    expect(asked.map((one) => [one.url, one.body])).toEqual([
      ["/api/agent-focus", { handle: "cobalt-otter" }],
    ]);
    const answer = reply(200, { said: "Shown in t7" });
    asked[0]?.answer(answer);
    await read(answer);
    expect(control.words.mock.calls).toEqual([[""], ["Shown in t7"]]);
    expect(control.busy.mock.calls).toEqual([[true], [false]]);
  });

  it("here says why a focus failed, and is ready again", async () => {
    const asked = page();
    const control = drawn(paned, true, new AbortController());
    control.buttons.get("Go to session")?.();
    const answer = reply(409, { error: "No Herdr app answers" });
    asked[0]?.answer(answer);
    await read(answer);
    expect(control.words.mock.calls).toEqual([[""], ["No Herdr app answers"]]);
    expect(control.busy.mock.calls).toEqual([[true], [false]]);
  });

  it("does nothing more once its page is gone, whatever the focus answered", async () => {
    const asked = page();
    const mount = new AbortController();
    const control = drawn(paned, true, mount);
    control.buttons.get("Go to session")?.();
    mount.abort();
    const answer = reply(200, { said: "Shown in t7" });
    asked[0]?.answer(answer);
    await read(answer);
    expect(control.words.mock.calls).toEqual([[""]]);
    expect(control.busy.mock.calls).toEqual([[true]]);
  });

  it("asks nothing while busy", () => {
    const asked = page();
    for (const local of [true, false]) {
      const control = drawn(paned, local, new AbortController(), { busy: true });
      control.buttons.get("Go to session")?.();
      expect([control.words.mock.calls, control.busy.mock.calls], `${local}`).toEqual([[], []]);
    }
    expect(asked).toEqual([]);
  });

  it("elsewhere reads the pane's screen and shows it, read only", async () => {
    const asked = page();
    const control = drawn(paned, false, new AbortController());
    control.buttons.get("Go to session")?.();
    expect(asked.map((one) => one.url)).toEqual(["/api/agent-screen?handle=cobalt-otter"]);
    const answer = reply(200, screen);
    asked[0]?.answer(answer);
    await read(answer);
    expect(control.shown.mock.calls).toEqual([[screen]]);
    expect(control.words.mock.calls).toEqual([[""]]);
    expect(control.busy.mock.calls).toEqual([[true], [false]]);
  });

  it("names the handle a screen is asked for as a query's value", () => {
    const asked = page();
    hooks.signal = new AbortController().signal;
    hooks.states = ["", false, null];
    const element = GoToSession({ handle: "a&b c", go: go("a&b c", null, "t7"), local: false });
    buttons(element).get("Go to session")?.();
    expect(asked.map((one) => one.url)).toEqual(["/api/agent-screen?handle=a%26b%20c"]);
  });

  it("says a screen it cannot read, or why the read failed", async () => {
    for (const [answer, words] of [
      [reply(200, { ...screen, read_at: "now" }), "The session's screen is not readable"],
      [reply(200, { handle: "cobalt-otter", pane: "t7" }), "The session's screen is not readable"],
      [reply(404, { error: "No pane t7" }), "No pane t7 (the server answered 404)"],
    ] as const) {
      const asked = page();
      const control = drawn(paned, false, new AbortController());
      control.buttons.get("Go to session")?.();
      asked[0]?.answer(answer);
      await read(answer);
      expect(control.words.mock.calls, words).toEqual([[""], [words]]);
      expect(control.shown).not.toHaveBeenCalled();
      expect(control.busy.mock.calls).toEqual([[true], [false]]);
    }
  });

  it("shows nothing once its page is gone, read or failed", async () => {
    for (const [status, body] of [
      [200, screen],
      [500, { error: "down" }],
    ] as const) {
      const asked = page();
      const mount = new AbortController();
      const control = drawn(paned, false, mount);
      control.buttons.get("Go to session")?.();
      // The page goes while the read is out, and its answer still comes: nothing is shown or said.
      mount.abort();
      const answer = reply(status, body);
      asked[0]?.answer(answer);
      await read(answer);
      expect([control.shown.mock.calls, control.words.mock.calls], `${status}`).toEqual([
        [],
        [[""]],
      ]);
      expect(control.busy.mock.calls).toEqual([[true]]);
    }
  });

  it("with a screen shown reads it again or closes it", () => {
    const asked = page();
    const control = drawn(paned, false, new AbortController(), { screen });
    expect([...control.buttons.keys()]).toEqual(["Read the screen again", "Close"]);
    control.buttons.get("Close")?.();
    expect(control.shown.mock.calls).toEqual([[null]]);
    expect(asked).toEqual([]);
    control.buttons.get("Read the screen again")?.();
    expect(asked.map((one) => one.url)).toEqual(["/api/agent-screen?handle=cobalt-otter"]);
  });

  it("draws its words and the screen it was given", () => {
    vi.stubGlobal("location", { pathname: "/task/visual/in-progress" });
    hooks.signal = new AbortController().signal;
    hooks.states = ["No pane t7", false, screen];
    const html = renderToStaticMarkup(
      GoToSession({ handle: "cobalt-otter", go: paned, local: false }),
    );
    expect(html).toContain('<span role="status">No pane t7</span>');
    expect(html).toContain(
      '<span class="session-screen" role="region" aria-label="t7 in cobalt-otter&#x27;s session, read only"><pre>$ cargo test</pre>',
    );
  });
});
