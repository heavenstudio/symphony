import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Button } from "./button";
import { answered, failed, NOT_IN_TIME, ready, TRYING, Trouble, UNREACHABLE } from "./trouble";

describe("a read the server answered with an error", () => {
  it("says the server's words, then its status", () => {
    expect(answered(400, "No such card.")).toBe("No such card (the server answered 400)");
    expect(answered(500, "  ")).toBe("The server answered 500");
    expect(answered(502)).toBe("The server answered 502");
  });
});

describe("a read that did not finish", () => {
  it("sends the reader to the connection only when the server could not be reached", () => {
    expect(failed(UNREACHABLE)).toBe(
      "The server could not be reached. Check the connection and try again.",
    );
    expect(failed("No such card (the server answered 400).")).toBe(
      "No such card (the server answered 400).",
    );
    expect(failed(" ")).toBe("The read did not finish. Try again.");
  });

  it("says a page tries it again by itself, and then asks nothing of the reader", () => {
    expect(failed(NOT_IN_TIME, true)).toBe("The server did not answer in time. Trying again…");
    expect(failed(UNREACHABLE, true)).toBe(
      "The server could not be reached; check the connection. Trying again…",
    );
    expect(failed("Busy (the server answered 503).", true)).toBe(
      "Busy (the server answered 503). Trying again…",
    );
    expect(failed(" ", true)).toBe("The read did not finish. Trying again…");
    // A reason that says it goes on by itself says so once: the board's own notice, and a
    // sentence said here before.
    expect(failed("The task board is unavailable. Reconnecting…", true)).toBe(
      "The task board is unavailable. Reconnecting…",
    );
    expect(failed(failed("Busy", true), true)).toBe("Busy. Trying again…");
    // One that does not try again ends its reason's ellipsis.
    expect(failed("Reading…")).toBe("Reading.");
    for (const why of [NOT_IN_TIME, UNREACHABLE, "Busy", ""]) {
      const said = failed(why, true);
      expect(said.endsWith(TRYING), said).toBe(true);
      expect(said.toLowerCase(), said).not.toContain("try again");
    }
  });

  it("says a read got no answer in a read's words, never a write's", () => {
    expect(failed(NOT_IN_TIME)).toBe("The server did not answer in time.");
    expect(NOT_IN_TIME).not.toContain("sending");
  });
});

describe("a region with nothing of its own to show", () => {
  it("is missing, failed, or loading", () => {
    expect(ready(true, true)).toBe("missing");
    expect(ready(false, true)).toBe("failed");
    expect(ready(false, false)).toBe("loading");
  });
});

describe("the trouble block", () => {
  it("says what failed, with its retry and the way back", () => {
    const markup = renderToStaticMarkup(
      <Trouble title="Gone" text="It went." back={["/queue", "Queue"]} retry={() => {}} />,
    );
    expect(markup).toBe(
      '<section class="trouble card" role="status"><h1>Gone</h1><p>It went.</p>' +
        '<p class="trouble-actions"><button class="dx-button" data-style="outline" data-size="default" type="button">Retry</button>' +
        '<a class="trouble-back" href="/queue"><span aria-hidden="true">← </span>Queue</a></p></section>',
    );
  });

  it("offers neither when it has none", () => {
    expect(renderToStaticMarkup(<Trouble title="T" text="t" />)).toContain(
      '<p class="trouble-actions"></p>',
    );
  });
});

describe("a button", () => {
  it("is drawn as the components draw theirs, an outline one at the default size unless named", () => {
    expect(renderToStaticMarkup(<Button variant="ghost" size="sm" disabled />)).toBe(
      '<button class="dx-button" data-style="ghost" data-size="sm" disabled=""></button>',
    );
    expect(renderToStaticMarkup(<Button />)).toBe(
      '<button class="dx-button" data-style="outline" data-size="default"></button>',
    );
  });
});
