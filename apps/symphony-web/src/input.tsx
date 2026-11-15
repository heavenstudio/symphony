// The Symphony's text field, drawn as the Dioxus components draw theirs (`dx-input`), so the style
// sheets every page shares style it. Its `onChange` runs on each of the field's `input` events, as
// the text box's does (textarea.tsx): a value a script set before firing `input` is taken like a
// keystroke.

import type { ChangeEvent, InputHTMLAttributes } from "react";

export function Input({
  className,
  onChange,
  ...attributes
}: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={className === undefined ? "dx-input" : `dx-input ${className}`}
      {...attributes}
      onInput={
        onChange === undefined
          ? undefined
          : // The field's own input event, its target the field, as a change event's is.
            (event) => onChange(event as unknown as ChangeEvent<HTMLInputElement>)
      }
    />
  );
}
