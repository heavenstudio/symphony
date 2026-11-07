//! The usage and help of the commands that close or bring back agents' terminals (`restore`,
//! `cleanup`, `close`, `reap`), kept beside [`super::COMMANDS`]: each its usage, then its help.

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
