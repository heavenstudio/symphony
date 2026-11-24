import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AttemptsProps } from "./attempts";
import type { NextActionsProps } from "./nextactions";
import type { OwnerLineProps } from "./owner";
import type { GoToSessionProps } from "./session";
import { AttemptsPart, WorkflowPart } from "./task-page";
import { fire, reading, turn, until } from "./testing";

// React's state and effects stood in for, as no DOM runs here: a part's states are kept by the
// order it asks for them, from one draw to the next, and each effect it sets is kept to be run by
// its test.
const react = vi.hoisted(() => ({
  effects: [] as (() => undefined | (() => void))[],
  states: [] as unknown[],
  at: 0,
}));

vi.mock("react", async (original) => ({
  ...(await original<typeof import("react")>()),
  useEffect: (effect: () => undefined | (() => void)) => {
    react.effects.push(effect);
  },
  useState: (first: unknown) => {
    const at = react.at;
    react.at += 1;
    if (!(at in react.states)) react.states[at] = typeof first === "function" ? first() : first;
    const set = (next: unknown) => {
      react.states[at] = typeof next === "function" ? next(react.states[at]) : next;
    };
    return [react.states[at], set];
  },
}));

// The page's chunks stood in for: each comes as a part that says what it was given, or does
// not come, as its test says; `asked` counts each import.
const chunks = vi.hoisted(() => ({ come: true, asked: 0 }));

/** A chunk whose export `name` draws what it was given, or one that does not come. */
function chunk(name: string) {
  chunks.asked += 1;
  if (!chunks.come) throw new Error("no such chunk");
  return { [name]: (props: unknown) => <i data-part={name}>{JSON.stringify(props)}</i> };
}

/** From here on the chunks come, or do not (`come`): each is imported afresh, as a chunk that came
 * once is kept and would not be asked for again. */
function chunksCome(come: boolean) {
  chunks.come = come;
  vi.doMock("./nextactions", () => chunk("NextActions"));
  vi.doMock("./attempts", () => chunk("Attempts"));
  vi.doMock("./owner-part", () => chunk("OwnerLine"));
  vi.doMock("./session-part", () => chunk("GoToSession"));
  vi.resetModules();
}

/** One draw of `part`: its markup, and the effect it set, which the test runs. */
function draw(part: ReactElement) {
  react.at = 0;
  react.effects.length = 0;
  const html = renderToStaticMarkup(part);
  const [effect] = react.effects;
  if (effect === undefined) throw new Error("The part set no effect");
  return { html, effect };
}

/** The page's shell, which says its build is `gone`; none with no shell. */
function aShell(gone?: boolean) {
  const asked = vi.fn(async () => gone as boolean);
  vi.stubGlobal("window", gone === undefined ? {} : { reviewShell: { gone: asked } });
  return asked;
}

const WORKFLOW = { scope: "", slug: "w/a", updated: 4 } as NextActionsProps;
const ATTEMPTS: AttemptsProps = { scope: "", slug: "w/a", count: 2, updated: 4 };

/** The two parts imported as they are first drawn: each with what it says when it does not come. */
const PARTS = [
  ["NextActions", <WorkflowPart key="w" {...WORKFLOW} />, "The workflow&#x27;s actions"],
  ["Attempts", <AttemptsPart key="a" {...ATTEMPTS} />, "The task&#x27;s attempts"],
] as const;

