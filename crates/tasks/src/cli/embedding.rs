//! Derived-cache maintenance, separate from board mutations and agent identity.
use super::commands::{Context, Stop};
use crate::Store;
use crate::embedding::{Bge, Chunk, Embedder, MODEL_ID, text_hash};
use std::cell::{Cell, RefCell};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::time::Instant;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let batch = usize::try_from(c.parsed.int("batch_size", 8))
        .ok()
        .filter(|n| (1..=64).contains(n))
        .ok_or_else(|| Stop::Refused("--batch-size must be between 1 and 64".into()))?;
    let threads = usize::try_from(c.parsed.int("threads", 1))
        .ok()
        .filter(|n| (1..=64).contains(n))
        .ok_or_else(|| Stop::Refused("--threads must be between 1 and 64".into()))?;
    let slugs = c.parsed.get("task").map(|slug| vec![slug.to_string()]);
    if let Some(slugs) = &slugs {
        store.get(&slugs[0])?;
    }
    let common = crate::git::common_dir(&c.env.cwd).map_err(|e| Stop::Board(e.to_string()))?;
    let Some(_held) = lock(&common, store.client()).map_err(Stop::Board)? else {
        if c.parsed.flag("maintenance") {
            c.say("an embedding pass is already running for this board");
            return Ok(());
        }
        return Err(Stop::Refused(
            "another embedding pass is already running for this board".into(),
        ));
    };
    let loaded = Cell::new(None);
    let load_error = RefCell::new(None);
    let mut embedder = Deferred::new(MODEL_ID, || {
        let start = Instant::now();
        let model = Bge::load_with_threads(&common, threads).inspect_err(|error| {
            *load_error.borrow_mut() = Some(error.clone());
        })?;
        loaded.set(Some(start.elapsed().as_secs_f64()));
        Ok(model)
    });
    let start = Instant::now();
    let mut last = Instant::now();
    let mut progress = |done, total| {
        if last.elapsed().as_secs() >= 10 || done == total {
            let _ = writeln!(c.err, "embedding chunks {done}/{total}");
            last = Instant::now();
        }
    };
    let result = if c.parsed.flag("maintenance") && slugs.is_none() {
        store.maintain_embeddings(&mut embedder, batch, &mut progress)
    } else {
        store.catch_up_embeddings(&mut embedder, slugs.as_deref(), batch, &mut progress)
    };
    if let Some(error) = load_error.borrow_mut().take() {
        return Err(Stop::Board(error));
    }
    let stats = result?;
    let elapsed = start.elapsed().as_secs_f64();
    if let Some(seconds) = loaded.get() {
        c.say(&format!(
            "model {MODEL_ID} loaded in {seconds:.3}s, {threads} inference thread(s)"
        ));
    }
    c.say(&format!(
        "embedded {} tasks, {} changed sources, {} chunks in {:.3}s",
        stats.tasks,
        stats.sources,
        stats.chunks,
        (elapsed - loaded.get().unwrap_or(0.0)).max(0.0)
    ));
    Ok(())
}

/// One writer per database, while other boards and ordinary commands remain concurrent.
fn lock(common: &Path, client: &crate::rpc::Client) -> Result<Option<File>, String> {
    let dir = common.join("sf-task-models");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let key = text_hash(&format!(
        "{}\n{}\n{}",
        client.addr(),
        crate::NAMESPACE,
        client.database()
    ));
    let path = dir.join(format!("maintenance-{key}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(format!("{}: {error}", path.display())),
    }
}

struct Deferred<E, F> {
    model: &'static str,
    state: RefCell<(Option<E>, F)>,
}

impl<E: Embedder, F: FnMut() -> Result<E, String>> Deferred<E, F> {
    fn new(model: &'static str, load: F) -> Self {
        Self {
            model,
            state: RefCell::new((None, load)),
        }
    }

    fn with<T>(&self, run: impl FnOnce(&mut E) -> Result<T, String>) -> Result<T, String> {
        let mut state = self.state.borrow_mut();
        if state.0.is_none() {
            let model = (state.1)()?;
            if model.model_id() != self.model {
                return Err("loaded embedder has a different model id".into());
            }
            state.0 = Some(model);
        }
        run(state.0.as_mut().expect("model was loaded"))
    }
}

impl<E: Embedder, F: FnMut() -> Result<E, String>> Embedder for Deferred<E, F> {
    fn model_id(&self) -> &str {
        self.model
    }
    fn chunks(&self, text: &str) -> Result<Vec<Chunk>, String> {
        self.with(|model| model.chunks(text))
    }
    fn embed(&mut self, chunks: &[Chunk]) -> Result<Vec<Vec<f32>>, String> {
        self.with(|model| model.embed(chunks))
    }
    fn query(&mut self, text: &str) -> Result<Vec<f32>, String> {
        self.with(|model| model.query(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl Embedder for Fake {
        fn model_id(&self) -> &str {
            "fake"
        }
        fn chunks(&self, text: &str) -> Result<Vec<Chunk>, String> {
            Ok(vec![Chunk {
                text: text.into(),
                tokens: vec![1],
            }])
        }
        fn embed(&mut self, chunks: &[Chunk]) -> Result<Vec<Vec<f32>>, String> {
            Ok(vec![vec![1.0]; chunks.len()])
        }
        fn query(&mut self, _: &str) -> Result<Vec<f32>, String> {
            Ok(vec![1.0])
        }
    }

    #[test]
    fn cache_checks_need_no_model_and_pending_work_loads_one_model_for_the_whole_pass() {
        let loads = Cell::new(0);
        let mut model = Deferred::new("fake", || {
            loads.set(loads.get() + 1);
            Ok(Fake)
        });
        assert_eq!(model.model_id(), "fake");
        assert_eq!(
            loads.get(),
            0,
            "unchanged-source checks use only the model id"
        );
        let chunks = model.chunks("new source").unwrap();
        assert_eq!(chunks[0].text, "new source");
        assert_eq!(model.embed(&chunks).unwrap(), vec![vec![1.0]]);
        assert_eq!(model.query("query").unwrap(), vec![1.0]);
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn a_failed_model_load_can_retry_and_cannot_publish_under_a_wrong_model_id() {
        let loads = Cell::new(0);
        let model = Deferred::new("fake", || {
            loads.set(loads.get() + 1);
            if loads.get() == 1 {
                Err("not available yet".into())
            } else {
                Ok(Fake)
            }
        });
        assert_eq!(model.chunks("text").unwrap_err(), "not available yet");
        assert_eq!(model.chunks("text").unwrap().len(), 1);
        assert_eq!(loads.get(), 2);
        let wrong = Deferred::new("other", || Ok(Fake));
        assert_eq!(
            wrong.chunks("text").unwrap_err(),
            "loaded embedder has a different model id"
        );
    }

    #[test]
    fn maintenance_admits_one_writer_per_board_without_serialising_other_databases() {
        let common = std::env::temp_dir().join(format!("sf-embedding-lock-{}", std::process::id()));
        std::fs::create_dir_all(&common).unwrap();
        let client = crate::rpc::Client::new("127.0.0.1:1", crate::NAMESPACE, "one");
        let other = crate::rpc::Client::new("127.0.0.1:1", crate::NAMESPACE, "two");
        let held = lock(&common, &client).unwrap().unwrap();
        assert!(lock(&common, &client).unwrap().is_none());
        assert!(lock(&common, &other).unwrap().is_some());
        drop(held);
        assert!(lock(&common, &client).unwrap().is_some());
        std::fs::remove_dir_all(common).unwrap();
    }
}
