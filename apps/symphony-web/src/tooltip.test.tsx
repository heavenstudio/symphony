import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Tooltip, type TriggerProps } from "./tooltip";

describe("a tooltip", () => {
  it("draws its trigger, carrying what the tip needs, and no tip until it opens", () => {
    let given: TriggerProps | null = null;
    const markup = renderToStaticMarkup(
      <Tooltip
        side="left"
        trigger={(props) => {
          given = props;
          return (
            <button {...props} className={`${props.className} collapse`} type="button">
              ⌄
            </button>
          );
        }}
      >
        Collapse
      </Tooltip>,
    );
    const props = given as unknown as TriggerProps;
    expect(markup).toBe(
      '<div class="dx-tooltip" data-state="closed" data-disabled="false">' +
        `<button class="dx-tooltip-trigger collapse" tabindex="0" aria-describedby="${props["aria-describedby"]}" type="button">⌄</button>` +
        "</div>",
    );
    expect(props["aria-describedby"]).not.toBe("");
  });

  it("lets an Escape through while its tip is closed", () => {
    let given: TriggerProps | null = null;
    renderToStaticMarkup(
      <Tooltip
        side="top"
        trigger={(props) => {
          given = props;
          return null;
        }}
      >
        Tip
      </Tooltip>,
    );
    let prevented = false;
    (given as unknown as TriggerProps).onKeyDown({
      key: "Escape",
      preventDefault: () => {
        prevented = true;
      },
    } as never);
    expect(prevented).toBe(false);
  });
});
