//! Local BGE inference and tokenizer-aware chunks. Only explicit embedding/search commands load
//! this runtime; the board's write path has no inference or network work.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::path::Path;
use tokenizers::Tokenizer;
use tract_linalg::multithread::{Executor, multithread_tract_scope};
use tract_onnx::prelude::*;

pub const DIMENSIONS: usize = 384;
pub const MAX_TOKENS: usize = 512;
pub const REVISION: &str = "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a";
/// The models' folder in the git common dir, shared by every worktree.
pub const MODELS: &str = crate::folders::MODELS;
// Includes the chunking, query instruction and pooling contract, not just the weights' name.
pub const MODEL_ID: &str = "BAAI/bge-small-en-v1.5@5c38ec7c:cls-l2:chunks510:v1";
const QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";
const WEIGHTS_SHA: &str = "828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35";
const TOKENIZER_SHA: &str = "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66";

#[derive(Debug, Clone)]
pub struct Chunk {
    pub text: String,
    /// WordPiece ids without CLS/SEP. Keeping these avoids re-tokenizing a split subword.
    pub tokens: Vec<u32>,
}

/// Production and deterministic test embedders share the same catch-up and search paths.
pub trait Embedder {
    fn model_id(&self) -> &str;
    fn chunks(&self, text: &str) -> Result<Vec<Chunk>, String>;
    fn embed(&mut self, chunks: &[Chunk]) -> Result<Vec<Vec<f32>>, String>;
    fn query(&mut self, text: &str) -> Result<Vec<f32>, String>;
}

pub fn text_hash(text: &str) -> String {
    sha256(text.as_bytes())
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(hex, "{byte:02x}").expect("writing to a String");
    }
    hex
}

/// Reciprocal rank fusion, retaining the exact identifier rule and the original keyword scores
/// when the board has no vector hits. It stays opt-in because the measured gain is one useful hit
/// in 50 (41/50 versus keyword's 40/50), while regressing the `golden` query.
pub fn fuse(
    keyword: &[crate::SearchHit],
    vectors: &[crate::VectorHit],
    query: &str,
) -> Vec<crate::SearchHit> {
    if vectors.is_empty() {
        return keyword.to_vec();
    }
    let mut scores = std::collections::BTreeMap::<String, f64>::new();
    for (rank, hit) in keyword.iter().take(100).enumerate() {
        *scores.entry(hit.slug.clone()).or_default() += 1.0 / (60.0 + (rank + 1) as f64);
    }
    for (rank, hit) in vectors.iter().take(100).enumerate() {
        *scores.entry(hit.slug.clone()).or_default() += 1.0 / (60.0 + (rank + 1) as f64);
    }
    if let Some(score) = scores.get_mut(&query.trim().to_lowercase()) {
        *score += 1.0;
    }
    let mut hits: Vec<_> = scores
        .into_iter()
        .map(|(slug, score)| crate::SearchHit { slug, score })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    hits
}

pub struct Bge {
    tokenizer: Tokenizer,
    model: TypedRunnableModel<TypedModel>,
    executor: Executor,
}

impl Bge {
    /// Fetch pinned public artifacts once into a cache shared by all worktrees. No board text
    /// enters a download request. The lock and atomic rename also cover concurrent first uses.
    pub fn load(common: &Path) -> Result<Self, String> {
        Self::load_with_threads(common, 1)
    }

    /// Parallel matrix multiplication for bulk catch-up; short interactive queries use one
    /// thread. The weights, tokenization, pooling and stored model id remain identical.
    pub fn load_with_threads(common: &Path, threads: usize) -> Result<Self, String> {
        if !(1..=64).contains(&threads) {
            return Err("inference threads must be between 1 and 64".into());
        }
        let dir = common.join(MODELS).join(REVISION);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let lock = download_lock(&dir)?;
        for (remote, local, sha) in [
            ("onnx/model.onnx", "model.onnx", WEIGHTS_SHA),
            ("tokenizer.json", "tokenizer.json", TOKENIZER_SHA),
        ] {
            fetch(&dir.join(local), remote, sha)?;
        }
        drop(lock);
        let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| format!("BGE tokenizer: {e}"))?;
        tokenizer
            .with_truncation(None)
            .map_err(|e| format!("BGE tokenizer: {e}"))?;
        tokenizer.with_padding(None);
        let model = (|| -> TractResult<_> {
            let mut model = tract_onnx::onnx().model_for_path(dir.join("model.onnx"))?;
            let batch = model.symbols.sym("batch");
            let length = model.symbols.sym("length");
            for input in 0..3 {
                model.set_input_fact(input, i64::fact([batch.to_dim(), length.to_dim()]).into())?;
            }
            model.into_optimized()?.into_runnable()
        })()
        .map_err(|e| format!("BGE ONNX model: {e:#}"))?;
        let executor = if threads == 1 {
            Executor::SingleThread
        } else {
            Executor::multithread_with_name(threads, "task-embedding")
        };
        Ok(Self {
            tokenizer,
            model,
            executor,
        })
    }
}

