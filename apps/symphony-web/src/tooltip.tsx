// A tooltip drawn as the pinned primitive draws it (dioxus-primitives `tooltip`, with the
// components' classes: `dx-tooltip`, `dx-tooltip-trigger`, `dx-tooltip-content`): its tip shows
// while the pointer is over its trigger or the trigger has the focus, and Escape takes it down,
// that key's default prevented, so a panel that closes on Escape stays open for it.

import {
  type FocusEventHandler,
  type KeyboardEvent,
  type MouseEventHandler,
  type ReactNode,
  useId,
  useState,
} from "react";

/** What the trigger is given to carry: its class, its place among the stops for Tab, the tip it
 * names and the events that show and hide the tip. */
export interface TriggerProps {
  className: string;
  tabIndex: number;
  "aria-describedby": string;
  onMouseEnter: MouseEventHandler;
  onMouseLeave: MouseEventHandler;
  onFocus: FocusEventHandler;
  onBlur: FocusEventHandler;
  onKeyDown: (event: KeyboardEvent) => void;
}

export interface TooltipProps {
  /** The side of the trigger the tip opens on. */
  side: "top" | "right" | "bottom" | "left";
  /** The trigger, drawn with what `TriggerProps` gives it. */
  trigger: (props: TriggerProps) => ReactNode;
  children: ReactNode;
}

export function Tooltip({ side, trigger, children }: TooltipProps) {
  const [open, setOpen] = useState(false);
  const tip = useId();
  return (
    <div className="dx-tooltip" data-state={open ? "open" : "closed"} data-disabled={false}>
      {trigger({
        className: "dx-tooltip-trigger",
        tabIndex: 0,
        "aria-describedby": tip,
        onMouseEnter: () => setOpen(true),
        onMouseLeave: () => setOpen(false),
        onFocus: () => setOpen(true),
        onBlur: () => setOpen(false),
        onKeyDown: (event) => {
          if (event.key !== "Escape" || !open) return;
          event.preventDefault();
          setOpen(false);
        },
      })}
      {open && (
        <div
          id={tip}
          role="tooltip"
          className="dx-tooltip-content"
          data-state="open"
          data-side={side}
          data-align="center"
        >
          {children}
        </div>
      )}
    </div>
  );
}
