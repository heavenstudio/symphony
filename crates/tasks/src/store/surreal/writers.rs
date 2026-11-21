//! The statements of `store/writers.rs`.

/// The opening's one read: the board's schema version, or NONE before its tables are made, the
/// line of the writer `[$host, $path]`, or NONE, or `false` where the table is not there, and how
/// many additive steps on its contract the board has.
pub(in crate::store) const OPENING: &str = "LET $tables = (INFO FOR DB).tables;
RETURN [IF $tables.meta = NONE { NONE } ELSE { (SELECT VALUE version FROM ONLY meta:schema) }, IF $tables.writer = NONE { false } ELSE { (SELECT schema, seen FROM ONLY type::record('writer', [$host, $path])) }, IF $tables.meta = NONE { 0 } ELSE { (SELECT VALUE version FROM ONLY meta:additions) ?? 0 }];";

/// Every writer's line, or none where the table is not there.
pub(in crate::store) const LINES: &str = "RETURN IF (INFO FOR DB).tables.writer = NONE { [] } ELSE { (SELECT program, path, host, schema, seen FROM writer) };";

/// A writer's line, made or replaced.
pub(in crate::store) const WRITE_LINE: &str = "UPSERT type::record('writer', [$host, $path]) CONTENT { program: $program, path: $path, host: $host, schema: $schema, seen: $seen } RETURN NONE;";