impl Embedder for Bge {
    fn model_id(&self) -> &str {
        MODEL_ID
    }

    fn chunks(&self, text: &str) -> Result<Vec<Chunk>, String> {
        tokenize_chunks(&self.tokenizer, text)
    }

    fn embed(&mut self, chunks: &[Chunk]) -> Result<Vec<Vec<f32>>, String> {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }
        let (length, inputs) = model_inputs(chunks)?;
        let output = multithread_tract_scope(self.executor.clone(), || self.model.run(inputs))
            .map_err(|e| format!("BGE inference: {e:#}"))?;
        cls_vectors(&output[0], chunks.len(), length)
    }

    fn query(&mut self, text: &str) -> Result<Vec<f32>, String> {
        let chunks = self.chunks(&format!("{QUERY_PREFIX}{text}"))?;
        self.embed(&chunks[..1])?
            .pop()
            .ok_or_else(|| "BGE query produced no vector".into())
    }
}

fn model_inputs(chunks: &[Chunk]) -> Result<(usize, TVec<TValue>), String> {
    let length = chunks.iter().map(|c| c.tokens.len() + 2).max().unwrap_or(2);
    if length > MAX_TOKENS {
        return Err(format!(
            "BGE input has {length} tokens; maximum is {MAX_TOKENS}"
        ));
    }
    let shape = [chunks.len(), length];
    let mut ids = tract_ndarray::Array2::<i64>::zeros(shape);
    let mut mask = ids.clone();
    for (row, chunk) in chunks.iter().enumerate() {
        ids[[row, 0]] = 101;
        for (col, token) in chunk.tokens.iter().enumerate() {
            ids[[row, col + 1]] = i64::from(*token);
        }
        ids[[row, chunk.tokens.len() + 1]] = 102;
        for col in 0..chunk.tokens.len() + 2 {
            mask[[row, col]] = 1;
        }
    }
    let types = tract_ndarray::Array2::<i64>::zeros(shape);
    Ok((
        length,
        tvec!(
            ids.into_tensor().into(),
            mask.into_tensor().into(),
            types.into_tensor().into()
        ),
    ))
}

fn cls_vectors(output: &Tensor, batch: usize, length: usize) -> Result<Vec<Vec<f32>>, String> {
    let hidden = output
        .to_array_view::<f32>()
        .map_err(|e| format!("BGE output: {e}"))?;
    if hidden.shape() != [batch, length, DIMENSIONS] {
        return Err(format!(
            "BGE output shape {:?}, expected [{batch}, {length}, {DIMENSIONS}]",
            hidden.shape()
        ));
    }
    (0..batch)
        .map(|row| {
            let mut vector: Vec<f32> = (0..DIMENSIONS).map(|col| hidden[[row, 0, col]]).collect();
            normalize(&mut vector)?;
            Ok(vector)
        })
        .collect()
}

fn tokenize_chunks(tokenizer: &Tokenizer, text: &str) -> Result<Vec<Chunk>, String> {
    let encoded = tokenizer
        .encode(text, false)
        .map_err(|e| format!("BGE tokenize: {e}"))?;
    let ids = encoded.get_ids();
    let offsets = encoded.get_offsets();
    let mut chunks = Vec::new();
    for start in (0..ids.len()).step_by(MAX_TOKENS - 2) {
        let end = (start + MAX_TOKENS - 2).min(ids.len());
        let from = if start == 0 { 0 } else { offsets[start].0 };
        let to = if end == ids.len() {
            text.len()
        } else {
            offsets[end].0
        };
        chunks.push(Chunk {
            text: text
                .get(from..to)
                .ok_or_else(|| format!("invalid tokenizer offsets {from}..{to}"))?
                .to_string(),
            tokens: ids[start..end].to_vec(),
        });
    }
    Ok(chunks)
}

fn normalize(vector: &mut [f32]) -> Result<(), String> {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err("BGE produced a zero or non-finite vector".into());
    }
    for v in vector {
        *v /= norm;
    }
    Ok(())
}

/// The model folder `dir`'s download lock, waited for: one fetch of its artifacts at a time.
fn download_lock(dir: &Path) -> Result<crate::sys::FileLock, String> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(dir.join("download.lock"))
        .map_err(|e| format!("model download lock: {e}"))?;
    file.lock()
        .map_err(|e| format!("model download lock: {e}"))?;
    Ok(crate::sys::FileLock(file))
}

