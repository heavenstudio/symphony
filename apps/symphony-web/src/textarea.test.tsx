import type { InputEvent, ReactElement, TextareaHTMLAttributes } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { Textarea } from "./textarea";

type Props = TextareaHTMLAttributes<HTMLTextAreaElement>;

describe("the text box", () => {
  it("is drawn as the components draw theirs, with the page's own class after theirs", () => {
    expect(renderToStaticMarkup(<Textarea id="note" defaultValue="Typed" />)).toBe(
      '<textarea class="dx-textarea" data-slot="textarea" data-style="default" id="note">Typed</textarea>',
    );
    expect(renderToStaticMarkup(<Textarea className="wide" readOnly value="" />)).toContain(
      'class="dx-textarea wide"',
    );
  });

  it("hears each input, whatever React last saw of its value", () => {
    const changed = vi.fn();
    const box = Textarea({ value: "", onChange: changed }) as ReactElement<Props>;
    // React's own change event would miss a value a script set before its input event.
    expect(box.props.onChange).toBeUndefined();
    const input = {
      target: { value: "Typed by a script" },
    } as unknown as InputEvent<HTMLTextAreaElement>;
    box.props.onInput?.(input);
    expect(changed).toHaveBeenCalledWith(input);
  });

  it("hears nothing when the page listens to nothing", () => {
    const box = Textarea({ defaultValue: "" }) as ReactElement<Props>;
    expect(box.props.onInput).toBeUndefined();
  });
});
