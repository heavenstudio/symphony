//! The statements of `store/recovery.rs`.

use super::logged;

/// The board's recovery generation, as a value.
macro_rules! generation {
    () => {
        "(SELECT VALUE generation FROM ONLY recovery:board)"
    };
}

/// The board's recovery generation, as a read of its own.
pub(in crate::store) const GENERATION: &str = concat!("RETURN ", generation!(), ";");

/// The board's recovery generation replaced by one the database makes, which is the answer:
/// every token read before it is refused from this transaction on.
pub(in crate::store) const REBIND: &str = concat!(
    "UPSERT recovery:board SET generation = <string> rand::uuid::v4() RETURN NONE;\nRETURN ",
    generation!(),
    ";"
);

/// The task `$slug` with the generation it is read under, in one statement: none for no task.
pub(in crate::store) const READ: &str = concat!(
    "RETURN (SELECT VALUE { generation: ",
    generation!(),
    ", id: sid, version: version, title: title, body: body } FROM ONLY type::record('task', $slug));"
);

/// The fence a bound write starts with, before anything of its own: refused (`$bound_restored`)
/// unless the board is at the generation `$bound` was read under, (`$missing`) unless `$slug` is
/// a task, and (`$bound_stale`) unless it is the task `$bound` names at the version it names. The
/// write it fences writes that task's row, where it meets a copy's swap, which replaces it.
pub(in crate::store) const FENCED: &str = concat!(
    "LET $bound_found = ",
    generation!(),
    ";
IF $bound_found = NONE OR $bound_found != $bound.generation { THROW $bound_restored };
LET $bound_task = (SELECT sid, version FROM ONLY type::record('task', $slug));
IF $bound_task = NONE { THROW $missing };
IF $bound_task.sid != $bound.id OR $bound_task.version != $bound.version { THROW $bound_stale };
"
);

/// A bound retitle: [`FENCED`], then the retitle as an unbound one makes it
/// ([`super::work::amend`]), then the task as the write left it ([`READ`]).
pub(in crate::store) fn amend(acts: bool) -> String {
    format!("{FENCED}{}\n{READ}", super::work::amend(acts))
}

/// A bound edit ([`crate::store::recovery::Edit`]): [`FENCED`], then the task's title replaced
/// when `$retitle`, its brief when `$rewrite` and `$links` added, each change of the two a line on
/// the thread, then the task as the write left it ([`READ`]).
pub(in crate::store) fn edit(acts: bool) -> String {
    let act = if acts { super::write::ACT } else { "" };
    let said = logged("$slug", "$actor", "'system'", "$said", "NONE");
    format!(
        "{FENCED}{act}
LET $task = (SELECT title, body FROM ONLY type::record('task', $slug));
LET $new_title = IF $retitle {{ $title }} ELSE {{ $task.title }};
LET $new_body = IF $rewrite {{ $body }} ELSE {{ $task.body }};
FOR $link IN $links {{
    UPSERT type::record('link', [$slug, 'doc', $link])
        SET slug = $slug, kind = 'doc', target = $link RETURN NONE;
}};
UPDATE type::record('task', $slug) SET title = $new_title, body = $new_body, version += 1 RETURN NONE;
IF $task.title != $new_title {{
    LET $said = $actor + ' retitled it: ' + $task.title + ' → ' + $new_title;
    {said}
}};
IF $task.body != $new_body {{
    LET $said = $actor + ' rewrote its brief';
    {said}
}};
{READ}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_write_checks_its_fence_before_any_statement_that_writes() {
        let amend = amend(true);
        let fence = amend.find("THROW $bound_stale").unwrap();
        let first_write = ["UPDATE", "UPSERT", "CREATE", "DELETE"]
            .iter()
            .filter_map(|word| amend.find(word))
            .min()
            .unwrap();
        assert!(fence < first_write, "{amend}");
        assert!(amend.ends_with(READ));
        for acts in [true, false] {
            let edit = edit(acts);
            assert!(edit.starts_with(FENCED) && edit.ends_with(READ), "{edit}");
            assert_eq!(edit.contains(super::super::write::ACT), acts);
            assert!(crate::schema::mutates(&edit));
        }
        assert!(!crate::schema::mutates(READ) && !crate::schema::mutates(GENERATION));
        assert!(crate::schema::mutates(REBIND) && REBIND.ends_with(GENERATION));
    }
}
