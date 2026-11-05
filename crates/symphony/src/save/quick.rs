//! Quick chat and task saves retain their page context and task intent.

use super::{Failure, Saves, quick_task, string};
use crate::asks;
use serde_json::{Value, json};

struct PageContext {
    page: String,
    doc: String,
    anchor: String,
}

impl Saves {
    /// Apply a quick save with its page context and any already recorded task intent.
    pub(super) fn quick(
        &self,
        scope: &str,
        route: &str,
        payload: &Value,
    ) -> Result<Value, Failure> {
        let title = string(payload, "title").trim();
        if route == "/api/quick-task" {
            if title.chars().count() > symphony_api::QUICK_TASK_TITLE_MAX {
                return Err(format!(
                    "Use at most {} characters for the task title",
                    symphony_api::QUICK_TASK_TITLE_MAX
                )
                .into());
            }
            let identity = quick_task::identity(payload)?;
            let mut receipt = quick_task::Receipt::open(&self.root, &identity, scope, payload)?;
            if let Some(result) = receipt.completed() {
                return result;
            }
            let result = self.quick_task(scope, payload, &identity, &receipt);
            return receipt.finish(result);
        }
        let PageContext { page, doc, anchor } = self.quick_context(scope, payload)?;
        let mut text = string(payload, "text").trim().to_string();
        if doc.is_empty() && !page.is_empty() && !text.is_empty() {
            text.push_str(&format!("\n\n(sent from {page})"));
        }
        let store = asks::AskStore::open(&asks::default_path(
            &self.root,
            std::env::var("ASKS_DB").ok().as_deref(),
        ))?;
        let readme = crate::project::Project::of(&self.root).entrypoints[0].route("README.md");
        let id = store.ask(
            if doc.is_empty() { &readme } else { &doc },
            &text,
            if doc.is_empty() { "" } else { &anchor },
            "",
        )?;
        Ok(json!({"id":id}))
    }

    fn quick_task(
        &self,
        scope: &str,
        payload: &Value,
        identity: &str,
        receipt: &quick_task::Receipt,
    ) -> Result<Value, Failure> {
        let record = quick_task::load(&self.root, identity, scope, payload).map_err(|error| {
            if receipt.fresh {
                before_dispatch(error)
            } else {
                error
            }
        })?;
        if let Some(record) = record {
            return match self.store() {
                Ok(store) => quick_task::create(
                    &store,
                    &record.task(),
                    &receipt.fingerprint,
                    quick_task::Attempt::Check,
                ),
                Err(error) => Ok(quick_task::pending(&error.error)),
            };
        }
        if !receipt.fresh {
            return Err(Failure {
                status: 422,
                error: "This task save stopped before dispatch; your text is kept for another try"
                    .into(),
            });
        }
        let context = self.quick_context(scope, payload)?;
        let store = self.store().map_err(before_dispatch)?;
        let new = self.quick_task_fields(payload, context, &store)?;
        let record = quick_task::reserve(&self.root, identity, scope, payload, &new)
            .map_err(before_dispatch)?;
        quick_task::create(
            &store,
            &record.task(),
            &receipt.fingerprint,
            quick_task::Attempt::First,
        )
    }

    fn quick_task_fields(
        &self,
        payload: &Value,
        context: PageContext,
        store: &tasks::Store,
    ) -> Result<tasks::NewTask, Failure> {
        let PageContext { page, doc, anchor } = context;
        let title = string(payload, "title").trim();
        let mut slug = string(payload, "slug").trim().to_string();
        if slug.is_empty() {
            let title = regex::Regex::new("[^a-z0-9]+")
                .expect("slug pattern")
                .replace_all(&title.to_lowercase(), "-")
                .trim_matches('-')
                .chars()
                .take(40)
                .collect::<String>();
            let base = format!(
                "inbox/{}",
                if title.is_empty() {
                    "task"
                } else {
                    title.trim_matches('-')
                }
            );
            slug.clone_from(&base);
            let mut n = 1;
            loop {
                match store.get(&slug) {
                    Ok(_) => {
                        n += 1;
                        slug = format!("{base}-{n}");
                    }
                    Err(tasks::Error::Refused(_)) => break,
                    Err(e) => return Err(before_dispatch(e.into())),
                }
            }
        }
        let mut body = string(payload, "body").trim().to_string();
        if !page.is_empty() {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str(&format!("Sent from [{page}]({page})"));
        }
        let priority = if payload["priority"].is_null() {
            2
        } else {
            payload["priority"]
                .as_i64()
                .or_else(|| payload["priority"].as_str().and_then(|s| s.parse().ok()))
                .ok_or("priority must be a number".to_string())?
        };
        Ok(tasks::NewTask {
            slug,
            title: title.into(),
            body,
            by: "owner".into(),
            priority,
            status: "concept".into(),
            links: if doc.is_empty() {
                vec![]
            } else {
                vec![if anchor.is_empty() {
                    doc
                } else {
                    format!("{doc}#{anchor}")
                }]
            },
            ..tasks::NewTask::default()
        })
    }

    fn quick_context(&self, scope: &str, payload: &Value) -> Result<PageContext, Failure> {
        let raw: String = string(payload, "page").chars().take(500).collect();
        let raw = raw.split_once("://").map_or(raw.as_str(), |(_, tail)| {
            tail.find('/').map_or("/", |at| &tail[at..])
        });
        let (path, anchor) = raw.split_once('#').unwrap_or((raw, ""));
        let path = path.split('?').next().unwrap_or("");
        let path = decode(path).unwrap_or_else(|| path.to_string());
        let mut page = format!(
            "{path}{}",
            if anchor.is_empty() {
                String::new()
            } else {
                format!("#{anchor}")
            }
        );
        let (root, rel) = self.scope(scope, &path)?;
        if root != self.root {
            page = self.page(&root, &rel, anchor)?;
        }
        // Keep the branch page's full URL when resolving its quick-chat fallback.
        let doc = if root == self.root && Self::target(&root, &rel).is_ok() {
            rel
        } else {
            String::new()
        };
        Ok(PageContext {
            page,
            doc,
            anchor: anchor.into(),
        })
    }
}

fn before_dispatch(error: Failure) -> Failure {
    Failure {
        status: if error.status >= 500 {
            422
        } else {
            error.status
        },
        error: error.error,
    }
}

fn decode(value: &str) -> Option<String> {
    let mut bytes = value.bytes();
    let mut out = Vec::new();
    while let Some(b) = bytes.next() {
        out.push(if b == b'%' {
            u8::try_from(
                char::from(bytes.next()?).to_digit(16)? * 16
                    + char::from(bytes.next()?).to_digit(16)?,
            )
            .ok()?
        } else {
            b
        });
    }
    String::from_utf8(out).ok()
}
