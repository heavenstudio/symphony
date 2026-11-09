//! The usage and help of the commands that close or bring back agents' terminals (`restore`,
//! `cleanup`, `clean`, `close`, `reap`), kept beside [`super::COMMANDS`]: each its usage, then its
//! help.

pub const RESTORE: (&str, &str) = (
    r#"usage: just tasks restore [-h] [--as ACTOR] slug [handle]
"#,
    r#"usage: just tasks restore [-h] [--as ACTOR] slug [handle]

positional arguments:
  slug
  handle

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
);

pub const CLEANUP: (&str, &str) = (
    r#"usage: just tasks cleanup [-h] [--as ACTOR] [--dry-run]
"#,
    r#"usage: just tasks cleanup [-h] [--as ACTOR] [--dry-run]

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --dry-run
"#,
);

pub const CLOSE: (&str, &str) = (
    r#"usage: just tasks close [-h] [--as ACTOR] slug handle
"#,
    r#"usage: just tasks close [-h] [--as ACTOR] slug handle

positional arguments:
  slug
  handle

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
);

pub const CLEAN: (&str, &str) = (
    r#"usage: just tasks clean [-h] [--as ACTOR] [--dry-run]
"#,
    r#"usage: just tasks clean [-h] [--as ACTOR] [--dry-run]

cleanup, then one idle sweep: what closed and why each open terminal stays

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --dry-run   name what it would close and why the rest stay; close nothing
"#,
);

pub const REAP: (&str, &str) = (
    r#"usage: just tasks reap [-h] [--as ACTOR] [--dry-run]
"#,
    r#"usage: just tasks reap [-h] [--as ACTOR] [--dry-run]

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --dry-run   name what the next sweep closes; count nothing
"#,
);
