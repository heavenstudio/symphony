import type { InputEvent, InputHTMLAttributes, ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { Input } from "./input";

type Props = InputHTMLAttributes<HTMLInputElement>;

describe("the text field", () => {
  it("is drawn as the components draw theirs, with the page's own class after theirs", () => {
    expect(renderToStaticMarkup(<Input id="title" defaultValue="Typed" />)).toBe(
      '<input class="dx-input" id="title" value="Typed"/>',
    );
    expect(renderToStaticMarkup(<Input className="wide" readOnly value="" />)).toContain(
      'class="dx-input wide"',
    );
  });

  it("hears each input, whatever React last saw of its value", () => {
    const changed = vi.fn();
    const field = Input({ value: "", onChange: changed }) as ReactElement<Props>;
    // React's own change event would miss a value a script set before its input event.
    expect(field.props.onChange).toBeUndefined();
    const input = {
      target: { value: "Typed by a script" },
    } as unknown as InputEvent<HTMLInputElement>;
    field.props.onInput?.(input);
    expect(changed).toHaveBeenCalledWith(input);
  });

  it("hears nothing when the page listens to nothing", () => {
    const field = Input({ defaultValue: "" }) as ReactElement<Props>;
    expect(field.props.onInput).toBeUndefined();
  });
});
