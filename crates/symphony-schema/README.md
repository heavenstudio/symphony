# symphony-schema

Local JSON Schema validation (draft 2020-12) with stable diagnostics and no external reference reads: a schema's `$ref` resolves inside the schema alone, nothing is fetched or read from disk. `text` writes a string's `repr` and an OS error as Python prints them.

One of the helpers of Symphony, a document, review and task workspace. Each helper has a version of its own, from 1.0.0: a release that breaks a name it exports is a new major version, and any other change is a minor or a patch.

MIT licensed: the terms are in the LICENSE file beside this README.
