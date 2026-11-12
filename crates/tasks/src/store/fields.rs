//! A plugin's board fields: a typed value a project's plugin declares on every task, kept in the
//! task record's `fields` object under the plugin's name (`fields.<plugin>.<name>`). The board
//! stores what it is handed; the Symphony server asks the plugin to validate a value before it
//! writes one. A task without a field set carries no `fields` at all, so a reader from before the
//! field (schema 9) never meets one.

use serde_json::{Value, json};

use super::work::OWNER;
use super::{ACT, Error, REFUSED, Store, Task, acting, logged, refused, repr};

impl Store {
    /// Set the field `name` of `plugin` on the task `slug` to `value`, a number or text, recording
    /// the actor and the value on the task's thread. The task's other fields stay as they were.
    pub fn set_field(
        &self,
        slug: &str,
        plugin: &str,
        name: &str,
        value: &Value,
        handle: &str,
    ) -> Result<Task, Error> {
        for (what, word) in [("plugin", plugin), ("field", name)] {
            if word.is_empty()
                || !word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(refused(format!(
                    "a {what} name is letters, digits, - and _, not {}",
                    repr(word)
                )));
            }
        }
        if plugin == crate::red::FIELDS {
            return Err(refused(format!(
                "fields.{plugin} are the board's own: a red is marked by its filer (mark_red), a fast fix where it is filed (create --fast-fix), a landing's record by record_waits"
            )));
        }
        let shown = match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            other => {
                return Err(refused(format!(
                    "a field holds a number or text, not {other}"
                )));
            }
        };
        let act = if handle == OWNER { "" } else { ACT };
        let body = format!(
            "{act}
IF !record::exists(type::record('task', $slug)) {{ THROW $missing }};
UPDATE type::record('task', $slug) MERGE {{ fields: $patch }} RETURN NONE;
UPDATE type::record('task', $slug) SET version += 1 RETURN NONE;
LET $said = $actor + ' set ' + $label + ' to ' + $shown;
{}",
            logged("$slug", "$actor", "'system'", "$said", "NONE")
        );
        self.retrying(|| {
            let vars = acting(
                handle,
                (self.clock)(),
                json!({
                    "slug": slug,
                    "patch": {plugin: {name: value}},
                    "label": format!("{plugin}.{name}"),
                    "shown": shown,
                    "missing": format!("{REFUSED}no task {}", repr(slug)),
                }),
            );
            self.written(&body, vars, slug)
        })
    }
}
