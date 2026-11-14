# symphony-docs

A project's documents as its docs build checks them, and the Markdown syntax every reader of them shares: lines, headings and anchors, fences, links, components, and card lines with their entries. `check` runs the generic checks and a project's own `DocCheck`s over one reading and names each reference that does not resolve by its `file:line`. `text` holds Python's text rules a reader of the files keeps (whitespace, lines, a file read with universal newlines and written whole), and `markup` the HTML escaping and URL quoting a page's links follow. It builds wherever Rust's standard library does; on Unix the stamp that tells a file changed reads its device, inode and change time as well.

Part of the plugin kit of Symphony, a document, review and task workspace: what a project's own code links to extend it, with `symphony-plugin` and `symphony-blocks`. The three always carry one version, from 1.0.0: a release that breaks a name they export is a new major version, and any other change is a minor or a patch.

MIT licensed: the terms are in the LICENSE file beside this README.
