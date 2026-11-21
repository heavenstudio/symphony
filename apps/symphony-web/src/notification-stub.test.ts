// The Mac app's one script in a server's pages (apps/symphony-desktop/src/notification.ts), which
// the app puts in each page as it is written, with no build between: TypeScript with no syntax of
// its own, which `tsc` checks here (tsconfig.json) and the browser target's `desktop_stub` runs in
// Chrome.

import { describe, expect, it, vi } from "vitest";
import STUB from "../../symphony-desktop/src/notification.ts?raw";

/** A page's window once the stub ran in it: the API it put in the page's own place. */
function stubbed(): { Notification: typeof Notification } {
  const window = {} as { Notification: typeof Notification };
  new Function("window", STUB)(window);
  return window;
}

describe("the Mac app's notification stub", () => {
  it("is plain JavaScript, so a page runs it as written", () => {
    expect(() => new Function(STUB)).not.toThrow();
  });

  it("reads as denied, asked with a callback or without", async () => {
    const { Notification } = stubbed();
    expect(Notification.permission).toBe("denied");
    const answer = vi.fn();
    await expect(Notification.requestPermission(answer)).resolves.toBe("denied");
    expect(answer).toHaveBeenCalledWith("denied");
    await expect(Notification.requestPermission()).resolves.toBe("denied");
  });

  it("refuses a notice as an unsupported browser does", () => {
    const { Notification } = stubbed();
    expect(() => new Notification("Waiting on you")).toThrow(
      new TypeError("Notifications are the Symphony app's own"),
    );
  });

  it("leaves the page free to put an API of its own in its place", () => {
    const window = stubbed();
    const own = Object.getOwnPropertyDescriptor(window, "Notification");
    expect(own).toMatchObject({ writable: true, configurable: true, enumerable: false });
  });
});
