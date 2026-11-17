# The client's style sheets

`shared.css` is the stylesheet every page loads: it imports `board.css`, `controls-theme.css`,
`frame.css`, `textarea.css`, `input.css`, `button.css` and `select-trigger.css` in the order it
writes, which is the cascade's. `media.css` comes with the media layer and `attach.css` with the
attach control, so neither is in it.

## The copied controls

`button.css`, `input.css`, `textarea.css` and `select-trigger.css` are the sheets of
[DioxusLabs/dioxus-components](https://github.com/DioxusLabs/dioxus-components/tree/9a758255ea26e2b20c8cecf4c1c946feb9e71da7)
at revision `9a758255ea26e2b20c8cecf4c1c946feb9e71da7`, adapted, and keep its class names
(`dx-button`, `dx-input`, `dx-textarea`, `dx-select-trigger`), which the client's controls draw
(`../button.tsx`, `../input.tsx`, `../textarea.tsx`, `../select.tsx`). The MIT and Apache licences
are beside them (`LICENSE-MIT`, `LICENSE-APACHE`). The distribution selects MIT, as upstream's
README permits; `../../copied-controls.txt` records the provenance and adaptations carried with
that exact permission text in each build's `assets/third-party-HASH.txt`. `select-trigger.css`
is the closed field's rules that come before `.dx-select-list` upstream; the popup's are the menus' (`../overlay.css`), with
the upstream dialog's focus trap ported as `trapFocus`.

The adaptations bind radii and fonts to theme tokens and default a button to Outline. A button is
2.25rem tall at the theme's radius with a pointer, as a plain button is (`board.css`), and its
focus is the page's one outline: no control draws a second ring of shadow. A button eases only
what its states change (its colours, shadow, opacity and its press's scale), never `all`. `controls-theme.css`
maps the shadcn registry's `cssVars` names to the palette these sheets read; `../theme.ts` imports
that format at run time.

## Checks

The sheets are formatted and linted with the rest of the package, but `board.css` and `frame.css`
are not formatted: the server inlines its copies of them in its own pages as they are written
(`apps/symphony-server/src/`), a rule a line, and a formatter's lines would be bytes of every such
page. Two style rules are off for this
folder alone (`biome.json`): the order of selectors by specificity and `!important`, as these
sheets were carried whole from the client they were written for, and their rules' order is the
cascade the browser tests hold.
