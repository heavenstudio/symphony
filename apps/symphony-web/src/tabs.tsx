// Tabs drawn as the pinned primitive draws them (dioxus-primitives `tabs`, with the components'
// classes: `dx-tabs`, `dx-tabs-list`, `dx-tabs-trigger`, `dx-tabs-content`): a list of tab buttons
// and the panels they show, one at a time. A click chooses a tab. The arrows along the list, Home
// and End move the focus among the tabs, round the list, without choosing one. One tab is the
// list's stop for Tab: the one focused last, else the chosen one. Disabled, no tab takes the focus.

import {
  createContext,
  type KeyboardEvent,
  type ReactNode,
  useContext,
  useRef,
  useState,
} from "react";

/** The tab the arrows, Home or End move the focus to from `from` (the tab focused last, else none)
 * among `count`, round the list; none for a key that moves nothing. */
export function movedTo(
  key: string,
  horizontal: boolean,
  from: number | null,
  count: number,
): number | null {
  if (count === 0) return null;
  const next = from === null ? 0 : (from + 1) % count;
  const previous = from === null ? count - 1 : (from - 1 + count) % count;
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  if (key === (horizontal ? "ArrowRight" : "ArrowDown")) return next;
  if (key === (horizontal ? "ArrowLeft" : "ArrowUp")) return previous;
  return null;
}

/** The `tabindex` of the tab at `index`: 0 for the list's one stop (the tab focused last, else the
 * chosen one at `chosen`, else the first), -1 for the others and for every tab while disabled. */
export function tabStop(
  index: number,
  recent: number | null,
  chosen: number,
  disabled: boolean,
): 0 | -1 {
  if (disabled) return -1;
  const stop = recent ?? Math.max(chosen, 0);
  return index === stop ? 0 : -1;
}

interface TabsState {
  value: string;
  values: readonly string[];
  choose: (value: string) => void;
  disabled: boolean;
  horizontal: boolean;
  recent: number | null;
  focused: (index: number) => void;
  buttons: Map<number, HTMLButtonElement>;
}

const TabsContext = createContext<TabsState | null>(null);

function useTabs(): TabsState {
  const tabs = useContext(TabsContext);
  if (tabs === null) throw new Error("a tab outside its tabs");
  return tabs;
}

export interface TabsProps {
  /** The chosen tab's value. */
  value: string;
  /** Every tab's value, in the list's order. */
  values: readonly string[];
  onChange: (value: string) => void;
  disabled?: boolean;
  horizontal?: boolean;
  children: ReactNode;
}

export function Tabs({
  value,
  values,
  onChange,
  disabled = false,
  horizontal = false,
  children,
}: TabsProps) {
  const [recent, setRecent] = useState<number | null>(null);
  const buttons = useRef(new Map<number, HTMLButtonElement>()).current;
  return (
    <TabsContext.Provider
      value={{
        value,
        values,
        choose: onChange,
        disabled,
        horizontal,
        recent,
        focused: setRecent,
        buttons,
      }}
    >
      <div
        className="dx-tabs"
        data-variant="default"
        data-orientation={horizontal ? "horizontal" : "vertical"}
        data-disabled={disabled}
      >
        {children}
      </div>
    </TabsContext.Provider>
  );
}

export function TabList({ children }: { children: ReactNode }) {
  return (
    <div role="tablist" className="dx-tabs-list">
      {children}
    </div>
  );
}

export interface TabTriggerProps {
  id: string;
  value: string;
  /** The id of the panel it shows. */
  controls: string;
  children: ReactNode;
}

export function TabTrigger({ id, value, controls, children }: TabTriggerProps) {
  const tabs = useTabs();
  const index = tabs.values.indexOf(value);
  const selected = tabs.value === value;
  const keyed = (event: KeyboardEvent<HTMLButtonElement>) => {
    const to = movedTo(event.key, tabs.horizontal, tabs.recent, tabs.values.length);
    if (to === null) return;
    event.preventDefault();
    tabs.focused(to);
    tabs.buttons.get(to)?.focus();
  };
  return (
    <button
      ref={(button) => {
        if (button) tabs.buttons.set(index, button);
        else tabs.buttons.delete(index);
      }}
      role="tab"
      id={id}
      className="dx-tabs-trigger"
      tabIndex={tabStop(index, tabs.recent, tabs.values.indexOf(tabs.value), tabs.disabled)}
      type="button"
      aria-selected={selected}
      aria-controls={controls}
      data-state={selected ? "active" : "inactive"}
      data-disabled={tabs.disabled}
      disabled={tabs.disabled}
      onClick={() => {
        if (!selected) tabs.choose(value);
      }}
      onFocus={() => tabs.focused(index)}
      onKeyDown={keyed}
    >
      {children}
    </button>
  );
}

export interface TabContentProps {
  id: string;
  value: string;
  /** The id of the tab that names it. */
  labelledBy: string;
  children: ReactNode;
}

/** A tab's panel, its content drawn only while its tab is chosen. */
export function TabContent({ id, value, labelledBy, children }: TabContentProps) {
  const selected = useTabs().value === value;
  return (
    <div
      role="tabpanel"
      id={id}
      className="dx-tabs-content"
      // biome-ignore lint/a11y/noNoninteractiveTabindex: a tab's panel is a stop for Tab, as the pattern's
      tabIndex={0}
      data-state={selected ? "active" : "inactive"}
      hidden={!selected}
      aria-labelledby={labelledBy}
    >
      {selected && children}
    </div>
  );
}
