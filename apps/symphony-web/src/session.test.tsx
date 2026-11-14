import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  focus,
  type Going,
  GoToSession,
  press,
  readScreen,
  type SessionControl,
  SessionView,
  screen,
  sessionControl,
  spaceOrControl,
  viaParent,
  webAddress,
} from "./session";
import { asking, CASES, reply, server } from "./testing";
import type { AgentScreen } from "./wire";

const SCREEN: AgentScreen = { handle: "amber", pane: "w1:p2", text: "$ just test-rs", read_at: 4 };

/** A control as the cases write it: `{kind: target}`. */
function written(control: SessionControl | null): Record<string, string> | null {
  if (control === null) return null;
  return control.kind === "link" ? { link: control.url } : { [control.kind]: control.pane };
}

describe("a session's web address", () => {
  it("is linked as the Rust links it", () => {
    expect(CASES.webAddress.length).toBeGreaterThan(20);
    for (const [url, linked] of CASES.webAddress)
      expect([url, webAddress(url)]).toEqual([url, linked]);
  });

  it("holds no space or control character Rust names, at each end of their ranges", () => {
    const named = [0x00, 0x09, 0x1f, 0x20, 0x7f, 0x85, 0x9f, 0xa0, 0x1680, 0x2000, 0x200a];
    const more = [0x2028, 0x2029, 0x202f, 0x205f, 0x3000];
    for (const code of [...named, ...more])
      expect([code, spaceOrControl(code)]).toEqual([code, true]);
    const others = [
      0x21, 0x7e, 0xa1, 0x167f, 0x1681, 0x1fff, 0x200b, 0x2027, 0x202a, 0xd800, 0xfeff,
    ];
    for (const code of others) expect([code, spaceOrControl(code)]).toEqual([code, false]);
  });
});

describe("the Go to session control", () => {
  it("is the one the Rust gives each way in, here and on another machine", () => {
    expect(CASES.sessionControl.length).toBeGreaterThan(10);
    for (const [go, local, control, parent] of CASES.sessionControl) {
      expect([go, local, written(sessionControl(go, local))]).toEqual([go, local, control]);
      expect(viaParent(go, "amber")).toBe(parent);
    }
  });
});

describe("a screen read", () => {
  it("is read whole, or not at all", () => {
    expect(readScreen(CASES.samples.agentScreen)).toEqual(SCREEN);
    expect(readScreen({ ...SCREEN, read_at: "4" })).toBeNull();
    expect(readScreen({ handle: "amber", pane: "w1:p2", read_at: 4 })).toBeNull();
    expect(readScreen({ ...SCREEN, pane: null })).toBeNull();
    expect(readScreen({ ...SCREEN, handle: 1 })).toBeNull();
    expect(readScreen(null)).toBeNull();
  });
});

describe("a focus", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("posts the handle under the checkout's scope and says what the server did", async () => {
    const asked = server();
    const done = focus("amber", "wt");
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/@wt/api/agent-focus");
    expect(asked[0]?.body).toEqual({ handle: "amber" });
    asked[0]?.answer(reply(200, CASES.samples.agentFocus));
    expect(await done).toBe("Shown in the Herdr app");
  });

  it("says what failed: the server's words, or an answer without what it said", async () => {
    const asked = server();
    const refused = focus("amber", "");
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/agent-focus");
    asked[0]?.answer(reply(403, { error: "Only on the server's machine" }));
    expect(await refused).toBe("Only on the server's machine");
    const bare = focus("amber", "");
    await asking(asked, 2);
    asked[1]?.answer(reply(200, { said: 1 }));
    expect(await bare).toBe("missing field `said`");
  });
});

describe("a screen", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("reads the handle's pane, the handle encoded, under the checkout's scope", async () => {
    const asked = server();
    const done = screen("a b&c", "wt", new AbortController().signal);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/@wt/api/agent-screen?handle=a%20b%26c");
    asked[0]?.answer(reply(200, CASES.samples.agentScreen));
    expect(await done).toEqual(SCREEN);
  });

  it("says why it is not shown: an unreadable answer, or the server's refusal", async () => {
    const asked = server();
    const unreadable = screen("amber", "", new AbortController().signal);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, { pane: "w1:p2" }));
    expect(await unreadable).toBe("The screen is not readable");
    const refused = screen("amber", "", new AbortController().signal);
    await asking(asked, 2);
    asked[1]?.answer(reply(404, { error: "No such pane" }));
    expect(await refused).toContain("No such pane");
  });
});

/** What a press showed, in order. */
function going(): [Going, unknown[][]] {
  const shown: unknown[][] = [];
  return [
    {
      busy: (busy) => shown.push(["busy", busy]),
      said: (text) => shown.push(["said", text]),
      screen: (read) => shown.push(["screen", read]),
    },
    shown,
  ];
}

