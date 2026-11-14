// The Symphony's button, drawn as the Dioxus components draw theirs (`dx-button`, a class given
// after it), so the style sheets every page shares style it: an outline button unless another
// variant is named, at the default size unless a smaller one is.

import type { ButtonHTMLAttributes } from "react";

export type ButtonVariant = "primary" | "secondary" | "destructive" | "outline" | "ghost" | "link";
export type ButtonSize =
  | "xs"
  | "sm"
  | "default"
  | "lg"
  | "icon"
  | "icon-xs"
  | "icon-sm"
  | "icon-lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

export function Button({
  variant = "outline",
  size = "default",
  className,
  ...attributes
}: ButtonProps) {
  return (
    <button
      className={className === undefined ? "dx-button" : `dx-button ${className}`}
      data-style={variant}
      data-size={size}
      {...attributes}
    />
  );
}