beforeEach(() => {
  react.states.length = 0;
  chunks.asked = 0;
  chunksCome(true);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("a part of a task's page that is a chunk of its own", () => {
  it("draws nothing until its chunk has come, then the chunk's part with what it was given", async () => {
    for (const [name, part] of PARTS) {
      react.states.length = 0;
      aShell(false);
      const first = draw(part);
      expect([name, first.html]).toEqual([name, ""]);
      first.effect();
      const html = await until(() => {
        const drawn = draw(part).html;
        expect(drawn).toContain(`data-part="${name}"`);
        return drawn;
      });
      expect(html).toContain("&quot;slug&quot;:&quot;w/a&quot;");
      expect(html).not.toContain('role="alert"');
    }
  });

  it("says a chunk that did not come, with its retry, once the shell says the build is there", async () => {
    for (const [name, part, said] of PARTS) {
      react.states.length = 0;
      chunksCome(false);
      const gone = aShell(false);
      draw(part).effect();
      const html = await until(() => {
        const drawn = draw(part).html;
        expect(drawn).toContain('role="alert"');
        return drawn;
      });
      expect([name, html]).toEqual([
        name,
        expect.stringMatching(
          new RegExp(
            `^<p role="alert">${said} could not be loaded\\. <button[^>]*>Retry</button></p>$`,
          ),
        ),
      ]);
      expect(gone).toHaveBeenCalledTimes(1);
      // While it is said, nothing is imported again by itself.
      const asked = chunks.asked;
      draw(part).effect();
      await turn();
      expect(chunks.asked).toBe(asked);
    }
  });

  it("is imported again by its Retry, and drawn once it has come", async () => {
    for (const [name, part] of PARTS) {
      react.states.length = 0;
      chunksCome(false);
      aShell(false);
      draw(part).effect();
      await until(() => expect(draw(part).html).toContain('role="alert"'));
      chunksCome(true);
      react.at = 0;
      fire(
        (part.type as (props: unknown) => ReactElement)(part.props),
        reading("Retry"),
        "onClick",
      );
      const again = draw(part);
      expect([name, again.html]).toEqual([name, ""]);
      again.effect();
      await until(() => expect(draw(part).html).toContain(`data-part="${name}"`));
    }
  });

  it("says a chunk that did not come where no shell runs the page", async () => {
    chunksCome(false);
    aShell();
    const [, part] = PARTS[0];
    draw(part).effect();
    await until(() => expect(draw(part).html).toContain('role="alert"'));
  });

  it("says nothing of a chunk that did not come where the build is gone: the page loads again", async () => {
    chunksCome(false);
    const gone = aShell(true);
    const [, part] = PARTS[0];
    draw(part).effect();
    await until(() => expect(gone).toHaveBeenCalledTimes(1));
    await turn();
    expect(draw(part).html).toBe("");
  });

  it("sets nothing once its page left, whether its chunk came or not", async () => {
    const gone = aShell(false);
    const [, part] = PARTS[1];
    for (const come of [true, false]) {
      react.states.length = 0;
      chunksCome(come);
      const asked = chunks.asked;
      const leave = draw(part).effect();
      leave?.();
      await until(() => expect(chunks.asked).toBe(asked + 1));
      await turn();
      expect([come, draw(part).html]).toEqual([come, ""]);
    }
    expect(gone).not.toHaveBeenCalled();
  });
});

/** A page module of its own, as a chunk once come is kept by the module that asked for it. */
type Page = typeof import("./task-page");

const OWNER: OwnerLineProps = {
  owner: { name: "design", kind: "group", by: "map", said: "from the area app", hand_to: ["bo"] },
  id: "task-hand",
  hand: async () => ({}),
};
const SESSION: GoToSessionProps = {
  handle: "amber",
  go: { handle: "amber", url: null, pane: "t7" },
  local: true,
  scope: "",
};

/** What `element`, a part whose control is a component of its own, draws: that component's tree. */
function inner(element: ReactElement): ReactElement {
  return (element.type as (props: unknown) => ReactElement)(element.props);
}

/** The parts of a task's head imported for a task that has them, each held in its place until its
 * chunk has come: the chunk's export, the part, what it says when it does not come, and the tree
 * that draws its Retry. */
const HEAD_PARTS = [
  [
    "OwnerLine",
    (page: Page) => <page.OwnerPart {...OWNER} />,
    "The task&#x27;s ownership",
    (page: Page) => page.OwnerPart(OWNER),
  ],
  [
    "GoToSession",
    (page: Page) => <page.SessionPart {...SESSION} />,
    "The session&#x27;s control",
    (page: Page) => inner(page.SessionPart(SESSION) as ReactElement),
  ],
] as const;

describe("a part of a task's head that is a chunk of its own", () => {
  it("holds its place while out, says one that did not come with its retry, and is kept once come", async () => {
    for (const [name, element, said, tree] of HEAD_PARTS) {
      react.states.length = 0;
      chunksCome(false);
      aShell(false);
      const page = await import("./task-page");
      const part = element(page);
      const first = draw(part);
      expect([name, first.html]).toEqual([name, expect.stringContaining('class="ghost-field"')]);
      first.effect();
      await until(() =>
        expect(draw(part).html).toMatch(
          new RegExp(
            `^<p role="alert">${said} could not be loaded\\. <button[^>]*>Retry</button></p>$`,
          ),
        ),
      );
      chunksCome(true);
      react.at = 0;
      fire(tree(page), reading("Retry"), "onClick");
      const again = draw(part);
      expect(again.html).toContain("ghost-field");
      again.effect();
      await until(() => expect(draw(part).html).toContain(`data-part="${name}"`));
      // Another task's head, drawn after, draws it at once, and asks for it no more.
      react.states.length = 0;
      const asked = chunks.asked;
      expect(draw(part).html).toContain(`data-part="${name}"`);
      expect(chunks.asked).toBe(asked);
    }
  });

  it("draws and imports nothing for a holder whose session the page names none of", async () => {
    const { SessionPart } = await import("./task-page");
    react.at = 0;
    react.effects.length = 0;
    expect(renderToStaticMarkup(<SessionPart {...SESSION} go={null} />)).toBe("");
    expect(react.effects).toEqual([]);
  });
});
