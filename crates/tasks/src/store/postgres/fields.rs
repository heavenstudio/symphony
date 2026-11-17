//! The statements of `store/fields.rs` on PostgreSQL.

use super::work::said;
use super::{Pg, act};

/// Sets a plugin's field on `v ->> 'slug'` (`patch`, one plugin's one field; `missing` refuses a
/// task not there) and says so on its thread (`label`, `shown`), by the actor: a session's write
/// when `acts`, else the owner's.
pub(in crate::store) fn set_field(acts: bool) -> Pg<'static> {
    let act = if acts { act() } else { String::new() };
    Pg::script(format!(
        "{act}
IF NOT EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug') THEN
    RAISE EXCEPTION '%', v ->> 'missing';
END IF;
UPDATE task SET fields = board_set(fields, ARRAY[p.key, n.key], n.value), version = version + 1
    FROM jsonb_each(v -> 'patch') AS p, jsonb_each(p.value) AS n WHERE slug = v ->> 'slug';
{}",
        said("(v ->> 'actor') || ' set ' || (v ->> 'label') || ' to ' || (v ->> 'shown')")
    ))
}
