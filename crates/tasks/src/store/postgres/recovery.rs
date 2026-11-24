//! The statements of `store/recovery.rs` on PostgreSQL.

use super::Pg;

/// The board's recovery generation, as a value.
const VALUE: &str = "(SELECT generation FROM recovery WHERE id = '\"board\"')";

/// The board's recovery generation replaced by one the database makes.
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

/// The fence a bound write starts with. The shared lock on the generation's row meets a copy's or
/// a rebind's replacement of it, from an older snapshot too, as the schema's guard meets a
/// migration.
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

/// A bound edit: [`FENCED`], then the title replaced when `retitle`, the brief when `rewrite` and
/// `links` added, each change of the two a line on the thread, then the task as the write left it.
pub(in crate::store) fn edit(acts: bool) -> Pg<'static> {
    let act = if acts { super::act() } else { String::new() };
    let said = super::work::said;
    let block = format!(
        "{FENCED}
{act} \
DECLARE \
    title_ text; \
    body_ text; \
    new_title_ text; \
    new_body_ text; \
BEGIN \
    SELECT title, body INTO title_, body_ FROM task WHERE slug = v ->> 'slug'; \
    new_title_ := CASE WHEN (v ->> 'retitle')::boolean THEN v ->> 'title' ELSE title_ END; \
    new_body_ := CASE WHEN (v ->> 'rewrite')::boolean THEN v ->> 'body' ELSE body_ END; \
    INSERT INTO link (id, slug, kind, target) \
        SELECT DISTINCT jsonb_build_array(v ->> 'slug', 'doc', l), v ->> 'slug', 'doc', l \
        FROM jsonb_array_elements_text(v -> 'links') AS l \
        ON CONFLICT (id) DO NOTHING; \
    UPDATE task SET title = new_title_, body = new_body_, version = version + 1 \
        WHERE slug = v ->> 'slug'; \
    IF title_ IS DISTINCT FROM new_title_ THEN \
        {} \
    END IF; \
    IF body_ IS DISTINCT FROM new_body_ THEN \
        {} \
    END IF; \
END;",
        said("(v ->> 'actor') || ' retitled it: ' || title_ || ' → ' || new_title_"),
        said("(v ->> 'actor') || ' rewrote its brief'")
    );
    Pg::script_then(block, read_value())
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
        for acts in [true, false] {
            let Pg::Script(block, Some(answer)) = edit(acts) else {
                panic!("a bound edit is a block and its read");
            };
            assert!(block.starts_with(FENCED), "{block}");
            assert_eq!(block.contains("UPDATE session"), acts, "{block}");
            assert!(block.find("bound_stale").unwrap() < block.find("UPDATE task").unwrap());
            assert_eq!(answer, read_value());
        }
        assert_eq!(read(), Pg::read(read_value()));
        assert_eq!(rebind(), Pg::script_then(ROTATE, VALUE));
        assert_eq!(generation(), Pg::read(VALUE));
    }
}
