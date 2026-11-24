import { describe, expect, it } from "vitest";
import { type SessionControl, sessionControl, viaParent } from "./session-control";
import { CASES } from "./testing";

/** A control as the cases write it: `{kind: target}`. */
function written(control: SessionControl | null): Record<string, string> | null {
  if (control === null) return null;
  return control.kind === "link" ? { link: control.url } : { [control.kind]: control.pane };
}

describe("the Go to session control", () => {
  it("is the one the Rust gives each way in, here and on another machine", () => {
    expect(CASES.sessionControl.length).toBeGreaterThan(10);
    for (const [go, local, control, parent] of CASES.sessionControl) {
      expect([go, local, written(sessionControl(go, local))]).toEqual([go, local, control]);
      expect(viaParent(go, "amber")).toBe(parent);
    }
  });
});
