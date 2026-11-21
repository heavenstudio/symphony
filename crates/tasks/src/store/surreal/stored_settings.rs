//! The statements of `store/stored_settings.rs`.

/// Every stored entry, by key, or none where the board lacks the step that defines the table.
pub(in crate::store) const ENTRIES: &str = "RETURN IF (INFO FOR DB).tables.setting = NONE { [] } ELSE { (SELECT * OMIT id FROM setting ORDER BY key) };";

/// The entry `$key` written as `$doc`, only while its revision is still `$expected` (0: no row):
/// whether it was.
pub(in crate::store) const WRITE: &str = "
LET $row = type::record('setting', $key);
LET $before = (SELECT * FROM ONLY $row);
IF ($before.revision ?? 0) != $expected { RETURN false; };
UPSERT $row CONTENT $doc RETURN NONE;
RETURN true;
";
