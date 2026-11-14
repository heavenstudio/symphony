// The Symphony's button, drawn as the Dioxus components draw theirs (`dx-button`), so the style
// sheets every page shares style it: an outline button unless another variant is named, at the
// default size unless a smaller one is.

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

export function Button({ variant = "outline", size = "default", ...attributes }: ButtonProps) {
  return <button className="dx-button" data-style={variant} data-size={size} {...attributes} />;
}
