# A component's own read, weighed

A probe, not a client: what each way of letting a component read and redraw its own data adds to
a React page's first visit. What it found is
`docs/analysis/components-draw-when-their-data-arrives.md`. Nothing the Symphony builds, tests,
ships or serves reads this folder.

One React page per way, each drawing the same two components (the board's counts and one
column's cards), each component from a read of its own: its copy from the device first, its read
naming that copy's tag, its value replaced when the feed names its key.

| page | the way |
|---|---|
| `baseline` | the page read as a whole: one effect reads both, nothing drawn until both answered |
| `store` | a keyed store of the client's own (`useSyncExternalStore`): one read a key, a failed read tried again, a value the feed pushes drawn at once |
| `suspense` | React's own `use()` under a Suspense boundary a component, a transition for the fresh read |
| `swr` | SWR, its fetcher the client's read |
| `tanstack` | TanStack Query, its `queryFn` the client's read, the device's copy as placeholder data |
| `tanstack-persist` | TanStack Query with its own persistence in place of the device cache |

`src/shared.jsx` holds what every page shares with the client as it is (the read naming a tag, the
device's store, the feed's socket), so a page's bytes beyond the baseline's are its way of joining
them. Each is built alone by Vite with the web client's settings, and its script gzipped at the
best level, as the server sends it:

```sh
npm ci
npm run measure
```

The pages are built, not run: no page here asks a server anything.
