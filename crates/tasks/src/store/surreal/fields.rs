//! The statements of `store/fields.rs`.

use super::logged;
use super::write::ACT;

/// Sets a plugin's field on `$slug` (`$patch`; `$missing` refuses a task not there) and says so on
/// its thread (`$label`, `$shown`), by `$actor`: a session's write when `acts`, else the owner's.
pub(in crate::store) fn set_field(acts: bool) -> String {
    let act = if acts { ACT } else { "" };
    format!(
        "{act}
IF !record::exists(type::record('task', $slug)) {{ THROW $missing }};
UPDATE type::record('task', $slug) MERGE {{ fields: $patch }} RETURN NONE;
UPDATE type::record('task', $slug) SET version += 1 RETURN NONE;
LET $said = $actor + ' set ' + $label + ' to ' + $shown;
{}",
        logged("$slug", "$actor", "'system'", "$said", "NONE")
    )
}
