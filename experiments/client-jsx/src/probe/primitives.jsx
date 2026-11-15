// Radix's primitives for every control the Dioxus client takes from dioxus-primitives today
// (`apps/symphony-ui/src/components`), the ones the experiment's pages left inert or drew without
// their popups: a select (a task's priority and the message's recipient, a card's comment kind),
// a dropdown menu (the frame's menus), a dialog (the picture viewer), tabs, a tooltip and a
// checkbox. Each control writes what it did into an `output`, so a check reads the outcome rather
// than the markup.
import { useState } from "react";
import { Checkbox, Dialog, DropdownMenu, Select, Tabs, Tooltip } from "radix-ui";

const PRIORITIES = ["p0", "p1", "p2", "p3"];

export default function Primitives() {
  const [priority, setPriority] = useState("p2");
  const [picked, setPicked] = useState("");
  const [open, setOpen] = useState(false);
  const [closes, setCloses] = useState(0);
  const [tab, setTab] = useState("thread");
  const [checked, setChecked] = useState(false);
  return (
    <Tooltip.Provider delayDuration={0}>
      <section id="primitives" data-ready="true">
        <Select.Root value={priority} onValueChange={setPriority}>
          <Select.Trigger id="priority" aria-label="Priority">
            <Select.Value />
          </Select.Trigger>
          <Select.Portal>
            <Select.Content id="priority-popup" position="popper">
              <Select.Viewport>
                {PRIORITIES.map((value) => (
                  <Select.Item key={value} value={value} data-value={value}>
                    <Select.ItemText>{value}</Select.ItemText>
                  </Select.Item>
                ))}
              </Select.Viewport>
            </Select.Content>
          </Select.Portal>
        </Select.Root>
        <output id="priority-value">{priority}</output>

        <DropdownMenu.Root>
          <DropdownMenu.Trigger id="menu">Menu</DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content id="menu-popup">
              <DropdownMenu.Item data-value="copy" onSelect={() => setPicked("copy")}>
                Copy link
              </DropdownMenu.Item>
              <DropdownMenu.Item data-value="open" onSelect={() => setPicked("open")}>
                Open in a new tab
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
        <output id="menu-value">{picked}</output>

        <Dialog.Root
          open={open}
          onOpenChange={(next) => {
            setOpen(next);
            if (!next) setCloses((n) => n + 1);
          }}
        >
          <Dialog.Trigger id="dialog">Open the picture</Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Overlay />
            <Dialog.Content id="dialog-popup">
              <Dialog.Title>A picture</Dialog.Title>
              <Dialog.Description>The picture viewer's dialog.</Dialog.Description>
              <Dialog.Close id="dialog-close">Close</Dialog.Close>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
        <output id="dialog-closes">{closes}</output>

        <Tabs.Root id="tabs" value={tab} onValueChange={setTab}>
          <Tabs.List aria-label="The task's views">
            <Tabs.Trigger id="tab-brief" value="brief">Brief</Tabs.Trigger>
            <Tabs.Trigger id="tab-thread" value="thread">Thread</Tabs.Trigger>
            <Tabs.Trigger id="tab-history" value="history">History</Tabs.Trigger>
          </Tabs.List>
          <Tabs.Content value="brief">The brief.</Tabs.Content>
          <Tabs.Content value="thread">The thread.</Tabs.Content>
          <Tabs.Content value="history">The history.</Tabs.Content>
        </Tabs.Root>
        <output id="tab-value">{tab}</output>

        <Tooltip.Root>
          <Tooltip.Trigger id="tip">Stage</Tooltip.Trigger>
          <Tooltip.Portal>
            <Tooltip.Content id="tip-popup">In review since an hour</Tooltip.Content>
          </Tooltip.Portal>
        </Tooltip.Root>

        <label>
          <Checkbox.Root id="check" checked={checked} onCheckedChange={(next) => setChecked(next === true)}>
            <Checkbox.Indicator>✓</Checkbox.Indicator>
          </Checkbox.Root>
          Finished only
        </label>
        <output id="check-value">{String(checked)}</output>
      </section>
    </Tooltip.Provider>
  );
}
