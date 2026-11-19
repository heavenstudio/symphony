//! The scoped private artifact ledger's load and compare-and-set.

pub(in crate::store) const LOAD: &str =
    "RETURN (SELECT revision, body FROM ONLY type::record('artifact_ledger', $key)) ?? NULL;";

pub(in crate::store) const CAS: &str = "
LET $row = type::record('artifact_ledger', $key);
LET $before = (SELECT * FROM ONLY $row);
IF $expected = 0 {
    IF $before != NONE { RETURN false; };
    CREATE $row CONTENT { revision: 1, body: $body };
    RETURN true;
};
IF $before = NONE OR $before.revision != $expected { RETURN false; };
UPDATE $row SET revision = $expected + 1, body = $body;
RETURN true;
";
