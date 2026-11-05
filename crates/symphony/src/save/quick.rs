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
        let identity = if route == "/api/quick-task" {
            if title.chars().count() > symphony_api::QUICK_TASK_TITLE_MAX {
                return Err(format!(
                    "Use at most {} characters for the task title",
                    symphony_api::QUICK_TASK_TITLE_MAX
                )
                .into());
            }
            Some(quick_task::identity(payload)?)
        } else {
            None
        };
        if let Some(identity) = identity.as_deref()
            && let Some(record) = quick_task::load(&self.root, identity, scope, payload)?
        {
            return match self.store() {
                Ok(store) => quick_task::create(&store, &record.task(), quick_task::Attempt::Check),
                Err(error) => Ok(quick_task::pending(&error.error)),
            };
        }
        let PageContext { page, doc, anchor } = self.quick_context(scope, payload)?;
        if route == "/api/quick-chat" {
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
            return Ok(json!({"id":id}));
        }
        let identity = identity.expect("the quick-task route has a save identity");
        let store = self.store()?;
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
                    Err(e) => return Err(e.into()),
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
        let new = tasks::NewTask {
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
        };
        let record = quick_task::reserve(&self.root, &identity, scope, payload, &new)?;
        quick_task::create(&store, &record.task(), quick_task::Attempt::First)
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