describe("a press of Go to session", () => {
  afterEach(() => vi.unstubAllGlobals());
  const live = () => new AbortController().signal;

  it("does nothing while one is on its way, nor for a link the browser follows", async () => {
    const asked = server();
    const [shows, shown] = going();
    await press({ kind: "focus", pane: "p" }, "amber", "", true, live(), shows);
    await press({ kind: "link", url: "https://h" }, "amber", "", false, live(), shows);
    expect(asked).toHaveLength(0);
    expect(shown).toEqual([]);
  });

  it("focuses the pane, busy until the server says what it did", async () => {
    const asked = server();
    const [shows, shown] = going();
    const done = press({ kind: "focus", pane: "w1:p2" }, "amber", "", false, live(), shows);
    await asking(asked, 1);
    expect(shown).toEqual([
      ["busy", true],
      ["said", ""],
    ]);
    asked[0]?.answer(reply(200, CASES.samples.agentFocus));
    await done;
    expect(shown.slice(2)).toEqual([
      ["said", "Shown in the Herdr app"],
      ["busy", false],
    ]);
  });

  it("shows the screen read, or says what failed and keeps the one shown", async () => {
    const asked = server();
    const [shows, shown] = going();
    const read = press({ kind: "screen", pane: "w1:p2" }, "amber", "", false, live(), shows);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.agentScreen));
    await read;
    expect(shown).toEqual([
      ["busy", true],
      ["said", ""],
      ["screen", SCREEN],
      ["busy", false],
    ]);
    shown.length = 0;
    const failed = press({ kind: "screen", pane: "w1:p2" }, "amber", "", false, live(), shows);
    await asking(asked, 2);
    asked[1]?.answer(reply(500, { error: "hc is away" }));
    await failed;
    expect(shown).toEqual([
      ["busy", true],
      ["said", ""],
      ["said", "hc is away (the server answered 500)"],
      ["busy", false],
    ]);
  });

  it("shows nothing more once the page is gone while the server answered", async () => {
    const asked = server();
    for (const control of [
      { kind: "focus", pane: "w1:p2" },
      { kind: "screen", pane: "w1:p2" },
    ] as SessionControl[]) {
      const [shows, shown] = going();
      const mount = new AbortController();
      const done = press(control, "amber", "", false, mount.signal, shows);
      await asking(asked, control.kind === "focus" ? 1 : 2);
      mount.abort();
      asked.at(-1)?.answer(reply(200, CASES.samples.agentScreen));
      await done;
      expect(shown).toEqual([
        ["busy", true],
        ["said", ""],
      ]);
    }
  });
});

describe("the control drawn", () => {
  const view = (control: SessionControl, fields: Partial<Parameters<typeof SessionView>[0]> = {}) =>
    renderToStaticMarkup(
      <SessionView
        control={control}
        parent={null}
        busy={false}
        told=""
        screen={null}
        onPress={() => {}}
        onClose={() => {}}
        {...fields}
      />,
    );

  it("links a web address in a tab of its own", () => {
    expect(view({ kind: "link", url: "https://example.com/s" })).toBe(
      '<span class="go-session" data-go="link"><a class="go-session-link" href="https://example.com/s" target="_blank" rel="noopener noreferrer">Go to session</a></span>',
    );
  });

  it("is a button that says what it shows, held while it is on its way", () => {
    const focused = view({ kind: "focus", pane: "w1:p2" }, { busy: true });
    expect(focused).toContain('data-go="focus"');
    expect(focused).toContain('title="Shows w1:p2 in the Herdr app on this machine"');
    expect(focused).toMatch(/<button[^>]*disabled=""[^>]*>Go to session<\/button>/);
    const screened = view({ kind: "screen", pane: "w1:p2" });
    expect(screened).toContain(`title="Shows w1:p2&#x27;s screen, read only"`);
    expect(screened).not.toContain("disabled");
    expect(screened).toContain('data-size="xs"');
  });

  it("says whose session it goes to, and what the server said", () => {
    const via = view({ kind: "link", url: "https://h" }, { parent: "birch", told: "Shown" });
    expect(via).toContain('<span class="go-session-via">in birch&#x27;s session</span>');
    expect(via).toContain('<span role="status">Shown</span>');
    expect(view({ kind: "link", url: "https://h" })).not.toContain("role=");
  });

  it("shows the screen read, read only, to read again or close", () => {
    const shown = view({ kind: "screen", pane: "w1:p2" }, { screen: SCREEN });
    expect(shown).toContain(">Read the screen again</button>");
    expect(shown).toContain(
      '<span class="session-screen" role="region" aria-label="w1:p2 in amber&#x27;s session, read only"><pre>$ just test-rs</pre>',
    );
    expect(shown).toContain(">Close</button></span></span>");
    const focused = view({ kind: "focus", pane: "w1:p2" }, { screen: SCREEN });
    expect(focused).toContain(">Go to session</button>");
  });

  it("is none where nothing is known of the session, or it has no way in", () => {
    const go = (go: Parameters<typeof GoToSession>[0]["go"]) =>
      renderToStaticMarkup(<GoToSession handle="amber" go={go} local={false} scope="" />);
    expect(go(null)).toBe("");
    expect(go({ handle: "amber", url: "javascript:alert(1)", pane: null })).toBe("");
    expect(go({ handle: "birch", url: null, pane: "w1:p2" })).toContain(
      '<span class="go-session-via">in birch&#x27;s session</span>',
    );
    expect(go({ handle: "amber", url: null, pane: "w1:p2" })).toContain('data-go="screen"');
  });
});
