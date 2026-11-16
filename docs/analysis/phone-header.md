# The header on a phone, in one row

A smaller header for a phone, as proposed on the card **phone-header-one-row** in
[DECISIONS](../DECISIONS.md). The header today is two rows on a phone, as [The header](header-names.md#on-a-phone)
describes; this would make it one.

## The proposal

At 760 px wide or less (the width at which the header takes its phone layout today) the header is one
row, 53 px tall where today's two rows are 97 px:

- the fermata and the name, as today;
- a menu button, the ☰ sign and the name of the page shown (Board on the board and on a task,
  Docs on a document, a project's own page by its label, Pages where no page is lit);
- the ◑ Appearance button;
- the bell, without its word: Activity stays its name for a screen reader, and the unread dot
  stays on it.

The button opens a list under it, over the page, so nothing on the page moves: Projects first (on
the machine's server, where it is drawn today), then the pages in today's order, the page shown lit
as in today's row. Enter or a tap opens it, Tab moves into it from the button, Escape or a tap
anywhere else closes it, and Escape gives the focus back to the button. A link followed closes it.

It is the browser's own popover (`popover` on the list, `popovertarget` on the button), which
closes on Escape and on a tap outside with no script of the client's. So the server's own pages
(the project list, a missing page, a file no entrypoint serves), which carry no script, draw the
same menu. The list hangs from the button by CSS anchor positioning, and below the header where a
browser has none.

At desk width nothing changes: the button is not drawn and the row of pages is today's. A theme
that picks the compact header (`[theme.layouts] header = "compact"`) keeps its one row of pages at
every width.

## How the pictures were made

On 2026-11-16, at main 212a1c6f, with `just symphony-cli shot --phone` (390 wide at 3x) against
two private servers over one scratch copy of this repository's board: one serving main's client,
one serving a throwaway build of the proposal (the frame's markup and its phone rules changed in
a worktree, built into a scratch folder, never committed). A private server has no project list,
so it draws no Projects link: today's pictures have it added to the page before the shot, and
the throwaway build draws it as the machine's server does. Each picture is cropped to the top of
the screen.

Measured on those builds:

| | Today | One row |
|---|---|---|
| Header height on a phone, 390 wide | 97 px | 53 px |
| Where the page starts, menu open | | 53 px, as closed |
| Header at 320, 360 and 375 wide | two rows | one row, the name whole, no sideways scroll |
| Header at 1440 wide | 57 px | 57 px, the same pixels |

## What building it touches

- `apps/symphony-web/src/frame.tsx`: the button, the list as the popover, Projects inside it, the
  bell's word kept for screen readers.
- `apps/symphony-web/src/styles/frame.css`, its phone rules, and the copy the server's own pages
  inline (`apps/symphony-server/src/frame.css`).
- `apps/symphony-server/src/pages.rs`: the same markup on the server's own pages.
- The browser tests that read the phone's two rows, and new ones for opening, closing, the focus
  and the page shown; every page's first screen moves up 44 px on a phone, so the tests of the
  ghosts' and the floating controls' places run again.

## Not in this proposal

A header that stays at the top as the page scrolls: on a phone it scrolls away with the page, as
today.
