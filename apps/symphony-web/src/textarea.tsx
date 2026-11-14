// The Symphony's text box, drawn as the Dioxus components draw theirs (`dx-textarea`, its default
// look), so the style sheets every page shares style it.
//
// Its `onChange` runs on each of the box's `input` events, as the Dioxus box's `oninput` did, and
// never only when React's own record of the value differs: a value set by a script before it fires
// `input` (an extension filling the box, a test typing in it, the browser restoring a form) is
// taken like a keystroke. React's `onChange` alone would miss it, as it compares the box with the
// value the script set.

import type { ChangeEvent, TextareaHTMLAttributes } from "react";

export function Textarea({
  className,
  onChange,
  ...attributes
}: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      className={className === undefined ? "dx-textarea" : `dx-textarea ${className}`}
      data-slot="textarea"
      data-style="default"
      {...attributes}
      onInput={
        onChange === undefined
          ? undefined
          : // The box's own input event, its target the box, as a change event's is.
            (event) => onChange(event as unknown as ChangeEvent<HTMLTextAreaElement>)
      }
    />
  );
}
