# symphony-docs

A project's documents as its docs build checks them, and the Markdown syntax every reader of them shares: lines, headings and anchors, fences, links, components, and card lines with their entries. `check` runs the generic checks and a project's own `DocCheck`s over one reading and names each reference that does not resolve by its `file:line`.

Part of the plugin kit of Symphony, a document, review and task workspace: what a project's own code links to extend it, with `symphony-plugin` and `symphony-api`. The three always carry one version. Below 1.0 a release that breaks a name they export moves the minor version (0.1 to 0.2), as Cargo reads a 0.x version; the kit moves to 1.0.0 once its blocks have left the app's wire types in `symphony-api`, and from then such a release is a new major version.

MIT licensed: the terms are in the LICENSE file beside this README.
