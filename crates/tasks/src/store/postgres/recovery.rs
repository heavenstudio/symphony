//! The statements of `store/recovery.rs` on PostgreSQL.

use super::Pg;

/// The board's recovery generation, as a value.
const VALUE: &str = "(SELECT generation FROM recovery WHERE id = '\"board\"')";

/// The board's recovery generation replaced by one the database makes
/// (`super::super::surreal::recovery::REBIND`).
pub(in crate::store) const ROTATE: &str =
    "INSERT INTO recovery (id, generation) VALUES ('\"board\"', gen_random_uuid()::text)
    ON CONFLICT (id) DO UPDATE SET generation = EXCLUDED.generation;";

/// The board's recovery generation, as a read of its own.
pub(in crate::store) fn generation() -> Pg<'static> {
    Pg::read(VALUE)
}

/// [`ROTATE`], answering the generation it made.
pub(in crate::store) fn rebind() -> Pg<'static> {
    Pg::script_then(ROTATE, VALUE)
}

/// The task `v ->> 'slug'` with the generation it is read under, in one statement: null for no
/// task.
fn read_value() -> String {
    format!(
        "(SELECT jsonb_build_object('generation', {VALUE}, 'id', sid, 'version', version, \
         'title', title, 'body', body) FROM task WHERE slug = v ->> 'slug')"
    )
}

/// [`read_value`] as a read of its own.
pub(in crate::store) fn read() -> Pg<'static> {
    Pg::read(read_value())
}

/// The fence a bound write starts with (`super::super::surreal::recovery::FENCED`). The shared
/// lock on the generation's row meets a copy's or a rebind's replacement of it, from an older
/// snapshot too, as the schema's guard meets a migration.
pub(in crate::store) const FENCED: &str = "PERFORM 1 FROM recovery WHERE id = '\"board\"'
    AND generation = v -> 'bound' ->> 'generation' FOR SHARE;
IF NOT FOUND THEN
    RAISE EXCEPTION '%', v ->> 'bound_restored';
END IF;
PERFORM 1 FROM task WHERE slug = v ->> 'slug';
IF NOT FOUND THEN
    RAISE EXCEPTION '%', v ->> 'missing';
END IF;
PERFORM 1 FROM task WHERE slug = v ->> 'slug' AND sid = (v -> 'bound' ->> 'id')::bigint
    AND version = (v -> 'bound' ->> 'version')::bigint FOR UPDATE;
IF NOT FOUND THEN
    RAISE EXCEPTION '%', v ->> 'bound_stale';
END IF;";

/// A bound retitle: [`FENCED`], then the retitle as an unbound one makes it
/// ([`super::work::amend`]), then the task as the write left it.
pub(in crate::store) fn amend(acts: bool) -> Pg<'static> {
    let Pg::Script(block, None) = super::work::amend(acts) else {
        unreachable!("a retitle is a block");
    };
    Pg::script_then(format!("{FENCED}\n{block}"), read_value())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_write_checks_its_fence_before_the_retitles_block_and_answers_the_task() {
        let Pg::Script(block, Some(answer)) = amend(true) else {
            panic!("a bound retitle is a block and its read");
        };
        assert!(block.starts_with(FENCED), "{block}");
        let first_write = ["UPDATE session", "UPDATE task", "INSERT INTO"]
            .iter()
            .filter_map(|write| block.find(write))
            .min()
            .unwrap();
        assert!(block.find("bound_stale").unwrap() < first_write, "{block}");
        assert_eq!(answer, read_value());
        assert_eq!(read(), Pg::read(read_value()));
        assert_eq!(rebind(), Pg::script_then(ROTATE, VALUE));
        assert_eq!(generation(), Pg::read(VALUE));
    }
}
