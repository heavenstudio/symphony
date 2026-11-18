# symphony-chrome

Headless Chrome, driven over its DevTools protocol: one browser per launch, on a profile of its own, leashed so it dies with the process that launched it however that ends. Its page can be left for one in a fresh browser context (`Chrome::fresh_context`), which holds no cookie, storage or cache entry of the pages before: a first visit again, without another Chrome started. A program that launches one runs `leash_main()` first, as the leash is that program run again. Every wait is bounded by the browser's own work, the CPU its processes spend and the turns its page takes, so a loaded machine slows a wait without failing it; a wall-clock backstop several times the allowance still ends a wait that stalls.

It is what Symphony's `symphony shot` and its client's browser tests drive. Symphony is a document, review and task workspace.

Its version, below 1.0, is the product's: `symphony-app`, `symphony-board`, `symphony-server`, `symphony-chrome` and `symphony-api` carry one version, require each other at exactly that version, and promise no name: any release may change one.

MIT licensed: the terms are in the LICENSE file beside this README.
