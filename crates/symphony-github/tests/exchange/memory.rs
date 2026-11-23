//! A board held in memory that answers the exchange's five calls as the board's api does: a
//! task's number and version behind a token under a recovery generation, an edit refused unless
//! its token is the task's own, a slug taken once, and a note written once for its key.

use serde_json::json;
use std::path::PathBuf;
use std::sync::Mutex;
use symphony_github::board::{Board, BoardError, Bound, Task, Token};

/// What goes wrong with one call.
#[derive(Clone, Debug)]
pub(crate) enum Fault {
    /// It is carried out, and its answer never arrives.
    LoseAnswer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Note {
    pub(crate) slug: String,
    pub(crate) key: String,
    pub(crate) text: String,
    pub(crate) to: Option<String>,
}

struct Inner {
    tasks: Vec<(Task, i64)>,
    generation: String,
    next: i64,
    notes: Vec<Note>,
    faults: Vec<(&'static str, Fault)>,
    unbound: bool,
    /// A brief someone rewrites just after the next listing of the tasks is read.
    after_listing: Option<(String, String, String)>,
    /// Every call that writes, by name and slug.
    writes: Vec<String>,
}

pub(crate) struct MemBoard {
    inner: Mutex<Inner>,
    folder: PathBuf,
}

impl MemBoard {
    pub(crate) fn new(folder: PathBuf) -> MemBoard {
        MemBoard {
            inner: Mutex::new(Inner {
                tasks: Vec::new(),
                generation: "generation-1".into(),
                next: 1,
                notes: Vec::new(),
                faults: Vec::new(),
                unbound: false,
                after_listing: None,
                writes: Vec::new(),
            }),
            folder,
        }
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A task filed by someone here.
    pub(crate) fn file(&self, slug: &str, title: &str, body: &str) -> i64 {
        self.inner().add(slug, title, body)
    }

    /// The task's brief rewritten by someone here: its version moves.
    pub(crate) fn rewrite(&self, slug: &str, title: &str, body: &str) {
        let mut inner = self.inner();
        let (task, version) = inner
            .tasks
            .iter_mut()
            .find(|(task, _)| task.slug == slug)
            .expect("a task of that slug");
        task.title = title.into();
        task.body = body.into();
        *version += 1;
    }

    /// The same, made just after the next listing of the tasks is read: between what a
    /// preview lists and the token it then reads.
    pub(crate) fn rewrite_after_listing(&self, slug: &str, title: &str, body: &str) {
        self.inner().after_listing = Some((slug.into(), title.into(), body.into()));
    }

    /// The board put back from an older copy: another generation, every version started again.
    pub(crate) fn restore(&self) {
        let mut inner = self.inner();
        inner.generation = format!("{}-restored", inner.generation);
        for (_, version) in &mut inner.tasks {
            *version = 1;
        }
    }

    /// The slug given to another task, as a restore does to a number made after its dump.
    pub(crate) fn replace(&self, slug: &str, title: &str, body: &str) {
        let mut inner = self.inner();
        inner.tasks.retain(|(task, _)| task.slug != slug);
        inner.add(slug, title, body);
    }

    pub(crate) fn remove(&self, slug: &str) {
        self.inner().tasks.retain(|(task, _)| task.slug != slug);
    }

    pub(crate) fn unbind(&self) {
        self.inner().unbound = true;
    }

    pub(crate) fn bind(&self) {
        self.inner().unbound = false;
    }

    pub(crate) fn fault(&self, call: &'static str, fault: Fault) {
        self.inner().faults.push((call, fault));
    }

    pub(crate) fn task(&self, slug: &str) -> Option<Task> {
        self.inner()
            .tasks
            .iter()
            .find(|(task, _)| task.slug == slug)
            .map(|(task, _)| task.clone())
    }

    pub(crate) fn slugs(&self) -> Vec<String> {
        self.inner()
            .tasks
            .iter()
            .map(|(task, _)| task.slug.clone())
            .collect()
    }

    pub(crate) fn notes(&self) -> Vec<Note> {
        self.inner().notes.clone()
    }

    pub(crate) fn writes(&self) -> Vec<String> {
        self.inner().writes.clone()
    }
}

impl Inner {
    fn add(&mut self, slug: &str, title: &str, body: &str) -> i64 {
        let id = self.next;
        self.next += 1;
        self.tasks.push((
            Task {
                slug: slug.into(),
                id,
                title: title.into(),
                body: body.into(),
            },
            1,
        ));
        id
    }

