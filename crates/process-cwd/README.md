# process-cwd

The working directories of running processes, read through `lsof`, for sweeps that must leave a tree some process still stands in; and children started without a fork, read through close-on-exec pipes, with an async wait that kills the child if the wait is dropped. Unix only.

One of [Symphony](https://github.com/heavenstudio/symphony)'s helpers, on a version line of its own from 1.0.0: a new major is the only change that may break a name it exports ([RELEASES](https://github.com/heavenstudio/symphony/blob/main/docs/RELEASES.md#packages-on-cratesio)).

MIT licensed: see [LICENSE](LICENSE).
