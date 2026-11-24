import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type Going, GoToSession, pressed, SessionView, UNPRESSED, whiteSpace } from "./session";
import type { SessionControl } from "./session-control";
import { focus, press, readScreen, screen } from "./session-press";
import { asking, CASES, reply, server } from "./testing";
import type { AgentScreen } from "./wire";

const SCREEN: AgentScreen = { handle: "amber", pane: "w1:p2", text: "$ just test-rs", read_at: 4 };

describe("a space as Rust trims one", () => {
  it("is one `char::is_whitespace` names, and no other", () => {
    const spaces = [0x09, 0x0d, 0x20, 0x85, 0xa0, 0x1680, 0x2000, 0x200a, 0x2028, 0x2029];
    for (const code of [...spaces, 0x202f, 0x205f, 0x3000]) {
      expect([code, whiteSpace(code)]).toEqual([code, true]);
    }
    for (const code of [0x00, 0x08, 0x0e, 0x1c, 0x1f, 0x7f, 0x84, 0x9f, 0x180e, 0x200b, 0xfeff]) {
      expect([code, whiteSpace(code)]).toEqual([code, false]);
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

  it("is given up on the wire once its signal ends", async () => {
    const asked = server();
    const mount = new AbortController();
    const done = screen("amber", "", mount.signal);
    await asking(asked, 1);
    expect(asked[0]?.signal?.aborted).toBe(false);
    mount.abort();
    expect(asked[0]?.signal?.aborted).toBe(true);
    asked[0]?.answer(reply(200, CASES.samples.agentScreen));
    await done;
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

describe("a press before what it does has come", () => {
  afterEach(() => vi.unstubAllGlobals());
  const FOCUS: SessionControl = { kind: "focus", pane: "w1:p2" };

  /** What a press does, come as `come` says: a press it records, or none, which fails. */
  function chunk(come: boolean) {
    const made = vi.fn(async () => {});
    const load = vi.fn(async () => {
      if (!come) throw new Error("no such chunk");
      return { press: made };
    });
    return { made, load };
  }

  /** The page's shell, which says its build is `gone`. */
  function aShell(gone: boolean) {
    const asked = vi.fn(async () => gone);
    vi.stubGlobal("window", { reviewShell: { gone: asked } });
    return asked;
  }

  it("asks for nothing while one is on its way, nor for a link the browser follows", async () => {
    const { made, load } = chunk(true);
    const [shows, shown] = going();
    const live = new AbortController().signal;
    await pressed(FOCUS, "amber", "", true, live, shows, load);
    await pressed({ kind: "link", url: "https://h" }, "amber", "", false, live, shows, load);
    expect(load).not.toHaveBeenCalled();
    expect(made).not.toHaveBeenCalled();
    expect(shown).toEqual([]);
  });

  it("is on its way while it comes, then is the press it does", async () => {
    const { made, load } = chunk(true);
    const [shows, shown] = going();
    const live = new AbortController().signal;
    await pressed(FOCUS, "amber", "wt", false, live, shows, load);
    expect(shown).toEqual([["busy", true]]);
    expect(made).toHaveBeenCalledExactlyOnceWith(FOCUS, "amber", "wt", false, live, shows);
  });

  it("says it did not come, and may be pressed again, once the shell says the build is there", async () => {
    const gone = aShell(false);
    const [shows, shown] = going();
    await pressed(
      FOCUS,
      "amber",
      "",
      false,
      new AbortController().signal,
      shows,
      chunk(false).load,
    );
    expect(gone).toHaveBeenCalledOnce();
    expect(shown).toEqual([
      ["busy", true],
      ["busy", false],
      ["said", UNPRESSED],
    ]);
  });

  it("says nothing where the build is gone, and the page loads again", async () => {
    const gone = aShell(true);
    const [shows, shown] = going();
    await pressed(
      FOCUS,
      "amber",
      "",
      false,
      new AbortController().signal,
      shows,
      chunk(false).load,
    );
    expect(gone).toHaveBeenCalledOnce();
    expect(shown).toEqual([["busy", true]]);
  });

  it("does nothing more once the page is gone while it came, or did not", async () => {
    const gone = aShell(false);
    for (const come of [true, false]) {
      const { made, load } = chunk(come);
      const [shows, shown] = going();
      const mount = new AbortController();
      const done = pressed(FOCUS, "amber", "", false, mount.signal, shows, load);
      mount.abort();
      await done;
      expect([come, shown]).toEqual([come, [["busy", true]]]);
      expect(made).not.toHaveBeenCalled();
    }
    expect(gone).not.toHaveBeenCalled();
  });

  it("imports what a press does, by default, and does it", async () => {
    const asked = server();
    const [shows, shown] = going();
    const done = pressed(FOCUS, "amber", "", false, new AbortController().signal, shows);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.agentFocus));
    await done;
    expect(shown).toEqual([
      ["busy", true],
      ["busy", true],
      ["said", ""],
      ["said", "Shown in the Herdr app"],
      ["busy", false],
    ]);
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
