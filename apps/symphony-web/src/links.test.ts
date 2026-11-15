import { describe, expect, it } from "vitest";
import { appPath, linkable, webAddress } from "./links";
import { CASES } from "./testing";

/**
 * Addresses that would run script, read data in place or leave for another host while looking like
 * a path of the app (crates/symphony-api's links.rs, `REFUSED`).
 */
const REFUSED = [
  "javascript:alert(1)",
  "JaVaScRiPt:alert(1)",
  " javascript:alert(1)",
  "\tjavascript:alert(1)",
  "\u{1}javascript:alert(1)",
  "java\nscript:alert(1)",
  "data:text/html,<script>alert(1)</script>",
  "DATA:text/html;base64,PHNjcmlwdD4=",
  "vbscript:msgbox(1)",
  "//evil.example/x",
  "/\\evil.example/x",
  "/\t/evil.example",
  " /task/x",
  "\u{0}/task/x",
  "task/x",
  "",
  "mailto:owner@example.com",
  "https:///x",
  " https://h",
  "https://h x",
  "ftp://h/x",
];

describe("a link a page draws", () => {
  it("is a path of the app or a web address and nothing else", () => {
    for (const linked of [
      "/task/tools/slot-pool-by-tree",
      "/task/tools/x#message-124",
      "/docs/analysis/x.md#card",
      "/docs/a file.md",
      "/",
      "https://claude.ai/code/session_01abc",
      "http://mac.tail.ts.net:8080/x?y=1#z",
      "HTTPS://Example.com",
    ]) {
      expect(linkable(linked), linked).toBe(true);
    }
    for (const refused of REFUSED) expect(linkable(refused), JSON.stringify(refused)).toBe(false);
  });

  it("is never one a control character past the first ASCII ones, or a line separator, splits", () => {
    for (const refused of ["/x\u{7f}y", "/x\u{85}y", "/x\u{9f}y", "https://h/\u{2028}x"]) {
      expect(linkable(refused), JSON.stringify(refused)).toBe(false);
    }
    expect(linkable("/x\u{a0}y"), "a no-break space is no control character").toBe(true);
    expect(linkable("/docs/é/ünïcode.md")).toBe(true);
  });
});

describe("a path of the app", () => {
  it("has one slash first and no backslash or control character", () => {
    expect(appPath("/task/x")).toBe(true);
    for (const refused of ["//h", "/\\h", "/x\\y", "/x\ny", "/x\ty", "x/y", "https://h"]) {
      expect(appPath(refused), JSON.stringify(refused)).toBe(false);
    }
  });
});

/** Addresses no page may link to, whatever wrote them to the board (agents/tests.rs). */
const UNLINKABLE = [
  "javascript:alert(1)",
  "JavaScript:alert(1)",
  " https://h/x",
  "data:text/html,<script>alert(1)</script>",
  "file:///etc/passwd",
  "vbscript:x",
  "//evil.example/x",
  "/task/x",
  "claude.ai/code/x",
  "https://",
  "https:///path",
  "http://?q",
  "https://a b",
  "https://a\u{7}b",
];

describe("a web address", () => {
  it("is linked as the Rust links it", () => {
    expect(CASES.webAddress.length).toBeGreaterThan(20);
    for (const [url, linked] of CASES.webAddress)
      expect([url, webAddress(url)]).toEqual([url, linked]);
  });

  it("is http or https naming a host with no space or control character", () => {
    for (const linked of [
      "https://claude.ai/code/session_01abc",
      "http://mac.tail.ts.net:8080/x?y=1#z",
      "HTTPS://Example.com",
      "Http://h",
      "https://h?q",
      "https://h#f",
    ]) {
      expect(webAddress(linked), linked).toBe(true);
    }
    for (const refused of [
      ...UNLINKABLE,
      "",
      "https",
      "https:/h",
      "https://h\u{a0}x",
      "ftp://h/x",
    ]) {
      expect(webAddress(refused), JSON.stringify(refused)).toBe(false);
    }
  });

  it("names its scheme in ASCII letters alone, and takes no Unicode space Rust would refuse", () => {
    expect(webAddress("HTTP://h")).toBe(true);
    expect(webAddress("\u{130}ttps://h"), "a dotted capital I is no `h`").toBe(false);
    for (const space of ["\u{2003}", "\u{3000}", "\u{205f}", "\u{1680}", "\u{85}"]) {
      expect(webAddress(`https://h${space}x`), JSON.stringify(space)).toBe(false);
    }
    expect(webAddress("https://h\u{feff}x"), "a byte-order mark is no space to Rust").toBe(true);
  });
});
