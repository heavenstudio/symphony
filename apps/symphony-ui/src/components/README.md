# Symphony controls

These sources were installed by the client's dx 0.7.10:

```sh
dx components add button dialog dropdown_menu tabs tooltip input textarea select checkbox label --git https://github.com/DioxusLabs/dioxus-components --rev 9a758255ea26e2b20c8cecf4c1c946feb9e71da7
```

Upstream: [DioxusLabs/dioxus-components](https://github.com/DioxusLabs/dioxus-components/tree/9a758255ea26e2b20c8cecf4c1c946feb9e71da7).
The primitive dependency uses that same revision. The MIT and Apache licences
are included here. All styles, icons and primitive scripts are served locally.
The pinned primitive's `src/js/focus-trap.js` has SHA-256
`0087b120ff13523366376f3c2cd6dca0c79f06e99bdd46f8c1c024dec6b55714`.

The Symphony adaptations bind radii and fonts to theme tokens, default buttons to
Outline, forward only supplied event handlers, and merge plain entry actions
with component attributes. A tab panel has no padding, border or fill of its
own (the demo's themed panel is gone): it is part of its container's surface.
`SelectField` keeps a field's accessible name,
current value and callback while the list is closed. Dropdown actions use
menu/menuitem roles. Closing a popup restores its trigger's focus.
Select fields focus their popup on every opening, commit the focused option
with Enter or Space, and namespace popup IDs by page kind.
Their closed triggers expose combobox name and value. Popups measure before
painting, flip or clamp into the visual viewport, stop at 20rem
(`select/position.rs`'s `MAX_HEIGHT`) and scroll keyboard-focused
options into view. Resize, scroll and focus listeners belong to the popup and
are removed when it unmounts. Select lists and dropdown menus draw on the
opaque `--popover` token with the theme's border and `--shadow-2`; the focused
option or item is their focus indicator, so neither a list nor its options
draw the page's focus ring.
Their styled triggers stay in each entry module. Popup data and an owned callback
cross to the existing deferred panel, which mounts a fresh primitive popup on
each opening. The same boundary carries picture-dialog data; menus and tooltips
already belong to that panel. Only its release feature enables primitives/icons.
Buttons, inputs and textareas use static CSS assets and a small attribute merge,
so entry modules do not enable Dioxus document support through `css_module`.
Their rich event attributes are built at the call site only when supplied;
the shared markup does not retain optional event-conversion branches. Select
triggers own a native key listener that is removed with their scope.
`select/trigger.css` copies the closed-field rules preceding `.dx-select-list`
in the upstream stylesheet. Popup rules load only with the panel. The shell
awaits the panel's stylesheet load events before running it; failed sheets are
removed and requested again on retry, so an overlay never mounts unstyled.
`dialog/focus-trap.js` is an unchanged local copy of the pinned primitive script
with the hash above. Dialogs await its import before mounting open and restore
their previous focus when unmounted.
`assets/dx-components-theme.css` bridges the copied styles
to shadcn registry `cssVars`; `theme.rs` imports that format at run time.

Adapted stylesheet SHA-256 values (paths below this folder, except the theme):

| File | SHA-256 |
|---|---|
| `../../assets/dx-components-theme.css` | `42611fd439c811a3f96155baf5f45b4fa7fd638e2f69aca6281470d9c88e6336` |
| `button/style.css` | `895aa12659309fe9b5f6c3fb202910b9b7de01dae281abf935b153aa792b0fe8` |
| `checkbox/style.css` | `f239c4ba7ced18961b774e873088009cb7414fca6ae754fff5bee4250bb8c1f2` |
| `dialog/style.css` | `5d4b5b0329a97653b634fab6b9e6d5e67fd0fe2c40042dcc28a7f27f4478c9d3` |
| `dropdown_menu/style.css` | `bfe25a8085a82ab96bb3562a7572d24f6ffa2a5cb2baf65543010bd98293d17c` |
| `input/style.css` | `953a21b193c707f671048aa790c4fdcd62751e24cadee6fc927fc9c620e90d7d` |
| `label/style.css` | `0906d269af5d9c84c7a63a8caa60fd7b4e9738d12fa6c80bf718b414cbb539bb` |
| `select/style.css` | `547ad86108f12cf410844c56a93c71bdd45bf41fd3cd0d605750242ce1b26886` |
| `select/trigger.css` | `830d5b55a9fcb9f5a25724c552b626eb5386178583ffa59ffaa5023c5b52cf2a` |
| `tabs/style.css` | `1c4313ed4647752c1d301157ffcbb7af3675f3767d5cbfb3e5d86a703beb35e4` |
| `textarea/style.css` | `c6b25f55e3f05d6b8044d8a537c4d203860642c8547d666a729be9f6a145647f` |
| `tooltip/style.css` | `15a27ad278601841700b7b0dd6445ab78d960110bd9a185e2c598df9d734ac5a` |
