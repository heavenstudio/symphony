//! Catch-up is explicit, never a side effect of a board write. Hashes are checked again on reads,
//! so an edit racing inference cannot make obsolete text appear as a current vector hit.
use super::copy::CHUNK_BYTES;
use super::surreal::embeddings as sql;
use super::*;
use crate::cbor;
use crate::embedding::{Chunk, DIMENSIONS, Embedder, text_hash};
use serde::Serialize;

#[derive(Debug, Clone, Deserialize)]
struct Source {
    slug: String,
    source: String,
    text: String,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Deserialize)]
struct Sources {
    briefs: Vec<Source>,
    messages: Vec<Source>,
}

#[derive(Debug, Deserialize)]
struct Cached {
    slug: String,
    source: String,
    source_hash: String,
    model: String,
    parts: usize,
}

#[derive(Deserialize)]
struct Parts {
    slug: String,
    source: String,
    parts: usize,
}

#[derive(Deserialize)]
struct Checkpoint {
    digest: String,
    current: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct EmbeddingStats {
    pub tasks: usize,
    pub sources: usize,
    pub chunks: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VectorHit {
    pub slug: String,
    pub score: f64,
    pub source: String,
    pub text: String,
}

/// Wall time by lookup phase, including each phase's database round trips. Model loading and
/// query inference happen before this operation and are measured separately by the benchmark.
#[derive(Debug, Default, Serialize)]
pub struct VectorSearchProfile {
    pub source_read_s: f64,
    pub source_hash_s: f64,
    pub eligibility_s: f64,
    pub count_s: f64,
    pub neighbours_s: f64,
    pub folding_s: f64,
    pub total_s: f64,
    pub rpc_requests: usize,
}

#[derive(Deserialize)]
struct ChunkHit {
    slug: String,
    source: String,
    source_hash: String,
    text: String,
    distance: f64,
}

impl Store {
    /// Whole-board maintenance skips source/cache reads after a successful pass while the model,
    /// message counter and task update stamps agree. The checkpoint precedes inference: a board
    /// write racing the pass therefore remains visible to the next child. Explicit catch-up keeps
    /// its full repair scan, including missing cache parts, even without a board write.
    ///
    /// The checkpoint keeps a SHA-256 of those stamps, computed by the server, never the stamps:
    /// every task's, kept whole, made a 165 KB row that each changed pass rewrote, about once a
    /// minute on a busy board (tools/tasks-db-space). `CONTENT` replaces a row an older pass left,
    /// and a row without a digest is never current.
    pub fn maintain_embeddings(
        &self,
        embedder: &mut dyn Embedder,
        batch_size: usize,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<EmbeddingStats, Error> {
        validate_batch_size(batch_size)?;
        let checkpoint: Checkpoint = self
            .retrying(|| self.transact(sql::CHECKPOINT, json!({"model": embedder.model_id()})))?;
        if checkpoint.current {
            return Ok(EmbeddingStats::default());
        }
        let stats = self.catch_up_embeddings(embedder, None, batch_size, progress)?;
        let _: Value = self.retrying(|| {
            self.transact(sql::KEEP_CHECKPOINT, json!({"digest": checkpoint.digest}))
        })?;
        Ok(stats)
    }

    fn embedding_sources(&self) -> Result<Vec<Source>, Error> {
        self.read_embedding_sources(sql::SOURCES, json!({}))
    }

    fn embedding_sources_named(&self, keys: &[(String, String)]) -> Result<Vec<Source>, Error> {
        let briefs: Vec<_> = keys
            .iter()
            .filter(|(_, source)| source == "brief")
            .map(|(slug, _)| slug)
            .collect();
        let messages: Vec<_> = keys
            .iter()
            .filter_map(|(slug, source)| {
                source
                    .strip_prefix("message:")
                    .and_then(|sid| sid.parse::<i64>().ok())
                    .map(|sid| json!([slug, sid]))
            })
            .collect();
        if briefs.is_empty() && messages.is_empty() {
            return Ok(Vec::new());
        }
        self.read_embedding_sources(
            sql::SOURCES_NAMED,
            json!({"brief_slugs": briefs, "message_keys": messages}),
        )
    }

    fn read_embedding_sources(&self, read: &str, vars: Value) -> Result<Vec<Source>, Error> {
        let sources: Sources = self.retrying(|| self.transact(read, vars.clone()))?;
        let mut sources: Vec<_> = sources.briefs.into_iter().chain(sources.messages).collect();
        // SurrealDB string::concat has a 1 MiB output ceiling. Board briefs can exceed it;
        // assemble the source here without truncating their knowledge or their hash.
        for source in &mut sources {
            if let Some(title) = source.title.take() {
                source.text = format!("{title}\n{}", source.text);
            }
        }
        Ok(sources)
    }

    /// Embed only missing/changed sources, in length-sorted CPU batches. A source's manifest and
    /// chunks publish atomically; an interrupted catch-up is safe to resume. System bookkeeping
    /// messages carry no task knowledge; user messages and review verdicts are all included.
    pub fn catch_up_embeddings(
        &self,
        embedder: &mut dyn Embedder,
        slugs: Option<&[String]>,
        batch_size: usize,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<EmbeddingStats, Error> {
        validate_batch_size(batch_size)?;
        let model = embedder.model_id().to_string();
        let sources = self.embedding_sources()?;
        let keys: std::collections::BTreeSet<_> = sources
            .iter()
            .map(|s| (s.slug.clone(), s.source.clone()))
            .collect();
        let cached: Vec<Cached> = self.retrying(|| self.transact(sql::CACHED, json!({})))?;
        let cached: BTreeMap<_, _> = cached
            .into_iter()
            .map(|r| ((r.slug.clone(), r.source.clone()), r))
            .collect();
        let parts: Vec<Parts> = self.retrying(|| self.transact(sql::PARTS, json!({})))?;
        let parts: BTreeMap<_, _> = parts
            .into_iter()
            .map(|p| ((p.slug, p.source), p.parts))
            .collect();
        let removed: std::collections::BTreeSet<_> = cached
            .keys()
            .chain(parts.keys())
            .filter(|key| !keys.contains(*key))
            .collect();
        if !removed.is_empty() {
            let _: Value =
                self.retrying(|| self.transact(sql::FORGET, json!({"removed": removed})))?;
        }
        let mut pending = Vec::new();
        for source in sources {
            if slugs.is_some_and(|slugs| !slugs.contains(&source.slug)) {
                continue;
            }
            let hash = text_hash(&source.text);
            if cached
                .get(&(source.slug.clone(), source.source.clone()))
                .is_some_and(|r| {
                    r.model == model
                        && r.source_hash == hash
                        && r.parts
                            == *parts
                                .get(&(source.slug.clone(), source.source.clone()))
                                .unwrap_or(&0)
                })
            {
                continue;
            }
            let chunks = embedder.chunks(&source.text).map_err(Error::Refused)?;
            pending.push((source, hash, chunks));
        }
        let mut order: Vec<_> = pending
            .iter()
            .enumerate()
            .flat_map(|(source, (_, _, chunks))| {
                chunks
                    .iter()
                    .enumerate()
                    .map(move |(part, chunk)| (source, part, chunk.clone()))
            })
            .collect();
        order.sort_by_key(|(source, part, chunk)| (chunk.tokens.len(), *source, *part));
        let mut embedded: BTreeMap<(usize, usize), Vec<f32>> = BTreeMap::new();
        let mut remaining: Vec<_> = pending.iter().map(|(_, _, chunks)| chunks.len()).collect();
        let mut stats = EmbeddingStats {
            sources: pending.len(),
            chunks: order.len(),
            ..Default::default()
        };
        let tasks: std::collections::BTreeSet<_> =
            pending.iter().map(|(s, _, _)| &s.slug).collect();
        stats.tasks = tasks.len();
        // Empty sources also get a manifest; whitespace should not be retried forever.
        for (source, hash, chunks) in &pending {
            if chunks.is_empty() {
                self.publish_embeddings(source, hash, &model, &[])?;
            }
        }
        let mut done = 0;
        for batch in order.chunks(batch_size) {
            let inputs: Vec<Chunk> = batch.iter().map(|(_, _, chunk)| chunk.clone()).collect();
            let vectors = embedder.embed(&inputs).map_err(Error::Refused)?;
            if vectors.len() != inputs.len() {
                return Err(refused(
                    "embedder returned a different number of vectors than inputs".into(),
                ));
            }
            for ((source, part, _), vector) in batch.iter().zip(vectors) {
                valid_vector(&vector)?;
                embedded.insert((*source, *part), vector);
                remaining[*source] -= 1;
                if remaining[*source] == 0 {
                    let (input, hash, chunks) = &pending[*source];
                    let rows: Vec<_> = chunks.iter().enumerate().map(|(i, chunk)| json!({
                        "text": chunk.text, "text_hash": text_hash(&chunk.text),
                        "embedding": embedded.remove(&(*source, i)).expect("all source parts embedded"),
                    })).collect();
                    self.publish_embeddings(input, hash, &model, &rows)?;
                }
            }
            done += batch.len();
            progress(done, order.len());
        }
        Ok(stats)
    }

    fn publish_embeddings(
        &self,
        source: &Source,
        hash: &str,
        model: &str,
        chunks: &[Value],
    ) -> Result<(), Error> {
        let rows: Vec<_> = chunks
            .iter()
            .enumerate()
            .map(|(part, row)| {
                let mut row = row.clone();
                row["part"] = json!(part);
                row
            })
            .collect();
        let mut vars = json!({"slug":source.slug,"source":source.source,"hash":hash,"model":model});
        if cbor::encode(&json!({"chunks":rows})).len() <= CHUNK_BYTES {
            vars["chunks"] = json!(rows);
            let _: Value = self.retrying(|| self.transact(&sql::publish(), vars.clone()))?;
            return Ok(());
        }
        // Reuse the board copy's bounded staging requests. Even a very long source publishes
        // its manifest and complete chunk set in one transaction, below the RPC request ceiling.
        let staging = format!(
            "embedding_{:016x}",
            RandomState::new().hash_one(std::process::id())
        );
        let staged = self.stage(&staging, &[("chunks", rows)]).and_then(|()| {
            let publish = sql::publish_staged(&staging);
            let _: Value = self.retrying(|| self.transact(&publish, vars.clone()))?;
            Ok(())
        });
        staged.map_err(|error| self.cleanup_staging(&sql::remove_staged(&staging), error))
    }

    /// Chunk neighbours fold to one task row by best cosine distance. Adaptive overfetch prevents
    /// duplicate/stale chunks from consuming the task limit. Filtered searches use exact cosine:
    /// HNSW post-filtering can lose eligible tasks before the limit, especially in small statuses.
    pub fn vector_search(
        &self,
        vector: &[f32],
        model: &str,
        status: Option<&str>,
        limit: usize,
        exact: bool,
    ) -> Result<Vec<VectorHit>, Error> {
        self.vector_search_profiled(vector, model, status, limit, exact)
            .map(|(hits, _)| hits)
    }

    /// The same lookup and results as [`Store::vector_search`], with diagnostic phase timings.
    pub fn vector_search_profiled(
        &self,
        vector: &[f32],
        model: &str,
        status: Option<&str>,
        limit: usize,
        exact: bool,
    ) -> Result<(Vec<VectorHit>, VectorSearchProfile), Error> {
        let started = Instant::now();
        let requests = self.client.requests();
        let mut profile = VectorSearchProfile::default();
        let result = self.vector_search_inner(vector, model, status, limit, exact, &mut profile);
        profile.total_s = started.elapsed().as_secs_f64();
        profile.rpc_requests = self.client.requests() - requests;
        result.map(|hits| (hits, profile))
    }

    fn vector_search_inner(
        &self,
        vector: &[f32],
        model: &str,
        status: Option<&str>,
        limit: usize,
        exact: bool,
        profile: &mut VectorSearchProfile,
    ) -> Result<Vec<VectorHit>, Error> {
        valid_vector(vector)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let started = Instant::now();
        let eligible = self.task_slugs(status)?;
        let eligible_set: std::collections::BTreeSet<_> = eligible.iter().cloned().collect();
        profile.eligibility_s += started.elapsed().as_secs_f64();
        let started = Instant::now();
        let total: usize = self.retrying(|| {
            self.transact(
                // HNSW can return older-model neighbours before the WHERE filter. A task-only
                // catch-up leaves those in the index, so the bound must cover every model.
                sql::TOTAL,
                json!({}),
            )
        })?;
        profile.count_s += started.elapsed().as_secs_f64();
        if total == 0 || eligible.is_empty() {
            return Ok(Vec::new());
        }
        // Cache freshness checks only for source keys the neighbour query actually reaches.
        // None records a deleted/malformed source so duplicate chunks do not fetch it again.
        let mut hashes: BTreeMap<(String, String), Option<String>> = BTreeMap::new();
        let mut count = limit.saturating_mul(8).max(40).min(total);
        loop {
            let neighbours = if exact || status.is_some() {
                sql::exact_neighbours(status.is_some())
            } else {
                sql::nearest_neighbours(count)
            };
            let started = Instant::now();
            let mut vars = json!({"vector": vector, "model": model});
            if status.is_some() {
                vars["eligible"] = json!(eligible);
            }
            let rows: Vec<ChunkHit> = self.retrying(|| self.transact(&neighbours, vars.clone()))?;
            profile.neighbours_s += started.elapsed().as_secs_f64();
            let started = Instant::now();
            // For an unfiltered query the SQL need not test every neighbour against the entire
            // board's slug array. Still discard deleted tasks before folding or limiting.
            let rows: Vec<_> = rows
                .into_iter()
                .filter(|row| eligible_set.contains(&row.slug))
                .collect();
            let needed: std::collections::BTreeSet<_> = rows
                .iter()
                .map(|row| (row.slug.clone(), row.source.clone()))
                .filter(|key| !hashes.contains_key(key))
                .collect();
            let sources = if needed.is_empty() {
                Vec::new()
            } else {
                self.embedding_sources_named(&needed.iter().cloned().collect::<Vec<_>>())?
            };
            profile.source_read_s += started.elapsed().as_secs_f64();
            let started = Instant::now();
            for key in needed {
                hashes.insert(key, None);
            }
            for source in sources {
                hashes.insert((source.slug, source.source), Some(text_hash(&source.text)));
            }
            profile.source_hash_s += started.elapsed().as_secs_f64();
            let started = Instant::now();
            let mut hits = BTreeMap::new();
            for row in rows {
                if !row.distance.is_finite()
                    || hashes
                        .get(&(row.slug.clone(), row.source.clone()))
                        .and_then(Option::as_ref)
                        != Some(&row.source_hash)
                {
                    continue;
                }
                hits.entry(row.slug.clone()).or_insert(VectorHit {
                    slug: row.slug,
                    score: 1.0 - row.distance,
                    source: if row.source == "brief" {
                        "body"
                    } else {
                        "message"
                    }
                    .into(),
                    text: row.text,
                });
            }
            let mut hits: Vec<_> = hits.into_values().collect();
            hits.sort_by(|a, b| {
                b.score
                    .total_cmp(&a.score)
                    .then_with(|| a.slug.cmp(&b.slug))
            });
            profile.folding_s += started.elapsed().as_secs_f64();
            if hits.len() >= limit || count == total || exact || status.is_some() {
                hits.truncate(limit);
                return Ok(hits);
            }
            count = count.saturating_mul(2).min(total);
        }
    }

    pub fn has_embeddings(&self, model: &str) -> Result<bool, Error> {
        self.retrying(|| self.transact(sql::HAS, json!({"model": model})))
    }
}

fn validate_batch_size(batch_size: usize) -> Result<(), Error> {
    if !(1..=64).contains(&batch_size) {
        return Err(refused(
            "embedding batch size must be between 1 and 64".into(),
        ));
    }
    Ok(())
}

fn valid_vector(vector: &[f32]) -> Result<(), Error> {
    if vector.len() != DIMENSIONS
        || vector.iter().any(|v| !v.is_finite())
        || !vector.iter().any(|v| *v != 0.0)
    {
        return Err(refused(format!(
            "embedding must contain {DIMENSIONS} finite F32 values and have nonzero norm"
        )));
    }
    Ok(())
}