fn fetch(path: &Path, remote: &str, sha: &str) -> Result<(), String> {
    if path.exists() {
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if sha256(&bytes) == sha {
            return Ok(());
        }
        return Err(format!(
            "{}: SHA-256 mismatch; remove this corrupt cache file and retry",
            path.display()
        ));
    }
    let temp = path.with_extension(format!("part-{}", std::process::id()));
    let url = format!("https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/{REVISION}/{remote}");
    let result = crate::sys::output(
        crate::sys::command("curl")
            .args([
                "-fsSL",
                "--retry",
                "2",
                "--connect-timeout",
                "15",
                "--max-time",
                "600",
                "--output",
            ])
            .arg(&temp)
            .arg(&url),
    )
    .map_err(|e| format!("download {url}: {e}"))?;
    let checked = (|| {
        if !result.status.success() {
            return Err(format!(
                "download {url}: {}: {}",
                result.status,
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        let bytes = fs::read(&temp).map_err(|e| format!("{}: {e}", temp.display()))?;
        if sha256(&bytes) != sha {
            return Err(format!("download {url}: SHA-256 mismatch"));
        }
        fs::rename(&temp, path).map_err(|e| format!("{}: {e}", path.display()))
    })();
    let _ = fs::remove_file(&temp);
    checked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_hashes_keep_the_pinned_sha256_hex_encoding() {
        assert_eq!(
            text_hash(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            text_hash("hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn invalid_thread_counts_are_refused_before_artifact_or_network_access() {
        for threads in [0, 65] {
            let error = Bge::load_with_threads(Path::new("unused-model-location"), threads)
                .err()
                .unwrap();
            assert_eq!(error, "inference threads must be between 1 and 64");
        }
    }

    #[test]
    fn an_ended_load_frees_the_download_while_a_forked_child_still_holds_the_descriptor() {
        let dir =
            std::env::temp_dir().join(format!("symphony-model-download-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let held = download_lock(&dir).unwrap();
        // What fork gives a child until it execs: the same open file, so the same lock.
        let inherited = held.0.try_clone().unwrap();
        drop(held);
        // Not waited for, so a lock still held fails here rather than hanging the test.
        let next = OpenOptions::new()
            .write(true)
            .open(dir.join("download.lock"))
            .unwrap();
        assert!(
            next.try_lock().is_ok(),
            "the ended load still holds the download"
        );
        drop((inherited, next));
        fs::remove_dir_all(dir).unwrap();
    }

    fn tokenizer() -> Tokenizer {
        Tokenizer::from_bytes(serde_json::to_vec(&serde_json::json!({
            "version": "1.0", "truncation": null, "padding": null,
            "added_tokens": [], "normalizer": null,
            "pre_tokenizer": {"type": "WhitespaceSplit"}, "post_processor": null, "decoder": null,
            "model": {"type": "WordPiece", "unk_token": "[UNK]", "continuing_subword_prefix": "##",
                "max_input_chars_per_word": 100, "vocab": {"[UNK]": 0, "hello": 1, "🚀": 2, "é": 3, "long": 4, "##word": 5}}
        })).unwrap()).unwrap()
    }

    #[test]
    fn chunks_reserve_cls_sep_and_preserve_every_token_at_the_512_token_boundary() {
        let tokenizer = tokenizer();
        for (count, sizes) in [
            (509, vec![509]),
            (510, vec![510]),
            (511, vec![510, 1]),
            (1020, vec![510, 510]),
            (1021, vec![510, 510, 1]),
        ] {
            let text = "hello ".repeat(count);
            let chunks = tokenize_chunks(&tokenizer, &text).unwrap();
            assert_eq!(
                chunks.iter().map(|c| c.tokens.len()).collect::<Vec<_>>(),
                sizes
            );
            assert_eq!(
                chunks.iter().map(|c| c.text.as_str()).collect::<String>(),
                text
            );
            assert!(chunks.iter().all(|c| c.tokens.iter().all(|id| *id == 1)));
        }
        let text = format!("{}é 🚀 longword", "hello ".repeat(509));
        let chunks = tokenize_chunks(&tokenizer, &text).unwrap();
        assert_eq!(chunks[0].tokens.len(), 510);
        assert_eq!(chunks[1].tokens, [2, 4, 5]);
        assert_eq!(
            chunks.iter().map(|c| c.text.as_str()).collect::<String>(),
            text
        );
        let chunks =
            tokenize_chunks(&tokenizer, &format!("{}longword", "hello ".repeat(509))).unwrap();
        assert_eq!(chunks[0].tokens.last(), Some(&4));
        assert_eq!(chunks[1].tokens, [5]); // preserve continuation id instead of re-tokenizing "word"
        for text in ["", "  \n\t"] {
            assert!(tokenize_chunks(&tokenizer, text).unwrap().is_empty());
        }
    }

    #[test]
    fn cls_vectors_are_unit_length_and_reject_invalid_norms() {
        let mut v = [3.0, 4.0];
        normalize(&mut v).unwrap();
        assert!((v[0] - 0.6).abs() < 0.00001 && (v[1] - 0.8).abs() < 0.00001);
        for mut v in [[0.0, 0.0], [f32::NAN, 1.0], [f32::INFINITY, 0.0]] {
            assert!(normalize(&mut v).is_err());
        }
    }

    #[test]
    fn onnx_inputs_add_special_tokens_mask_padding_and_reject_overlong_chunks() {
        let chunks = vec![
            Chunk {
                text: "short".into(),
                tokens: vec![12],
            },
            Chunk {
                text: "long".into(),
                tokens: vec![7, 8],
            },
        ];
        let (length, inputs) = model_inputs(&chunks).unwrap();
        assert_eq!(length, 4);
        assert_eq!(
            inputs[0].as_slice::<i64>().unwrap(),
            [101, 12, 102, 0, 101, 7, 8, 102]
        );
        assert_eq!(
            inputs[1].as_slice::<i64>().unwrap(),
            [1, 1, 1, 0, 1, 1, 1, 1]
        );
        assert_eq!(inputs[2].as_slice::<i64>().unwrap(), [0; 8]);
        assert!(
            model_inputs(&[Chunk {
                text: "too long".into(),
                tokens: vec![1; 511]
            }])
            .unwrap_err()
            .contains("513 tokens")
        );
    }

    #[test]
    fn pooling_selects_each_cls_token_and_refuses_an_unexpected_output_shape() {
        let mut hidden = tract_ndarray::Array3::<f32>::from_elem([2, 3, DIMENSIONS], 100.0);
        for col in 0..DIMENSIONS {
            hidden[[0, 0, col]] = 0.0;
            hidden[[1, 0, col]] = 0.0;
        }
        hidden[[0, 0, 0]] = 3.0;
        hidden[[0, 0, 1]] = 4.0;
        hidden[[1, 0, 2]] = 2.0;
        let tensor = hidden.into_tensor();
        let vectors = cls_vectors(&tensor, 2, 3).unwrap();
        assert!((vectors[0][0] - 0.6).abs() < 0.00001 && (vectors[0][1] - 0.8).abs() < 0.00001);
        assert_eq!(vectors[0][2], 0.0);
        assert_eq!(vectors[1][2], 1.0);
        assert_eq!(vectors[1][0], 0.0);
        assert!(
            cls_vectors(&tensor, 1, 3)
                .unwrap_err()
                .contains("output shape")
        );
    }

    #[test]
    fn fusion_keeps_exact_identifiers_first_and_keywords_unchanged_without_embeddings() {
        let keyword = vec![
            crate::SearchHit {
                slug: "sim/jump".into(),
                score: 1000000.0,
            },
            crate::SearchHit {
                slug: "sim/other".into(),
                score: 9.0,
            },
        ];
        let unchanged = fuse(&keyword, &[], "jump");
        assert_eq!(
            unchanged
                .iter()
                .map(|h| (&h.slug, h.score))
                .collect::<Vec<_>>(),
            keyword
                .iter()
                .map(|h| (&h.slug, h.score))
                .collect::<Vec<_>>()
        );
        let vectors = vec![crate::VectorHit {
            slug: "sim/other".into(),
            score: 1.0,
            source: "body".into(),
            text: "other".into(),
        }];
        for query in ["sim/jump", " SIM/JUMP "] {
            assert_eq!(fuse(&keyword, &vectors, query)[0].slug, "sim/jump");
        }
        assert_eq!(fuse(&keyword, &vectors, "jump")[0].slug, "sim/other");
    }

    #[test]
    fn corrupt_cached_artifacts_are_refused_before_network_or_model_loading() {
        let path = std::env::temp_dir().join(format!("bge-checksum-{}", std::process::id()));
        fs::write(&path, b"bad weights").unwrap();
        let error = fetch(&path, "never-fetched", WEIGHTS_SHA).unwrap_err();
        assert!(error.contains("SHA-256 mismatch"), "{error}");
        assert_eq!(fs::read(&path).unwrap(), b"bad weights");
        fs::remove_file(path).unwrap();
    }
}