    fn bound(&self, slug: &str) -> Result<Bound, BoardError> {
        if self.unbound {
            return Err(BoardError::Refused(
                "the board is bound to no recovery generation".into(),
            ));
        }
        let (task, version) = self
            .tasks
            .iter()
            .find(|(task, _)| task.slug == slug)
            .ok_or_else(|| BoardError::Refused(format!("no task {slug}")))?;
        Ok(Bound {
            token: Token(json!({
                "compat": 1, "generation": self.generation, "id": task.id, "version": version,
            })),
            title: task.title.clone(),
            body: task.body.clone(),
        })
    }

    /// The answer of a call that was carried out: lost if a fault waits for it.
    fn lose<T>(&mut self, call: &'static str, answer: T) -> Result<T, BoardError> {
        match self.faults.iter().position(|(waits, _)| *waits == call) {
            Some(at) => {
                self.faults.remove(at);
                Err(BoardError::Failed(format!("{call}: no answer came")))
            }
            None => Ok(answer),
        }
    }
}

impl Board for MemBoard {
    fn state_folder(&self) -> Result<PathBuf, BoardError> {
        let folder = self.folder.clone();
        self.inner().lose("state_folder", folder)
    }

    fn tasks(&self) -> Result<Vec<Task>, BoardError> {
        let (listed, edit): (Vec<Task>, _) = {
            let mut inner = self.inner();
            let listed = inner.tasks.iter().map(|(task, _)| task.clone()).collect();
            (listed, inner.after_listing.take())
        };
        if let Some((slug, title, body)) = edit {
            self.rewrite(&slug, &title, &body);
        }
        Ok(listed)
    }

    fn propose(&self, slug: &str, title: &str, body: &str, _by: &str) -> Result<Task, BoardError> {
        let mut inner = self.inner();
        if inner.tasks.iter().any(|(task, _)| task.slug == slug) {
            return Err(BoardError::Refused(format!("a task {slug} already exists")));
        }
        inner.add(slug, title, body);
        inner.writes.push(format!("propose {slug}"));
        let made = inner.tasks[inner.tasks.len() - 1].0.clone();
        inner.lose("propose", made)
    }

    fn bound(&self, slug: &str) -> Result<Bound, BoardError> {
        self.inner().bound(slug)
    }

    fn edit(
        &self,
        slug: &str,
        token: &Token,
        title: &str,
        body: &str,
        _by: &str,
    ) -> Result<Bound, BoardError> {
        let mut inner = self.inner();
        let now = inner.bound(slug)?;
        if now.token != *token {
            return Err(BoardError::Refused(
                "the task changed since this was read".into(),
            ));
        }
        let (task, version) = inner
            .tasks
            .iter_mut()
            .find(|(task, _)| task.slug == slug)
            .expect("the task just read");
        task.title = title.into();
        task.body = body.into();
        *version += 1;
        inner.writes.push(format!("edit {slug}"));
        let written = inner.bound(slug)?;
        inner.lose("edit", written)
    }

    fn note(
        &self,
        slug: &str,
        text: &str,
        to: Option<&str>,
        key: &str,
        _by: &str,
    ) -> Result<(), BoardError> {
        let mut inner = self.inner();
        if !inner.tasks.iter().any(|(task, _)| task.slug == slug) {
            return Err(BoardError::Refused(format!("no task {slug}")));
        }
        if !inner
            .notes
            .iter()
            .any(|note| note.slug == slug && note.key == key)
        {
            inner.notes.push(Note {
                slug: slug.into(),
                key: key.into(),
                text: text.into(),
                to: to.map(str::to_string),
            });
            inner.writes.push(format!("note {slug} {key}"));
        }
        inner.lose("note", ())
    }
}
